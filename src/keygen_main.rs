use std::ffi::CStr;
use crate::rustelo_error::RusteloResult;
use clap::{App, Arg};
use buffett::wallet::gen_keypair_file;
use std::error;


#[no_mangle]
pub extern "C" fn keygen_main_entry(parm01_outfile_ptr: *const libc::c_char) -> RusteloResult  {

    //handle parameters, convert ptr to &str
    let outfile_str = unsafe {CStr::from_ptr(parm01_outfile_ptr)}.to_str().unwrap();

    main_entry(outfile_str);
    
    RusteloResult::Success
}

fn main_entry(outfile_str:&str) -> Result<(), Box<error::Error>> {
    let mut path = dirs::home_dir().expect("home directory");
    let outfile = if !outfile_str.is_empty() {
        Some(outfile_str).unwrap()
    } else {
        path.extend(&[".config", "solana", "id.json"]);
        path.to_str().unwrap()
    };

    let serialized_keypair = gen_keypair_file(outfile.to_string())?;
    if outfile == "-" {
        println!("{}", serialized_keypair);
    }
    Ok(())
}
