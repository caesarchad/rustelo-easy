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
    tick_height: u64,
) -> Result<(), ProgramError> {
    soros_logger::setup();
    info!("noop: program_id: {:?}", program_id);
    info!("noop: keyed_accounts: {:#?}", keyed_accounts);
    info!("noop: data: {:?}", data);
    info!("noop: tick_height: {:?}", tick_height);
    Ok(())
}
