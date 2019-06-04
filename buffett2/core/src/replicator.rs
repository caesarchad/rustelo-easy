use crate::socket_streamer::BlobFetchStage;
use crate::crdt::{Crdt, Node, NodeInfo};
use crate::ncp::Ncp;
use crate::service::Service;
use std::net::SocketAddr;
use std::net::UdpSocket;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::channel;
use std::sync::{Arc, RwLock};
use std::thread::JoinHandle;
use std::time::Duration;
use store_ledger_stage::StoreLedgerStage;
use crate::streamer::BlobReceiver;
use crate::window;
use window_service::{window_service, WindowServiceReturnType};

pub struct Replicator {
    ncp: Ncp,
    fetch_stage: BlobFetchStage,
    store_ledger_stage: StoreLedgerStage,
    t_window: JoinHandle<Option<WindowServiceReturnType>>,
    pub retransmit_receiver: BlobReceiver,
}

impl Replicator {
    pub fn new(
        entry_height: u64,
        max_entry_height: u64,
        exit: &Arc<AtomicBool>,
        ledger_path: Option<&str>,
        node: Node,
        network_addr: Option<SocketAddr>,
        done: Arc<AtomicBool>,
    ) -> Replicator {
        let window = window::new_window_from_entries(&[], entry_height, &node.info);
        let shared_window = Arc::new(RwLock::new(window));

        let crdt = Arc::new(RwLock::new(Crdt::new(node.info).expect("Crdt::new")));

        let leader_info = network_addr.map(|i| NodeInfo::new_entry_point(&i));

        if let Some(leader_info) = leader_info.as_ref() {
            crdt.write().unwrap().insert(leader_info);
        } else {
            panic!("No Leader Node Information!");
        }

        let repair_socket = Arc::new(node.sockets.repair);
        let mut blob_sockets: Vec<Arc<UdpSocket>> =
            node.sockets.replicate.into_iter().map(Arc::new).collect();
        blob_sockets.push(repair_socket.clone());
        let (fetch_stage, blob_fetch_receiver) =
            BlobFetchStage::new_multi_socket(blob_sockets, exit.clone());

        let (entry_window_sender, entry_window_receiver) = channel();
        // todo: pull blobs off the retransmit_receiver and recycle them?
        let (retransmit_sender, retransmit_receiver) = channel();
        let t_window = window_service(
            crdt.clone(),
            shared_window.clone(),
            entry_height,
            max_entry_height,
            blob_fetch_receiver,
            entry_window_sender,
            retransmit_sender,
            repair_socket,
            done,
        );

        let store_ledger_stage = StoreLedgerStage::new(entry_window_receiver, ledger_path);

        let ncp = Ncp::new(
            &crdt,
            shared_window.clone(),
            ledger_path,
            node.sockets.gossip,
            exit.clone(),
        );

        Replicator {
            ncp,
            fetch_stage,
            store_ledger_stage,
            t_window,
            retransmit_receiver,
        }
    }

    pub fn join(self) {
        self.ncp.join().unwrap();
        self.fetch_stage.join().unwrap();
        self.t_window.join().unwrap();
        self.store_ledger_stage.join().unwrap();

        // Drain the queue here to prevent self.retransmit_receiver from being dropped
        // before the window_service thread is joined
        let mut retransmit_queue_count = 0;
        while let Ok(_blob) = self.retransmit_receiver.recv_timeout(Duration::new(1, 0)) {
            retransmit_queue_count += 1;
        }
        debug!("retransmit channel count: {}", retransmit_queue_count);
    }
}

