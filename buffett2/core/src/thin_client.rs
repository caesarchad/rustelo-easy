use crate::tx_vault::Bank;
use bincode::{deserialize, serialize};
use crate::crdt::{Crdt, CrdtError, NodeInfo};
use buffett_crypto::hash::Hash;
use log::Level;
use crate::ncp::Ncp;
use crate::request::{Request, Response};
use crate::result::{Error, Result};
use buffett_crypto::signature::{Keypair, Signature};
use buffett_interface::account::Account;
use buffett_interface::pubkey::Pubkey;
use std;
use std::collections::HashMap;
use std::io;
use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, RwLock};
use std::thread::sleep;
use std::time::Duration;
use std::time::Instant;
use crate::system_transaction::SystemTransaction;
use buffett_timing::timing;
use crate::transaction::Transaction;

use influx_db_client as influxdb;
use crate::metrics;


pub struct ThinClient {
    requests_addr: SocketAddr,
    requests_socket: UdpSocket,
    transactions_addr: SocketAddr,
    transactions_socket: UdpSocket,
    last_id: Option<Hash>,
    transaction_count: u64,
    balances: HashMap<Pubkey, Account>,
    signature_status: bool,
    finality: Option<usize>,
}

impl ThinClient {
    
    pub fn new(
        requests_addr: SocketAddr,
        requests_socket: UdpSocket,
        transactions_addr: SocketAddr,
        transactions_socket: UdpSocket,
    ) -> Self {
        ThinClient {
            requests_addr,
            requests_socket,
            transactions_addr,
            transactions_socket,
            last_id: None,
            transaction_count: 0,
            balances: HashMap::new(),
            signature_status: false,
            finality: None,
        }
    }

    pub fn recv_response(&self) -> io::Result<Response> {
        let mut buf = vec![0u8; 1024];
        trace!("start recv_from");
        match self.requests_socket.recv_from(&mut buf) {
            Ok((len, from)) => {
                trace!("end recv_from got {} {}", len, from);
                deserialize(&buf)
                    .or_else(|_| Err(io::Error::new(io::ErrorKind::Other, "deserialize")))
            }
            Err(e) => {
                trace!("end recv_from got {:?}", e);
                Err(e)
            }
        }
    }

    pub fn process_response(&mut self, resp: &Response) {
        match *resp {
            Response::Account {
                key,
                account: Some(ref account),
            } => {
                trace!("Response account {:?} {:?}", key, account);
                self.balances.insert(key, account.clone());
            }
            Response::Account { key, account: None } => {
                debug!("Response account {}: None ", key);
                self.balances.remove(&key);
            }
            Response::LastId { id } => {
                trace!("Response last_id {:?}", id);
                self.last_id = Some(id);
            }
            Response::TransactionCount { transaction_count } => {
                trace!("Response transaction count {:?}", transaction_count);
                self.transaction_count = transaction_count;
            }
            Response::SignatureStatus { signature_status } => {
                self.signature_status = signature_status;
                if signature_status {
                    trace!("Response found signature");
                } else {
                    trace!("Response signature not found");
                }
            }
            Response::Finality { time } => {
                trace!("Response finality {:?}", time);
                self.finality = Some(time);
            }
        }
    }

    
    pub fn transfer_signed(&self, tx: &Transaction) -> io::Result<Signature> {
        let data = serialize(&tx).expect("serialize Transaction in pub fn transfer_signed");
        self.transactions_socket
            .send_to(&data, &self.transactions_addr)?;
        Ok(tx.signature)
    }

   
    pub fn retry_transfer_signed(
        &mut self,
        tx: &Transaction,
        tries: usize,
    ) -> io::Result<Signature> {
        let data = serialize(&tx).expect("serialize Transaction in pub fn transfer_signed");
        for x in 0..tries {
            self.transactions_socket
                .send_to(&data, &self.transactions_addr)?;
            if self.poll_for_signature(&tx.signature).is_ok() {
                return Ok(tx.signature);
            }
            info!("{} tries failed transfer to {}", x, self.transactions_addr);
        }
        Err(io::Error::new(
            io::ErrorKind::Other,
            "retry_transfer_signed failed",
        ))
    }

    pub fn transfer(
        &self,
        n: i64,
        keypair: &Keypair,
        to: Pubkey,
        last_id: &Hash,
    ) -> io::Result<Signature> {
        let now = Instant::now();
        let tx = Transaction::system_new(keypair, to, n, *last_id);
        let result = self.transfer_signed(&tx);
        metrics::submit(
            influxdb::Point::new("thinclient")
                .add_tag("op", influxdb::Value::String("transfer".to_string()))
                .add_field(
                    "duration_ms",
                    influxdb::Value::Integer(timing::duration_in_milliseconds(&now.elapsed()) as i64),
                ).to_owned(),
        );
        result
    }
    pub fn get_balance(&mut self, pubkey: &Pubkey) -> io::Result<i64> {
        trace!("get_balance sending request to {}", self.requests_addr);
        let req = Request::GetAccount { key: *pubkey };
        let data = serialize(&req).expect("serialize GetAccount in pub fn get_balance");
        self.requests_socket
            .send_to(&data, &self.requests_addr)
            .expect("buffer error in pub fn get_balance");
        let mut done = false;
        while !done {
            let resp = self.recv_response()?;
            trace!("recv_response {:?}", resp);
            if let Response::Account { key, .. } = &resp {
                done = key == pubkey;
            }
            self.process_response(&resp);
        }
        trace!("get_balance {:?}", self.balances.get(pubkey));
        self.balances
            .get(pubkey)
            .map(Bank::read_balance)
            .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "AccountNotFound"))
    }

    pub fn get_finality(&mut self) -> usize {
        trace!("get_finality");
        let req = Request::GetFinality;
        let data = serialize(&req).expect("serialize GetFinality in pub fn get_finality");
        let mut done = false;
        while !done {
            debug!("get_finality send_to {}", &self.requests_addr);
            self.requests_socket
                .send_to(&data, &self.requests_addr)
                .expect("buffer error in pub fn get_finality");

            match self.recv_response() {
                Ok(resp) => {
                    if let Response::Finality { .. } = resp {
                        done = true;
                    }
                    self.process_response(&resp);
                }
                Err(e) => {
                    debug!("thin_client get_finality error: {}", e);
                }
            }
        }
        self.finality.expect("some finality")
    }

    pub fn transaction_count(&mut self) -> u64 {
        debug!("transaction_count");
        let req = Request::GetTransactionCount;
        let data =
            serialize(&req).expect("serialize GetTransactionCount in pub fn transaction_count");
        let mut tries_left = 5;
        while tries_left > 0 {
            self.requests_socket
                .send_to(&data, &self.requests_addr)
                .expect("buffer error in pub fn transaction_count");

            if let Ok(resp) = self.recv_response() {
                debug!("transaction_count recv_response: {:?}", resp);
                if let Response::TransactionCount { .. } = resp {
                    tries_left = 0;
                }
                self.process_response(&resp);
            } else {
                tries_left -= 1;
            }
        }
        self.transaction_count
    }

    
    pub fn get_last_id(&mut self) -> Hash {
        trace!("get_last_id");
        let req = Request::GetLastId;
        let data = serialize(&req).expect("serialize GetLastId in pub fn get_last_id");
        let mut done = false;
        while !done {
            debug!("get_last_id send_to {}", &self.requests_addr);
            self.requests_socket
                .send_to(&data, &self.requests_addr)
                .expect("buffer error in pub fn get_last_id");

            match self.recv_response() {
                Ok(resp) => {
                    if let Response::LastId { .. } = resp {
                        done = true;
                    }
                    self.process_response(&resp);
                }
                Err(e) => {
                    debug!("thin_client get_last_id error: {}", e);
                }
            }
        }
        self.last_id.expect("some last_id")
    }

    pub fn submit_poll_balance_metrics(elapsed: &Duration) {
        metrics::submit(
            influxdb::Point::new("thinclient")
                .add_tag("op", influxdb::Value::String("get_balance".to_string()))
                .add_field(
                    "duration_ms",
                    influxdb::Value::Integer(timing::duration_in_milliseconds(elapsed) as i64),
                ).to_owned(),
        );
    }

    pub fn poll_balance_with_timeout(
        &mut self,
        pubkey: &Pubkey,
        polling_frequency: &Duration,
        timeout: &Duration,
    ) -> io::Result<i64> {
        let now = Instant::now();
        loop {
            match self.get_balance(&pubkey) {
                Ok(bal) => {
                    ThinClient::submit_poll_balance_metrics(&now.elapsed());
                    return Ok(bal);
                }
                Err(e) => {
                    sleep(*polling_frequency);
                    if now.elapsed() > *timeout {
                        ThinClient::submit_poll_balance_metrics(&now.elapsed());
                        return Err(e);
                    }
                }
            };
        }
    }

    pub fn poll_get_balance(&mut self, pubkey: &Pubkey) -> io::Result<i64> {
        self.poll_balance_with_timeout(pubkey, &Duration::from_millis(100), &Duration::from_secs(1))
    }

    pub fn poll_for_signature(&mut self, signature: &Signature) -> io::Result<()> {
        let now = Instant::now();
        while !self.check_signature(signature) {
            if now.elapsed().as_secs() > 1 {
                // TODO: Return a better error.
                return Err(io::Error::new(io::ErrorKind::Other, "signature not found"));
            }
            sleep(Duration::from_millis(100));
        }
        Ok(())
    }

    
    pub fn check_signature(&mut self, signature: &Signature) -> bool {
        trace!("check_signature");
        let req = Request::GetSignature {
            signature: *signature,
        };
        let data = serialize(&req).expect("serialize GetSignature in pub fn check_signature");
        let now = Instant::now();
        let mut done = false;
        while !done {
            self.requests_socket
                .send_to(&data, &self.requests_addr)
                .expect("buffer error in pub fn get_last_id");

            if let Ok(resp) = self.recv_response() {
                if let Response::SignatureStatus { .. } = resp {
                    done = true;
                }
                self.process_response(&resp);
            }
        }
        metrics::submit(
            influxdb::Point::new("Client")
                .add_tag("Operation", influxdb::Value::String("Signature Validation".to_string()))
                .add_field(
                    "duration_ms",
                    influxdb::Value::Integer(timing::duration_in_milliseconds(&now.elapsed()) as i64),
                ).to_owned(),
        );
        self.signature_status
    }
}

impl Drop for ThinClient {
    fn drop(&mut self) {
        metrics::flush();
    }
}

pub fn poll_gossip_for_leader(leader_ncp: SocketAddr, timeout: Option<u64>) -> Result<NodeInfo> {
    let exit = Arc::new(AtomicBool::new(false));
    let (node, gossip_socket) = Crdt::spy_node();
    let my_addr = gossip_socket.local_addr().unwrap();
    let crdt = Arc::new(RwLock::new(Crdt::new(node).expect("Crdt::new")));
    let window = Arc::new(RwLock::new(vec![]));
    let ncp = Ncp::new(&crdt.clone(), window, None, gossip_socket, exit.clone());

    let leader_entry_point = NodeInfo::new_entry_point(&leader_ncp);
    crdt.write().unwrap().insert(&leader_entry_point);

    sleep(Duration::from_millis(100));

    let deadline = match timeout {
        Some(timeout) => Duration::new(timeout, 0),
        None => Duration::new(std::u64::MAX, 0),
    };
    let now = Instant::now();
    // Block until leader's correct contact info is received
    let leader;

    loop {
        trace!("polling {:?} for leader from {:?}", leader_ncp, my_addr);

        if let Some(l) = crdt.read().unwrap().leader_data() {
            leader = Some(l.clone());
            break;
        }

        if log_enabled!(Level::Trace) {
            trace!("{}", crdt.read().unwrap().node_info_trace());
        }

        if now.elapsed() > deadline {
            return Err(Error::CrdtError(CrdtError::NoLeader));
        }

        sleep(Duration::from_millis(100));
    }

    ncp.close()?;

    if log_enabled!(Level::Trace) {
        trace!("{}", crdt.read().unwrap().node_info_trace());
    }

    Ok(leader.unwrap().clone())
}

