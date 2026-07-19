// net.rs
// Handles all communication with the server.
//
// A background thread runs net_thread, which sends the player's current
// input 60 times a second and receives the server's state packets in
// return. The main game loop and this thread talk to each other through
// two channels: NetInput (main loop writes, this thread reads) and a
// state channel (this thread writes, main loop reads).

use std::net::UdpSocket;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use shared::protocol::{InputPacket, MAX_PACKET_BYTES, ShotEvent, StatePacket};

pub const NET_HZ: u64 = 60;

// The player's current input, written by the main game loop and read by
// the network thread. Everything is stored as an atomic so both sides
// can access it without a lock, since it is just simple values updated
// every frame.
pub struct NetInput {
    pub forward: AtomicBool,
    pub backward: AtomicBool,
    pub shoot: AtomicBool,
    pub angle_bits: AtomicU32, // yaw stored as raw f32 bits, there is no AtomicF32
    pub x_bits: AtomicU32,
    pub y_bits: AtomicU32,
    pub spawned: AtomicBool,
    /// set to true by the main loop when the player presses the quit
    /// key, tells net_thread to send one final goodbye packet and then
    /// stop running, instead of waiting for the server to notice the
    /// player is gone through the usual timeout
    pub quitting: AtomicBool,
}

impl NetInput {
    pub fn new() -> Self {
        Self {
            forward: AtomicBool::new(false),
            backward: AtomicBool::new(false),
            shoot: AtomicBool::new(false),
            angle_bits: AtomicU32::new(0f32.to_bits()),
            x_bits: AtomicU32::new(0f32.to_bits()),
            y_bits: AtomicU32::new(0f32.to_bits()),
            spawned: AtomicBool::new(false),
            quitting: AtomicBool::new(false),
        }
    }
}

// Runs forever on a background thread. Every 1/60th of a second it sends
// the player's current input to the server, then reads back whatever
// state packets have arrived and forwards the newest one to the main loop.
//
// The loop ends early, on purpose, the moment `quitting` is set to true.
// The very last packet sent before exiting has `disconnecting: true`, so
// the server can remove this player immediately, instead of waiting out
// its normal five second timeout for a client that has gone silent.
pub fn net_thread(
    server_addr: String,
    username: String,
    input: Arc<NetInput>,
    state_tx: mpsc::SyncSender<StatePacket>,
) {
    let socket = UdpSocket::bind("0.0.0.0:0").expect("bind failed");
    socket.connect(&server_addr).expect("connect failed");
    socket
        .set_read_timeout(Some(Duration::from_millis(1)))
        .unwrap();
    println!("net thread connected to {server_addr}");

    let interval = Duration::from_millis(1000 / NET_HZ);
    let mut seq = 0u32;
    let mut buf = vec![0u8; MAX_PACKET_BYTES];

    // shot events are transient, they only exist for one server tick, so we
    // must never lose one to a full channel. This buffer holds any events
    // that could not be sent yet and keeps trying until they get through.
    let mut pending_events: Vec<ShotEvent> = Vec::new();

    // the username is only sent once, on the very first packet, after
    // that the server already knows who we are
    let mut username_sent = false;

    loop {
        let t0 = Instant::now();
        seq += 1;

        let quitting = input.quitting.load(Ordering::Relaxed);

        let pkt = InputPacket {
            sequence: seq,
            player_id: 0,
            session_token: 0,
            forward: input.forward.load(Ordering::Relaxed),
            backward: input.backward.load(Ordering::Relaxed),
            turn_left: false, // no longer used, the angle field carries turning now
            turn_right: false,
            shoot: input.shoot.load(Ordering::Relaxed),
            angle: f32::from_bits(input.angle_bits.load(Ordering::Relaxed)),
            // do not claim a position until we have adopted our server spawn,
            // the server rejects negative coordinates, so it keeps its own
            // assigned spawn point until we send something real
            x: if input.spawned.load(Ordering::Relaxed) {
                f32::from_bits(input.x_bits.load(Ordering::Relaxed))
            } else {
                -1.0
            },
            y: if input.spawned.load(Ordering::Relaxed) {
                f32::from_bits(input.y_bits.load(Ordering::Relaxed))
            } else {
                -1.0
            },
            username: if username_sent {
                String::new()
            } else {
                username.clone()
            },
            disconnecting: quitting,
        };

        if let Ok(enc) = postcard::to_allocvec(&pkt) {
            let _ = socket.send(&enc);
            username_sent = true;
        }

        // this was the goodbye packet, our work here is done
        if quitting {
            break;
        }

        // Drain every packet currently waiting in the socket. We only need
        // the newest one for positions, but shot events must all be kept,
        // so we collect them from every packet we see this pass.
        let mut newest: Option<StatePacket> = None;
        while let Ok(len) = socket.recv(&mut buf) {
            if let Ok(state) = postcard::from_bytes::<StatePacket>(&buf[..len]) {
                pending_events.extend(state.shot_events.iter().cloned());
                newest = Some(state);
            }
        }

        if let Some(mut state) = newest {
            state.shot_events = pending_events.clone();
            if state_tx.try_send(state).is_ok() {
                pending_events.clear(); // only clear once it actually got through
            }
            // if the send failed, pending_events is kept and retried next loop
        }

        let elapsed = t0.elapsed();
        if elapsed < interval {
            thread::sleep(interval - elapsed);
        }
    }
}
