use crate::tx_vault::Bank;
use crate::socket_streamer::BlobFetchStage;
use crate::crdt::Crdt;
use replicate_stage::ReplicateStage;
use retransmit_stage::{RetransmitStage, RetransmitStageReturnType};
use crate::service::Service;
use buffett_crypto::signature::Keypair;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::thread;
use crate::window::SharedWindow;

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum TvuReturnType {
    LeaderRotation(u64),
}

pub struct Tvu {
    replicate_stage: ReplicateStage,
    fetch_stage: BlobFetchStage,
    retransmit_stage: RetransmitStage,
    exit: Arc<AtomicBool>,
}

impl Tvu {
    
    #[cfg_attr(feature = "cargo-clippy", allow(too_many_arguments))]
    pub fn new(
        keypair: Arc<Keypair>,
        bank: &Arc<Bank>,
        entry_height: u64,
        crdt: Arc<RwLock<Crdt>>,
        window: SharedWindow,
        replicate_sockets: Vec<UdpSocket>,
        repair_socket: UdpSocket,
        retransmit_socket: UdpSocket,
        ledger_path: Option<&str>,
    ) -> Self {
        let exit = Arc::new(AtomicBool::new(false));

        let repair_socket = Arc::new(repair_socket);
        let mut blob_sockets: Vec<Arc<UdpSocket>> =
            replicate_sockets.into_iter().map(Arc::new).collect();
        blob_sockets.push(repair_socket.clone());
        let (fetch_stage, blob_fetch_receiver) =
            BlobFetchStage::new_multi_socket(blob_sockets, exit.clone());
        
        let (retransmit_stage, blob_window_receiver) = RetransmitStage::new(
            &crdt,
            window,
            entry_height,
            Arc::new(retransmit_socket),
            repair_socket,
            blob_fetch_receiver,
        );

        let replicate_stage = ReplicateStage::new(
            keypair,
            bank.clone(),
            crdt,
            blob_window_receiver,
            ledger_path,
            exit.clone(),
        );

        Tvu {
            replicate_stage,
            fetch_stage,
            retransmit_stage,
            exit,
        }
    }

    pub fn exit(&self) -> () {
        self.exit.store(true, Ordering::Relaxed);
    }

    pub fn close(self) -> thread::Result<Option<TvuReturnType>> {
        self.fetch_stage.close();
        self.join()
    }
}

impl Service for Tvu {
    type JoinReturnType = Option<TvuReturnType>;

    fn join(self) -> thread::Result<Option<TvuReturnType>> {
        self.replicate_stage.join()?;
        self.fetch_stage.join()?;
        match self.retransmit_stage.join()? {
            Some(RetransmitStageReturnType::LeaderRotation(entry_height)) => {
                Ok(Some(TvuReturnType::LeaderRotation(entry_height)))
            }
            _ => Ok(None),
        }
    }
}

