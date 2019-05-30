extern crate buffett_program_interface;

use buffett_program_interface::account::KeyedAccount;

#[no_mangle]
pub extern "C" fn process(_infos: &mut Vec<KeyedAccount>, _data: &[u8]) {}
