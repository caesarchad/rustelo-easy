use clap::{App, Arg};
use buffett::crdt::FULLNODE_PORT_RANGE;
use buffett::fullnode::Config;
use buffett::logger;
use buffett::netutil::{get_ip_addr, get_public_ip_addr, parse_port_or_addr};
use buffett::signature::read_pkcs8;
use std::io;
use std::net::SocketAddr;
use std::ffi::CStr;
use crate::rustelo_error::RusteloResult;

#[no_mangle]
pub extern "C" fn fullnode_config_main_entry(parm01_local_ptr: *const libc::c_char,
                                             parm02_keypair_ptr: *const libc::c_char,
                                             parm03_public_ptr: *const libc::c_char,
                                             parm04_bind_ptr: *const libc::c_char) {
    //setup log and pannic hook
    logger::setup();
    //handle parameters, convert ptr to &str
    let local_str = unsafe { CStr::from_ptr(parm01_local_ptr) }.to_str().unwrap(); 
    let keypair_str= unsafe { CStr::from_ptr(parm02_keypair_ptr) }.to_str().unwrap(); 
    let public_str= unsafe { CStr::from_ptr(parm03_public_ptr) }.to_str().unwrap(); 
    let bind_str= unsafe { CStr::from_ptr(parm04_bind_ptr) }.to_str().unwrap();
    /*let matches = App::new("fullnode-config")
        .version(crate_version!())
        .arg(
            Arg::with_name("local")
                .short("l")
                .long("local")
                .takes_value(false)
                .help("Detect network address from local machine configuration"),
        ).arg(
            Arg::with_name("keypair")
                .short("k")
                .long("keypair")
                .value_name("PATH")
                .takes_value(true)
                .help("/path/to/id.json"),
        ).arg(
            Arg::with_name("public")
                .short("p")
                .long("public")
                .takes_value(false)
                .help("Detect public network address using public servers"),
        ).arg(
            Arg::with_name("bind")
                .short("b")
                .long("bind")
                .value_name("PORT")
                .takes_value(true)
                .help("Bind to port or address"),
        ).get_matches(); */

    let bind_addr: SocketAddr = {
        //let mut bind_addr = parse_port_or_addr(matches.value_of("bind"), FULLNODE_PORT_RANGE.0);
        let mut bind_addr = parse_port_or_addr(Some(bind_str), FULLNODE_PORT_RANGE.0);
        
        //if matches.is_present("local") {
        if local_str == "TRUE" {
            let ip = get_ip_addr().unwrap();
            bind_addr.set_ip(ip);
        }
        
        //if matches.is_present("public") {
        if public_str == "TRUE" {   
            let ip = get_public_ip_addr().unwrap();
            bind_addr.set_ip(ip);
        }

        bind_addr
    };

    let mut path = dirs::home_dir().expect("home directory");

    
    /*
    let id_path = if matches.is_present("keypair") {
        matches.value_of("keypair").unwrap()
    } else {
        path.extend(&[".config", "solana", "id.json"]);
        path.to_str().unwrap()
    };
    */
    let id_path = if !keypair_str.is_empty() {
        keypair_str
    } else {
        path.extend(&[".config", "solana", "id.json"]);
        path.to_str().unwrap()
    };


    //read the client keypair from id file
    let pkcs8 = read_pkcs8(id_path).expect("client keypair");

    // we need all the receiving sockets to be bound within the expected
    // port range that we open on aws
    let config = Config::new(&bind_addr, pkcs8);
    let stdout = io::stdout();
    serde_json::to_writer(stdout, &config).expect("serialize");
}
