// bot — simulated player for testing multiplayer features.
// Connects to the server as a UDP client and wanders the maze automatically.
// Strategy: walk forward; when stuck against a wall, turn right and try again.
//
// Run with: cargo run -p client --bin bot -- --server 127.0.0.1:34254

use std::net::UdpSocket;
use std::time::{Duration, Instant};

use clap::Parser;
use shared::protocol::{InputPacket, StatePacket, MAX_PACKET_BYTES};

#[derive(Parser, Debug)]
#[command(name = "bot")]
struct Args {
    #[arg(short, long, default_value = "127.0.0.1:34254")]
    server: String,
}

#[derive(Debug, PartialEq)]
enum BotState {
    Walking,
    Turning { ticks_left: u32 },
}

fn main() {
    let args = Args::parse();

    let socket = UdpSocket::bind("0.0.0.0:0").expect("bind failed");
    socket.connect(&args.server).expect("connect failed");
    socket.set_read_timeout(Some(Duration::from_millis(2))).unwrap();

    println!("bot connected to {}", args.server);

    let tick       = Duration::from_millis(16);
    let mut seq    = 0u32;
    let mut buf    = vec![0u8; MAX_PACKET_BYTES];
    let mut state  = BotState::Walking;
    let mut last_x = 0.0f32;
    let mut last_y = 0.0f32;
    let mut stuck_ticks = 0u32;
    let mut my_id: Option<u32> = None;

    loop {
        let t0 = Instant::now();
        seq += 1;

        let (forward, turn_right) = match &mut state {
            BotState::Walking => (true, false),
            BotState::Turning { ticks_left } => {
                *ticks_left -= 1;
                if *ticks_left == 0 { state = BotState::Walking; }
                (false, true)
            }
        };

        let pkt = InputPacket {
            sequence: seq, player_id: 0, session_token: 0,
            forward, backward: false, turn_left: false, turn_right,
        };
        if let Ok(enc) = postcard::to_allocvec(&pkt) { let _ = socket.send(&enc); }

        // receive latest state
        if let Ok(len) = socket.recv(&mut buf) {
            if let Ok(server_state) = postcard::from_bytes::<StatePacket>(&buf[..len]) {
                if my_id.is_none() { my_id = Some(server_state.your_id); }

                if let Some(id) = my_id {
                    if let Some(me) = server_state.players.iter().find(|p| p.id == id) {
                        // detect being stuck: position barely moved while walking
                        if matches!(state, BotState::Walking) {
                            let dx = (me.x - last_x).abs();
                            let dy = (me.y - last_y).abs();
                            if dx + dy < 0.001 {
                                stuck_ticks += 1;
                            } else {
                                stuck_ticks = 0;
                            }
                            // stuck for >10 ticks — turn right by ~30 ticks (~0.5s)
                            if stuck_ticks > 10 {
                                state = BotState::Turning { ticks_left: 30 };
                                stuck_ticks = 0;
                            }
                        }
                        last_x = me.x;
                        last_y = me.y;

                        // simple status line
                        if seq % 60 == 0 {
                            println!(
                                "bot#{} pos=({:.2},{:.2}) fuel={:.0}% rescued={}",
                                id, me.x, me.y, me.fuel,
                                server_state.miner_rescued
                            );
                        }
                    }
                }
            }
        }

        let elapsed = t0.elapsed();
        if elapsed < tick { std::thread::sleep(tick - elapsed); }
    }
}
