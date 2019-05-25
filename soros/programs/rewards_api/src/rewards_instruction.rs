use crate::id;
use serde_derive::{Deserialize, Serialize};
use soros_sdk::pubkey::Pubkey;
use soros_sdk::transaction_builder::BuilderInstruction;

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum RewardsInstruction {
    RedeemVoteCredits,
}

impl RewardsInstruction {
    pub fn new_redeem_vote_credits(vote_id: &Pubkey, rewards_id: &Pubkey) -> BuilderInstruction {
        BuilderInstruction::new(
            id(),
            &RewardsInstruction::RedeemVoteCredits,
            vec![(*vote_id, true), (*rewards_id, false)],
        )
    }
}
