use crate::tx_vault::{Bank, BankError};
use bincode::deserialize;
use bs58;
use jsonrpc_core::*;
use jsonrpc_http_server::*;
use crate::service::Service;
use buffett_crypto::signature::Signature;
use buffett_interface::account::Account;
use buffett_interface::pubkey::Pubkey;
use std::mem;
use std::net::{SocketAddr, UdpSocket};
use std::result;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, sleep, Builder, JoinHandle};
use std::time::Duration;
use std::time::Instant;
use crate::transaction::Transaction;
use wallet::request_airdrop;

pub const RPC_PORT: u16 = 8899;

pub struct JsonRpcService {
    thread_hdl: JoinHandle<()>,
}

impl JsonRpcService {
    pub fn new(
        bank: &Arc<Bank>,
        transactions_addr: SocketAddr,
        drone_addr: SocketAddr,
        rpc_addr: SocketAddr,
        exit: Arc<AtomicBool>,
    ) -> Self {
        let request_processor = JsonRpcRequestProcessor::new(bank.clone());
        let thread_hdl = Builder::new()
            .name("bitconch-jsonrpc".to_string())
            .spawn(move || {
                let mut io = MetaIoHandler::default();
                let rpc = RpcSolImpl;
                io.extend_with(rpc.to_delegate());

                let server =
                    ServerBuilder::with_meta_extractor(io, move |_req: &hyper::Request<hyper::Body>| Meta {
                        request_processor: request_processor.clone(),
                        transactions_addr,
                        drone_addr,
                    }).threads(4)
                        .cors(DomainsValidation::AllowOnly(vec![
                            AccessControlAllowOrigin::Any,
                        ]))
                        .start_http(&rpc_addr);
                if server.is_err() {
                    warn!("JSON RPC service unavailable: unable to bind to RPC port {}. \nMake sure this port is not already in use by another application", rpc_addr.port());
                    return;
                }
                loop {
                    if exit.load(Ordering::Relaxed) {
                        server.unwrap().close();
                        break;
                    }
                    sleep(Duration::from_millis(100));
                }
                ()
            })
            .unwrap();
        JsonRpcService { thread_hdl }
    }
}

impl Service for JsonRpcService {
    type JoinReturnType = ();

    fn join(self) -> thread::Result<()> {
        self.thread_hdl.join()
    }
}

#[derive(Clone)]
pub struct Meta {
    pub request_processor: JsonRpcRequestProcessor,
    pub transactions_addr: SocketAddr,
    pub drone_addr: SocketAddr,
}
impl Metadata for Meta {}

#[derive(PartialEq, Serialize)]
pub enum RpcSignatureStatus {
    Confirmed,
    SignatureNotFound,
    ProgramRuntimeError,
    GenericFailure,
}

build_rpc_trait! {
    pub trait RpcSol {
        type Metadata;

        #[rpc(meta, name = "confirmTransaction")]
        fn confirm_transaction(&self, Self::Metadata, String) -> Result<bool>;

        #[rpc(meta, name = "getAccountInfo")]
        fn get_account_info(&self, Self::Metadata, String) -> Result<Account>;

        #[rpc(meta, name = "getBalance")]
        fn get_balance(&self, Self::Metadata, String) -> Result<i64>;

        #[rpc(meta, name = "getFinality")]
        fn get_finality(&self, Self::Metadata) -> Result<usize>;

        #[rpc(meta, name = "getLastId")]
        fn get_last_id(&self, Self::Metadata) -> Result<String>;

        #[rpc(meta, name = "getSignatureStatus")]
        fn get_signature_status(&self, Self::Metadata, String) -> Result<RpcSignatureStatus>;

        #[rpc(meta, name = "getTransactionCount")]
        fn get_transaction_count(&self, Self::Metadata) -> Result<u64>;

        #[rpc(meta, name= "requestAirdrop")]
        fn request_airdrop(&self, Self::Metadata, String, u64) -> Result<String>;

        #[rpc(meta, name = "sendTransaction")]
        fn send_transaction(&self, Self::Metadata, Vec<u8>) -> Result<String>;
    }
}

pub struct RpcSolImpl;
impl RpcSol for RpcSolImpl {
    type Metadata = Meta;

    fn confirm_transaction(&self, meta: Self::Metadata, id: String) -> Result<bool> {
        self.get_signature_status(meta, id)
            .map(|status| status == RpcSignatureStatus::Confirmed)
    }

    fn get_account_info(&self, meta: Self::Metadata, id: String) -> Result<Account> {
        let pubkey_vec = bs58::decode(id)
            .into_vec()
            .map_err(|_| Error::invalid_request())?;
        if pubkey_vec.len() != mem::size_of::<Pubkey>() {
            return Err(Error::invalid_request());
        }
        let pubkey = Pubkey::new(&pubkey_vec);
        meta.request_processor.get_account_info(pubkey)
    }
    fn get_balance(&self, meta: Self::Metadata, id: String) -> Result<i64> {
        let pubkey_vec = bs58::decode(id)
            .into_vec()
            .map_err(|_| Error::invalid_request())?;
        if pubkey_vec.len() != mem::size_of::<Pubkey>() {
            return Err(Error::invalid_request());
        }
        let pubkey = Pubkey::new(&pubkey_vec);
        meta.request_processor.get_balance(pubkey)
    }
    fn get_finality(&self, meta: Self::Metadata) -> Result<usize> {
        meta.request_processor.get_finality()
    }
    fn get_last_id(&self, meta: Self::Metadata) -> Result<String> {
        meta.request_processor.get_last_id()
    }
    fn get_signature_status(&self, meta: Self::Metadata, id: String) -> Result<RpcSignatureStatus> {
        let signature_vec = bs58::decode(id)
            .into_vec()
            .map_err(|_| Error::invalid_request())?;
        if signature_vec.len() != mem::size_of::<Signature>() {
            return Err(Error::invalid_request());
        }
        let signature = Signature::new(&signature_vec);
        Ok(
            match meta.request_processor.get_signature_status(signature) {
                Ok(_) => RpcSignatureStatus::Confirmed,
                Err(BankError::ProgramRuntimeError) => RpcSignatureStatus::ProgramRuntimeError,
                Err(BankError::SignatureNotFound) => RpcSignatureStatus::SignatureNotFound,
                Err(err) => {
                    trace!("mapping {:?} to GenericFailure", err);
                    RpcSignatureStatus::GenericFailure
                }
            },
        )
    }
    fn get_transaction_count(&self, meta: Self::Metadata) -> Result<u64> {
        meta.request_processor.get_transaction_count()
    }
    fn request_airdrop(&self, meta: Self::Metadata, id: String, tokens: u64) -> Result<String> {
        let pubkey_vec = bs58::decode(id)
            .into_vec()
            .map_err(|_| Error::invalid_request())?;
        if pubkey_vec.len() != mem::size_of::<Pubkey>() {
            return Err(Error::invalid_request());
        }
        let pubkey = Pubkey::new(&pubkey_vec);
        let signature = request_airdrop(&meta.drone_addr, &pubkey, tokens)
            .map_err(|_| Error::internal_error())?;
        let now = Instant::now();
        let mut signature_status;
        loop {
            signature_status = meta.request_processor.get_signature_status(signature);

            if signature_status.is_ok() {
                return Ok(bs58::encode(signature).into_string());
            } else if now.elapsed().as_secs() > 5 {
                return Err(Error::internal_error());
            }
            sleep(Duration::from_millis(100));
        }
    }
    fn send_transaction(&self, meta: Self::Metadata, data: Vec<u8>) -> Result<String> {
        let tx: Transaction = deserialize(&data).map_err(|err| {
            debug!("send_transaction: deserialize error: {:?}", err);
            Error::invalid_request()
        })?;
        let transactions_socket = UdpSocket::bind("0.0.0.0:0").unwrap();
        transactions_socket
            .send_to(&data, &meta.transactions_addr)
            .map_err(|err| {
                debug!("send_transaction: send_to error: {:?}", err);
                Error::internal_error()
            })?;
        Ok(bs58::encode(tx.signature).into_string())
    }
}
#[derive(Clone)]
pub struct JsonRpcRequestProcessor {
    bank: Arc<Bank>,
}
impl JsonRpcRequestProcessor {
    /// Create a new request processor that wraps the given Bank.
    pub fn new(bank: Arc<Bank>) -> Self {
        JsonRpcRequestProcessor { bank }
    }

    /// Process JSON-RPC request items sent via JSON-RPC.
    fn get_account_info(&self, pubkey: Pubkey) -> Result<Account> {
        self.bank
            .get_account(&pubkey)
            .ok_or_else(Error::invalid_request)
    }
    fn get_balance(&self, pubkey: Pubkey) -> Result<i64> {
        let val = self.bank.get_balance(&pubkey);
        Ok(val)
    }
    fn get_finality(&self) -> Result<usize> {
        Ok(self.bank.finality())
    }
    fn get_last_id(&self) -> Result<String> {
        let id = self.bank.last_id();
        Ok(bs58::encode(id).into_string())
    }
    fn get_signature_status(&self, signature: Signature) -> result::Result<(), BankError> {
        self.bank.get_signature_status(&signature)
    }
    fn get_transaction_count(&self) -> Result<u64> {
        Ok(self.bank.transaction_count() as u64)
    }
}

