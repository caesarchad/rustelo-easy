//! The `ncp` module implements the network control plane.

use crate::crdt::Crdt;
use crate::service::Service;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::channel;
use std::sync::{Arc, RwLock};
use std::thread::{self, JoinHandle};
use crate::streamer;
use crate::window::SharedWindow;

pub struct Ncp {
    exit: Arc<AtomicBool>,
    thread_hdls: Vec<JoinHandle<()>>,
}

impl Ncp {
    pub fn new(
        crdt: &Arc<RwLock<Crdt>>,
        window: SharedWindow,
        ledger_path: Option<&str>,
        gossip_socket: UdpSocket,
        exit: Arc<AtomicBool>,
    ) -> Self {
        let (request_sender, request_receiver) = channel();
        let gossip_socket = Arc::new(gossip_socket);
        trace!(
            "Ncp: id: {:?}, listening on: {:?}",
            &crdt.read().unwrap().id.as_ref()[..4],
            gossip_socket.local_addr().unwrap()
        );
        let t_receiver =
            streamer::blob_receiver(gossip_socket.clone(), exit.clone(), request_sender);
        let (response_sender, response_receiver) = channel();
        let t_responder = streamer::responder("ncp", gossip_socket, response_receiver);
        let t_listen = Crdt::listen(
            crdt.clone(),
            window,
            ledger_path,
            request_receiver,
            response_sender.clone(),
            exit.clone(),
        );
        let t_gossip = Crdt::gossip(crdt.clone(), response_sender, exit.clone());
        let thread_hdls = vec![t_receiver, t_responder, t_listen, t_gossip];
        Ncp { exit, thread_hdls }
    }

    pub fn close(self) -> thread::Result<()> {
        self.exit.store(true, Ordering::Relaxed);
        self.join()
    }
}

impl Service for Ncp {
    type JoinReturnType = ();

    fn join(self) -> thread::Result<()> {
        for thread_hdl in self.thread_hdls {
            thread_hdl.join()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::crdt::{Crdt, Node};
    use crate::ncp::Ncp;
    use std::sync::atomic::AtomicBool;
    use std::sync::{Arc, RwLock};

    #[test]
    #[ignore]
    // test that stage will exit when flag is set
    fn test_exit() {
        let exit = Arc::new(AtomicBool::new(false));
        let tn = Node::new_localhost();
        let crdt = Crdt::new(tn.info.clone()).expect("Crdt::new");
        let c = Arc::new(RwLock::new(crdt));
        let w = Arc::new(RwLock::new(vec![]));
        let d = Ncp::new(&c, w, None, tn.sockets.gossip, exit.clone());
        d.close().expect("thread join");
    }
}
