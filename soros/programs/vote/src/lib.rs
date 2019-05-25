//! Vote program
//! Receive and processes votes from validators

use bincode::deserialize;
use log::*;
use soros_sdk::account::KeyedAccount;
use soros_sdk::native_program::ProgramError;
use soros_sdk::pubkey::Pubkey;
use soros_sdk::soros_entrypoint;
use soros_vote_api::vote_instruction::VoteInstruction;
use soros_vote_api::vote_state;

soros_entrypoint!(entrypoint);
fn entrypoint(
    _program_id: &Pubkey,
    keyed_accounts: &mut [KeyedAccount],
    data: &[u8],
    _tick_height: u64,
) -> Result<(), ProgramError> {
    soros_logger::setup();

    trace!("process_instruction: {:?}", data);
    trace!("keyed_accounts: {:?}", keyed_accounts);

    match deserialize(data).map_err(|_| ProgramError::InvalidInstructionData)? {
        VoteInstruction::InitializeAccount => vote_state::initialize_account(keyed_accounts),
        VoteInstruction::DelegateStake(delegate_id) => {
            vote_state::delegate_stake(keyed_accounts, &delegate_id)
        }
        VoteInstruction::AuthorizeVoter(voter_id) => {
            vote_state::authorize_voter(keyed_accounts, &voter_id)
        }
        VoteInstruction::Vote(vote) => {
            debug!("{:?} by {}", vote, keyed_accounts[0].signer_key().unwrap());
            soros_metrics::submit(
                soros_metrics::influxdb::Point::new("vote-native")
                    .add_field("count", soros_metrics::influxdb::Value::Integer(1))
                    .to_owned(),
            );
            vote_state::process_vote(keyed_accounts, vote)
        }
        VoteInstruction::ClearCredits => vote_state::clear_credits(keyed_accounts),
    }
}
