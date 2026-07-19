// tick.rs
// The main game simulation, run once every 16 milliseconds.
// Handles shooting, movement, fuel, respawning, and the win condition.

use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::time;

use shared::protocol::{KILL_LIMIT, ShotEvent};

use crate::config::{
    FUEL_DRAIN, MatchState, OpenCells, PLAYER_SPEED, RESPAWN_SECS, SHOOT_COOLDOWN_MS, SHOOT_RANGE,
    SHOOT_WIDTH, ShotEvents, TICK_MS, TIMEOUT_SECS, WIN_DISPLAY_SECS,
};
use crate::player::{Players, spawn_pos};

pub async fn game_tick(
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

        // drop any player we have not heard from in a while
        players.retain(|addr, p| {
            let alive = p.last_seen.elapsed().as_secs() < TIMEOUT_SECS;
            if !alive {
                tracing::info!("player {} ({}) timed out", p.id, addr);
            }
            alive
        });

        resolve_shots(&mut players, &map, &open, &shot_events_shared).await;
        apply_hits_and_check_winner(&mut players, &match_state).await;
        reset_match_if_display_time_elapsed(&mut players, &map, &open, &match_state).await;
        apply_movement_fuel_and_respawn(&mut players, &map, &open);
    }
}

// Fires a ray for every player currently pressing shoot (and past their
// cooldown), checks it against every other player's position, and records
// a hit or miss. Every shot, hit or not, is recorded as a ShotEvent so
// clients can show the cosmetic flying projectile even on a miss.
async fn resolve_shots(
    players: &mut tokio::sync::MutexGuard<
        '_,
        std::collections::HashMap<std::net::SocketAddr, crate::player::Player>,
    >,
    map: &shared::map::Map,
    open: &OpenCells,
    shot_events_shared: &ShotEvents,
) {
    let now_tick = Instant::now();

    // snapshot the data we need before mutably borrowing players again below
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

    let mut hits: Vec<(u32, u32)> = Vec::new();
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
                break;
            }
            if let Some((target_id, _, _, _, _)) =
                shooter_data.iter().find(|(tid, tx, ty, _, _)| {
                    *tid != *shooter_id
                        && ((rx - tx).powi(2) + (ry - ty).powi(2)).sqrt() < SHOOT_WIDTH
                })
            {
                hit_target = Some(*target_id);
                break;
            }
        }

        if let Some(target_id) = hit_target {
            hits.push((*shooter_id, target_id));
            tracing::info!("player {} hit player {}", shooter_id, target_id);
        }

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

    if !tick_shot_events.is_empty() {
        shot_events_shared.lock().await.extend(tick_shot_events);
    }

    apply_hits(players, map, open, &hits).await;
}

// Splitting hit application out keeps resolve_shots focused on finding
// hits, this part just carries out the consequences.
async fn apply_hits(
    players: &mut tokio::sync::MutexGuard<
        '_,
        std::collections::HashMap<std::net::SocketAddr, crate::player::Player>,
    >,
    map: &shared::map::Map,
    open: &OpenCells,
    hits: &[(u32, u32)],
) {
    for (shooter_id, victim_id) in hits {
        if let Some(shooter) = players.values_mut().find(|p| p.id == *shooter_id) {
            shooter.kills += 1;
            tracing::info!("player {} kills: {}", shooter_id, shooter.kills);
        }

        // Move the victim to a fresh spawn point exactly once, at the
        // moment they are shot. This must not happen again on later ticks
        // while they are still frozen, doing so would send a new random
        // position to every client on every tick for the whole 3 second
        // freeze, which looks like the player rapidly teleporting across
        // the map instead of respawning cleanly in one place.
        let occupied: Vec<(f32, f32)> = players
            .values()
            .filter(|p| p.id != *victim_id)
            .map(|p| (p.x, p.y))
            .collect();
        if let Some(victim) = players.values_mut().find(|p| p.id == *victim_id) {
            victim.respawn_at = Some(Instant::now() + Duration::from_secs(RESPAWN_SECS));
            let (sx, sy, sangle) = crate::player::spawn_pos(map, open, &occupied);
            victim.x = sx;
            victim.y = sy;
            victim.angle = sangle;
            tracing::info!("player {} was shot, respawning", victim_id);
        }
    }
}

// After hits are applied, check whether anyone has reached the kill
// limit. The actual match reset is delayed (see reset_match_if_display_time_elapsed)
// so the win screen has time to show before scores are cleared.
async fn apply_hits_and_check_winner(
    players: &mut tokio::sync::MutexGuard<
        '_,
        std::collections::HashMap<std::net::SocketAddr, crate::player::Player>,
    >,
    match_state: &MatchState,
) {
    let winner_id = players
        .values()
        .find(|p| p.kills >= KILL_LIMIT)
        .map(|p| p.id);

    if let Some(winner) = winner_id {
        let mut ms = match_state.lock().await;
        if ms.is_none() {
            tracing::info!("player {} wins the match!", winner);
            *ms = Some((winner, Instant::now()));
        }
    }
}

// Once the win screen has been showing for WIN_DISPLAY_SECS, reset every
// player's kills to zero and give them a fresh spawn point for the new match.
async fn reset_match_if_display_time_elapsed(
    players: &mut tokio::sync::MutexGuard<
        '_,
        std::collections::HashMap<std::net::SocketAddr, crate::player::Player>,
    >,
    map: &shared::map::Map,
    open: &OpenCells,
    match_state: &MatchState,
) {
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
                    player.respawn(map, open, &occupied);
                }
            }
            *ms = None;
            tracing::info!("match reset, new round starting");
        }
    }
}

// Applies movement from input flags, drains fuel, and handles the
// transition into and out of the respawn freeze.
fn apply_movement_fuel_and_respawn(
    players: &mut tokio::sync::MutexGuard<
        '_,
        std::collections::HashMap<std::net::SocketAddr, crate::player::Player>,
    >,
    map: &shared::map::Map,
    open: &OpenCells,
) {
    for player in players.values_mut() {
        player.just_shot = player.input_shoot;

        // still frozen from a death, check if the freeze has expired
        if let Some(at) = player.respawn_at {
            if Instant::now() >= at {
                tracing::info!("player {} respawning", player.id);
                player.respawn(map, open, &[]);
            }
            continue;
        }

        player.fuel -= FUEL_DRAIN;
        if player.fuel <= 0.0 {
            player.fuel = 0.0;
            player.respawn_at = Some(Instant::now() + Duration::from_secs(RESPAWN_SECS));
            let (sx, sy, sangle) = spawn_pos(map, open, &[]);
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
