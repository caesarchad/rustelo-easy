use crate::crdt::{CrdtError, NodeInfo};
use rand::distributions::{Distribution, Weighted, WeightedChoice};
use rand::thread_rng;
use crate::result::Result;
use buffett_interface::pubkey::Pubkey;
use std;
use std::collections::HashMap;

pub const DEFAULT_WEIGHT: u32 = 1;

pub trait ChooseGossipPeerStrategy {
    fn choose_peer<'a>(&self, options: Vec<&'a NodeInfo>) -> Result<&'a NodeInfo>;
}

pub struct ChooseRandomPeerStrategy<'a> {
    random: &'a Fn() -> u64,
}


impl<'a, 'b> ChooseRandomPeerStrategy<'a> {
    pub fn new(random: &'a Fn() -> u64) -> Self {
        ChooseRandomPeerStrategy { random }
    }
}

impl<'a> ChooseGossipPeerStrategy for ChooseRandomPeerStrategy<'a> {
    fn choose_peer<'b>(&self, options: Vec<&'b NodeInfo>) -> Result<&'b NodeInfo> {
        if options.is_empty() {
            Err(CrdtError::NoPeers)?;
        }

        let n = ((self.random)() as usize) % options.len();
        Ok(options[n])
    }
}


pub struct ChooseWeightedPeerStrategy<'a> {
    
    remote: &'a HashMap<Pubkey, u64>,
    
    external_liveness: &'a HashMap<Pubkey, HashMap<Pubkey, u64>>,
    
    get_stake: &'a Fn(Pubkey) -> f64,
}

impl<'a> ChooseWeightedPeerStrategy<'a> {
    pub fn new(
        remote: &'a HashMap<Pubkey, u64>,
        external_liveness: &'a HashMap<Pubkey, HashMap<Pubkey, u64>>,
        get_stake: &'a Fn(Pubkey) -> f64,
    ) -> Self {
        ChooseWeightedPeerStrategy {
            remote,
            external_liveness,
            get_stake,
        }
    }

    fn calculate_weighted_remote_index(&self, peer_id: Pubkey) -> u32 {
        let mut last_seen_index = 0;
        
        if let Some(index) = self.remote.get(&peer_id) {
            last_seen_index = *index;
        }

        let liveness_entry = self.external_liveness.get(&peer_id);
        if liveness_entry.is_none() {
            return DEFAULT_WEIGHT;
        }

        let votes = liveness_entry.unwrap();

        if votes.is_empty() {
            return DEFAULT_WEIGHT;
        }

        
        let mut relevant_votes = vec![];

        let total_stake = votes.iter().fold(0.0, |total_stake, (&id, &vote)| {
            let stake = (self.get_stake)(id);
            
            if std::f64::MAX - total_stake < stake {
                if stake > total_stake {
                    relevant_votes = vec![(stake, vote)];
                    stake
                } else {
                    total_stake
                }
            } else {
                relevant_votes.push((stake, vote));
                total_stake + stake
            }
        });

        let weighted_vote = relevant_votes.iter().fold(0.0, |sum, &(stake, vote)| {
            if vote < last_seen_index {
                

                warn!("weighted peer index was smaller than local entry in remote table");
                return sum;
            }

            let vote_difference = (vote - last_seen_index) as f64;
            let new_weight = vote_difference * (stake / total_stake);

            if std::f64::MAX - sum < new_weight {
                return f64::max(new_weight, sum);
            }

            sum + new_weight
        });

        
        if weighted_vote >= f64::from(std::u32::MAX) {
            return std::u32::MAX;
        }

        
        weighted_vote as u32 + DEFAULT_WEIGHT
    }
}

impl<'a> ChooseGossipPeerStrategy for ChooseWeightedPeerStrategy<'a> {
    fn choose_peer<'b>(&self, options: Vec<&'b NodeInfo>) -> Result<&'b NodeInfo> {
        if options.is_empty() {
            Err(CrdtError::NoPeers)?;
        }

        let mut weighted_peers = vec![];
        for peer in options {
            let weight = self.calculate_weighted_remote_index(peer.id);
            weighted_peers.push(Weighted { weight, item: peer });
        }

        let mut rng = thread_rng();
        Ok(WeightedChoice::new(&mut weighted_peers).sample(&mut rng))
    }
}

