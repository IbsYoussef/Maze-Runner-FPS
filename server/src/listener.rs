// listener.rs
// Listens for incoming UDP packets from clients and updates player state.
// Runs as one of the three background tasks alongside the game tick and broadcast.

use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::UdpSocket;

use shared::protocol::{InputPacket, MAX_PACKET_BYTES};

use crate::config::{OpenCells, RATE_LIMIT_PER_SEC};
use crate::player::{Player, Players};

pub async fn udp_listener(
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
                // Windows raises this error (WSAECONNRESET, os error 10054)
                // when an earlier send to a now-disconnected client bounces
                // back. It is not a real problem, the timeout cleanup in the
                // game tick already handles the disconnected player, so we
                // just ignore this specific error instead of logging noise.
                if e.raw_os_error() != Some(10054) {
                    tracing::warn!("recv error: {e}");
                }
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

        // register a brand new player the first time we hear from them
        if !players.contains_key(&src) {
            let token = src.port() as u64 ^ next_id as u64 ^ 0xdeadbeefcafe;
            let occupied: Vec<(f32, f32)> = players.values().map(|p| (p.x, p.y)).collect();
            tracing::info!("new player {} from {}", next_id, src);
            players.insert(src, Player::spawn(next_id, token, &map, &open, &occupied));
            next_id += 1;
        }

        let player = players.get_mut(&src).unwrap();

        // the client sends its username once, on its very first packet
        if !packet.username.is_empty() && player.username.is_empty() {
            player.username = packet.username.clone();
            tracing::info!("player {} identified as '{}'", player.id, player.username);
        }

        // rate limiting, resets the count once per second
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

        // reject a packet claiming the wrong session token for this player
        if packet.session_token != 0 && packet.session_token != player.session_token {
            tracing::warn!("bad session token from {src}, dropping");
            continue;
        }

        // discard any packet that arrived out of order
        if packet.sequence <= player.last_sequence {
            continue;
        }
        player.last_sequence = packet.sequence;
        player.last_seen = now;

        // store the input flags, the game tick task applies movement from these
        player.input_forward = packet.forward;
        player.input_backward = packet.backward;
        player.input_turn_left = packet.turn_left;
        player.input_turn_right = packet.turn_right;
        player.input_shoot = packet.shoot;

        // The client reports its own position and view angle.
        // We only accept it if the player has actually spawned (x is -1
        // until then) and is not currently dead and waiting to respawn.
        // This stops a stale packet from overwriting the server's own
        // spawn placement during the brief window right after a death.
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
