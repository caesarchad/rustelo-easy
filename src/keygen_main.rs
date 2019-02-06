use std::ffi::CStr;
use crate::rustelo_error::RusteloResult;
use clap::{App, Arg};
use buffett::wallet::gen_keypair_file;
use std::error;

#[no_mangle]
pub extern "C" fn keygen_main_entry(parm01_outfile_ptr: *const libc::c_char) -> Result<(), Box<std::error::Error>> {
//pub extern "C" fn keygen_main_entry(parm01_outfile_ptr: *const libc::c_char) -> RusteloResult  {

    println!("Keymaker!");
    //handle parameters, convert ptr to &str
    let outfile_str = { CStr::from_ptr(parm01_outfile_ptr) }.to_str().unwrap();

    /*
    let matches = clap::App::new("buffett-keymaker")
        .version(crate_version!())
        .arg(
            clap::Arg::with_name("outfile")
                .short("o")
                .long("outfile")
                .value_name("PATH")
                .takes_value(true)
                .help("Path to generated file"),
        ).get_matches();
    */

    let mut path = dirs::home_dir().expect("home directory");


    //let outfile = if matches.is_present("outfile") {
    let outfile = if !outfile_str.is_empty() {
        println!("argument outfile is present ");
        //matches.value_of("outfile").unwrap()
        outfile_str
    } else {
        println!("argument outfile is not present ");
        path.extend(&[".config", "solana", "id.json"]);
        path.to_str().unwrap()
    };
    println!("generate keypair, and write to {}",outfile.to_string());
    let serialized_keypair = buffett::wallet::gen_keypair_file(outfile.to_string())?;
    if outfile == "-" {
        println!("{}", serialized_keypair);
    }
    Ok(())
    //RusteloResult::Success
}
