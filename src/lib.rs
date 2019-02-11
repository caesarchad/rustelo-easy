extern crate libc;
#[macro_use]
extern crate clap;
extern crate getopts;
#[macro_use]
extern crate log;
extern crate serde_json;
#[macro_use]
extern crate buffett;
extern crate dirs;
extern crate ring;
extern crate bincode;
extern crate bytes;
extern crate tokio;
extern crate tokio_codec;
extern crate influx_db_client;
extern crate rayon;

#[macro_use]
pub mod benchcaster_main;
pub mod benchmarker_main;
#[macro_use]
pub mod coincaster_main;
pub mod fullnode_config_main;
pub mod fullnode_main;
#[macro_use]
pub mod genesis_main;
#[macro_use]
pub mod keygen_main;
pub mod ledgertool_main;
pub mod propagator_main;
pub mod upload_enhancer_main;
#[macro_use]
pub mod wallet_main;
pub mod rustelo_error;
#[macro_use]
pub mod macros;


#[macro_export]
macro_rules! try_ffi {
    ($expr:expr) => {
        match $expr {
            Ok(expr) => expr,
            Err(err) => {
                //crate::rustelo_error::ERROR
                //    .lock()
                //    .replace(failure::Error::from(err));
                println!("Expr error when running {:?}", err);
                return crate::rustelo_error::RusteloResult::Failure;
            }
            
        }
    };
}