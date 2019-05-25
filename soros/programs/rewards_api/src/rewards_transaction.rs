//! The `rewards_transaction` module provides functionality for creating a global
//! rewards account and enabling stakers to redeem credits from their vote accounts.

use crate::id;
use crate::rewards_instruction::RewardsInstruction;
use crate::rewards_state::RewardsState;
use soros_sdk::hash::Hash;
use soros_sdk::pubkey::Pubkey;
use soros_sdk::signature::{Keypair, KeypairUtil};
use soros_sdk::system_transaction::SystemTransaction;
use soros_sdk::transaction::Transaction;
use soros_sdk::transaction_builder::TransactionBuilder;
use soros_vote_api::vote_instruction::VoteInstruction;

pub struct RewardsTransaction {}

impl RewardsTransaction {
    pub fn new_account(
        from_keypair: &Keypair,
        rewards_id: &Pubkey,
        blockhash: Hash,
        lamports: u64,
        fee: u64,
    ) -> Transaction {
        SystemTransaction::new_program_account(
            from_keypair,
            rewards_id,
            blockhash,
            lamports,
            RewardsState::max_size() as u64,
            &id(),
            fee,
        )
    }

    pub fn new_redeem_credits(
        vote_keypair: &Keypair,
        rewards_id: &Pubkey,
        blockhash: Hash,
        fee: u64,
    ) -> Transaction {
        let vote_id = vote_keypair.pubkey();
        TransactionBuilder::new(fee)
            .push(RewardsInstruction::new_redeem_vote_credits(
                &vote_id, rewards_id,
            ))
            .push(VoteInstruction::new_clear_credits(&vote_id))
            .sign(&[vote_keypair], blockhash)
    }
}
