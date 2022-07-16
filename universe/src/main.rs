use aw_core::*;

mod client;
pub use client::{Client, ClientType};
mod universe_server;
pub use universe_server::UniverseServer;
pub mod attributes;
pub mod universe_license;
pub use attributes::send_attributes;
pub mod config;
mod database;
pub mod packet_handler;
pub mod player;
pub mod system;
pub mod world;

use env_logger::Builder;
pub use log::{debug, error, info, trace, warn};

use clap::Parser;

#[derive(Parser, Debug)]
struct Args {
    #[clap(long, value_parser, default_value_t = log::LevelFilter::Info)]
    /// Verbosity of logging: <off | error | warn | info | debug | trace>
    log_level: log::LevelFilter,
}

fn init_logging(level: log::LevelFilter) {
    let mut builder = Builder::new();
    builder.filter_level(level);
    builder.init();
}

fn main() {
    let args = Args::parse();
    init_logging(args.log_level);

    match config::Config::get() {
        Ok(config) => {
            start_universe(config);
        }
        Err(err) => {
            eprintln!("Could not get universe configuration: {err}");
        }
    }
}

fn start_universe(config: config::Config) {
    match UniverseServer::new(config) {
        Ok(mut universe) => {
            universe.run();
        }
        Err(err) => {
            eprintln!("Could not create universe: {err}");
        }
    }
}
