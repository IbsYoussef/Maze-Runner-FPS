// args.rs
// Command line arguments the server accepts on startup.

use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "maze-wars-server")]
pub struct Args {
    /// Port to listen on
    #[arg(short, long, default_value_t = 34254)]
    pub port: u16,

    /// Level to load (1, 2, or 3)
    #[arg(short, long, default_value_t = 1)]
    pub level: u8,
}
