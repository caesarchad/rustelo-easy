use clap::{crate_version, App, Arg};

fn main() {
    soros_logger::setup();
    let matches = App::new("soros-ip-address")
        .version(crate_version!())
        .arg(
            Arg::with_name("host_port")
                .index(1)
                .required(true)
                .help("Host:port to connect to"),
        )
        .get_matches();

    let host_port = matches.value_of("host_port").unwrap();
    let addr = soros_netutil::parse_host_port(host_port)
        .unwrap_or_else(|_| panic!("failed to parse {}", host_port));

    match soros_netutil::get_public_ip_addr(&addr) {
        Ok(ip) => println!("{}", ip),
        Err(err) => {
            eprintln!("{}: {}", addr, err);
            std::process::exit(1)
        }
    }
}
