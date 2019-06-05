//! The `window` module defines data structure for storing the tail of the ledger.
//!
use buffett_metrics::counter::Counter;
use crate::crdt::{Crdt, NodeInfo};
use crate::entry::Entry;
#[cfg(feature = "erasure")]
use crate::erasure;
use crate::ledger::{reconstruct_entries_from_blobs, Block};
use log::Level;
use crate::packet::SharedBlob;
use crate::result::Result;
use buffett_interface::pubkey::Pubkey;
use std::cmp;
use std::mem;
use std::net::SocketAddr;
use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, RwLock};
use buffett_metrics::sub_new_counter_info;

pub const WINDOW_SIZE: u64 = 2 * 1024;

#[derive(Default, Clone)]
pub struct WindowSlot {
    pub data: Option<SharedBlob>,
    pub coding: Option<SharedBlob>,
    pub leader_unknown: bool,
}

impl WindowSlot {
    fn blob_index(&self) -> Option<u64> {
        match self.data {
            Some(ref blob) => blob.read().unwrap().get_index().ok(),
            None => None,
        }
    }

    fn clear_data(&mut self) {
        self.data.take();
    }
}

type Window = Vec<WindowSlot>;
pub type SharedWindow = Arc<RwLock<Window>>;

#[derive(Debug)]
pub struct WindowIndex {
    pub data: u64,
    pub coding: u64,
}

pub trait WindowUtil {
    /// Finds available slots, clears them, and returns their indices.
    fn clear_slots(&mut self, consumed: u64, received: u64) -> Vec<u64>;

    fn repair(
        &mut self,
        crdt: &Arc<RwLock<Crdt>>,
        id: &Pubkey,
        times: usize,
        consumed: u64,
        received: u64,
        max_entry_height: u64,
    ) -> Vec<(SocketAddr, Vec<u8>)>;

    fn print(&self, id: &Pubkey, consumed: u64) -> String;

    #[cfg_attr(feature = "cargo-clippy", allow(too_many_arguments))]
    fn process_blob(
        &mut self,
        id: &Pubkey,
        crdt: &Arc<RwLock<Crdt>>,
        blob: SharedBlob,
        pix: u64,
        consume_queue: &mut Vec<Entry>,
        consumed: &mut u64,
        leader_unknown: bool,
        pending_retransmits: &mut bool,
        leader_rotation_interval: u64,
    );
}

impl WindowUtil for Window {
    fn clear_slots(&mut self, consumed: u64, received: u64) -> Vec<u64> {
        (consumed..received)
            .filter_map(|pix| {
                let i = (pix % WINDOW_SIZE) as usize;
                if let Some(blob_idx) = self[i].blob_index() {
                    if blob_idx == pix {
                        return None;
                    }
                }
                self[i].clear_data();
                Some(pix)
            }).collect()
    }

    fn repair(
        &mut self,
        crdt: &Arc<RwLock<Crdt>>,
        id: &Pubkey,
        times: usize,
        consumed: u64,
        received: u64,
        max_entry_height: u64,
    ) -> Vec<(SocketAddr, Vec<u8>)> {
        let rcrdt = crdt.read().unwrap();
        let leader_rotation_interval = rcrdt.get_leader_rotation_interval();
        // Calculate the next leader rotation height and check if we are the leader
        let next_leader_rotation =
            consumed + leader_rotation_interval - (consumed % leader_rotation_interval);
        let is_next_leader = rcrdt.get_scheduled_leader(next_leader_rotation) == Some(*id);
        let num_peers = rcrdt.table.len() as u64;

        let max_repair = if max_entry_height == 0 {
            calculate_max_repair(num_peers, consumed, received, times, is_next_leader)
        } else {
            max_entry_height + 1
        };

        let idxs = self.clear_slots(consumed, max_repair);
        let reqs: Vec<_> = idxs
            .into_iter()
            .filter_map(|pix| rcrdt.window_index_request(pix).ok())
            .collect();

        drop(rcrdt);

        sub_new_counter_info!("streamer-repair_window-repair", reqs.len());

        if log_enabled!(Level::Trace) {
            trace!(
                "{}: repair_window counter times: {} consumed: {} received: {} max_repair: {} missing: {}",
                id,
                times,
                consumed,
                received,
                max_repair,
                reqs.len()
            );
            for (to, _) in &reqs {
                trace!("{}: repair_window request to {}", id, to);
            }
        }
        reqs
    }

    fn print(&self, id: &Pubkey, consumed: u64) -> String {
        let pointer: Vec<_> = self
            .iter()
            .enumerate()
            .map(|(i, _v)| {
                if i == (consumed % WINDOW_SIZE) as usize {
                    "V"
                } else {
                    " "
                }
            }).collect();

        let buf: Vec<_> = self
            .iter()
            .map(|v| {
                if v.data.is_none() && v.coding.is_none() {
                    "O"
                } else if v.data.is_some() && v.coding.is_some() {
                    "D"
                } else if v.data.is_some() {
                    // coding.is_none()
                    "d"
                } else {
                    // data.is_none()
                    "c"
                }
            }).collect();
        format!(
            "\n{}: WINDOW ({}): {}\n{}: WINDOW ({}): {}",
            id,
            consumed,
            pointer.join(""),
            id,
            consumed,
            buf.join("")
        )
    }

    /// process a blob: Add blob to the window. If a continuous set of blobs
    ///      starting from consumed is thereby formed, add that continuous
    ///      range of blobs to a queue to be sent on to the next stage.
    ///
    /// * `self` - the window we're operating on
    /// * `id` - this node's id
    /// * `blob` -  the blob to be processed into the window and rebroadcast
    /// * `pix` -  the index of the blob, corresponds to
    ///            the entry height of this blob
    /// * `consume_queue` - output, blobs to be rebroadcast are placed here
    /// * `consumed` - input/output, the entry-height to which this
    ///                 node has populated and rebroadcast entries
    fn process_blob(
        &mut self,
        id: &Pubkey,
        crdt: &Arc<RwLock<Crdt>>,
        blob: SharedBlob,
        pix: u64,
        consume_queue: &mut Vec<Entry>,
        consumed: &mut u64,
        leader_unknown: bool,
        pending_retransmits: &mut bool,
        leader_rotation_interval: u64,
    ) {
        let w = (pix % WINDOW_SIZE) as usize;

        let is_coding = blob.read().unwrap().is_coding();

        // insert a newly received blob into a window slot, clearing out and recycling any previous
        //  blob unless the incoming blob is a duplicate (based on idx)
        // returns whether the incoming is a duplicate blob
        fn insert_blob_is_dup(
            id: &Pubkey,
            blob: SharedBlob,
            pix: u64,
            window_slot: &mut Option<SharedBlob>,
            c_or_d: &str,
        ) -> bool {
            if let Some(old) = mem::replace(window_slot, Some(blob)) {
                let is_dup = old.read().unwrap().get_index().unwrap() == pix;
                trace!(
                    "{}: occupied {} window slot {:}, is_dup: {}",
                    id,
                    c_or_d,
                    pix,
                    is_dup
                );
                is_dup
            } else {
                trace!("{}: empty {} window slot {:}", id, c_or_d, pix);
                false
            }
        }

        // insert the new blob into the window, overwrite and recycle old (or duplicate) entry
        let is_duplicate = if is_coding {
            insert_blob_is_dup(id, blob, pix, &mut self[w].coding, "coding")
        } else {
            insert_blob_is_dup(id, blob, pix, &mut self[w].data, "data")
        };

        if is_duplicate {
            return;
        }

        self[w].leader_unknown = leader_unknown;
        *pending_retransmits = true;

        #[cfg(feature = "erasure")]
        {
            if erasure::recover(id, self, *consumed, (*consumed % WINDOW_SIZE) as usize).is_err() {
                trace!("{}: erasure::recover failed", id);
            }
        }

        // push all contiguous blobs into consumed queue, increment consumed
        loop {
            if *consumed != 0 && *consumed % (leader_rotation_interval as u64) == 0 {
                let rcrdt = crdt.read().unwrap();
                let my_id = rcrdt.my_data().id;
                match rcrdt.get_scheduled_leader(*consumed) {
                    // If we are the next leader, exit
                    Some(id) if id == my_id => {
                        break;
                    }
                    _ => (),
                }
            }

            let k = (*consumed % WINDOW_SIZE) as usize;
            trace!("{}: k: {} consumed: {}", id, k, *consumed,);

            let k_data_blob;
            let k_data_slot = &mut self[k].data;
            if let Some(blob) = k_data_slot {
                if blob.read().unwrap().get_index().unwrap() < *consumed {
                    // window wrap-around, end of received
                    break;
                }
                k_data_blob = (*blob).clone();
            } else {
                // self[k].data is None, end of received
                break;
            }

            // Check that we can get the entries from this blob
            match reconstruct_entries_from_blobs(vec![k_data_blob]) {
                Ok(entries) => {
                    consume_queue.extend(entries);
                }
                Err(_) => {
                    // If the blob can't be deserialized, then remove it from the
                    // window and exit. *k_data_slot cannot be None at this point,
                    // so it's safe to unwrap.
                    k_data_slot.take();
                    break;
                }
            }

            *consumed += 1;
        }
    }
}

fn calculate_max_repair(
    num_peers: u64,
    consumed: u64,
    received: u64,
    times: usize,
    is_next_leader: bool,
) -> u64 {
    // Calculate the highest blob index that this node should have already received
    // via avalanche. The avalanche splits data stream into nodes and each node retransmits
    // the data to their peer nodes. So there's a possibility that a blob (with index lower
    // than current received index) is being retransmitted by a peer node.
    let max_repair = if times >= 8 || is_next_leader {
        // if repair backoff is getting high, or if we are the next leader,
        // don't wait for avalanche
        cmp::max(consumed, received)
    } else {
        cmp::max(consumed, received.saturating_sub(num_peers))
    };

    // This check prevents repairing a blob that will cause window to roll over. Even if
    // the highes_lost blob is actually missing, asking to repair it might cause our
    // current window to move past other missing blobs
    cmp::min(consumed + WINDOW_SIZE - 1, max_repair)
}

pub fn blob_idx_in_window(id: &Pubkey, pix: u64, consumed: u64, received: &mut u64) -> bool {
    // Prevent receive window from running over
    // Got a blob which has already been consumed, skip it
    // probably from a repair window request
    if pix < consumed {
        trace!(
            "{}: received: {} but older than consumed: {} skipping..",
            id,
            pix,
            consumed
        );
        false
    } else {
        // received always has to be updated even if we don't accept the packet into
        //  the window.  The worst case here is the server *starts* outside
        //  the window, none of the packets it receives fits in the window
        //  and repair requests (which are based on received) are never generated
        *received = cmp::max(pix, *received);

        if pix >= consumed + WINDOW_SIZE {
            trace!(
                "{}: received: {} will overrun window: {} skipping..",
                id,
                pix,
                consumed + WINDOW_SIZE
            );
            false
        } else {
            true
        }
    }
}

pub fn default_window() -> Window {
    (0..WINDOW_SIZE).map(|_| WindowSlot::default()).collect()
}

pub fn index_blobs(
    node_info: &NodeInfo,
    blobs: &[SharedBlob],
    receive_index: &mut u64,
) -> Result<()> {
    // enumerate all the blobs, those are the indices
    trace!("{}: INDEX_BLOBS {}", node_info.id, blobs.len());
    for (i, b) in blobs.iter().enumerate() {
        // only leader should be broadcasting
        let mut blob = b.write().unwrap();
        blob.set_id(node_info.id)
            .expect("set_id in pub fn broadcast");
        blob.set_index(*receive_index + i as u64)
            .expect("set_index in pub fn broadcast");
        blob.set_flags(0).unwrap();
    }

    Ok(())
}

/// Initialize a rebroadcast window with most recent Entry blobs
/// * `crdt` - gossip instance, used to set blob ids
/// * `blobs` - up to WINDOW_SIZE most recent blobs
/// * `entry_height` - current entry height
pub fn initialized_window(
    node_info: &NodeInfo,
    blobs: Vec<SharedBlob>,
    entry_height: u64,
) -> Window {
    let mut window = default_window();
    let id = node_info.id;

    trace!(
        "{} initialized window entry_height:{} blobs_len:{}",
        id,
        entry_height,
        blobs.len()
    );

    // Index the blobs
    let mut received = entry_height - blobs.len() as u64;
    index_blobs(&node_info, &blobs, &mut received).expect("index blobs for initial window");

    // populate the window, offset by implied index
    let diff = cmp::max(blobs.len() as isize - window.len() as isize, 0) as usize;
    for b in blobs.into_iter().skip(diff) {
        let ix = b.read().unwrap().get_index().expect("blob index");
        let pos = (ix % WINDOW_SIZE) as usize;
        trace!("{} caching {} at {}", id, ix, pos);
        assert!(window[pos].data.is_none());
        window[pos].data = Some(b);
    }

    window
}

pub fn new_window_from_entries(
    ledger_tail: &[Entry],
    entry_height: u64,
    node_info: &NodeInfo,
) -> Window {
    // convert to blobs
    let blobs = ledger_tail.to_blobs();
    initialized_window(&node_info, blobs, entry_height)
}

