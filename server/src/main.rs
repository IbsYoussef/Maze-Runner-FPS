// main.rs
// Server entry point.
//
// The server runs three tasks side by side, all sharing the same player
// list through an Arc<Mutex<...>>:
//   listener   receives input packets from clients and updates player state
//   tick       runs the game simulation every 16 milliseconds
//   broadcast  sends the current state to every client every tick

mod args;
mod broadcast;
mod config;
mod listener;
mod player;
mod tick;

use std::collections::HashMap;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::Mutex;

use clap::Parser;
use shared::map::get_level;

use args::Args;
use config::{MatchState, OpenCells, ShotEvents};
use player::{Players, open_cells};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let args = Args::parse();
    let addr = format!("0.0.0.0:{}", args.port);
    let socket = Arc::new(
        UdpSocket::bind(&addr)
            .await
            .expect("failed to bind UDP socket"),
    );
    tracing::info!("server listening on {}", addr);

    let map = Arc::new(get_level(args.level));
    tracing::info!("loaded level {}", args.level);

    let open: OpenCells = Arc::new(open_cells(&map));
    tracing::info!("{} open floor cells for spawning", open.len());

    let players: Players = Arc::new(Mutex::new(HashMap::new()));
    let match_state: MatchState = Arc::new(Mutex::new(None));
    let shot_events: ShotEvents = Arc::new(Mutex::new(Vec::new()));

    let listener_handle = tokio::spawn(listener::udp_listener(
        Arc::clone(&socket),
        Arc::clone(&players),
        Arc::clone(&map),
        Arc::clone(&open),
    ));

    let tick_handle = tokio::spawn(tick::game_tick(
        Arc::clone(&players),
        Arc::clone(&map),
        Arc::clone(&open),
        Arc::clone(&match_state),
        Arc::clone(&shot_events),
    ));

    let broadcast_handle = tokio::spawn(broadcast::broadcast(
        Arc::clone(&socket),
        Arc::clone(&players),
        Arc::clone(&match_state),
        Arc::clone(&shot_events),
        args.level,
    ));

    let _ = tokio::try_join!(listener_handle, tick_handle, broadcast_handle);
}
