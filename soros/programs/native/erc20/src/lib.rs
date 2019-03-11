//! The `erc20` library implements a generic erc20-like token

use log::*;
use bitconch_sdk::account::KeyedAccount;
use bitconch_sdk::native_program::ProgramError;
use bitconch_sdk::pubkey::Pubkey;
use bitconch_sdk::bitconch_entrypoint;

mod token_program;

bitconch_entrypoint!(entrypoint);
fn entrypoint(
    program_id: &Pubkey,
    info: &mut [KeyedAccount],
    input: &[u8],
    _tick_height: u64,
) -> Result<(), ProgramError> {
    bitconch_logger::setup();

    token_program::TokenProgram::process(program_id, info, input).map_err(|err| {
        error!("error: {:?}", err);
        ProgramError::GenericError
    })
}
