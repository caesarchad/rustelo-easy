use bincode::{deserialize, serialize};
use bytes::Bytes;
use clap::{App, Arg};
use buffett::drone::{Drone, DroneRequest, DRONE_PORT};
use buffett::logger;
use buffett::metrics::set_panic_hook;
use buffett::signature::read_keypair;
use std::error;
use std::io;
use std::net::{Ipv4Addr, SocketAddr};
use std::process::exit;
use std::sync::{Arc, Mutex};
use std::thread;
use tokio::net::TcpListener;
use tokio::prelude::*;
use tokio_codec::{BytesCodec, Decoder};

use std::ffi::CStr;
use crate::rustelo_error::RusteloResult;

macro_rules! socketaddr {
    ($ip:expr, $port:expr) => {
        SocketAddr::from((Ipv4Addr::from($ip), $port))
    };
    ($str:expr) => {{
        let a: SocketAddr = $str.parse().unwrap();
        a
    }};
}

#[no_mangle]
//pub extern "C" fn coincaster_main_entry() -> Result<(), Box<error::Error>> {
pub extern "C" fn coincaster_main_entry(parm01_network_ptr:    *const libc::c_char,
                                        parm02_keypair_ptr:    *const libc::c_char,
                                        parm03_slice_ptr:  *const libc::c_char,
                                        parm04_cap_ptr:    *const libc::c_char) -> RusteloResult {
    //set the logger and the panic hooker
    logger::setup();
    set_panic_hook("drone");

    //handle parameters, convert ptr to &str
    let network_str = unsafe { CStr::from_ptr(parm01_network_ptr) }.to_str().unwrap();
    let keypair_str = unsafe { CStr::from_ptr(parm02_keypair_ptr) }.to_str().unwrap();
    let slice_str = unsafe { CStr::from_ptr(parm03_slice_ptr) }.to_str().unwrap();
    let cap_str = unsafe { CStr::from_ptr(parm04_cap_ptr) }.to_str().unwrap();

    /*
    let matches = App::new("drone")
        .version(crate_version!())
        .arg(
            Arg::with_name("network")
                .short("n")
                .long("network")
                .value_name("HOST:PORT")
                .takes_value(true)
                .required(true)
                .help("Rendezvous with the network at this gossip entry point"),
        ).arg(
            Arg::with_name("keypair")
                .short("k")
                .long("keypair")
                .value_name("PATH")
                .takes_value(true)
                .required(true)
                .help("File from which to read the mint's keypair"),
        ).arg(
            Arg::with_name("slice")
                .long("slice")
                .value_name("SECS")
                .takes_value(true)
                .help("Time slice over which to limit requests to drone"),
        ).arg(
            Arg::with_name("cap")
                .long("cap")
                .value_name("NUM")
                .takes_value(true)
                .help("Request limit for time slice"),
        ).get_matches();
    */

    /*
    let network = matches
        .value_of("network")
        .unwrap()
        .parse()
        .unwrap_or_else(|e| {
            eprintln!("failed to parse network: {}", e);
            exit(1)
        });
    */
    //parse the network
    let network = Some(network_str)
        .unwrap()
        .parse()
        .unwrap_or_else(|e| {
            eprintln!("failed to parse network: {}", e);
            exit(1)
        });

    /*
    let mint_keypair =
        read_keypair(matches.value_of("keypair").unwrap()).expect("failed to read client keypair");
    */
    //parse the mint keypair 
    let mint_keypair =
        read_keypair(Some(keypair_str).unwrap()).expect("failed to read client keypair");

    /*
    let time_slice: Option<u64>;
    if let Some(secs) = matches.value_of("slice") {
        time_slice = Some(secs.to_string().parse().expect("failed to parse slice"));
    } else {
        time_slice = None;
    }
    */
    //parse the time slice
    let time_slice: Option<u64>;
    if let Some(secs) = Some(slice_str) {
        time_slice = Some(secs.to_string().parse().expect("failed to parse slice"));
    } else {
        time_slice = None;
    }

    /*
    let request_cap: Option<u64>;
    if let Some(c) = matches.value_of("cap") {
        request_cap = Some(c.to_string().parse().expect("failed to parse cap"));
    } else {
        request_cap = None;
    }
    */
    let request_cap: Option<u64>;
    if let Some(c) = Some(cap_str) {
        request_cap = Some(c.to_string().parse().expect("failed to parse cap"));
    } else {
        request_cap = None;
    }


    let drone_addr = socketaddr!(0, DRONE_PORT);

    let drone = Arc::new(Mutex::new(Drone::new(
        mint_keypair,
        drone_addr,
        network,
        time_slice,
        request_cap,
    )));

    let drone1 = drone.clone();
    thread::spawn(move || loop {
        let time = drone1.lock().unwrap().time_slice;
        thread::sleep(time);
        drone1.lock().unwrap().clear_request_count();
    });

    let socket = TcpListener::bind(&drone_addr).unwrap();
    println!("Drone started. Listening on: {}", drone_addr);
    let done = socket
        .incoming()
        .map_err(|e| println!("failed to accept socket; error = {:?}", e))
        .for_each(move |socket| {
            let drone2 = drone.clone();
            // let client_ip = socket.peer_addr().expect("drone peer_addr").ip();
            let framed = BytesCodec::new().framed(socket);
            let (writer, reader) = framed.split();

            let processor = reader.and_then(move |bytes| {
                /*
                let req: DroneRequest = deserialize(&bytes).or_else(|err| {
                    Err(io::Error::new(
                        io::ErrorKind::Other,
                        format!("deserialize packet in drone: {:?}", err),
                    ))
                })?;
                */
                let req: DroneRequest = deserialize(&bytes).or_else(|err| {
                    Err(io::Error::new(
                        io::ErrorKind::Other,
                        format!("deserialize packet in drone: {:?}", err),
                    ))
                });

                println!("Airdrop requested...");
                // let res = drone2.lock().unwrap().check_rate_limit(client_ip);
                let res1 = drone2.lock().unwrap().send_airdrop(req);
                match res1 {
                    Ok(_) => println!("Airdrop sent!"),
                    Err(_) => println!("Request limit reached for this time slice"),
                }
                
                //let response = res1?; replace ? operator with match
                let response = res1;
                match res1 {
                    Ok(_) => println!("Respond success"),
                    Err(_) => {println!("Respond failure");
                               return RusteloResult::Failure;}
                }

                /*
                println!("Airdrop tx signature: {:?}", response);
                let response_vec = serialize(&response).or_else(|err| {
                    Err(io::Error::new(
                        io::ErrorKind::Other,
                        format!("serialize signature in drone: {:?}", err),
                    ))
                })?;
                let response_bytes = Bytes::from(response_vec.clone());
                Ok(response_bytes)
                */
                println!("Airdrop tx signature: {:?}", response);
                let response_vec = serialize(&response).or_else(|err| {
                    Err(io::Error::new(
                        io::ErrorKind::Other,
                        format!("serialize signature in drone: {:?}", err),
                    ))
                });
                let response_bytes = Bytes::from(response_vec.clone());
                RusteloResult::Success
            });
            let server = writer
                .send_all(processor.or_else(|err| {
                    Err(io::Error::new(
                        io::ErrorKind::Other,
                        format!("Drone response: {:?}", err),
                    ))
                })).then(|_| Ok(()));
            tokio::spawn(server)
        });
    tokio::run(done);
    //Ok(())
    RusteloResult::Success
}
