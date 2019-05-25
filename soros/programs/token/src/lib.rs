use bincode::serialize;
use log::*;
use soros_sdk::account::KeyedAccount;
use soros_sdk::native_program::ProgramError;
use soros_sdk::pubkey::Pubkey;
use soros_sdk::soros_entrypoint;

mod token_program;

soros_entrypoint!(entrypoint);
fn entrypoint(
    program_id: &Pubkey,
    info: &mut [KeyedAccount],
    input: &[u8],
    _tick_height: u64,
) -> Result<(), ProgramError> {
    soros_logger::setup();

    token_program::TokenProgram::process(program_id, info, input).map_err(|e| {
        error!("error: {:?}", e);
        ProgramError::CustomError(serialize(&e).unwrap())
    })
}
