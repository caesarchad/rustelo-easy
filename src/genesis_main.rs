//! A command-line executable for generating the chain's genesis block.
use atty::{is, Stream};
use clap::{App, Arg};
use buffett::ledger::LedgerWriter;
use buffett::mint::Mint;
use std::error;
use std::io::{stdin, Read};
use std::process::exit;

#[no_mangle]
//pub extern "C" fn genesis_main_entry() -> Result<(), Box<error::Error>> {
pub extern "C" fn genesis_main_entry(parm01_tokens_ptr: *const libc::c_char,
                                     parm02_ledger_ptr: *const libc::c_char,) -> RusteloResult {  
    
    //handle parameters, convert ptr to &str
    let tokens_str  = unsafe { CStr::from_ptr(parm01_tokens_ptr) }.to_str().unwrap();  
    let ledger_str  = unsafe { CStr::from_ptr(parm02_ledger_ptr) }.to_str().unwrap();  
    
    /*let matches = App::new("solana-genesis")
        .version(crate_version!())
        .arg(
            Arg::with_name("tokens")
                .short("t")
                .long("tokens")
                .value_name("NUM")
                .takes_value(true)
                .required(true)
                .help("Number of tokens with which to initialize mint"),
        ).arg(
            Arg::with_name("ledger")
                .short("l")
                .long("ledger")
                .value_name("DIR")
                .takes_value(true)
                .required(true)
                .help("Use directory as persistent ledger location"),
        ).get_matches(); */

    //cast token_str to i64
    //let tokens = value_t_or_exit!(matches, "tokens", i64);
    if !tokens_str.is_empty(){
        match tokens_str.parse::<i64>(){
            Ok(i)  => {
                        let tokens =i;
            },
            Err(e) => {
                        println!("{} '{}' isn't a valid value\n\n{}\n\nPlease re-run with {} for \
                                        more information",
                                        ::clap::Format::Error("error:"),
                                        ::clap::Format::Warning(tokens_str.to_string()),
                                        matches.usage(),
                                        ::clap::Format::Good("--help"));
                                    ::std::process::exit(1);
            }
        }
    }

    //ledger path 
    //let ledger_path = matches.value_of("ledger").unwrap();
    let ledger_path = ledger_str.unwrap();

    if is(Stream::Stdin) {
        eprintln!("nothing found on stdin, expected a json file");
        exit(1);
    }

    let mut buffer = String::new();
    let num_bytes = stdin().read_to_string(&mut buffer)?;
    if num_bytes == 0 {
        eprintln!("empty file on stdin, expected a json file");
        exit(1);
    }

    let pkcs8: Vec<u8> = serde_json::from_str(&buffer)?;
    let mint = Mint::new_with_pkcs8(tokens, pkcs8);

    let mut ledger_writer = LedgerWriter::open(&ledger_path, true)?;
    ledger_writer.write_entries(mint.create_entries())?;

    //Ok(())
    RusteloResult::Success
}


