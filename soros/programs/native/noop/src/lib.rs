use log::*;
use bitconch_sdk::account::KeyedAccount;
use bitconch_sdk::native_program::ProgramError;
use bitconch_sdk::pubkey::Pubkey;
use bitconch_sdk::bitconch_entrypoint;

bitconch_entrypoint!(entrypoint);
fn entrypoint(
    program_id: &Pubkey,
    keyed_accounts: &mut [KeyedAccount],
    data: &[u8],
    tick_height: u64,
) -> Result<(), ProgramError> {
    bitconch_logger::setup();
    info!("noop: program_id: {:?}", program_id);
    info!("noop: keyed_accounts: {:#?}", keyed_accounts);
    info!("noop: data: {:?}", data);
    info!("noop: tick_height: {:?}", tick_height);
    Ok(())
}
