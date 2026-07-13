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

use shared::map::get_level;
use shared::protocol::{InputPacket, KILL_LIMIT, MAX_PACKET_BYTES, PlayerState, StatePacket};

use clap::Parser;

const TICK_MS: u64 = 16;
const PLAYER_SPEED: f32 = 0.05;
const TIMEOUT_SECS: u64 = 10;
const RATE_LIMIT_PER_SEC: u32 = 128;
const FUEL_MAX: f32 = 100.0;
// depletes over ~90s at 62.5 ticks/sec
const FUEL_DRAIN: f32 = FUEL_MAX / (90.0 * (1000.0 / TICK_MS as f32));
const RESPAWN_SECS: u64 = 3;

const SHOOT_RANGE: f32 = 10.0;
const SHOOT_WIDTH: f32 = 0.3;

#[derive(Parser, Debug)]
#[command(name = "maze-wars-server")]
struct Args {
    /// Port to listen on
    #[arg(short, long, default_value_t = 34254)]
    port: u16,

    /// Level to load (1, 2, or 3)
    #[arg(short, long, default_value_t = 1)]
    level: u8,
}

#[derive(Debug)]
struct Player {
    id: u32,
    x: f32,
    y: f32,
    angle: f32,
    fuel: f32,
    kills: u32,
    respawn_at: Option<Instant>,
    session_token: u64,
    last_sequence: u32,
    last_seen: Instant,
    packet_count: u32,
    rate_window_start: Instant,

    // input flags -- set by listener, consumed by game tick
    input_forward: bool,
    input_backward: bool,
    input_turn_left: bool,
    input_turn_right: bool,
    input_shoot: bool,
    just_shot: bool,
}

impl Player {
    fn spawn(id: u32, token: u64) -> Self {
        Self {
            id,
            x: 1.5,
            y: 1.5,
            angle: 0.0,
            fuel: FUEL_MAX,
            kills: 0,
            respawn_at: None,
            session_token: token,
            last_sequence: 0,
            last_seen: Instant::now(),
            packet_count: 0,
            rate_window_start: Instant::now(),
            input_forward: false,
            input_backward: false,
            input_turn_left: false,
            input_turn_right: false,
            input_shoot: false,
            just_shot: false,
        }
    }

    fn respawn(&mut self) {
        self.x = 1.5;
        self.y = 1.5;
        self.angle = 0.0;
        self.fuel = FUEL_MAX;
        self.respawn_at = None;
    }
}

type Players = Arc<Mutex<HashMap<SocketAddr, Player>>>;

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

    let players: Players = Arc::new(Mutex::new(HashMap::new()));

    // spawn the three tasks
    let listener_handle = tokio::spawn(udp_listener(Arc::clone(&socket), Arc::clone(&players)));
    let tick_handle = tokio::spawn(game_tick(Arc::clone(&players), Arc::clone(&map)));
    let broadcast_handle = tokio::spawn(broadcast(Arc::clone(&socket), Arc::clone(&players)));

    let _ = tokio::try_join!(listener_handle, tick_handle, broadcast_handle);
}

// UDP listener task — receives InputPackets, registers new players, stores input flags
async fn udp_listener(socket: Arc<UdpSocket>, players: Players) {
    let mut buf = vec![0u8; MAX_PACKET_BYTES];
    let mut next_id: u32 = 1;

    loop {
        let (len, src) = match socket.recv_from(&mut buf).await {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("recv error: {e}");
                continue;
            }
        };

        if len > MAX_PACKET_BYTES {
            tracing::warn!("oversized packet from {src}, dropping");
            continue;
        }

        let packet: InputPacket = match postcard::from_bytes(&buf[..len]) {
            Ok(p) => p,
            Err(_) => {
                tracing::warn!("malformed packet from {src}, dropping");
                continue;
            }
        };

        let mut players = players.lock().await;

        // register new player
        if !players.contains_key(&src) {
            // random session token using address hash + time as entropy source
            let token = src.port() as u64 ^ next_id as u64 ^ 0xdeadbeefcafe;
            tracing::info!("new player {} from {}", next_id, src);
            players.insert(src, Player::spawn(next_id, token));
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

        // session token check
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

        // store input flags -- game tick will apply movement
        player.input_forward = packet.forward;
        player.input_backward = packet.backward;
        player.input_turn_left = packet.turn_left;
        player.input_turn_right = packet.turn_right;
        player.input_shoot = packet.shoot;
        player.angle = packet.angle; // client-authoritative view angle
    }
}

// Game tick task — runs every 16ms, applies movement, checks collisions, drops timeouts
async fn game_tick(players: Players, map: Arc<shared::map::Map>) {
    let mut interval = time::interval(Duration::from_millis(TICK_MS));
    loop {
        interval.tick().await;
        let mut players = players.lock().await;

        // drop timed-out players
        players.retain(|addr, p| {
            let alive = p.last_seen.elapsed().as_secs() < TIMEOUT_SECS;
            if !alive {
                tracing::info!("player {} ({}) timed out", p.id, addr);
            }
            alive
        });

        // collect shooter data before mutably iterating
        let shooter_data: Vec<(u32, f32, f32, f32, bool)> = players
            .values()
            .map(|p| (p.id, p.x, p.y, p.angle, p.input_shoot && !p.just_shot))
            .collect();

        // resolve shots — find hit player ids
        let mut hits: Vec<(u32, u32)> = Vec::new(); // (shooter_id, victim_id)
        for (shooter_id, sx, sy, angle, shooting) in &shooter_data {
            if !shooting {
                continue;
            }
            for (target_id, tx, ty, _, _) in &shooter_data {
                if shooter_id == target_id {
                    continue;
                }
                // step ray forward from shooter
                let mut hit = false;
                let mut rx = *sx;
                let mut ry = *sy;
                let dx = angle.cos();
                let dy = angle.sin();
                let steps = (SHOOT_RANGE / 0.05) as u32;
                for _ in 0..steps {
                    rx += dx * 0.05;
                    ry += dy * 0.05;
                    let ix = rx as i32;
                    let iy = ry as i32;
                    if ix < 0 || iy < 0 || map.is_wall(ix as usize, iy as usize) {
                        break;
                    }
                    let dist = ((rx - tx).powi(2) + (ry - ty).powi(2)).sqrt();
                    if dist < SHOOT_WIDTH {
                        hit = true;
                        break;
                    }
                }
                if hit {
                    hits.push((*shooter_id, *target_id));
                    tracing::info!("player {} hit player {}", shooter_id, target_id);
                    break; // one hit per shot
                }
            }
        }

        // apply hits
        let mut winner_id: Option<u32> = None;
        for (shooter_id, victim_id) in hits {
            // give shooter a kill
            if let Some(shooter) = players.values_mut().find(|p| p.id == shooter_id) {
                shooter.kills += 1;
                tracing::info!("player {} kills: {}", shooter_id, shooter.kills);
                if shooter.kills >= KILL_LIMIT {
                    winner_id = Some(shooter_id);
                }
            }
            // respawn victim
            if let Some(victim) = players.values_mut().find(|p| p.id == victim_id) {
                victim.respawn_at = Some(Instant::now() + Duration::from_secs(RESPAWN_SECS));
                tracing::info!("player {} was shot, respawning", victim_id);
            }
        }

        // reset match if someone won
        if let Some(winner) = winner_id {
            tracing::info!("player {} wins the match!", winner);
            for player in players.values_mut() {
                player.kills = 0;
                player.respawn();
            }
        }

        // apply movement
        for player in players.values_mut() {
            // update just_shot flag
            player.just_shot = player.input_shoot;

            if let Some(at) = player.respawn_at {
                if Instant::now() >= at {
                    tracing::info!("player {} respawning", player.id);
                    player.respawn();
                }
                continue;
            }

            player.fuel -= FUEL_DRAIN;
            if player.fuel <= 0.0 {
                player.fuel = 0.0;
                player.respawn_at = Some(Instant::now() + Duration::from_secs(RESPAWN_SECS));
                tracing::info!(
                    "player {} out of fuel, respawn in {}s",
                    player.id,
                    RESPAWN_SECS
                );
                continue;
            }

            let mut new_x = player.x;
            let mut new_y = player.y;
            if player.input_forward {
                new_x += player.angle.cos() * PLAYER_SPEED;
                new_y += player.angle.sin() * PLAYER_SPEED;
            }
            if player.input_backward {
                new_x -= player.angle.cos() * PLAYER_SPEED;
                new_y -= player.angle.sin() * PLAYER_SPEED;
            }

            let nx = new_x as i32;
            let ny = new_y as i32;
            if nx >= 0 && ny >= 0 && !map.is_wall(nx as usize, ny as usize) {
                player.x = new_x;
                player.y = new_y;
            }
        }
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
        if players.is_empty() {
            continue;
        }

        // build the player list once, reuse for every client
        let player_list: Vec<PlayerState> = players
            .values()
            .map(|p| PlayerState {
                id: p.id,
                x: p.x,
                y: p.y,
                angle: p.angle,
                fuel: p.fuel,
                kills: p.kills,
            })
            .collect();

        // check for winner
        let winner = player_list.iter().find(|p| p.kills >= KILL_LIMIT);
        let match_over = winner.is_some();
        let winner_id = winner.map(|p| p.id).unwrap_or(0);

        // send each client a StatePacket with their own your_id set
        for (addr, player) in players.iter() {
            let state = StatePacket {
                sequence,
                your_id: player.id,
                players: player_list.clone(),
                match_over,
                winner_id,
            };

            let encoded = match postcard::to_allocvec(&state) {
                Ok(b) => b,
                Err(e) => {
                    tracing::error!("serialize error: {e}");
                    continue;
                }
            };
            if let Err(e) = socket.send_to(&encoded, addr).await {
                tracing::warn!("send error to {addr}: {e}");
            }
        }
    }
}
