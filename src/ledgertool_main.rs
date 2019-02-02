use clap::{App, Arg, SubCommand};
use buffett::bank::Bank;
use buffett::ledger::{read_ledger, verify_ledger};
use buffett::logger;
use std::io::{stdout, Write};
use std::process::exit;

#[no_mangle]
pub extern "C" fn ledgertool_main_entry(parm01_ledger_ptr: *const libc::c_char,
                                        parm02_head_ptr: *const libc::c_char,
                                        parm03_precheck_ptr: *const libc::c_char,
                                        parm04_continue_ptr: *const libc::c_char,
                                        parm05_subcommand_ptr: *const libc::c_char) {
    //setup log and pannic hook
    logger::setup();

    //handle parameters, convert ptr to &str
    let ledger_str = unsafe { CStr::from_ptr(parm01_ledger_ptr) }.to_str().unwrap();
    let head_str = unsafe { CStr::from_ptr(parm02_head_ptr) }.to_str().unwrap();
    let precheck_str = unsafe { CStr::from_ptr(parm03_precheck_ptr) }.to_str().unwrap();
    let continue_str = unsafe { CStr::from_ptr(parm04_continue_ptr) }.to_str().unwrap();
    let subcommand_str = unsafe { CStr::from_ptr(parm05_subcommand_ptr) }.to_str().unwrap();


    /*
    let matches = App::new("ledger-tool")
        .version(crate_version!())
        .arg(
            Arg::with_name("ledger")
                .short("l")
                .long("ledger")
                .value_name("DIR")
                .takes_value(true)
                .required(true)
                .help("Use directory for ledger location"),
        )
        .arg(
            Arg::with_name("head")
                .short("n")
                .long("head")
                .value_name("NUM")
                .takes_value(true)
                .help("Limit to at most the first NUM entries in ledger\n  (only applies to verify, print, json commands)"),
        )
        .arg(
            Arg::with_name("precheck")
                .short("p")
                .long("precheck")
                .help("Use ledger_verify() to check internal ledger consistency before proceeding"),
        )
        .arg(
            Arg::with_name("continue")
                .short("c")
                .long("continue")
                .help("Continue verify even if verification fails"),
        )
        .subcommand(SubCommand::with_name("print").about("Print the ledger"))
        .subcommand(SubCommand::with_name("json").about("Print the ledger in JSON format"))
        .subcommand(SubCommand::with_name("verify").about("Verify the ledger's PoH"))
        .get_matches();
    */

    //retrieve the ledger file location
    //let ledger_path = matches.value_of("ledger").unwrap();
    let ledger_path = ledger_str;

    //if matches.is_present("precheck") {
    if  precheck_str == "TRUE" {    
        if let Err(e) = verify_ledger(&ledger_path) {
            eprintln!("ledger precheck failed, error: {:?} ", e);
            exit(1);
        }
    }

    let entries = match read_ledger(ledger_path, true) {
        Ok(entries) => entries,
        Err(err) => {
            eprintln!("Failed to open ledger at {}: {}", ledger_path, err);
            exit(1);
        }
    };


    //retreive the head number 
    /*
    let head = match matches.value_of("head") {
        Some(head) => head.parse().expect("please pass a number for --head"),
        None => <usize>::max_value(),
    };
    */
    let head = if !head_str.is_empty() {
        //cast the head_str to usize
        match head_str.parse::<usize>(){
            Ok(i)  => {
                         i
            },
            Err(e) => {
                        println!("A correct number is needed for head");
                        std::process::exit(1);
            }
        }
    } else {
        <usize>::max_value()
    };

    /*
    match matches.subcommand() {
        ("print", _) => {
            let entries = match read_ledger(ledger_path, true) {
                Ok(entries) => entries,
                Err(err) => {
                    eprintln!("Failed to open ledger at {}: {}", ledger_path, err);
                    exit(1);
                }
            };
            for (i, entry) in entries.enumerate() {
                if i >= head {
                    break;
                }
                let entry = entry.unwrap();
                println!("{:?}", entry);
            }
        }
        ("json", _) => {
            stdout().write_all(b"{\"ledger\":[\n").expect("open array");
            for (i, entry) in entries.enumerate() {
                if i >= head {
                    break;
                }
                let entry = entry.unwrap();
                serde_json::to_writer(stdout(), &entry).expect("serialize");
                stdout().write_all(b",\n").expect("newline");
            }
            stdout().write_all(b"\n]}\n").expect("close array");
        }
        ("verify", _) => {
            if head < 2 {
                eprintln!("verify requires at least 2 entries to run");
                exit(1);
            }
            let bank = Bank::default();

            {
                let genesis = match read_ledger(ledger_path, true) {
                    Ok(entries) => entries,
                    Err(err) => {
                        eprintln!("Failed to open ledger at {}: {}", ledger_path, err);
                        exit(1);
                    }
                };

                let genesis = genesis.take(2).map(|e| e.unwrap());

                if let Err(e) = bank.process_ledger(genesis) {
                    eprintln!("verify failed at genesis err: {:?}", e);
                    if !matches.is_present("continue") {
                        exit(1);
                    }
                }
            }
            let entries = entries.map(|e| e.unwrap());

            let head = head - 2;
            for (i, entry) in entries.skip(2).enumerate() {
                if i >= head {
                    break;
                }
                if !entry.verify(&bank.last_id()) {
                    eprintln!("entry.verify() failed at entry[{}]", i + 2);
                    if !matches.is_present("continue") {
                        exit(1);
                    }
                }
                if let Err(e) = bank.process_entry(&entry) {
                    eprintln!("verify failed at entry[{}], err: {:?}", i + 2, e);
                    if !matches.is_present("continue") {
                        exit(1);
                    }
                }
            }
        }
        ("", _) => {
            eprintln!("{}", matches.usage());
            exit(1);
        }
        _ => unreachable!(),
    };
    */
    //handle subcommand parmeter: PRINT, JSON, VERIFY
    match subcommand_str {
        "PRINT" => {
            let entries = match read_ledger(ledger_path, true) {
                Ok(entries) => entries,
                Err(err) => {
                    eprintln!("Failed to open ledger at {}: {}", ledger_path, err);
                    exit(1);
                }
            };
            for (i, entry) in entries.enumerate() {
                if i >= head {
                    break;
                }
                let entry = entry.unwrap();
                println!("{:?}", entry);
            }
        },
        "JSON" => {
             stdout().write_all(b"{\"ledger\":[\n").expect("open array");
            for (i, entry) in entries.enumerate() {
                if i >= head {
                    break;
                }
                let entry = entry.unwrap();
                serde_json::to_writer(stdout(), &entry).expect("serialize");
                stdout().write_all(b",\n").expect("newline");
            }
            stdout().write_all(b"\n]}\n").expect("close array");
        },
        "VERIFY" => {
            
            if head < 2 {
                eprintln!("verify requires at least 2 entries to run");
                exit(1);
            }
            let bank = Bank::default();

            {
                let genesis = match read_ledger(ledger_path, true) {
                    Ok(entries) => entries,
                    Err(err) => {
                        eprintln!("Failed to open ledger at {}: {}", ledger_path, err);
                        exit(1);
                    }
                };

                let genesis = genesis.take(2).map(|e| e.unwrap());

                if let Err(e) = bank.process_ledger(genesis) {
                    eprintln!("verify failed at genesis err: {:?}", e);
                    if !matches.is_present("continue") {
                        exit(1);
                    }
                }
            }
            let entries = entries.map(|e| e.unwrap());

            let head = head - 2;
            for (i, entry) in entries.skip(2).enumerate() {
                if i >= head {
                    break;
                }
                if !entry.verify(&bank.last_id()) {
                    eprintln!("entry.verify() failed at entry[{}]", i + 2);
                    //if !matches.is_present("continue") {
                    if continue_str == "FALSE" {
                        exit(1);
                    }
                }
                if let Err(e) = bank.process_entry(&entry) {
                    eprintln!("verify failed at entry[{}], err: {:?}", i + 2, e);
                    //if !matches.is_present("continue") {
                    if continue_str == "FALSE" {    
                        exit(1);
                    }
                }
            }
        },
        "NULL" =>  {
            eprintln!("{}", "something wrong with subcommand");
            exit(1);
        }
        _ =>  unreachable!(),
    }
}
