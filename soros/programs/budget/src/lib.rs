mod budget_program;

use crate::budget_program::process_instruction;
use bincode::serialize;
use log::*;
use soros_sdk::account::KeyedAccount;
use soros_sdk::native_program::ProgramError;
use soros_sdk::pubkey::Pubkey;
use soros_sdk::soros_entrypoint;

soros_entrypoint!(entrypoint);
fn entrypoint(
    program_id: &Pubkey,
    keyed_accounts: &mut [KeyedAccount],
    data: &[u8],
    _tick_height: u64,
) -> Result<(), ProgramError> {
    soros_logger::setup();

    trace!("process_instruction: {:?}", data);
    trace!("keyed_accounts: {:?}", keyed_accounts);
    process_instruction(program_id, keyed_accounts, data)
        .map_err(|e| ProgramError::CustomError(serialize(&e).unwrap()))
}
