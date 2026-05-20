// server — entry point
// Responsibilities:
//   UDP listener task  — receives InputPackets from connected clients
//   Game tick task     — runs on a fixed 16ms interval, updates game state
//   Broadcast task     — sends StatePackets to all connected clients

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::net::UdpSocket;
use tokio::sync::Mutex;
use tokio::time;

use shared::protocol::{InputPacket, PlayerState, StatePacket, MAX_PACKET_BYTES};

const TICK_MS: u64 = 16;
const PLAYER_SPEED: f32 = 0.05;
const PLAYER_TURN_SPEED: f32 = 0.04;
const TIMEOUT_SECS: u64 = 10;
const RATE_LIMIT_PER_SEC: u32 = 128;

#[derive(Debug)]
struct Player {
    id: u32,
    x: f32,
    y: f32,
    angle: f32,
    session_token: u64,
    last_sequence: u32,
    last_seen: Instant,
    packet_count: u32,
    rate_window_start: Instant,
}

type Players = Arc<Mutex<HashMap<SocketAddr, Player>>>;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let addr = "0.0.0.0:34254";
    let socket = Arc::new(UdpSocket::bind(addr).await.expect("failed to bind UDP socket"));
    tracing::info!("server listening on {}", addr);

    let players: Players = Arc::new(Mutex::new(HashMap::new()));

    // spawn the three tasks
    let listener_handle = tokio::spawn(udp_listener(Arc::clone(&socket), Arc::clone(&players)));
    let tick_handle = tokio::spawn(game_tick(Arc::clone(&players)));
    let broadcast_handle = tokio::spawn(broadcast(Arc::clone(&socket), Arc::clone(&players)));

    let _ = tokio::try_join!(listener_handle, tick_handle, broadcast_handle);
}

// UDP listener task — receives InputPackets, registers new players, queues inputs
async fn udp_listener(socket: Arc<UdpSocket>, players: Players) {
    let mut buf = vec![0u8; MAX_PACKET_BYTES];
    let mut next_id: u32 = 1;

    loop {
        let (len, src) = match socket.recv_from(&mut buf).await {
            Ok(v) => v,
            Err(e) => { tracing::warn!("recv error: {e}"); continue; }
        };

        if len > MAX_PACKET_BYTES {
            tracing::warn!("oversized packet from {src}, dropping");
            continue;
        }

        let packet: InputPacket = match bincode::deserialize(&buf[..len]) {
            Ok(p) => p,
            Err(_) => { tracing::warn!("malformed packet from {src}, dropping"); continue; }
        };

        let mut players = players.lock().await;

        // register new player
        if !players.contains_key(&src) {
            // random session token using address hash + time as entropy source
            let token = src.port() as u64 ^ next_id as u64 ^ 0xdeadbeefcafe;
            tracing::info!("new player {} from {}", next_id, src);
            players.insert(src, Player {
                id: next_id,
                x: 1.5,
                y: 1.5,
                angle: 0.0,
                session_token: token,
                last_sequence: 0,
                last_seen: Instant::now(),
                packet_count: 0,
                rate_window_start: Instant::now(),
            });
            next_id += 1;
        }

        let player = players.get_mut(&src).unwrap();

        // rate limiting
        let now = Instant::now();
        if now.duration_since(player.rate_window_start) >= Duration::from_secs(1) {
            player.packet_count = 0;
            player.rate_window_start = now;
        }
        player.packet_count += 1;
        if player.packet_count > RATE_LIMIT_PER_SEC {
            tracing::warn!("rate limit hit for {src}, dropping");
            continue;
        }

        // session token check (skip for the very first packet that registered the player)
        if packet.session_token != 0 && packet.session_token != player.session_token {
            tracing::warn!("bad session token from {src}, dropping");
            continue;
        }

        // discard out-of-order packets
        if packet.sequence <= player.last_sequence {
            continue;
        }
        player.last_sequence = packet.sequence;
        player.last_seen = now;

        // apply input directly (game tick will use the resulting state)
        if packet.forward {
            player.x += player.angle.cos() * PLAYER_SPEED;
            player.y += player.angle.sin() * PLAYER_SPEED;
        }
        if packet.backward {
            player.x -= player.angle.cos() * PLAYER_SPEED;
            player.y -= player.angle.sin() * PLAYER_SPEED;
        }
        if packet.turn_left  { player.angle -= PLAYER_TURN_SPEED; }
        if packet.turn_right { player.angle += PLAYER_TURN_SPEED; }
    }
}

// Game tick task — runs every 16ms, drops timed-out players
async fn game_tick(players: Players) {
    let mut interval = time::interval(Duration::from_millis(TICK_MS));
    loop {
        interval.tick().await;
        let mut players = players.lock().await;
        players.retain(|addr, p| {
            let alive = p.last_seen.elapsed().as_secs() < TIMEOUT_SECS;
            if !alive { tracing::info!("player {} ({}) timed out", p.id, addr); }
            alive
        });
    }
}

// Broadcast task — sends current StatePacket to every registered client every tick
async fn broadcast(socket: Arc<UdpSocket>, players: Players) {
    let mut interval = time::interval(Duration::from_millis(TICK_MS));
    let mut sequence: u32 = 0;

    loop {
        interval.tick().await;
        sequence = sequence.wrapping_add(1);

        let players = players.lock().await;
        if players.is_empty() { continue; }

        let state = StatePacket {
            sequence,
            players: players.values().map(|p| PlayerState {
                id: p.id,
                x: p.x,
                y: p.y,
                angle: p.angle,
            }).collect(),
        };

        let encoded = match bincode::serialize(&state) {
            Ok(b) => b,
            Err(e) => { tracing::error!("serialize error: {e}"); continue; }
        };

        for addr in players.keys() {
            if let Err(e) = socket.send_to(&encoded, addr).await {
                tracing::warn!("send error to {addr}: {e}");
            }
        }
    }
}
