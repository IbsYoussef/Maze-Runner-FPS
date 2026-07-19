// broadcast.rs
// Sends the current game state to every connected client, once every tick.

use std::sync::Arc;
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio::time;

use shared::protocol::{PlayerState, StatePacket};

use crate::config::{MatchState, ShotEvents, TICK_MS};
use crate::player::Players;

pub async fn broadcast(
    socket: Arc<UdpSocket>,
    players: Players,
    match_state: MatchState,
    shot_events: ShotEvents,
    level: u8,
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

        // build the player list once and reuse it for every client
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
                username: p.username.clone(),
            })
            .collect();

        // We read the held win state here rather than recomputing it from
        // live kills, because by the time WIN_DISPLAY_SECS has elapsed the
        // kills have already been reset. This held state is the only way
        // clients ever actually see match_over as true.
        let (match_over, winner_id) = match *match_state.lock().await {
            Some((wid, _)) => (true, wid),
            None => (false, 0),
        };

        // Draining (not just cloning) the shot events matters: each event
        // must be delivered exactly once. If we only cloned them, the same
        // event could be sent again on the next broadcast, and if we never
        // cleared them, a backlog would build up forever.
        let events: Vec<shared::protocol::ShotEvent> = {
            let mut guard = shot_events.lock().await;
            std::mem::take(&mut *guard)
        };

        for (addr, player) in players.iter() {
            let state = StatePacket {
                sequence,
                your_id: player.id,
                players: player_list.clone(),
                match_over,
                winner_id,
                shot_events: events.clone(),
                level,
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
