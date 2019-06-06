use bincode::{deserialize, serialize};
use bs58;
use crate::budget_program::BudgetState;
use crate::budget_transaction::BudgetTransaction;
use chrono::prelude::*;
use clap::ArgMatches;
use crate::crdt::NodeInfo;
use crate::token_service::DroneRequest;
use crate::fullnode::Config;
use buffett_crypto::hash::Hash;
use reqwest;
use reqwest::header::CONTENT_TYPE;
use ring::rand::SystemRandom;
use ring::signature::Ed25519KeyPair;
use serde_json::{self, Value};
use buffett_crypto::signature::{Keypair, KeypairUtil,Signature};
use buffett_interface::pubkey::Pubkey;
use std::fs::{self, File};
use std::io::prelude::*;
use std::io::{Error, ErrorKind, Write};
use std::mem::size_of;
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::Path;
use std::thread::sleep;
use std::time::Duration;
use std::{error, fmt, mem};
use crate::system_transaction::SystemTransaction;
use crate::transaction::Transaction;

#[derive(Debug, PartialEq)]
pub enum WalletCommand {
    Address,
    AirDrop(i64),
    Balance,
    Cancel(Pubkey),
    Confirm(Signature),
    // Pay(tokens, to, timestamp, timestamp_pubkey, witness(es), cancelable)
    Pay(
        i64,
        Pubkey,
        Option<DateTime<Utc>>,
        Option<Pubkey>,
        Option<Vec<Pubkey>>,
        Option<Pubkey>,
    ),
    // TimeElapsed(to, process_id, timestamp)
    TimeElapsed(Pubkey, Pubkey, DateTime<Utc>),
    // Witness(to, process_id)
    Witness(Pubkey, Pubkey),
}

#[derive(Debug, Clone)]
pub enum WalletError {
    CommandNotRecognized(String),
    BadParameter(String),
    RpcRequestError(String),
}

impl fmt::Display for WalletError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "invalid")
    }
}

impl error::Error for WalletError {
    fn description(&self) -> &str {
        "invalid"
    }

    fn cause(&self) -> Option<&error::Error> {
        // Generic error, underlying cause isn't tracked.
        None
    }
}

pub struct WalletConfig {
    pub leader: NodeInfo,
    pub id: Keypair,
    pub drone_addr: SocketAddr,
    pub rpc_addr: String,
    pub command: WalletCommand,
}

impl Default for WalletConfig {
    fn default() -> WalletConfig {
        let default_addr = socketaddr!(0, 8000);
        WalletConfig {
            leader: NodeInfo::new_with_socketaddr(&default_addr),
            id: Keypair::new(),
            drone_addr: default_addr,
            rpc_addr: default_addr.to_string(),
            command: WalletCommand::Balance,
        }
    }
}

pub fn parse_command(
    pubkey: Pubkey,
    matches: &ArgMatches,
) -> Result<WalletCommand, Box<error::Error>> {
    let response = match matches.subcommand() {
        ("address", Some(_address_matches)) => Ok(WalletCommand::Address),
        ("airdrop", Some(airdrop_matches)) => {
            let tokens = airdrop_matches.value_of("tokens").unwrap().parse()?;
            Ok(WalletCommand::AirDrop(tokens))
        }
        ("balance", Some(_balance_matches)) => Ok(WalletCommand::Balance),
        ("cancel", Some(cancel_matches)) => {
            let pubkey_vec = bs58::decode(cancel_matches.value_of("process-id").unwrap())
                .into_vec()
                .expect("base58-encoded public key");

            if pubkey_vec.len() != mem::size_of::<Pubkey>() {
                eprintln!("{}", cancel_matches.usage());
                Err(WalletError::BadParameter("Invalid public key".to_string()))?;
            }
            let process_id = Pubkey::new(&pubkey_vec);
            Ok(WalletCommand::Cancel(process_id))
        }
        ("confirm", Some(confirm_matches)) => {
            let signatures = bs58::decode(confirm_matches.value_of("signature").unwrap())
                .into_vec()
                .expect("base58-encoded signature");

            if signatures.len() == mem::size_of::<Signature>() {
                let signature = Signature::new(&signatures);
                Ok(WalletCommand::Confirm(signature))
            } else {
                eprintln!("{}", confirm_matches.usage());
                Err(WalletError::BadParameter("Invalid signature".to_string()))
            }
        }
        ("pay", Some(pay_matches)) => {
            let tokens = pay_matches.value_of("tokens").unwrap().parse()?;
            let to = if pay_matches.is_present("to") {
                let pubkey_vec = bs58::decode(pay_matches.value_of("to").unwrap())
                    .into_vec()
                    .expect("base58-encoded public key");

                if pubkey_vec.len() != mem::size_of::<Pubkey>() {
                    eprintln!("{}", pay_matches.usage());
                    Err(WalletError::BadParameter(
                        "Invalid to public key".to_string(),
                    ))?;
                }
                Pubkey::new(&pubkey_vec)
            } else {
                pubkey
            };
            let timestamp = if pay_matches.is_present("timestamp") {
                // Parse input for serde_json
                let date_string = if !pay_matches.value_of("timestamp").unwrap().contains('Z') {
                    format!("\"{}Z\"", pay_matches.value_of("timestamp").unwrap())
                } else {
                    format!("\"{}\"", pay_matches.value_of("timestamp").unwrap())
                };
                Some(serde_json::from_str(&date_string)?)
            } else {
                None
            };
            let timestamp_pubkey = if pay_matches.is_present("timestamp-pubkey") {
                let pubkey_vec = bs58::decode(pay_matches.value_of("timestamp-pubkey").unwrap())
                    .into_vec()
                    .expect("base58-encoded public key");

                if pubkey_vec.len() != mem::size_of::<Pubkey>() {
                    eprintln!("{}", pay_matches.usage());
                    Err(WalletError::BadParameter(
                        "Invalid timestamp public key".to_string(),
                    ))?;
                }
                Some(Pubkey::new(&pubkey_vec))
            } else {
                None
            };
            let witness_vec = if pay_matches.is_present("witness") {
                let witnesses = pay_matches.values_of("witness").unwrap();
                let mut collection = Vec::new();
                for witness in witnesses {
                    let pubkey_vec = bs58::decode(witness)
                        .into_vec()
                        .expect("base58-encoded public key");

                    if pubkey_vec.len() != mem::size_of::<Pubkey>() {
                        eprintln!("{}", pay_matches.usage());
                        Err(WalletError::BadParameter(
                            "Invalid witness public key".to_string(),
                        ))?;
                    }
                    collection.push(Pubkey::new(&pubkey_vec));
                }
                Some(collection)
            } else {
                None
            };
            let cancelable = if pay_matches.is_present("cancelable") {
                Some(pubkey)
            } else {
                None
            };

            Ok(WalletCommand::Pay(
                tokens,
                to,
                timestamp,
                timestamp_pubkey,
                witness_vec,
                cancelable,
            ))
        }
        ("send-signature", Some(sig_matches)) => {
            let pubkey_vec = bs58::decode(sig_matches.value_of("to").unwrap())
                .into_vec()
                .expect("base58-encoded public key");

            if pubkey_vec.len() != mem::size_of::<Pubkey>() {
                eprintln!("{}", sig_matches.usage());
                Err(WalletError::BadParameter("Invalid public key".to_string()))?;
            }
            let to = Pubkey::new(&pubkey_vec);

            let pubkey_vec = bs58::decode(sig_matches.value_of("process-id").unwrap())
                .into_vec()
                .expect("base58-encoded public key");

            if pubkey_vec.len() != mem::size_of::<Pubkey>() {
                eprintln!("{}", sig_matches.usage());
                Err(WalletError::BadParameter("Invalid public key".to_string()))?;
            }
            let process_id = Pubkey::new(&pubkey_vec);
            Ok(WalletCommand::Witness(to, process_id))
        }
        ("send-timestamp", Some(timestamp_matches)) => {
            let pubkey_vec = bs58::decode(timestamp_matches.value_of("to").unwrap())
                .into_vec()
                .expect("base58-encoded public key");

            if pubkey_vec.len() != mem::size_of::<Pubkey>() {
                eprintln!("{}", timestamp_matches.usage());
                Err(WalletError::BadParameter("Invalid public key".to_string()))?;
            }
            let to = Pubkey::new(&pubkey_vec);

            let pubkey_vec = bs58::decode(timestamp_matches.value_of("process-id").unwrap())
                .into_vec()
                .expect("base58-encoded public key");

            if pubkey_vec.len() != mem::size_of::<Pubkey>() {
                eprintln!("{}", timestamp_matches.usage());
                Err(WalletError::BadParameter("Invalid public key".to_string()))?;
            }
            let process_id = Pubkey::new(&pubkey_vec);
            let dt = if timestamp_matches.is_present("datetime") {
                // Parse input for serde_json
                let date_string = if !timestamp_matches
                    .value_of("datetime")
                    .unwrap()
                    .contains('Z')
                {
                    format!("\"{}Z\"", timestamp_matches.value_of("datetime").unwrap())
                } else {
                    format!("\"{}\"", timestamp_matches.value_of("datetime").unwrap())
                };
                serde_json::from_str(&date_string)?
            } else {
                Utc::now()
            };
            Ok(WalletCommand::TimeElapsed(to, process_id, dt))
        }
        ("", None) => {
            eprintln!("{}", matches.usage());
            Err(WalletError::CommandNotRecognized(
                "no subcommand given".to_string(),
            ))
        }
        _ => unreachable!(),
    }?;
    Ok(response)
}

pub fn process_command(config: &WalletConfig) -> Result<String, Box<error::Error>> {
    match config.command {
        // Get address of this client
        WalletCommand::Address => Ok(format!("{}", config.id.pubkey())),
        // Request an airdrop from tokenbots;
        WalletCommand::AirDrop(tokens) => {
            println!(
                "Requesting airdrop of {:?} tokens from {}",
                tokens, config.drone_addr
            );
            let params = json!(format!("{}", config.id.pubkey()));
            let previous_balance = match WalletRpcRequest::GetBalance
                .make_rpc_request(&config.rpc_addr, 1, Some(params))?
                .as_i64()
            {
                Some(tokens) => tokens,
                None => Err(WalletError::RpcRequestError(
                    "Received result of an unexpected type".to_string(),
                ))?,
            };
            request_airdrop(&config.drone_addr, &config.id.pubkey(), tokens as u64)?;

            // TODO: return airdrop Result from Drone instead of polling the
            //       network
            let mut current_balance = previous_balance;
            for _ in 0..20 {
                sleep(Duration::from_millis(500));
                let params = json!(format!("{}", config.id.pubkey()));
                current_balance = WalletRpcRequest::GetBalance
                    .make_rpc_request(&config.rpc_addr, 1, Some(params))?
                    .as_i64()
                    .unwrap_or(previous_balance);

                if previous_balance != current_balance {
                    break;
                }
                println!(".");
            }
            if current_balance - previous_balance != tokens {
                Err("Airdrop failed!")?;
            }
            Ok(format!("Your balance is: {:?}", current_balance))
        }
        // Check client balance
        WalletCommand::Balance => {
            println!("Balance requested...");
            let params = json!(format!("{}", config.id.pubkey()));
            let balance = WalletRpcRequest::GetBalance
                .make_rpc_request(&config.rpc_addr, 1, Some(params))?
                .as_i64();
            match balance {
                Some(0) => Ok("No account found! Request an airdrop to get started.".to_string()),
                Some(tokens) => Ok(format!("Your balance is: {:?}", tokens)),
                None => Err(WalletError::RpcRequestError(
                    "Received result of an unexpected type".to_string(),
                ))?,
            }
        }
        // Cancel a contract by contract Pubkey
        WalletCommand::Cancel(pubkey) => {
            let last_id = get_last_id(&config)?;

            let tx =
                Transaction::budget_new_signature(&config.id, pubkey, config.id.pubkey(), last_id);
            let signature_str = serialize_and_send_tx(&config, &tx)?;

            Ok(signature_str.to_string())
        }
        // Confirm the last client transaction by signature
        WalletCommand::Confirm(signature) => {
            let params = json!(format!("{}", signature));
            let confirmation = WalletRpcRequest::ConfirmTransaction
                .make_rpc_request(&config.rpc_addr, 1, Some(params))?
                .as_bool();
            match confirmation {
                Some(b) => {
                    if b {
                        Ok("Confirmed".to_string())
                    } else {
                        Ok("Not found".to_string())
                    }
                }
                None => Err(WalletError::RpcRequestError(
                    "Received result of an unexpected type".to_string(),
                ))?,
            }
        }
        // If client has positive balance, pay tokens to another address
        WalletCommand::Pay(tokens, to, timestamp, timestamp_pubkey, ref witnesses, cancelable) => {
            let last_id = get_last_id(&config)?;

            if timestamp == None && *witnesses == None {
                let tx = Transaction::system_new(&config.id, to, tokens, last_id);
                let signature_str = serialize_and_send_tx(&config, &tx)?;
                Ok(signature_str.to_string())
            } else if *witnesses == None {
                let dt = timestamp.unwrap();
                let dt_pubkey = match timestamp_pubkey {
                    Some(pubkey) => pubkey,
                    None => config.id.pubkey(),
                };

                let contract_funds = Keypair::new();
                let contract_state = Keypair::new();
                let budget_program_id = BudgetState::id();

                // Create account for contract funds
                let tx = Transaction::system_create(
                    &config.id,
                    contract_funds.pubkey(),
                    last_id,
                    tokens,
                    0,
                    budget_program_id,
                    0,
                );
                let _signature_str = serialize_and_send_tx(&config, &tx)?;

                // Create account for contract state
                let tx = Transaction::system_create(
                    &config.id,
                    contract_state.pubkey(),
                    last_id,
                    1,
                    196,
                    budget_program_id,
                    0,
                );
                let _signature_str = serialize_and_send_tx(&config, &tx)?;

                // Initializing contract
                let tx = Transaction::budget_new_on_date(
                    &contract_funds,
                    to,
                    contract_state.pubkey(),
                    dt,
                    dt_pubkey,
                    cancelable,
                    tokens,
                    last_id,
                );
                let signature_str = serialize_and_send_tx(&config, &tx)?;

                Ok(json!({
                    "signature": signature_str,
                    "processId": format!("{}", contract_state.pubkey()),
                }).to_string())
            } else if timestamp == None {
                let last_id = get_last_id(&config)?;

                let witness = if let Some(ref witness_vec) = *witnesses {
                    witness_vec[0]
                } else {
                    Err(WalletError::BadParameter(
                        "Could not parse required signature pubkey(s)".to_string(),
                    ))?
                };

                let contract_funds = Keypair::new();
                let contract_state = Keypair::new();
                let budget_program_id = BudgetState::id();

                // Create account for contract funds
                let tx = Transaction::system_create(
                    &config.id,
                    contract_funds.pubkey(),
                    last_id,
                    tokens,
                    0,
                    budget_program_id,
                    0,
                );
                let _signature_str = serialize_and_send_tx(&config, &tx)?;

                // Create account for contract state
                let tx = Transaction::system_create(
                    &config.id,
                    contract_state.pubkey(),
                    last_id,
                    1,
                    196,
                    budget_program_id,
                    0,
                );
                let _signature_str = serialize_and_send_tx(&config, &tx)?;

                // Initializing contract
                let tx = Transaction::budget_new_when_signed(
                    &contract_funds,
                    to,
                    contract_state.pubkey(),
                    witness,
                    cancelable,
                    tokens,
                    last_id,
                );
                let signature_str = serialize_and_send_tx(&config, &tx)?;

                Ok(json!({
                    "signature": signature_str,
                    "processId": format!("{}", contract_state.pubkey()),
                }).to_string())
            } else {
                Ok("Combo transactions not yet handled".to_string())
            }
        }
        // Apply time elapsed to contract
        WalletCommand::TimeElapsed(to, pubkey, dt) => {
            let params = json!(format!("{}", config.id.pubkey()));
            let balance = WalletRpcRequest::GetBalance
                .make_rpc_request(&config.rpc_addr, 1, Some(params))?
                .as_i64();
            if let Some(0) = balance {
                request_airdrop(&config.drone_addr, &config.id.pubkey(), 1)?;
            }

            let last_id = get_last_id(&config)?;

            let tx = Transaction::budget_new_timestamp(&config.id, pubkey, to, dt, last_id);
            let signature_str = serialize_and_send_tx(&config, &tx)?;

            Ok(signature_str.to_string())
        }
        // Apply witness signature to contract
        WalletCommand::Witness(to, pubkey) => {
            let last_id = get_last_id(&config)?;

            let params = json!(format!("{}", config.id.pubkey()));
            let balance = WalletRpcRequest::GetBalance
                .make_rpc_request(&config.rpc_addr, 1, Some(params))?
                .as_i64();
            if let Some(0) = balance {
                request_airdrop(&config.drone_addr, &config.id.pubkey(), 1)?;
            }

            let tx = Transaction::budget_new_signature(&config.id, pubkey, to, last_id);
            let signature_str = serialize_and_send_tx(&config, &tx)?;

            Ok(signature_str.to_string())
        }
    }
}

pub fn read_leader(path: &str) -> Result<Config, WalletError> {
    let file = File::open(path.to_string()).or_else(|err| {
        Err(WalletError::BadParameter(format!(
            "{}: Unable to open leader file: {}",
            err, path
        )))
    })?;

    serde_json::from_reader(file).or_else(|err| {
        Err(WalletError::BadParameter(format!(
            "{}: Failed to parse leader file: {}",
            err, path
        )))
    })
}

pub fn request_airdrop(
    drone_addr: &SocketAddr,
    id: &Pubkey,
    tokens: u64,
) -> Result<Signature, Error> {
    // TODO: make this async tokio client
    let mut stream = TcpStream::connect(drone_addr)?;
    let req = DroneRequest::GetAirdrop {
        airdrop_request_amount: tokens,
        client_pubkey: *id,
    };
    let tx = serialize(&req).expect("serialize drone request");
    stream.write_all(&tx)?;
    let mut buffer = [0; size_of::<Signature>()];
    stream
        .read_exact(&mut buffer)
        .or_else(|_| Err(Error::new(ErrorKind::Other, "Airdrop failed")))?;
    let signature: Signature = deserialize(&buffer).or_else(|err| {
        Err(Error::new(
            ErrorKind::Other,
            format!("deserialize signature in request_airdrop: {:?}", err),
        ))
    })?;
    // TODO: add timeout to this function, in case of unresponsive drone
    Ok(signature)
}

pub fn gen_keypair_file(outfile: String) -> Result<String, Box<error::Error>> {
    let rnd = SystemRandom::new();
    let pkcs8_bytes = Ed25519KeyPair::generate_pkcs8(&rnd)?;
    let serialized = serde_json::to_string(&pkcs8_bytes.to_vec())?;

    if outfile != "-" {
        if let Some(outdir) = Path::new(&outfile).parent() {
            fs::create_dir_all(outdir)?;
        }
        let mut f = File::create(outfile)?;
        f.write_all(&serialized.clone().into_bytes())?;
    }
    Ok(serialized)
}

pub enum WalletRpcRequest {
    ConfirmTransaction,
    GetAccountInfo,
    GetBalance,
    GetFinality,
    GetLastId,
    GetTransactionCount,
    RequestAirdrop,
    SendTransaction,
}
impl WalletRpcRequest {
    fn make_rpc_request(
        &self,
        rpc_addr: &str,
        id: u64,
        params: Option<Value>,
    ) -> Result<Value, Box<error::Error>> {
        let jsonrpc = "2.0";
        let method = match self {
            WalletRpcRequest::ConfirmTransaction => "confirmTransaction",
            WalletRpcRequest::GetAccountInfo => "getAccountInfo",
            WalletRpcRequest::GetBalance => "getBalance",
            WalletRpcRequest::GetFinality => "getFinality",
            WalletRpcRequest::GetLastId => "getLastId",
            WalletRpcRequest::GetTransactionCount => "getTransactionCount",
            WalletRpcRequest::RequestAirdrop => "requestAirdrop",
            WalletRpcRequest::SendTransaction => "sendTransaction",
        };
        let client = reqwest::Client::new();
        let mut request = json!({
           "jsonrpc": jsonrpc,
           "id": id,
           "method": method,
        });
        if let Some(param_string) = params {
            request["params"] = json!(vec![param_string]);
        }
        let mut response = client
            .post(rpc_addr)
            .header(CONTENT_TYPE, "application/json")
            .body(request.to_string())
            .send()?;
        let json: Value = serde_json::from_str(&response.text()?)?;
        if json["error"].is_object() {
            Err(WalletError::RpcRequestError(format!(
                "RPC Error response: {}",
                serde_json::to_string(&json["error"]).unwrap()
            )))?
        }
        Ok(json["result"].clone())
    }
}

fn get_last_id(config: &WalletConfig) -> Result<Hash, Box<error::Error>> {
    let result = WalletRpcRequest::GetLastId.make_rpc_request(&config.rpc_addr, 1, None)?;
    if result.as_str().is_none() {
        Err(WalletError::RpcRequestError(
            "Received bad last_id".to_string(),
        ))?
    }
    let last_id_str = result.as_str().unwrap();
    let last_id_vec = bs58::decode(last_id_str)
        .into_vec()
        .map_err(|_| WalletError::RpcRequestError("Received bad last_id".to_string()))?;
    Ok(Hash::new(&last_id_vec))
}

fn serialize_and_send_tx(
    config: &WalletConfig,
    tx: &Transaction,
) -> Result<String, Box<error::Error>> {
    let serialized = serialize(tx).unwrap();
    let params = json!(serialized);
    let signature =
        WalletRpcRequest::SendTransaction.make_rpc_request(&config.rpc_addr, 2, Some(params))?;
    if signature.as_str().is_none() {
        Err(WalletError::RpcRequestError(
            "Received result of an unexpected type".to_string(),
        ))?
    }
    Ok(signature.as_str().unwrap().to_string())
}

