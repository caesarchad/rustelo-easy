use bitconch_sdk::account::KeyedAccount;
use bitconch_sdk::native_program::ProgramError;
use bitconch_sdk::pubkey::Pubkey;
use bitconch_sdk::bitconch_entrypoint;

bitconch_entrypoint!(entrypoint);
fn entrypoint(
    _program_id: &Pubkey,
    _keyed_accounts: &mut [KeyedAccount],
    _data: &[u8],
    _tick_height: u64,
) -> Result<(), ProgramError> {
    Err(ProgramError::GenericError)
}
