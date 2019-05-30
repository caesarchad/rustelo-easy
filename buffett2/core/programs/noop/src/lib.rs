extern crate buffett_interface;

use buffett_interface::account::KeyedAccount;

#[no_mangle]
pub extern "C" fn process(_infos: &mut Vec<KeyedAccount>, _data: &[u8]) {}
