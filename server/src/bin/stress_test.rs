// stress_test — verifies the server handles 10+ simultaneous UDP connections.
//
// Starts the server in-process on an ephemeral port, connects NUM_CLIENTS
// UDP sockets concurrently, drives them for TEST_SECS seconds, then asserts:
//   1. Every client received at least one StatePacket.
//   2. At least one StatePacket per client listed 10+ players simultaneously.
//
// Run with: cargo run -p server --bin stress_test

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::net::UdpSocket;
use tokio::sync::{Mutex, mpsc};
use tokio::time;

use shared::map::get_level;
use shared::protocol::{InputPacket, MAX_PACKET_BYTES, StatePacket};

const NUM_CLIENTS: usize = 12;
const TEST_SECS: u64 = 4;
const TICK_MS: u64 = 16;
const PLAYER_SPEED: f32 = 0.05;
const PLAYER_TURN_SPEED: f32 = 0.04;
const TIMEOUT_SECS: u64 = 10;
const RATE_LIMIT_PER_SEC: u32 = 128;

// ── Inline server types ───────────────────────────────────────────────────────

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
    input_forward: bool,
    input_backward: bool,
    input_turn_left: bool,
    input_turn_right: bool,
}

type Players = Arc<Mutex<HashMap<SocketAddr, Player>>>;

// ── Inline server tasks (mirrors server/src/main.rs) ─────────────────────────

async fn udp_listener(socket: Arc<UdpSocket>, players: Players) {
    let mut buf = vec![0u8; MAX_PACKET_BYTES];
    let mut next_id: u32 = 1;
    loop {
        let (len, src) = match socket.recv_from(&mut buf).await {
            Ok(v) => v,
            Err(_) => continue,
        };
        let packet: InputPacket = match postcard::from_bytes(&buf[..len]) {
            Ok(p) => p,
            Err(_) => continue,
        };
        let mut players = players.lock().await;
        if !players.contains_key(&src) {
            let token = src.port() as u64 ^ next_id as u64 ^ 0xdeadbeefcafe;
            players.insert(src, Player {
                id: next_id,
                x: 1.5, y: 1.5, angle: 0.0,
                session_token: token,
                last_sequence: 0,
                last_seen: Instant::now(),
                packet_count: 0,
                rate_window_start: Instant::now(),
                input_forward: false, input_backward: false,
                input_turn_left: false, input_turn_right: false,
            });
            next_id += 1;
        }
        let player = players.get_mut(&src).unwrap();
        let now = Instant::now();
        if now.duration_since(player.rate_window_start) >= Duration::from_secs(1) {
            player.packet_count = 0;
            player.rate_window_start = now;
        }
        player.packet_count += 1;
        if player.packet_count > RATE_LIMIT_PER_SEC { continue; }
        if packet.session_token != 0 && packet.session_token != player.session_token { continue; }
        if packet.sequence <= player.last_sequence { continue; }
        player.last_sequence = packet.sequence;
        player.last_seen = now;
        player.input_forward = packet.forward;
        player.input_backward = packet.backward;
        player.input_turn_left = packet.turn_left;
        player.input_turn_right = packet.turn_right;
    }
}

async fn game_tick(players: Players, map: Arc<shared::map::Map>) {
    let mut interval = time::interval(Duration::from_millis(TICK_MS));
    loop {
        interval.tick().await;
        let mut players = players.lock().await;
        players.retain(|_, p| p.last_seen.elapsed().as_secs() < TIMEOUT_SECS);
        for player in players.values_mut() {
            let mut nx = player.x;
            let mut ny = player.y;
            if player.input_forward  { nx += player.angle.cos() * PLAYER_SPEED; ny += player.angle.sin() * PLAYER_SPEED; }
            if player.input_backward { nx -= player.angle.cos() * PLAYER_SPEED; ny -= player.angle.sin() * PLAYER_SPEED; }
            if player.input_turn_left  { player.angle -= PLAYER_TURN_SPEED; }
            if player.input_turn_right { player.angle += PLAYER_TURN_SPEED; }
            let ix = nx as i32;
            let iy = ny as i32;
            if ix >= 0 && iy >= 0 && !map.is_wall(ix as usize, iy as usize) {
                player.x = nx;
                player.y = ny;
            }
        }
    }
}

async fn broadcast(socket: Arc<UdpSocket>, players: Players) {
    let mut interval = time::interval(Duration::from_millis(TICK_MS));
    let mut sequence: u32 = 0;
    loop {
        interval.tick().await;
        sequence = sequence.wrapping_add(1);
        let players = players.lock().await;
        if players.is_empty() { continue; }
        let player_list: Vec<shared::protocol::PlayerState> = players.values()
            .map(|p| shared::protocol::PlayerState { id: p.id, x: p.x, y: p.y, angle: p.angle })
            .collect();
        // stress test broadcasts the same packet to all clients (your_id unused in test)
        for (addr, player) in players.iter() {
            let state = shared::protocol::StatePacket {
                sequence,
                your_id: player.id,
                players: player_list.clone(),
            };
            let encoded = match postcard::to_allocvec(&state) {
                Ok(b) => b,
                Err(_) => continue,
            };
            let _ = socket.send_to(&encoded, addr).await;
        }
    }
}

// ── Client task ───────────────────────────────────────────────────────────────

// Connects to the server, sends InputPackets for the test duration, collects
// the maximum player count seen in any single StatePacket, then reports via
// the result channel.
async fn run_client(
    client_id: usize,
    server_addr: SocketAddr,
    results_tx: mpsc::Sender<(usize, usize)>, // (client_id, max_players_seen)
) {
    let sock = UdpSocket::bind("127.0.0.1:0").await.expect("client bind failed");
    sock.connect(server_addr).await.expect("client connect failed");

    let mut sequence: u32 = 0;
    let mut max_players: usize = 0;
    let mut recv_buf = vec![0u8; MAX_PACKET_BYTES];
    let deadline = tokio::time::Instant::now() + Duration::from_secs(TEST_SECS);

    loop {
        if tokio::time::Instant::now() >= deadline {
            break;
        }

        // send an InputPacket (alternate forward/turn to exercise movement)
        sequence += 1;
        let pkt = InputPacket {
            sequence,
            player_id: 0,
            session_token: 0,
            forward: sequence % 3 != 0,
            backward: false,
            turn_left: sequence % 5 == 0,
            turn_right: sequence % 7 == 0,
        };
        let encoded = postcard::to_allocvec(&pkt).unwrap();
        let _ = sock.send(&encoded).await;

        // drain incoming StatePackets without blocking longer than one tick
        let recv_timeout = Duration::from_millis(TICK_MS);
        match tokio::time::timeout(recv_timeout, sock.recv(&mut recv_buf)).await {
            Ok(Ok(len)) => {
                if let Ok(state) = postcard::from_bytes::<StatePacket>(&recv_buf[..len]) {
                    max_players = max_players.max(state.players.len());
                }
            }
            _ => {}
        }
    }

    let _ = results_tx.send((client_id, max_players)).await;
}

// ── Main ──────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    println!("stress_test — {} simultaneous clients, {} second run", NUM_CLIENTS, TEST_SECS);

    // bind server to an ephemeral port (OS picks it)
    let server_socket = Arc::new(
        UdpSocket::bind("127.0.0.1:0").await.expect("server bind failed"),
    );
    let server_addr: SocketAddr = server_socket.local_addr().unwrap();
    println!("server listening on {}", server_addr);

    let map = Arc::new(get_level(1));
    let players: Players = Arc::new(Mutex::new(HashMap::new()));

    // start the three server tasks
    tokio::spawn(udp_listener(Arc::clone(&server_socket), Arc::clone(&players)));
    tokio::spawn(game_tick(Arc::clone(&players), Arc::clone(&map)));
    tokio::spawn(broadcast(Arc::clone(&server_socket), Arc::clone(&players)));

    // small delay to let the server tasks start
    tokio::time::sleep(Duration::from_millis(50)).await;

    // launch all clients
    let (tx, mut rx) = mpsc::channel::<(usize, usize)>(NUM_CLIENTS);
    for id in 0..NUM_CLIENTS {
        let tx = tx.clone();
        tokio::spawn(run_client(id, server_addr, tx));
    }
    drop(tx); // close sender side so rx drains cleanly

    // collect results
    let mut results: Vec<(usize, usize)> = Vec::with_capacity(NUM_CLIENTS);
    while let Some(r) = rx.recv().await {
        results.push(r);
    }
    results.sort_by_key(|(id, _)| *id);

    // ── Report ────────────────────────────────────────────────────────────────
    println!("\n{:<10} {}", "CLIENT", "MAX PLAYERS SEEN");
    println!("{}", "-".repeat(30));
    let mut all_received = true;
    let mut all_saw_10 = true;
    for (id, max) in &results {
        let received = *max > 0;
        let saw_10 = *max >= 10;
        if !received { all_received = false; }
        if !saw_10   { all_saw_10 = false; }
        println!("  {:>2}          {}  {}",
            id,
            max,
            match (received, saw_10) {
                (false, _) => "FAIL — no packets received",
                (true, false) => "WARN — never saw 10 players",
                (true, true)  => "OK",
            }
        );
    }

    println!("\n{}", "-".repeat(30));
    let passed = all_received && all_saw_10;
    if passed {
        println!("PASS — all {} clients connected and saw 10+ simultaneous players", NUM_CLIENTS);
    } else {
        if !all_received { println!("FAIL — one or more clients received no packets"); }
        if !all_saw_10   { println!("FAIL — one or more clients never saw 10 players in a StatePacket"); }
        std::process::exit(1);
    }
}
