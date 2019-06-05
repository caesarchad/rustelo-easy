//! The `vote_stage` votes on the `last_id` of the bank at a regular cadence

use crate::tx_vault::Bank;
use bincode::serialize;
use crate::budget_transaction::BudgetTransaction;
use buffett_metrics::counter::Counter;
use crate::crdt::Crdt;
use buffett_crypto::hash::Hash;
use influx_db_client as influxdb;
use log::Level;
use buffett_metrics::metrics;
use crate::packet::SharedBlob;
use crate::result::Result;
use buffett_crypto::signature::Keypair;
use buffett_interface::pubkey::Pubkey;
use std::result;
use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, RwLock};
use crate::streamer::BlobSender;
use buffett_timing::timing;
use crate::transaction::Transaction;
use buffett_metrics::sub_new_counter_info;

pub const VOTE_TIMEOUT_MS: u64 = 1000;

#[derive(Debug, PartialEq, Eq)]
enum VoteError {
    NoValidLastIdsToVoteOn,
}

pub fn create_new_signed_vote_blob(
    last_id: &Hash,
    keypair: &Keypair,
    crdt: &Arc<RwLock<Crdt>>,
) -> Result<SharedBlob> {
    let shared_blob = SharedBlob::default();
    let (vote, addr) = {
        let mut wcrdt = crdt.write().unwrap();
        //TODO: doesn't seem like there is a synchronous call to get height and id
        debug!("voting on {:?}", &last_id.as_ref()[..8]);
        wcrdt.new_vote(*last_id)
    }?;
    let tx = Transaction::budget_new_vote(&keypair, vote, *last_id, 0);
    {
        let mut blob = shared_blob.write().unwrap();
        let bytes = serialize(&tx)?;
        let len = bytes.len();
        blob.data[..len].copy_from_slice(&bytes);
        blob.meta.set_addr(&addr);
        blob.meta.size = len;
    }
    Ok(shared_blob)
}

fn get_last_id_to_vote_on(
    id: &Pubkey,
    ids: &[Hash],
    bank: &Arc<Bank>,
    now: u64,
    last_vote: &mut u64,
    last_valid_validator_timestamp: &mut u64,
) -> result::Result<(Hash, u64), VoteError> {
    let mut valid_ids = bank.count_valid_ids(&ids);
    let super_majority_index = (2 * ids.len()) / 3;

    //TODO(anatoly): this isn't stake based voting
    debug!(
        "{}: valid_ids {}/{} {}",
        id,
        valid_ids.len(),
        ids.len(),
        super_majority_index,
    );

    metrics::submit(
        influxdb::Point::new("voter_info")
            .add_field("total_peers", influxdb::Value::Integer(ids.len() as i64))
            .add_field(
                "valid_peers",
                influxdb::Value::Integer(valid_ids.len() as i64),
            ).to_owned(),
    );

    if valid_ids.len() > super_majority_index {
        *last_vote = now;

        // Sort by timestamp
        valid_ids.sort_by(|a, b| a.1.cmp(&b.1));

        let last_id = ids[valid_ids[super_majority_index].0];
        return Ok((last_id, valid_ids[super_majority_index].1));
    }

    if *last_valid_validator_timestamp != 0 {
        metrics::submit(
            influxdb::Point::new(&"leader-finality")
                .add_field(
                    "duration_ms",
                    influxdb::Value::Integer((now - *last_valid_validator_timestamp) as i64),
                ).to_owned(),
        );
    }

    Err(VoteError::NoValidLastIdsToVoteOn)
}

pub fn send_leader_vote(
    id: &Pubkey,
    keypair: &Keypair,
    bank: &Arc<Bank>,
    crdt: &Arc<RwLock<Crdt>>,
    vote_blob_sender: &BlobSender,
    last_vote: &mut u64,
    last_valid_validator_timestamp: &mut u64,
) -> Result<()> {
    let now = timing::timestamp();
    if now - *last_vote > VOTE_TIMEOUT_MS {
        let ids: Vec<_> = crdt.read().unwrap().valid_last_ids();
        if let Ok((last_id, super_majority_timestamp)) = get_last_id_to_vote_on(
            id,
            &ids,
            bank,
            now,
            last_vote,
            last_valid_validator_timestamp,
        ) {
            if let Ok(shared_blob) = create_new_signed_vote_blob(&last_id, keypair, crdt) {
                vote_blob_sender.send(vec![shared_blob])?;
                let finality_ms = now - super_majority_timestamp;

                *last_valid_validator_timestamp = super_majority_timestamp;
                debug!("{} leader_sent_vote finality: {} ms", id, finality_ms);
                sub_new_counter_info!("vote_stage-leader_sent_vote", 1);

                bank.set_finality((now - *last_valid_validator_timestamp) as usize);

                metrics::submit(
                    influxdb::Point::new(&"leader-finality")
                        .add_field("duration_ms", influxdb::Value::Integer(finality_ms as i64))
                        .to_owned(),
                );
            }
        }
    }
    Ok(())
}

pub fn send_validator_vote(
    bank: &Arc<Bank>,
    keypair: &Arc<Keypair>,
    crdt: &Arc<RwLock<Crdt>>,
    vote_blob_sender: &BlobSender,
) -> Result<()> {
    let last_id = bank.last_id();
    if let Ok(shared_blob) = create_new_signed_vote_blob(&last_id, keypair, crdt) {
        sub_new_counter_info!("replicate-vote_sent", 1);

        vote_blob_sender.send(vec![shared_blob])?;
    }
    Ok(())
}

