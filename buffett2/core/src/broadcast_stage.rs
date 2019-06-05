use buffett_metrics::counter::Counter;
use crate::crdt::{Crdt, CrdtError, NodeInfo};
use crate::entry::Entry;
#[cfg(feature = "erasure")]
use crate::erasure;
use crate::ledger::Block;
use log::Level;
use crate::packet::SharedBlobs;
use rayon::prelude::*;
use crate::result::{Error, Result};
use crate::service::Service;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, RwLock};
use std::thread::{self, Builder, JoinHandle};
use std::time::{Duration, Instant};
use buffett_timing::timing::duration_in_milliseconds;
use crate::window::{self, SharedWindow, WindowIndex, WindowUtil, WINDOW_SIZE};
use buffett_metrics::sub_new_counter_info;

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum BroadcastStageReturnType {
    LeaderRotation,
    ChannelDisconnected,
}

fn broadcast(
    crdt: &Arc<RwLock<Crdt>>,
    leader_rotation_interval: u64,
    node_info: &NodeInfo,
    broadcast_table: &[NodeInfo],
    window: &SharedWindow,
    receiver: &Receiver<Vec<Entry>>,
    sock: &UdpSocket,
    transmit_index: &mut WindowIndex,
    receive_index: &mut u64,
) -> Result<()> {
    let id = node_info.id;
    let timer = Duration::new(1, 0);
    let entries = receiver.recv_timeout(timer)?;
    let now = Instant::now();
    let mut num_entries = entries.len();
    let mut ventries = Vec::new();
    ventries.push(entries);
    while let Ok(entries) = receiver.try_recv() {
        num_entries += entries.len();
        ventries.push(entries);
    }
    sub_new_counter_info!("broadcast_stage-entries_received", num_entries);

    let to_blobs_start = Instant::now();
    let dq: SharedBlobs = ventries
        .into_par_iter()
        .flat_map(|p| p.to_blobs())
        .collect();

    let to_blobs_elapsed = duration_in_milliseconds(&to_blobs_start.elapsed());

    
    let blobs_vec: SharedBlobs = dq.into_iter().collect();

    let blobs_chunking = Instant::now();
    let blobs_chunked = blobs_vec.chunks(WINDOW_SIZE as usize).map(|x| x.to_vec());
    let chunking_elapsed = duration_in_milliseconds(&blobs_chunking.elapsed());

    trace!("{}", window.read().unwrap().print(&id, *receive_index));

    let broadcast_start = Instant::now();
    for mut blobs in blobs_chunked {
        let blobs_len = blobs.len();
        trace!("{}: broadcast blobs.len: {}", id, blobs_len);

        
        window::index_blobs(node_info, &blobs, receive_index)
            .expect("index blobs for initial window");

        
        sub_new_counter_info!("streamer-broadcast-sent", blobs.len());
        {
            let mut win = window.write().unwrap();
            assert!(blobs.len() <= win.len());
            for b in &blobs {
                let ix = b.read().unwrap().get_index().expect("blob index");
                let pos = (ix % WINDOW_SIZE) as usize;
                if let Some(x) = win[pos].data.take() {
                    trace!(
                        "{} popped {} at {}",
                        id,
                        x.read().unwrap().get_index().unwrap(),
                        pos
                    );
                }
                if let Some(x) = win[pos].coding.take() {
                    trace!(
                        "{} popped {} at {}",
                        id,
                        x.read().unwrap().get_index().unwrap(),
                        pos
                    );
                }

                trace!("{} null {}", id, pos);
            }
            for b in &blobs {
                let ix = b.read().unwrap().get_index().expect("blob index");
                let pos = (ix % WINDOW_SIZE) as usize;
                trace!("{} caching {} at {}", id, ix, pos);
                assert!(win[pos].data.is_none());
                win[pos].data = Some(b.clone());
            }
        }

         
        #[cfg(feature = "erasure")]
        {
            erasure::generate_coding(
                &id,
                &mut window.write().unwrap(),
                *receive_index,
                blobs_len,
                &mut transmit_index.coding,
            )?;
        }

        *receive_index += blobs_len as u64;

        
        Crdt::broadcast(
            crdt,
            leader_rotation_interval,
            &node_info,
            &broadcast_table,
            &window,
            &sock,
            transmit_index,
            *receive_index,
        )?;
    }
    let broadcast_elapsed = duration_in_milliseconds(&broadcast_start.elapsed());

    sub_new_counter_info!(
        "broadcast_stage-time_ms",
        duration_in_milliseconds(&now.elapsed()) as usize
    );
    info!(
        "Number of entries: {}, Duration of bundling a blob{}, Duration to create a block section {}  in  {}",
        num_entries, to_blobs_elapsed, chunking_elapsed, broadcast_elapsed
    );

    Ok(())
}


struct Finalizer {
    exit_sender: Arc<AtomicBool>,
}

impl Finalizer {
    fn new(exit_sender: Arc<AtomicBool>) -> Self {
        Finalizer { exit_sender }
    }
}

impl Drop for Finalizer {
    fn drop(&mut self) {
        self.exit_sender.clone().store(true, Ordering::Relaxed);
    }
}

pub struct BroadcastStage {
    thread_hdl: JoinHandle<BroadcastStageReturnType>,
}

impl BroadcastStage {
    fn run(
        sock: &UdpSocket,
        crdt: &Arc<RwLock<Crdt>>,
        window: &SharedWindow,
        entry_height: u64,
        receiver: &Receiver<Vec<Entry>>,
    ) -> BroadcastStageReturnType {
        let mut transmit_index = WindowIndex {
            data: entry_height,
            coding: entry_height,
        };
        let mut receive_index = entry_height;
        let me;
        let leader_rotation_interval;
        {
            let rcrdt = crdt.read().unwrap();
            me = rcrdt.my_data().clone();
            leader_rotation_interval = rcrdt.get_leader_rotation_interval();
        }

        loop {
            if transmit_index.data % (leader_rotation_interval as u64) == 0 {
                let rcrdt = crdt.read().unwrap();
                let my_id = rcrdt.my_data().id;
                match rcrdt.get_scheduled_leader(transmit_index.data) {
                    Some(id) if id == my_id => (),
                    
                    _ => {
                        return BroadcastStageReturnType::LeaderRotation;
                    }
                }
            }

            let broadcast_table = crdt.read().unwrap().compute_broadcast_table();
            if let Err(e) = broadcast(
                crdt,
                leader_rotation_interval,
                &me,
                &broadcast_table,
                &window,
                &receiver,
                &sock,
                &mut transmit_index,
                &mut receive_index,
            ) {
                match e {
                    Error::RecvTimeoutError(RecvTimeoutError::Disconnected) => {
                        return BroadcastStageReturnType::ChannelDisconnected
                    }
                    Error::RecvTimeoutError(RecvTimeoutError::Timeout) => (),
                    Error::CrdtError(CrdtError::NoPeers) => (), // TODO: Why are the unit-tests throwing hundreds of these?
                    _ => {
                        sub_new_counter_info!("streamer-broadcaster-error", 1, 1);
                        error!("broadcaster error: {:?}", e);
                    }
                }
            }
        }
    }

    
    pub fn new(
        sock: UdpSocket,
        crdt: Arc<RwLock<Crdt>>,
        window: SharedWindow,
        entry_height: u64,
        receiver: Receiver<Vec<Entry>>,
        exit_sender: Arc<AtomicBool>,
    ) -> Self {
        let thread_hdl = Builder::new()
            .name("bitconch-broadcaster".to_string())
            .spawn(move || {
                let _exit = Finalizer::new(exit_sender);
                Self::run(&sock, &crdt, &window, entry_height, &receiver)
            }).unwrap();

        BroadcastStage { thread_hdl }
    }
}

impl Service for BroadcastStage {
    type JoinReturnType = BroadcastStageReturnType;

    fn join(self) -> thread::Result<BroadcastStageReturnType> {
        self.thread_hdl.join()
    }
}

