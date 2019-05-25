use soros_sdk::account::KeyedAccount;
use soros_sdk::native_program::ProgramError;
use soros_sdk::pubkey::Pubkey;
use soros_sdk::soros_entrypoint;

soros_entrypoint!(entrypoint);
fn entrypoint(
    _program_id: &Pubkey,
    _keyed_accounts: &mut [KeyedAccount],
    _data: &[u8],
    _tick_height: u64,
) -> Result<(), ProgramError> {
    Err(ProgramError::GenericError)
}
