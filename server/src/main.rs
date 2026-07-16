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
use shared::protocol::{
    InputPacket, KILL_LIMIT, MAX_PACKET_BYTES, PlayerState, ShotEvent, StatePacket,
};

use clap::Parser;

const TICK_MS: u64 = 16;
const PLAYER_SPEED: f32 = 0.05;
const TIMEOUT_SECS: u64 = 5;
const RATE_LIMIT_PER_SEC: u32 = 128;
const FUEL_MAX: f32 = 100.0;
// depletes over ~90s at 62.5 ticks/sec
const FUEL_DRAIN: f32 = FUEL_MAX / (90.0 * (1000.0 / TICK_MS as f32));
const RESPAWN_SECS: u64 = 3;

const SHOOT_RANGE: f32 = 10.0;
const SHOOT_WIDTH: f32 = 0.3;
const WIN_DISPLAY_SECS: u64 = 4;
const SHOOT_COOLDOWN_MS: u64 = 250; // minimum gap between shots, click as fast as you want

type MatchState = Arc<Mutex<Option<(u32, Instant)>>>; // (winner_id, won_at)
type ShotEvents = Arc<Mutex<Vec<ShotEvent>>>;
type OpenCells = Arc<Vec<(usize, usize)>>;

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
    last_shot_at: Option<Instant>,
}

// collect every open floor cell once — reused for every spawn/respawn
fn open_cells(map: &shared::map::Map) -> Vec<(usize, usize)> {
    let mut cells = Vec::new();
    for gy in 0..map.height {
        for gx in 0..map.width {
            if !map.is_wall(gx, gy) {
                cells.push((gx, gy));
            }
        }
    }
    cells
}

// pick a random open cell, preferring one far from existing players (anti-camping,
// scales to any lobby size up to the brief's 10 players); facing is chosen the
// same way as before — an open cardinal direction pointing back toward the maze
// centre, so nobody ever spawns staring point-blank into a wall
fn spawn_pos(
    map: &shared::map::Map,
    open: &[(usize, usize)],
    occupied: &[(f32, f32)],
) -> (f32, f32, f32) {
    use rand::RngExt;
    use std::f32::consts::{FRAC_PI_2, PI};

    let mut rng = rand::rng();

    // sample a handful of candidates, keep the one furthest from any live player
    let mut best_cell = open[rng.random_range(0..open.len())];
    let mut best_dist = f32::MIN;
    for _ in 0..8 {
        let candidate = open[rng.random_range(0..open.len())];
        let (cx, cy) = (candidate.0 as f32 + 0.5, candidate.1 as f32 + 0.5);
        let min_dist = occupied
            .iter()
            .map(|(ox, oy)| (cx - ox).powi(2) + (cy - oy).powi(2))
            .fold(f32::MAX, f32::min);
        if min_dist > best_dist {
            best_dist = min_dist;
            best_cell = candidate;
        }
    }

    let x = best_cell.0 as f32 + 0.5;
    let y = best_cell.1 as f32 + 0.5;

    // candidate facings: (yaw, forward dx, forward dy) — forward = (sin yaw, cos yaw)
    let candidates: [(f32, f32, f32); 4] = [
        (0.0, 0.0, 1.0),         // +y
        (FRAC_PI_2, 1.0, 0.0),   // +x
        (PI, 0.0, -1.0),         // -y
        (-FRAC_PI_2, -1.0, 0.0), // -x
    ];

    let (cx, cy) = (8.0 - x, 8.0 - y); // rough direction to centre
    let mut best_yaw = 0.0f32;
    let mut best_score = f32::MIN;
    for (yaw, dx, dy) in candidates {
        let nx = (x + dx) as i32;
        let ny = (y + dy) as i32;
        if nx < 0 || ny < 0 || map.is_wall(nx as usize, ny as usize) {
            continue; // that way is a wall — never face it
        }
        let score = dx * cx + dy * cy; // dot product: prefer centre-ward
        if score > best_score {
            best_score = score;
            best_yaw = yaw;
        }
    }
    (x, y, best_yaw)
}

impl Player {
    fn spawn(
        id: u32,
        token: u64,
        map: &shared::map::Map,
        open: &[(usize, usize)],
        occupied: &[(f32, f32)],
    ) -> Self {
        let (x, y, angle) = spawn_pos(map, open, occupied);
        Self {
            id,
            x,
            y,
            angle,
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
            last_shot_at: None,
        }
    }

    fn respawn(
        &mut self,
        map: &shared::map::Map,
        open: &[(usize, usize)],
        occupied: &[(f32, f32)],
    ) {
        let (x, y, angle) = spawn_pos(map, open, occupied);
        self.x = x;
        self.y = y;
        self.angle = angle;
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

    let open: OpenCells = Arc::new(open_cells(&map));
    tracing::info!("{} open floor cells for spawning", open.len());

    let players: Players = Arc::new(Mutex::new(HashMap::new()));
    let match_state: MatchState = Arc::new(Mutex::new(None));
    let shot_events: ShotEvents = Arc::new(Mutex::new(Vec::new()));

    // spawn the three tasks
    let listener_handle = tokio::spawn(udp_listener(
        Arc::clone(&socket),
        Arc::clone(&players),
        Arc::clone(&map),
        Arc::clone(&open),
    ));
    let tick_handle = tokio::spawn(game_tick(
        Arc::clone(&players),
        Arc::clone(&map),
        Arc::clone(&open),
        Arc::clone(&match_state),
        Arc::clone(&shot_events),
    ));
    let broadcast_handle = tokio::spawn(broadcast(
        Arc::clone(&socket),
        Arc::clone(&players),
        Arc::clone(&match_state),
        Arc::clone(&shot_events),
    ));

    let _ = tokio::try_join!(listener_handle, tick_handle, broadcast_handle);
}

// UDP listener task — receives InputPackets, registers new players, stores input flags
async fn udp_listener(
    socket: Arc<UdpSocket>,
    players: Players,
    map: Arc<shared::map::Map>,
    open: OpenCells,
) {
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
            let occupied: Vec<(f32, f32)> = players.values().map(|p| (p.x, p.y)).collect();
            tracing::info!("new player {} from {}", next_id, src);
            players.insert(src, Player::spawn(next_id, token, &map, &open, &occupied));
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

        // client-authoritative position AND angle (reject while the client
        // hasn't spawned yet — x is -1 — or while dead/respawning, so the
        // server's assigned spawn position and facing survive the race)
        let px = packet.x as i32;
        let py = packet.y as i32;
        if player.respawn_at.is_none()
            && px >= 0
            && py >= 0
            && !map.is_wall(px as usize, py as usize)
        {
            player.x = packet.x;
            player.y = packet.y;
            player.angle = packet.angle;
        }
    }
}

// Game tick task — runs every 16ms, applies movement, checks collisions, drops timeouts
async fn game_tick(
    players: Players,
    map: Arc<shared::map::Map>,
    open: OpenCells,
    match_state: MatchState,
    shot_events_shared: ShotEvents,
) {
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

        // collect shooter data before mutably iterating —
        // respawning (dead) players are excluded so they can neither
        // shoot nor be shot while invulnerable
        let now_tick = Instant::now();
        let shooter_data: Vec<(u32, f32, f32, f32, bool)> = players
            .values()
            .filter(|p| p.respawn_at.is_none())
            .map(|p| {
                let can_fire = p.input_shoot
                    && p.last_shot_at
                        .map(|t| now_tick.duration_since(t).as_millis() as u64 >= SHOOT_COOLDOWN_MS)
                        .unwrap_or(true);
                (p.id, p.x, p.y, p.angle, can_fire)
            })
            .collect();

        // resolve shots — one ray per shooting player, checked against all targets
        let mut hits: Vec<(u32, u32)> = Vec::new(); // (shooter_id, victim_id)
        let mut tick_shot_events: Vec<ShotEvent> = Vec::new();

        for (shooter_id, sx, sy, angle, shooting) in &shooter_data {
            if !shooting {
                continue;
            }
            if let Some(shooter) = players.values_mut().find(|p| p.id == *shooter_id) {
                shooter.last_shot_at = Some(now_tick);
            }

            let dx = angle.sin();
            let dy = angle.cos();
            let mut rx = *sx;
            let mut ry = *sy;
            let steps = (SHOOT_RANGE / 0.05) as u32;
            let mut hit_target: Option<u32> = None;

            for _ in 0..steps {
                rx += dx * 0.05;
                ry += dy * 0.05;
                let ix = rx as i32;
                let iy = ry as i32;
                if ix < 0 || iy < 0 || map.is_wall(ix as usize, iy as usize) {
                    break; // ray stops at a wall — rx, ry is the endpoint
                }
                // check against every other player at this point along the ray
                if let Some((target_id, _, _, _, _)) =
                    shooter_data.iter().find(|(tid, tx, ty, _, _)| {
                        *tid != *shooter_id
                            && ((rx - tx).powi(2) + (ry - ty).powi(2)).sqrt() < SHOOT_WIDTH
                    })
                {
                    hit_target = Some(*target_id);
                    break; // ray stops at the player it hit
                }
            }

            if let Some(target_id) = hit_target {
                hits.push((*shooter_id, target_id));
                tracing::info!("player {} hit player {}", shooter_id, target_id);
            }

            // record the shot for cosmetic FX regardless of hit or miss
            tick_shot_events.push(ShotEvent {
                shooter_id: *shooter_id,
                shooter_x: *sx,
                shooter_y: *sy,
                shooter_angle: *angle,
                hit_x: rx,
                hit_y: ry,
                hit: hit_target.is_some(),
            });
        }

        // accumulate this tick's shot events — broadcast drains them, so multiple
        // game_tick iterations between broadcast reads won't clobber each other
        if !tick_shot_events.is_empty() {
            shot_events_shared.lock().await.extend(tick_shot_events);
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
            // respawn victim — move them to a random open cell immediately so
            // the position they broadcast while dead is the new spawn, not
            // the death spot (and not always the same corner)
            let occupied: Vec<(f32, f32)> = players
                .values()
                .filter(|p| p.id != victim_id)
                .map(|p| (p.x, p.y))
                .collect();
            if let Some(victim) = players.values_mut().find(|p| p.id == victim_id) {
                victim.respawn_at = Some(Instant::now() + Duration::from_secs(RESPAWN_SECS));
                let (sx, sy, sangle) = spawn_pos(&map, &open, &occupied);
                victim.x = sx;
                victim.y = sy;
                victim.angle = sangle;
                tracing::info!("player {} was shot, respawning", victim_id);
            }
        }

        // record the win — actual reset happens after WIN_DISPLAY_SECS
        if let Some(winner) = winner_id {
            let mut ms = match_state.lock().await;
            if ms.is_none() {
                tracing::info!("player {} wins the match!", winner);
                *ms = Some((winner, Instant::now()));
            }
        }

        // after the display window, reset for a new match
        {
            let mut ms = match_state.lock().await;
            if let Some((_, won_at)) = *ms {
                if won_at.elapsed().as_secs() >= WIN_DISPLAY_SECS {
                    let ids: Vec<u32> = players.values().map(|p| p.id).collect();
                    for id in ids {
                        let occupied: Vec<(f32, f32)> = players
                            .values()
                            .filter(|p| p.id != id)
                            .map(|p| (p.x, p.y))
                            .collect();
                        if let Some(player) = players.values_mut().find(|p| p.id == id) {
                            player.kills = 0;
                            player.respawn(&map, &open, &occupied);
                        }
                    }
                    *ms = None;
                    tracing::info!("match reset — new round starting");
                }
            }
        }

        // apply movement
        for player in players.values_mut() {
            // update just_shot flag
            player.just_shot = player.input_shoot;

            if let Some(at) = player.respawn_at {
                if Instant::now() >= at {
                    tracing::info!("player {} respawning", player.id);
                    // no other-players context needed here for occupied avoidance;
                    // keep it simple — this path is timer expiry, not the death moment
                    player.respawn(&map, &open, &[]);
                }
                continue;
            }

            player.fuel -= FUEL_DRAIN;
            if player.fuel <= 0.0 {
                player.fuel = 0.0;
                player.respawn_at = Some(Instant::now() + Duration::from_secs(RESPAWN_SECS));
                let (sx, sy, sangle) = spawn_pos(&map, &open, &[]);
                player.x = sx;
                player.y = sy;
                player.angle = sangle;
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
                new_x += player.angle.sin() * PLAYER_SPEED;
                new_y += player.angle.cos() * PLAYER_SPEED;
            }

            if player.input_backward {
                new_x -= player.angle.sin() * PLAYER_SPEED;
                new_y -= player.angle.cos() * PLAYER_SPEED;
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
async fn broadcast(
    socket: Arc<UdpSocket>,
    players: Players,
    match_state: MatchState,
    shot_events: ShotEvents,
) {
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
                respawning: p.respawn_at.is_some(),
            })
            .collect();

        // read the held win state instead of recomputing from live kills —
        // kills are already reset by the time WIN_DISPLAY_SECS elapses,
        // so this is the only way clients ever see match_over = true
        let (match_over, winner_id) = match *match_state.lock().await {
            Some((wid, _)) => (true, wid),
            None => (false, 0),
        };

        // drain (not just clone) — each event must be sent exactly once, never
        // re-sent on the next broadcast and never silently lost between ticks
        let events: Vec<shared::protocol::ShotEvent> = {
            let mut guard = shot_events.lock().await;
            std::mem::take(&mut *guard)
        };

        // send each client a StatePacket with their own your_id set
        for (addr, player) in players.iter() {
            let state = StatePacket {
                sequence,
                your_id: player.id,
                players: player_list.clone(),
                match_over,
                winner_id,
                shot_events: events.clone(),
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
