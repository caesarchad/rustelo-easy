mod budget_program;

use crate::budget_program::process_instruction;
use log::*;
use bitconch_sdk::account::KeyedAccount;
use bitconch_sdk::native_program::ProgramError;
use bitconch_sdk::pubkey::Pubkey;
use bitconch_sdk::bitconch_entrypoint;

bitconch_entrypoint!(entrypoint);
fn entrypoint(
    _program_id: &Pubkey,
    keyed_accounts: &mut [KeyedAccount],
    data: &[u8],
    _tick_height: u64,
) -> Result<(), ProgramError> {
    bitconch_logger::setup();

    trace!("process_instruction: {:?}", data);
    trace!("keyed_accounts: {:?}", keyed_accounts);
    process_instruction(keyed_accounts, data).map_err(|_| ProgramError::GenericError)
}
