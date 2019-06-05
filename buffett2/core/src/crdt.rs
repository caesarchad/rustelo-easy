use bincode::{deserialize, serialize};
use buffett_budget::budget_instruction::Vote;
use choose_gossip_peer_strategy::{ChooseGossipPeerStrategy, ChooseWeightedPeerStrategy};
use buffett_metrics::counter::Counter;
use buffett_crypto::hash::Hash;
use crate::ledger::LedgerWindow;
use log::Level;
use netutil::{bind_in_range, bind_to, multi_bind_in_range};
use crate::packet::{to_blob, Blob, SharedBlob, BLOB_SIZE};
use rand::{thread_rng, Rng};
use rayon::prelude::*;
use crate::result::{Error, Result};
use buffett_crypto::signature::{Keypair, KeypairUtil};
use buffett_interface::pubkey::Pubkey;
use std;
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};
use std::thread::{sleep, Builder, JoinHandle};
use std::time::{Duration, Instant};
use crate::streamer::{BlobReceiver, BlobSender};
use buffett_timing::timing::{duration_in_milliseconds, timestamp};
use crate::window::{SharedWindow, WindowIndex};
use buffett_metrics::sub_new_counter_info;

pub const FULLNODE_PORT_RANGE: (u16, u16) = (8000, 10_000);


const GOSSIP_SLEEP_MILLIS: u64 = 100;
const GOSSIP_PURGE_MILLIS: u64 = 15000;


const MIN_TABLE_SIZE: usize = 2;

#[macro_export]
macro_rules! socketaddr {
    ($ip:expr, $port:expr) => {
        SocketAddr::from((Ipv4Addr::from($ip), $port))
    };
    ($str:expr) => {{
        let a: SocketAddr = $str.parse().unwrap();
        a
    }};
}
#[macro_export]
macro_rules! socketaddr_any {
    () => {
        socketaddr!(0, 0)
    };
}

#[derive(Debug, PartialEq, Eq)]
pub enum CrdtError {
    NoPeers,
    NoLeader,
    BadContactInfo,
    BadNodeInfo,
    BadGossipAddress,
}


#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ContactInfo {
    
    pub ncp: SocketAddr,
    
    pub tvu: SocketAddr,
    
    pub rpu: SocketAddr,
    
    pub tpu: SocketAddr,
    
    pub storage_addr: SocketAddr,
    
    pub version: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct LedgerState {
    
    pub last_id: Hash,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct NodeInfo {
    pub id: Pubkey,
    
    pub version: u64,
    
    pub contact_info: ContactInfo,
    
    pub leader_id: Pubkey,
    
    pub ledger_state: LedgerState,
}

impl NodeInfo {
    pub fn new(
        id: Pubkey,
        ncp: SocketAddr,
        tvu: SocketAddr,
        rpu: SocketAddr,
        tpu: SocketAddr,
        storage_addr: SocketAddr,
    ) -> Self {
        NodeInfo {
            id,
            version: 0,
            contact_info: ContactInfo {
                ncp,
                tvu,
                rpu,
                tpu,
                storage_addr,
                version: 0,
            },
            leader_id: Pubkey::default(),
            ledger_state: LedgerState {
                last_id: Hash::default(),
            },
        }
    }

    pub fn new_localhost(id: Pubkey) -> Self {
        Self::new(
            id,
            socketaddr!("127.0.0.1:1234"),
            socketaddr!("127.0.0.1:1235"),
            socketaddr!("127.0.0.1:1236"),
            socketaddr!("127.0.0.1:1237"),
            socketaddr!("127.0.0.1:1238"),
        )
    }

    
    
    fn next_port(addr: &SocketAddr, nxt: u16) -> SocketAddr {
        let mut nxt_addr = *addr;
        nxt_addr.set_port(addr.port() + nxt);
        nxt_addr
    }
    pub fn new_with_pubkey_socketaddr(pubkey: Pubkey, bind_addr: &SocketAddr) -> Self {
        let transactions_addr = *bind_addr;
        let gossip_addr = Self::next_port(&bind_addr, 1);
        let replicate_addr = Self::next_port(&bind_addr, 2);
        let requests_addr = Self::next_port(&bind_addr, 3);
        NodeInfo::new(
            pubkey,
            gossip_addr,
            replicate_addr,
            requests_addr,
            transactions_addr,
            "0.0.0.0:0".parse().unwrap(),
        )
    }
    pub fn new_with_socketaddr(bind_addr: &SocketAddr) -> Self {
        let keypair = Keypair::new();
        Self::new_with_pubkey_socketaddr(keypair.pubkey(), bind_addr)
    }
    //
    pub fn new_entry_point(gossip_addr: &SocketAddr) -> Self {
        let daddr: SocketAddr = socketaddr!("0.0.0.0:0");
        NodeInfo::new(Pubkey::default(), *gossip_addr, daddr, daddr, daddr, daddr)
    }
}


pub struct Crdt {
    
    pub table: HashMap<Pubkey, NodeInfo>,
    local: HashMap<Pubkey, u64>,
    pub remote: HashMap<Pubkey, u64>,
    pub alive: HashMap<Pubkey, u64>,
    pub update_index: u64,
    pub id: Pubkey,
    external_liveness: HashMap<Pubkey, HashMap<Pubkey, u64>>,
    pub scheduled_leaders: HashMap<u64, Pubkey>,
    pub leader_rotation_interval: u64,
}


#[derive(Serialize, Deserialize, Debug)]
enum Protocol {
    RequestUpdates(u64, NodeInfo),
    ReceiveUpdates(Pubkey, u64, Vec<NodeInfo>, Vec<(Pubkey, u64)>),
    RequestWindowIndex(NodeInfo, u64),
}

impl Crdt {
    pub fn new(node_info: NodeInfo) -> Result<Crdt> {
        if node_info.version != 0 {
            return Err(Error::CrdtError(CrdtError::BadNodeInfo));
        }
        let mut me = Crdt {
            table: HashMap::new(),
            local: HashMap::new(),
            remote: HashMap::new(),
            alive: HashMap::new(),
            external_liveness: HashMap::new(),
            id: node_info.id,
            update_index: 1,
            scheduled_leaders: HashMap::new(),
            leader_rotation_interval: 100,
        };
        me.local.insert(node_info.id, me.update_index);
        me.table.insert(node_info.id, node_info);
        Ok(me)
    }
    pub fn my_data(&self) -> &NodeInfo {
        &self.table[&self.id]
    }
    pub fn leader_data(&self) -> Option<&NodeInfo> {
        let leader_id = self.table[&self.id].leader_id;

        // leader_id can be 0s from network entry point
        if leader_id == Pubkey::default() {
            return None;
        }

        self.table.get(&leader_id)
    }

    pub fn node_info_trace(&self) -> String {
        let leader_id = self.table[&self.id].leader_id;

        let nodes: Vec<_> = self
            .table
            .values()
            .filter(|n| Self::is_valid_address(&n.contact_info.rpu))
            .cloned()
            .map(|node| {
                format!(
                    " ncp: {:20} | {}{}\n \
                     rpu: {:20} |\n \
                     tpu: {:20} |\n",
                    node.contact_info.ncp.to_string(),
                    node.id,
                    if node.id == leader_id {
                        " <==== leader"
                    } else {
                        ""
                    },
                    node.contact_info.rpu.to_string(),
                    node.contact_info.tpu.to_string()
                )
            }).collect();

        format!(
            " NodeInfo.contact_info     | Node identifier\n\
             ---------------------------+------------------\n\
             {}\n \
             Nodes: {}",
            nodes.join(""),
            nodes.len()
        )
    }

    pub fn set_leader(&mut self, key: Pubkey) -> () {
        let mut me = self.my_data().clone();
        warn!("{}: LEADER_UPDATE TO {} from {}", me.id, key, me.leader_id);
        me.leader_id = key;
        me.version += 1;
        self.insert(&me);
    }

    
    pub fn get_scheduled_leader(&self, entry_height: u64) -> Option<Pubkey> {
        match self.scheduled_leaders.get(&entry_height) {
            Some(x) => Some(*x),
            None => Some(self.my_data().leader_id),
        }
    }

    pub fn set_leader_rotation_interval(&mut self, leader_rotation_interval: u64) {
        self.leader_rotation_interval = leader_rotation_interval;
    }

    pub fn get_leader_rotation_interval(&self) -> u64 {
        self.leader_rotation_interval
    }

    
    pub fn set_scheduled_leader(&mut self, entry_height: u64, new_leader_id: Pubkey) -> () {
        self.scheduled_leaders.insert(entry_height, new_leader_id);
    }

    pub fn get_valid_peers(&self) -> Vec<NodeInfo> {
        let me = self.my_data().id;
        self.table
            .values()
            .into_iter()
            .filter(|x| x.id != me)
            .filter(|x| Crdt::is_valid_address(&x.contact_info.rpu))
            .cloned()
            .collect()
    }

    pub fn get_external_liveness_entry(&self, key: &Pubkey) -> Option<&HashMap<Pubkey, u64>> {
        self.external_liveness.get(key)
    }

    pub fn insert_vote(&mut self, pubkey: &Pubkey, v: &Vote, last_id: Hash) {
        if self.table.get(pubkey).is_none() {
            warn!("{}: VOTE for unknown id: {}", self.id, pubkey);
            return;
        }
        if v.contact_info_version > self.table[pubkey].contact_info.version {
            warn!(
                "{}: VOTE for new address version from: {} ours: {} vote: {:?}",
                self.id, pubkey, self.table[pubkey].contact_info.version, v,
            );
            return;
        }
        if *pubkey == self.my_data().leader_id {
            info!("{}: LEADER_VOTED! {}", self.id, pubkey);
            sub_new_counter_info!("crdt-insert_vote-leader_voted", 1);
        }

        if v.version <= self.table[pubkey].version {
            debug!("{}: VOTE for old version: {}", self.id, pubkey);
            self.update_liveness(*pubkey);
            return;
        } else {
            let mut data = self.table[pubkey].clone();
            data.version = v.version;
            data.ledger_state.last_id = last_id;

            debug!("{}: INSERTING VOTE! for {}", self.id, data.id);
            self.update_liveness(data.id);
            self.insert(&data);
        }
    }
    pub fn insert_votes(&mut self, votes: &[(Pubkey, Vote, Hash)]) {
        sub_new_counter_info!("crdt-vote-count", votes.len());
        if !votes.is_empty() {
            info!("{}: INSERTING VOTES {}", self.id, votes.len());
        }
        for v in votes {
            self.insert_vote(&v.0, &v.1, v.2);
        }
    }

    pub fn insert(&mut self, v: &NodeInfo) -> usize {
        
        if self.table.get(&v.id).is_none() || (v.version > self.table[&v.id].version) {
            
            trace!("{}: insert v.id: {} version: {}", self.id, v.id, v.version);
            if self.table.get(&v.id).is_none() {
                sub_new_counter_info!("crdt-insert-new_entry", 1, 1);
            }

            self.update_index += 1;
            let _ = self.table.insert(v.id, v.clone());
            let _ = self.local.insert(v.id, self.update_index);
            self.update_liveness(v.id);
            1
        } else {
            trace!(
                "{}: INSERT FAILED data: {} new.version: {} me.version: {}",
                self.id,
                v.id,
                v.version,
                self.table[&v.id].version
            );
            0
        }
    }

    fn update_liveness(&mut self, id: Pubkey) {
        
        let now = timestamp();
        trace!("{} updating liveness {} to {}", self.id, id, now);
        *self.alive.entry(id).or_insert(now) = now;
    }
    
    pub fn purge(&mut self, now: u64) {
        if self.table.len() <= MIN_TABLE_SIZE {
            trace!("purge: skipped: table too small: {}", self.table.len());
            return;
        }
        if self.leader_data().is_none() {
            trace!("purge: skipped: no leader_data");
            return;
        }
        let leader_id = self.leader_data().unwrap().id;
        let limit = GOSSIP_PURGE_MILLIS;
        let dead_ids: Vec<Pubkey> = self
            .alive
            .iter()
            .filter_map(|(&k, v)| {
                if k != self.id && (now - v) > limit {
                    Some(k)
                } else {
                    trace!("{} purge skipped {} {} {}", self.id, k, now - v, limit);
                    None
                }
            }).collect();

        sub_new_counter_info!("crdt-purge-count", dead_ids.len());

        for id in &dead_ids {
            self.alive.remove(id);
            self.table.remove(id);
            self.remote.remove(id);
            self.local.remove(id);
            self.external_liveness.remove(id);
            info!("{}: PURGE {}", self.id, id);
            for map in self.external_liveness.values_mut() {
                map.remove(id);
            }
            if *id == leader_id {
                info!("{}: PURGE LEADER {}", self.id, id,);
                sub_new_counter_info!("crdt-purge-purged_leader", 1, 1);
                self.set_leader(Pubkey::default());
            }
        }
    }


    pub fn compute_broadcast_table(&self) -> Vec<NodeInfo> {
        let live: Vec<_> = self.alive.iter().collect();
        let me = &self.table[&self.id];
        let cloned_table: Vec<NodeInfo> = live
            .iter()
            .map(|x| &self.table[x.0])
            .filter(|v| {
                if me.id == v.id {
                    false
                } else if !(Self::is_valid_address(&v.contact_info.tvu)) {
                    trace!(
                        "{}:broadcast skip not listening {} {}",
                        me.id,
                        v.id,
                        v.contact_info.tvu,
                    );
                    false
                } else {
                    trace!("{}:broadcast node {} {}", me.id, v.id, v.contact_info.tvu);
                    true
                }
            }).cloned()
            .collect();
        cloned_table
    }

    
    pub fn broadcast(
        crdt: &Arc<RwLock<Crdt>>,
        leader_rotation_interval: u64,
        me: &NodeInfo,
        broadcast_table: &[NodeInfo],
        window: &SharedWindow,
        s: &UdpSocket,
        transmit_index: &mut WindowIndex,
        received_index: u64,
    ) -> Result<()> {
        if broadcast_table.is_empty() {
            warn!("{}:not enough peers in crdt table", me.id);
            sub_new_counter_info!("crdt-broadcast-not_enough_peers_error", 1);
            Err(CrdtError::NoPeers)?;
        }
        trace!(
            "{} transmit_index: {:?} received_index: {} broadcast_len: {}",
            me.id,
            *transmit_index,
            received_index,
            broadcast_table.len()
        );

        let old_transmit_index = transmit_index.data;

        
        let mut orders = Vec::with_capacity((received_index - transmit_index.data + 1) as usize);
        let window_l = window.read().unwrap();

        let mut br_idx = transmit_index.data as usize % broadcast_table.len();

        for idx in transmit_index.data..received_index {
            let w_idx = idx as usize % window_l.len();

            trace!(
                "{} broadcast order data w_idx {} br_idx {}",
                me.id,
                w_idx,
                br_idx
            );

            
            let entry_height = idx + 1;
            if entry_height % leader_rotation_interval == 0 {
                let next_leader_id = crdt.read().unwrap().get_scheduled_leader(entry_height);
                if next_leader_id.is_some() && next_leader_id != Some(me.id) {
                    let info_result = broadcast_table
                        .iter()
                        .position(|n| n.id == next_leader_id.unwrap());
                    if let Some(index) = info_result {
                        orders.push((window_l[w_idx].data.clone(), &broadcast_table[index]));
                    }
                }
            }

            orders.push((window_l[w_idx].data.clone(), &broadcast_table[br_idx]));
            br_idx += 1;
            br_idx %= broadcast_table.len();
        }

        for idx in transmit_index.coding..received_index {
            let w_idx = idx as usize % window_l.len();

            if window_l[w_idx].coding.is_none() {
                continue;
            }

            trace!(
                "{} broadcast order coding w_idx: {} br_idx  :{}",
                me.id,
                w_idx,
                br_idx,
            );

            orders.push((window_l[w_idx].coding.clone(), &broadcast_table[br_idx]));
            br_idx += 1;
            br_idx %= broadcast_table.len();
        }

        trace!("broadcast orders table {}", orders.len());
        let errs: Vec<_> = orders
            .into_iter()
            .map(|(b, v)| {
                assert!(me.leader_id != v.id);
                let bl = b.unwrap();
                let blob = bl.read().unwrap();
                trace!(
                    "{}: BROADCAST idx: {} sz: {} to {},{} coding: {}",
                    me.id,
                    blob.get_index().unwrap(),
                    blob.meta.size,
                    v.id,
                    v.contact_info.tvu,
                    blob.is_coding()
                );
                assert!(blob.meta.size <= BLOB_SIZE);
                let e = s.send_to(&blob.data[..blob.meta.size], &v.contact_info.tvu);
                trace!(
                    "{}: done broadcast {} to {} {}",
                    me.id,
                    blob.meta.size,
                    v.id,
                    v.contact_info.tvu
                );
                e
            }).collect();

        trace!("broadcast results {}", errs.len());
        for e in errs {
            if let Err(e) = &e {
                trace!("broadcast result {:?}", e);
            }
            e?;
            if transmit_index.data < received_index {
                transmit_index.data += 1;
            }
        }
        sub_new_counter_info!(
            "crdt-broadcast-max_idx",
            (transmit_index.data - old_transmit_index) as usize
        );
        transmit_index.coding = transmit_index.data;

        Ok(())
    }

    
    pub fn retransmit(obj: &Arc<RwLock<Self>>, blob: &SharedBlob, s: &UdpSocket) -> Result<()> {
        let (me, table): (NodeInfo, Vec<NodeInfo>) = {
            let s = obj.read().expect("'obj' read lock in pub fn retransmit");
            (s.my_data().clone(), s.table.values().cloned().collect())
        };
        blob.write()
            .unwrap()
            .set_id(me.id)
            .expect("set_id in pub fn retransmit");
        let rblob = blob.read().unwrap();
        let orders: Vec<_> = table
            .iter()
            .filter(|v| {
                if me.id == v.id {
                    trace!("skip retransmit to self {:?}", v.id);
                    false
                } else if me.leader_id == v.id {
                    trace!("skip retransmit to leader {:?}", v.id);
                    false
                } else if !(Self::is_valid_address(&v.contact_info.tvu)) {
                    trace!(
                        "skip nodes that are not listening {:?} {}",
                        v.id,
                        v.contact_info.tvu
                    );
                    false
                } else {
                    true
                }
            }).collect();
        trace!("retransmit orders {}", orders.len());
        let errs: Vec<_> = orders
            .par_iter()
            .map(|v| {
                debug!(
                    "{}: retransmit blob {} to {} {}",
                    me.id,
                    rblob.get_index().unwrap(),
                    v.id,
                    v.contact_info.tvu,
                );
                //TODO profile this, may need multiple sockets for par_iter
                assert!(rblob.meta.size <= BLOB_SIZE);
                s.send_to(&rblob.data[..rblob.meta.size], &v.contact_info.tvu)
            }).collect();
        for e in errs {
            if let Err(e) = &e {
                sub_new_counter_info!("crdt-retransmit-send_to_error", 1, 1);
                error!("retransmit result {:?}", e);
            }
            e?;
        }
        Ok(())
    }

    // max number of nodes that we could be converged to
    pub fn convergence(&self) -> u64 {
        let max = self.remote.values().len() as u64 + 1;
        self.remote.values().fold(max, |a, b| std::cmp::min(a, *b))
    }

    // TODO: fill in with real implmentation once staking is implemented
    fn get_stake(_id: Pubkey) -> f64 {
        1.0
    }

    fn get_updates_since(&self, v: u64) -> (Pubkey, u64, Vec<NodeInfo>) {
        //trace!("get updates since {}", v);
        let data = self
            .table
            .values()
            .filter(|x| x.id != Pubkey::default() && self.local[&x.id] > v)
            .cloned()
            .collect();
        let id = self.id;
        let ups = self.update_index;
        (id, ups, data)
    }

    pub fn valid_last_ids(&self) -> Vec<Hash> {
        self.table
            .values()
            .filter(|r| {
                r.id != Pubkey::default()
                    && (Self::is_valid_address(&r.contact_info.tpu)
                        || Self::is_valid_address(&r.contact_info.tvu))
            }).map(|x| x.ledger_state.last_id)
            .collect()
    }

    pub fn window_index_request(&self, ix: u64) -> Result<(SocketAddr, Vec<u8>)> {
        // find a peer that appears to be accepting replication, as indicated
        //  by a valid tvu port location
        let valid: Vec<_> = self
            .table
            .values()
            .filter(|r| r.id != self.id && Self::is_valid_address(&r.contact_info.tvu))
            .collect();
        if valid.is_empty() {
            Err(CrdtError::NoPeers)?;
        }
        let n = thread_rng().gen::<usize>() % valid.len();
        let addr = valid[n].contact_info.ncp; // send the request to the peer's gossip port
        let req = Protocol::RequestWindowIndex(self.my_data().clone(), ix);
        let out = serialize(&req)?;
        Ok((addr, out))
    }

    fn gossip_request(&self) -> Result<(SocketAddr, Protocol)> {
        let options: Vec<_> = self
            .table
            .values()
            .filter(|v| {
                v.id != self.id
                    && !v.contact_info.ncp.ip().is_unspecified()
                    && !v.contact_info.ncp.ip().is_multicast()
            }).collect();

        let choose_peer_strategy = ChooseWeightedPeerStrategy::new(
            &self.remote,
            &self.external_liveness,
            &Self::get_stake,
        );

        let choose_peer_result = choose_peer_strategy.choose_peer(options);

        if let Err(Error::CrdtError(CrdtError::NoPeers)) = &choose_peer_result {
            trace!("crdt too small for gossip {} {}", self.id, self.table.len());
        };
        let v = choose_peer_result?;

        let remote_update_index = *self.remote.get(&v.id).unwrap_or(&0);
        let req = Protocol::RequestUpdates(remote_update_index, self.my_data().clone());
        trace!(
            "created gossip request from {} {:?} to {} {}",
            self.id,
            self.my_data(),
            v.id,
            v.contact_info.ncp
        );

        Ok((v.contact_info.ncp, req))
    }

    pub fn new_vote(&mut self, last_id: Hash) -> Result<(Vote, SocketAddr)> {
        let mut me = self.my_data().clone();
        let leader = self.leader_data().ok_or(CrdtError::NoLeader)?.clone();
        me.version += 1;
        me.ledger_state.last_id = last_id;
        let vote = Vote {
            version: me.version,
            contact_info_version: me.contact_info.version,
        };
        self.insert(&me);
        Ok((vote, leader.contact_info.tpu))
    }

    
    fn run_gossip(obj: &Arc<RwLock<Self>>, blob_sender: &BlobSender) -> Result<()> {
        
        let (remote_gossip_addr, req) = obj
            .read()
            .expect("'obj' read lock in fn run_gossip")
            .gossip_request()?;

        
        let blob = to_blob(req, remote_gossip_addr)?;
        blob_sender.send(vec![blob])?;
        Ok(())
    }
    
    fn top_leader(&self) -> Option<Pubkey> {
        let mut table = HashMap::new();
        let def = Pubkey::default();
        let cur = self.table.values().filter(|x| x.leader_id != def);
        for v in cur {
            let cnt = table.entry(&v.leader_id).or_insert(0);
            *cnt += 1;
            trace!("leader {} {}", v.leader_id, *cnt);
        }
        let mut sorted: Vec<(&Pubkey, usize)> = table.into_iter().collect();
        for x in &sorted {
            trace!("{}: sorted leaders {} votes: {}", self.id, x.0, x.1);
        }
        sorted.sort_by_key(|a| a.1);
        sorted.last().map(|a| *a.0)
    }

    
    fn update_leader(&mut self) {
        if let Some(leader_id) = self.top_leader() {
            if self.my_data().leader_id != leader_id && self.table.get(&leader_id).is_some() {
                self.set_leader(leader_id);
            }
        }
    }

    
    fn apply_updates(
        &mut self,
        from: Pubkey,
        update_index: u64,
        data: &[NodeInfo],
        external_liveness: &[(Pubkey, u64)],
    ) {
        trace!("got updates {}", data.len());
        
        let mut insert_total = 0;
        for v in data {
            insert_total += self.insert(&v);
        }
        sub_new_counter_info!("crdt-update-count", insert_total);

        for (pubkey, external_remote_index) in external_liveness {
            let remote_entry = if let Some(v) = self.remote.get(pubkey) {
                *v
            } else {
                0
            };

            if remote_entry >= *external_remote_index {
                continue;
            }

            let liveness_entry = self
                .external_liveness
                .entry(*pubkey)
                .or_insert_with(HashMap::new);
            let peer_index = *liveness_entry.entry(from).or_insert(*external_remote_index);
            if *external_remote_index > peer_index {
                liveness_entry.insert(from, *external_remote_index);
            }
        }

        *self.remote.entry(from).or_insert(update_index) = update_index;

        
        self.external_liveness.remove(&from);
    }

    
    pub fn gossip(
        obj: Arc<RwLock<Self>>,
        blob_sender: BlobSender,
        exit: Arc<AtomicBool>,
    ) -> JoinHandle<()> {
        Builder::new()
            .name("bitconch-gossip".to_string())
            .spawn(move || loop {
                let start = timestamp();
                let _ = Self::run_gossip(&obj, &blob_sender);
                if exit.load(Ordering::Relaxed) {
                    return;
                }
                obj.write().unwrap().purge(timestamp());
                
                obj.write().unwrap().update_leader();
                let elapsed = timestamp() - start;
                if GOSSIP_SLEEP_MILLIS > elapsed {
                    let time_left = GOSSIP_SLEEP_MILLIS - elapsed;
                    sleep(Duration::from_millis(time_left));
                }
            }).unwrap()
    }
    fn run_window_request(
        from: &NodeInfo,
        from_addr: &SocketAddr,
        window: &SharedWindow,
        ledger_window: &mut Option<&mut LedgerWindow>,
        me: &NodeInfo,
        ix: u64,
    ) -> Option<SharedBlob> {
        let pos = (ix as usize) % window.read().unwrap().len();
        if let Some(ref mut blob) = &mut window.write().unwrap()[pos].data {
            let mut wblob = blob.write().unwrap();
            let blob_ix = wblob.get_index().expect("run_window_request get_index");
            if blob_ix == ix {
                let num_retransmits = wblob.meta.num_retransmits;
                wblob.meta.num_retransmits += 1;
                
                let mut sender_id = from.id;

                
                if me.leader_id == me.id
                    && (num_retransmits == 0 || num_retransmits.is_power_of_two())
                {
                    sender_id = me.id
                }

                let out = SharedBlob::default();

                
                {
                    let mut outblob = out.write().unwrap();
                    let sz = wblob.meta.size;
                    outblob.meta.size = sz;
                    outblob.data[..sz].copy_from_slice(&wblob.data[..sz]);
                    outblob.meta.set_addr(from_addr);
                    outblob.set_id(sender_id).expect("blob set_id");
                }
                sub_new_counter_info!("crdt-window-request-pass", 1);

                return Some(out);
            } else {
                sub_new_counter_info!("crdt-window-request-outside", 1);
                trace!(
                    "requested ix {} != blob_ix {}, outside window!",
                    ix,
                    blob_ix
                );
                
            }
        }

        if let Some(ledger_window) = ledger_window {
            if let Ok(entry) = ledger_window.get_entry(ix) {
                sub_new_counter_info!("crdt-window-request-ledger", 1);

                let out = entry.to_blob(
                    Some(ix),
                    Some(me.id),
                    Some(from_addr),
                );

                return Some(out);
            }
        }

        sub_new_counter_info!("crdt-window-request-fail", 1);
        trace!(
            "{}: failed RequestWindowIndex {} {} {}",
            me.id,
            from.id,
            ix,
            pos,
        );

        None
    }

    
    fn handle_blob(
        obj: &Arc<RwLock<Self>>,
        window: &SharedWindow,
        ledger_window: &mut Option<&mut LedgerWindow>,
        blob: &Blob,
    ) -> Option<SharedBlob> {
        match deserialize(&blob.data[..blob.meta.size]) {
            Ok(request) => {
                Crdt::handle_protocol(obj, &blob.meta.addr(), request, window, ledger_window)
            }
            Err(_) => {
                warn!("deserialize crdt packet failed");
                None
            }
        }
    }

    fn handle_protocol(
        me: &Arc<RwLock<Self>>,
        from_addr: &SocketAddr,
        request: Protocol,
        window: &SharedWindow,
        ledger_window: &mut Option<&mut LedgerWindow>,
    ) -> Option<SharedBlob> {
        match request {
            
            Protocol::RequestUpdates(version, mut from) => {
                let id = me.read().unwrap().id;

                trace!(
                    "{} RequestUpdates {} from {}, professing to be {}",
                    id,
                    version,
                    from_addr,
                    from.contact_info.ncp
                );

                if from.id == me.read().unwrap().id {
                    warn!(
                        "RequestUpdates ignored, I'm talking to myself: me={} remoteme={}",
                        me.read().unwrap().id,
                        from.id
                    );
                    sub_new_counter_info!("crdt-window-request-loopback", 1);
                    return None;
                }

                
                if from.contact_info.ncp.ip().is_unspecified() {
                    sub_new_counter_info!("crdt-window-request-updates-unspec-ncp", 1);
                    from.contact_info.ncp = *from_addr;
                }

                let (from_id, ups, data, liveness) = {
                    let me = me.read().unwrap();

                    
                    let (from_id, ups, data) = me.get_updates_since(version);

                    (
                        from_id,
                        ups,
                        data,
                        me.remote.iter().map(|(k, v)| (*k, *v)).collect(),
                    )
                };

                
                {
                    let mut me = me.write().unwrap();
                    me.insert(&from);
                    me.update_liveness(from.id);
                }

                trace!("get updates since response {} {}", version, data.len());
                let len = data.len();

                if len < 1 {
                    let me = me.read().unwrap();
                    trace!(
                        "no updates me {} ix {} since {}",
                        id,
                        me.update_index,
                        version
                    );
                    None
                } else {
                    let rsp = Protocol::ReceiveUpdates(from_id, ups, data, liveness);

                    if let Ok(r) = to_blob(rsp, from.contact_info.ncp) {
                        trace!(
                            "sending updates me {} len {} to {} {}",
                            id,
                            len,
                            from.id,
                            from.contact_info.ncp,
                        );
                        Some(r)
                    } else {
                        warn!("to_blob failed");
                        None
                    }
                }
            }
            Protocol::ReceiveUpdates(from, update_index, data, external_liveness) => {
                let now = Instant::now();
                trace!(
                    "ReceivedUpdates from={} update_index={} len={}",
                    from,
                    update_index,
                    data.len()
                );
                me.write()
                    .expect("'me' write lock in ReceiveUpdates")
                    .apply_updates(from, update_index, &data, &external_liveness);

                report_time_spent(
                    "ReceiveUpdates",
                    &now.elapsed(),
                    &format!(" len: {}", data.len()),
                );
                None
            }

            Protocol::RequestWindowIndex(from, ix) => {
                let now = Instant::now();

                if from.id == me.read().unwrap().id {
                    warn!(
                        "{}: Ignored received RequestWindowIndex from ME {} {} ",
                        me.read().unwrap().id,
                        from.id,
                        ix,
                    );
                    sub_new_counter_info!("crdt-window-request-address-eq", 1);
                    return None;
                }

                me.write().unwrap().insert(&from);
                let me = me.read().unwrap().my_data().clone();
                sub_new_counter_info!("crdt-window-request-recv", 1);
                trace!("{}: received RequestWindowIndex {} {} ", me.id, from.id, ix,);
                let res =
                    Self::run_window_request(&from, &from_addr, &window, ledger_window, &me, ix);
                report_time_spent(
                    "RequestWindowIndex",
                    &now.elapsed(),
                    &format!(" ix: {}", ix),
                );
                res
            }
        }
    }

    
    fn run_listen(
        obj: &Arc<RwLock<Self>>,
        window: &SharedWindow,
        ledger_window: &mut Option<&mut LedgerWindow>,
        requests_receiver: &BlobReceiver,
        response_sender: &BlobSender,
    ) -> Result<()> {
        
        let timeout = Duration::new(1, 0);
        let mut reqs = requests_receiver.recv_timeout(timeout)?;
        while let Ok(mut more) = requests_receiver.try_recv() {
            reqs.append(&mut more);
        }
        let mut resps = Vec::new();
        for req in reqs {
            if let Some(resp) = Self::handle_blob(obj, window, ledger_window, &req.read().unwrap())
            {
                resps.push(resp);
            }
        }
        response_sender.send(resps)?;
        Ok(())
    }
    pub fn listen(
        me: Arc<RwLock<Self>>,
        window: SharedWindow,
        ledger_path: Option<&str>,
        requests_receiver: BlobReceiver,
        response_sender: BlobSender,
        exit: Arc<AtomicBool>,
    ) -> JoinHandle<()> {
        let mut ledger_window = ledger_path.map(|p| LedgerWindow::open(p).unwrap());

        Builder::new()
            .name("bitconch-listen".to_string())
            .spawn(move || loop {
                let e = Self::run_listen(
                    &me,
                    &window,
                    &mut ledger_window.as_mut(),
                    &requests_receiver,
                    &response_sender,
                );
                if exit.load(Ordering::Relaxed) {
                    return;
                }
                if e.is_err() {
                    let me = me.read().unwrap();
                    debug!(
                        "{}: run_listen timeout, table size: {}",
                        me.id,
                        me.table.len()
                    );
                }
            }).unwrap()
    }

    fn is_valid_ip(addr: IpAddr) -> bool {
        !(addr.is_unspecified() || addr.is_multicast())
    }
    
    pub fn is_valid_address(addr: &SocketAddr) -> bool {
        (addr.port() != 0) && Self::is_valid_ip(addr.ip())
    }

    pub fn spy_node() -> (NodeInfo, UdpSocket) {
        let (_, gossip_socket) = bind_in_range(FULLNODE_PORT_RANGE).unwrap();
        let pubkey = Keypair::new().pubkey();
        let daddr = socketaddr_any!();

        let node = NodeInfo::new(pubkey, daddr, daddr, daddr, daddr, daddr);
        (node, gossip_socket)
    }
}

#[derive(Debug)]
pub struct Sockets {
    pub gossip: UdpSocket,
    pub requests: UdpSocket,
    pub replicate: Vec<UdpSocket>,
    pub transaction: Vec<UdpSocket>,
    pub respond: UdpSocket,
    pub broadcast: UdpSocket,
    pub repair: UdpSocket,
    pub retransmit: UdpSocket,
}

#[derive(Debug)]
pub struct Node {
    pub info: NodeInfo,
    pub sockets: Sockets,
}

impl Node {
    pub fn new_localhost() -> Self {
        let pubkey = Keypair::new().pubkey();
        Self::new_localhost_with_pubkey(pubkey)
    }
    pub fn new_localhost_with_pubkey(pubkey: Pubkey) -> Self {
        let transaction = UdpSocket::bind("127.0.0.1:0").unwrap();
        let gossip = UdpSocket::bind("127.0.0.1:0").unwrap();
        let replicate = UdpSocket::bind("127.0.0.1:0").unwrap();
        let requests = UdpSocket::bind("127.0.0.1:0").unwrap();
        let repair = UdpSocket::bind("127.0.0.1:0").unwrap();

        let respond = UdpSocket::bind("0.0.0.0:0").unwrap();
        let broadcast = UdpSocket::bind("0.0.0.0:0").unwrap();
        let retransmit = UdpSocket::bind("0.0.0.0:0").unwrap();
        let storage = UdpSocket::bind("0.0.0.0:0").unwrap();
        let info = NodeInfo::new(
            pubkey,
            gossip.local_addr().unwrap(),
            replicate.local_addr().unwrap(),
            requests.local_addr().unwrap(),
            transaction.local_addr().unwrap(),
            storage.local_addr().unwrap(),
        );
        Node {
            info,
            sockets: Sockets {
                gossip,
                requests,
                replicate: vec![replicate],
                transaction: vec![transaction],
                respond,
                broadcast,
                repair,
                retransmit,
            },
        }
    }
    pub fn new_with_external_ip(pubkey: Pubkey, ncp: &SocketAddr) -> Node {
        fn bind() -> (u16, UdpSocket) {
            bind_in_range(FULLNODE_PORT_RANGE).expect("Failed to bind")
        };

        let (gossip_port, gossip) = if ncp.port() != 0 {
            (ncp.port(), bind_to(ncp.port(), false).expect("ncp bind"))
        } else {
            bind()
        };

        let (replicate_port, replicate_sockets) =
            multi_bind_in_range(FULLNODE_PORT_RANGE, 8).expect("tvu multi_bind");

        let (requests_port, requests) = bind();

        let (transaction_port, transaction_sockets) =
            multi_bind_in_range(FULLNODE_PORT_RANGE, 32).expect("tpu multi_bind");

        let (_, repair) = bind();
        let (_, broadcast) = bind();
        let (_, retransmit) = bind();
        let (storage_port, _) = bind();

        
        let respond = requests.try_clone().unwrap();

        let info = NodeInfo::new(
            pubkey,
            SocketAddr::new(ncp.ip(), gossip_port),
            SocketAddr::new(ncp.ip(), replicate_port),
            SocketAddr::new(ncp.ip(), requests_port),
            SocketAddr::new(ncp.ip(), transaction_port),
            SocketAddr::new(ncp.ip(), storage_port),
        );
        trace!("new NodeInfo: {:?}", info);

        Node {
            info,
            sockets: Sockets {
                gossip,
                requests,
                replicate: replicate_sockets,
                transaction: transaction_sockets,
                respond,
                broadcast,
                repair,
                retransmit,
            },
        }
    }
}

fn report_time_spent(label: &str, time: &Duration, extra: &str) {
    let count = duration_in_milliseconds(time);
    if count > 5 {
        info!("{} took: {} ms {}", label, count, extra);
    }
}

