#[no_mangle]
pub extern "C" fn keygen_main_entry() -> Result<(), Box<std::error::Error>> {
    println!("Keymaker!");
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

    let mut path = clap::dirs::home_dir().expect("home directory");
    let outfile = if matches.is_present("outfile") {
        println!("argument outfile is present ");
        matches.value_of("outfile").unwrap()
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
}
