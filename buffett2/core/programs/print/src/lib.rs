extern crate buffett_interface;

use buffett_interface::account::KeyedAccount;

#[no_mangle]
pub extern "C" fn process(infos: &mut Vec<KeyedAccount>, _data: &[u8]) {
    println!("AccountInfos: {:#?}", infos);
    //println!("data: {:#?}", data);
}
