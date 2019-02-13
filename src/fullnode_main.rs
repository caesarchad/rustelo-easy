use clap::{App, Arg};
use buffett::client::mk_client;
use buffett::crdt::Node;
use buffett::drone::DRONE_PORT;
use buffett::fullnode::{Config, Fullnode, FullnodeReturnType};
use buffett::logger;
use buffett::metrics::set_panic_hook;
use buffett::signature::{Keypair, KeypairUtil};
use buffett::thin_client::poll_gossip_for_leader;
use buffett::wallet::request_airdrop;
use std::fs::File;
use std::net::{Ipv4Addr, SocketAddr};
use std::process::exit;
use std::thread::sleep;
use std::time::Duration;

use std::ffi::c_void; //use ffi c_void
use std::ffi::CStr;
use crate::rustelo_error::RusteloResult;

#[no_mangle]
pub extern "C" fn fullnode_main_entry(parm01_identity_ptr: *const libc::c_char,
                                      parm02_network_ptr: *const libc::c_char,
                                      parm03_ledger_ptr: *const libc::c_char){

    //setup log and pannic hook                                   
    logger::setup();
    set_panic_hook("fullnode");

    //handle parameters, convert ptr to &str
    let identity_str =  unsafe { CStr::from_ptr(parm01_identity_ptr) }.to_str().unwrap(); 
    let network_str =  unsafe { CStr::from_ptr(parm02_network_ptr) }.to_str().unwrap(); 
    let ledger_str =  unsafe { CStr::from_ptr(parm03_ledger_ptr) }.to_str().unwrap(); 
    
    println!("identity:{:?}", identity_str);
    println!("network:{:?}", network_str);
    println!("ledger:{:?}", ledger_str);
    
    /*let matches = App::new("fullnode")
        .version(crate_version!())
        .arg(
            Arg::with_name("identity")
                .short("i")
                .long("identity")
                .value_name("PATH")
                .takes_value(true)
                .help("Run with the identity found in FILE"),
        ).arg(
            Arg::with_name("network")
                .short("n")
                .long("network")
                .value_name("HOST:PORT")
                .takes_value(true)
                .help("Rendezvous with the network at this gossip entry point"),
        ).arg(
            Arg::with_name("ledger")
                .short("l")
                .long("ledger")
                .value_name("DIR")
                .takes_value(true)
                .required(true)
                .help("use DIR as persistent ledger location"),
        ).get_matches();*/

    //part the keypair and the network control plane
    /*
    let (keypair, ncp) = if let Some(i) = matches.value_of("identity") {
        let path = i.to_string();
        if let Ok(file) = File::open(path.clone()) {
            let parse: serde_json::Result<Config> = serde_json::from_reader(file);
            if let Ok(data) = parse {
                (data.keypair(), data.node_info.contact_info.ncp)
            } else {
                eprintln!("failed to parse {}", path);
                exit(1);
            }
        } else {
            eprintln!("failed to read {}", path);
            exit(1);
        }
    } else {
        (Keypair::new(), socketaddr!(0, 8000))
    };
    */
    let (keypair, ncp) = if let Some(i) = Some(identity_str) {
        let path = i.to_string();
        if let Ok(file) = File::open(path.clone()) {
            let parse: serde_json::Result<Config> = serde_json::from_reader(file);
            if let Ok(data) = parse {
                (data.keypair(), data.node_info.contact_info.ncp)
            } else {
                eprintln!("failed to parse {}", path);
                exit(1);
            }
        } else {
            eprintln!("failed to read {}", path);
            exit(1);
        }
    } else {
        (Keypair::new(), socketaddr!(0, 8000))
    };

    //let ledger_path = matches.value_of("ledger").unwrap();
    let ledger_path = ledger_str;

    // socketaddr that is initial pointer into the network's gossip (ncp)
    /*
    let network = matches
        .value_of("network")
        .map(|network| network.parse().expect("failed to parse network address"));
    */
    let network = Some(network_str)
        .map(|network| network.parse().expect("failed to parse network address"));

    let node = Node::new_with_external_ip(keypair.pubkey(), &ncp);

    // save off some stuff for airdrop
    let node_info = node.info.clone();
    let pubkey = keypair.pubkey();

    let mut fullnode = Fullnode::new(node, ledger_path, keypair, network, false, None);

    // airdrop stuff, probably goes away at some point
    let leader = match network {
        Some(network) => {
            poll_gossip_for_leader(network, None).expect("can't find leader on network")
        }
        None => node_info,
    };

    let mut client = mk_client(&leader);

    // TODO: maybe have the drone put itself in gossip somewhere instead of hardcoding?
    let drone_addr = match network {
        Some(network) => SocketAddr::new(network.ip(), DRONE_PORT),
        None => SocketAddr::new(ncp.ip(), DRONE_PORT),
    };

    loop {
        let balance = client.poll_get_balance(&pubkey).unwrap_or(0);
        info!("balance is {}", balance);

        if balance >= 50 {
            info!("good to go!");
            break;
        }

        info!("requesting airdrop from {}", drone_addr);
        loop {
            if request_airdrop(&drone_addr, &pubkey, 50).is_ok() {
                break;
            }
            info!(
                "airdrop request, is the drone address correct {:?}, drone running?",
                drone_addr
            );
            sleep(Duration::from_secs(2));
        }
    }

    loop {
        let status = fullnode.handle_role_transition();
        match status {
            Ok(Some(FullnodeReturnType::LeaderRotation)) => (),
            _ => {
                // Fullnode tpu/tvu exited for some unexpected
                // reason, so exit
                exit(1);
            }
        }
    }
}
