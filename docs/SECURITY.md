# Security Considerations — UDP Multiplayer

This document covers the attack surfaces introduced by using UDP and the mitigations to implement before the server is considered stable.

---

## 1. IP Spoofing / Source Forgery

**Risk:** Any machine can send a UDP packet claiming to be from any IP address.

**Mitigation:**
- Issue each player a random `session_token: u64` when their first valid packet registers them.
- Include `session_token` in every `InputPacket`.
- Server rejects any packet where the token does not match the registered `SocketAddr`.

---

## 2. Packet Replay Attacks

**Risk:** An attacker records a valid packet and resends it later to repeat an action.

**Mitigation:**
- Already partially addressed: the `sequence` field in `InputPacket` discards out-of-order packets.
- Start sequence numbers at a random offset per session (not always `0`) so they are not guessable.
- Server tracks the last accepted sequence per client and drops anything ≤ that value.

---

## 3. Packet Flooding / Rate Limiting

**Risk:** A single client (or spoofed source) floods the server with packets, starving other players or exhausting CPU.

**Mitigation:**
- Track packet count per `SocketAddr` in a sliding time window.
- Drop all packets from a source that exceeds **128 packets/second**.
- Cap the rate at which unknown senders can register as new players (max ~5 new players/second).

---

## 4. Player Impersonation

**Risk:** A malicious client sends packets with another player's `player_id`, causing the server to update the wrong player's state.

**Mitigation:**
- The server must bind `player_id` to `SocketAddr` at registration time.
- Never trust the `player_id` field in an incoming packet — derive it from the registered `SocketAddr` instead.
- Packets from a known `SocketAddr` carrying a `player_id` that doesn't match its registration are silently dropped.

---

## 5. Malformed / Oversized Packets

**Risk:** A crafted packet could cause `bincode` deserialization to panic, allocate excessive memory, or crash the server.

**Mitigation:**
- Define a hard `MAX_PACKET_BYTES` constant in `shared` (suggested: `256` bytes for `InputPacket`).
- Check the received byte length against `MAX_PACKET_BYTES` before calling `bincode::deserialize`.
- Wrap all deserialization in `match` — never `unwrap()` on incoming bytes.

```rust
// Example pattern — apply everywhere network bytes are deserialized
match bincode::deserialize::<InputPacket>(&buf[..len]) {
    Ok(packet) => handle(packet),
    Err(_) => { /* drop silently, optionally log */ }
}
```

---

## 6. DDoS Amplification

**Risk:** The server can be used as a traffic amplifier — a small spoofed request triggers a large response sent to a victim IP.

**Mitigation:**
- Never send a response to an address that has not first sent a valid, token-bearing packet.
- Response packets (`StatePacket`) are only dispatched to the registered `SocketAddr` list — never to arbitrary addresses.

---

## Implementation Checklist

```
[ ] session_token field added to InputPacket in shared/protocol.rs
[ ] Server issues random session_token on first valid packet from new SocketAddr
[ ] Server validates session_token on every subsequent packet from that SocketAddr
[ ] Sequence numbers start at a random offset per session
[ ] Rate limiter: drop if > 128 packets/sec from one SocketAddr
[ ] MAX_PACKET_BYTES constant defined in shared; checked before deserializing
[ ] All bincode::deserialize calls wrapped in match (no unwrap on network bytes)
[ ] player_id derived from SocketAddr server-side, not trusted from packet field
[ ] StatePacket broadcast only sent to registered SocketAddrs
```

---

## Priority for This Project

For a LAN / course submission environment, implement in this order:

1. **Malformed packet handling** — prevents server crashes from bad input (highest risk)
2. **Rate limiting** — prevents one client from starving others
3. **session_token** — prevents impersonation and replay
4. **Sequence number randomisation** — hardens replay protection

---

_Last updated: April 2026_
