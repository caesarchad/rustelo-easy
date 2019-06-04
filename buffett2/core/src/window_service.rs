use crate::counter::Counter;
use crate::crdt::{Crdt, NodeInfo};
use crate::entry::EntrySender;
use log::Level;
use crate::packet::SharedBlob;
use rand::{thread_rng, Rng};
use crate::result::{Error, Result};
use buffett_interface::pubkey::Pubkey;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::RecvTimeoutError;
use std::sync::{Arc, RwLock};
use std::thread::{Builder, JoinHandle};
use std::time::{Duration, Instant};
use crate::streamer::{BlobReceiver, BlobSender};
use buffett_timing::timing::duration_in_milliseconds;
use crate::window::{blob_idx_in_window, SharedWindow, WindowUtil};

pub const MAX_REPAIR_BACKOFF: usize = 128;

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum WindowServiceReturnType {
    LeaderRotation(u64),
}

fn repair_backoff(last: &mut u64, times: &mut usize, consumed: u64) -> bool {
    
    if *last != consumed {
        
        *times = 1;
    }
    *last = consumed;
    *times += 1;

    
    if *times > MAX_REPAIR_BACKOFF {
        
        *times = MAX_REPAIR_BACKOFF / 2;
    }

    
    thread_rng().gen_range(0, *times as u64) == 0
}

fn add_block_to_retransmit_queue(
    b: &SharedBlob,
    leader_id: Pubkey,
    retransmit_queue: &mut Vec<SharedBlob>,
) {
    let p = b.read().unwrap();
    
    trace!(
        "idx: {} addr: {:?} id: {:?} leader: {:?}",
        p.get_index()
            .expect("get_index in fn add_block_to_retransmit_queue"),
        p.get_id()
            .expect("get_id in trace! fn add_block_to_retransmit_queue"),
        p.meta.addr(),
        leader_id
    );
    if p.get_id()
        .expect("get_id in fn add_block_to_retransmit_queue")
        == leader_id
    {
        
        let nv = SharedBlob::default();
        {
            let mut mnv = nv.write().unwrap();
            let sz = p.meta.size;
            mnv.meta.size = sz;
            mnv.data[..sz].copy_from_slice(&p.data[..sz]);
        }
        retransmit_queue.push(nv);
    }
}

fn retransmit_all_leader_blocks(
    window: &SharedWindow,
    maybe_leader: Option<NodeInfo>,
    dq: &[SharedBlob],
    id: &Pubkey,
    consumed: u64,
    received: u64,
    retransmit: &BlobSender,
    pending_retransmits: &mut bool,
) -> Result<()> {
    let mut retransmit_queue: Vec<SharedBlob> = Vec::new();
    if let Some(leader) = maybe_leader {
        let leader_id = leader.id;
        for b in dq {
            add_block_to_retransmit_queue(b, leader_id, &mut retransmit_queue);
        }

        if *pending_retransmits {
            for w in window
                .write()
                .expect("Window write failed in retransmit_all_leader_blocks")
                .iter_mut()
            {
                *pending_retransmits = false;
                if w.leader_unknown {
                    if let Some(ref b) = w.data {
                        add_block_to_retransmit_queue(b, leader_id, &mut retransmit_queue);
                        w.leader_unknown = false;
                    }
                }
            }
        }
    } else {
        warn!("{}: no leader to retransmit from", id);
    }
    if !retransmit_queue.is_empty() {
        trace!(
            "{}: RECV_WINDOW {} {}: retransmit {}",
            id,
            consumed,
            received,
            retransmit_queue.len(),
        );
        inc_new_counter_info!("streamer-recv_window-retransmit", retransmit_queue.len());
        retransmit.send(retransmit_queue)?;
    }
    Ok(())
}

#[cfg_attr(feature = "cargo-clippy", allow(too_many_arguments))]
fn recv_window(
    window: &SharedWindow,
    id: &Pubkey,
    crdt: &Arc<RwLock<Crdt>>,
    consumed: &mut u64,
    received: &mut u64,
    max_ix: u64,
    r: &BlobReceiver,
    s: &EntrySender,
    retransmit: &BlobSender,
    pending_retransmits: &mut bool,
    leader_rotation_interval: u64,
    done: &Arc<AtomicBool>,
) -> Result<()> {
    let timer = Duration::from_millis(200);
    let mut dq = r.recv_timeout(timer)?;
    let maybe_leader: Option<NodeInfo> = crdt
        .read()
        .expect("'crdt' read lock in fn recv_window")
        .leader_data()
        .cloned();
    let leader_unknown = maybe_leader.is_none();
    while let Ok(mut nq) = r.try_recv() {
        dq.append(&mut nq)
    }
    let now = Instant::now();
    inc_new_counter_info!("streamer-recv_window-recv", dq.len(), 100);
    trace!(
        "{}: RECV_WINDOW {} {}: got packets {}",
        id,
        *consumed,
        *received,
        dq.len(),
    );

    retransmit_all_leader_blocks(
        window,
        maybe_leader,
        &dq,
        id,
        *consumed,
        *received,
        retransmit,
        pending_retransmits,
    )?;

    let mut pixs = Vec::new();
    
    let mut consume_queue = Vec::new();
    for b in dq {
        let (pix, meta_size) = {
            let p = b.read().unwrap();
            (p.get_index()?, p.meta.size)
        };
        pixs.push(pix);

        if !blob_idx_in_window(&id, pix, *consumed, received) {
            continue;
        }

        
        if max_ix != 0 && pix > max_ix {
            continue;
        }

        trace!("{} window pix: {} size: {}", id, pix, meta_size);

        window.write().unwrap().process_blob(
            id,
            crdt,
            b,
            pix,
            &mut consume_queue,
            consumed,
            leader_unknown,
            pending_retransmits,
            leader_rotation_interval,
        );

        
        if max_ix != 0 && *consumed == (max_ix + 1) {
            done.store(true, Ordering::Relaxed);
        }
    }
    if log_enabled!(Level::Trace) {
        trace!("{}", window.read().unwrap().print(id, *consumed));
        trace!(
            "{}: consumed: {} received: {} sending consume.len: {} pixs: {:?} took {} ms",
            id,
            *consumed,
            *received,
            consume_queue.len(),
            pixs,
            duration_in_milliseconds(&now.elapsed())
        );
    }
    if !consume_queue.is_empty() {
        inc_new_counter_info!("streamer-recv_window-consume", consume_queue.len());
        s.send(consume_queue)?;
    }
    Ok(())
}

pub fn window_service(
    crdt: Arc<RwLock<Crdt>>,
    window: SharedWindow,
    entry_height: u64,
    max_entry_height: u64,
    r: BlobReceiver,
    s: EntrySender,
    retransmit: BlobSender,
    repair_socket: Arc<UdpSocket>,
    done: Arc<AtomicBool>,
) -> JoinHandle<Option<WindowServiceReturnType>> {
    Builder::new()
        .name("bitconch-window".to_string())
        .spawn(move || {
            let mut consumed = entry_height;
            let mut received = entry_height;
            let mut last = entry_height;
            let mut times = 0;
            let id;
            let leader_rotation_interval;
            {
                let rcrdt = crdt.read().unwrap();
                id = rcrdt.id;
                leader_rotation_interval = rcrdt.get_leader_rotation_interval();
            }
            let mut pending_retransmits = false;
            trace!("{}: RECV_WINDOW started", id);
            loop {
                if consumed != 0 && consumed % (leader_rotation_interval as u64) == 0 {
                    match crdt.read().unwrap().get_scheduled_leader(consumed) {
                        
                        Some(next_leader_id) if id == next_leader_id => {
                            return Some(WindowServiceReturnType::LeaderRotation(consumed));
                        }
                        
                        _ => (),
                    }
                }

                if let Err(e) = recv_window(
                    &window,
                    &id,
                    &crdt,
                    &mut consumed,
                    &mut received,
                    max_entry_height,
                    &r,
                    &s,
                    &retransmit,
                    &mut pending_retransmits,
                    leader_rotation_interval,
                    &done,
                ) {
                    match e {
                        Error::RecvTimeoutError(RecvTimeoutError::Disconnected) => break,
                        Error::RecvTimeoutError(RecvTimeoutError::Timeout) => (),
                        _ => {
                            inc_new_counter_info!("streamer-window-error", 1, 1);
                            error!("window error: {:?}", e);
                        }
                    }
                }

                if received <= consumed {
                    trace!(
                        "{} we have everything received:{} consumed:{}",
                        id,
                        received,
                        consumed
                    );
                    continue;
                }

                
                if !repair_backoff(&mut last, &mut times, consumed) {
                    trace!("{} !repair_backoff() times = {}", id, times);
                    continue;
                }
                trace!("{} let's repair! times = {}", id, times);

                let mut window = window.write().unwrap();
                let reqs = window.repair(&crdt, &id, times, consumed, received, max_entry_height);
                for (to, req) in reqs {
                    repair_socket.send_to(&req, to).unwrap_or_else(|e| {
                        info!("{} repair req send_to({}) error {:?}", id, to, e);
                        0
                    });
                }
            }
            None
        }).unwrap()
}

