# Pyrite — Milestone 2 Design: Compression & Login

**Date:** 2026-09-03
**Status:** Approved
**Scope:** First slice of Master Plan Phase 1's remainder, after Milestone 1.

---

## 1. Objective

Add packet compression and implement the Login state, so a client completes
the login handshake in offline mode and transitions to Configuration.

Non-goals: encryption, Mojang authentication, NBT, Configuration-state
packets, Play-state packets, world storage.

### 1.1 Decomposition context

Master Plan Phase 1 ends at "a standard unmodified client can connect,
authenticate, and enter an empty void world". That is five subsystems, not
one, and one of them (NBT) was deferred out of Milestone 1 while everything
downstream depends on it. Phase 1's remainder is therefore split:

| Milestone | Content | Depends on |
|---|---|---|
| **M2 (this spec)** | Compression, Login state, offline identity | — |
| M3 | NBT | — |
| M4 | Configuration state, minimum Play state → void world | M3 |
| M5 | Encryption (AES-128-CFB8), Mojang session authentication | M2 |

Encryption is deliberately not on the critical path to a joinable server: a
vanilla client against an offline-mode server never performs the encryption
handshake. M5 is what makes the server safe to expose, not what makes it work.

### 1.2 Success criteria

1. `cargo build --workspace` succeeds with no warnings.
2. `cargo clippy --workspace --all-targets -- -D warnings` is clean.
3. `cargo test --workspace` passes on Linux, macOS, and Windows.
4. Driven over `tokio::io::duplex()`, a client that sends Handshake
   (next state 2) then Login Start receives Set Compression then Login
   Success, and after sending Login Acknowledged the connection is in
   Configuration state.
5. Every packet after Set Compression uses the compressed frame format, and
   Set Compression itself does not.
6. A packet below the threshold round-trips uncompressed; one at or above it
   round-trips compressed; both decode to identical bytes.
7. A peer cannot make the server allocate or inflate beyond `MAX_PACKET_SIZE`
   by lying about a compressed packet's uncompressed length.
8. The server refuses to bind a non-loopback address without
   `--insecure-offline-mode`.

**What this milestone does not produce:** a satisfying result in a real
client. A vanilla client will pass "Logging in…", enter Configuration,
receive nothing further (registry data arrives in M4), and eventually time
out. That is the correct outcome for M2. Success is established by the
integration tests and a raw packet-sequence probe, not by a screenshot.

---

## 2. Decision log

Continues Milestone 1's D1–D7, which remain in force.

| # | Decision | Rationale |
|---|---|---|
| D8 | **Phase 1's remainder is four milestones** (§1.1), not one. | Five independent subsystems with a hard dependency on NBT. A single spec covering all of it could not be reviewed meaningfully. |
| D9 | **Deployment target is public multiplayer.** | Stated requirement. Raises M5's importance and makes offline mode a hazard rather than a convenience, hence D11. |
| D10 | **Compression is handled by branching inside `PacketCodec`**, not by a wrapping layer or a second codec type. | The compressed and uncompressed forms differ *inside* the frame — the outer length prefix covers the data-length field and the payload — so a wrapping "layer" would have to pass length information across a boundary that does not exist on the wire. A second codec type would duplicate framing logic in two places, which is the drift risk D2 exists to avoid. The `compression_threshold: Option<i32>` seam is already present from M1. |
| D11 | **Binding interlock.** While authentication is unimplemented, binding a non-loopback address requires an explicit `--insecure-offline-mode` flag. Without it the server exits with an error. | Offline mode on a public port lets anyone join under any username, including one holding operator rights. With D9 as the target, the window between M2 and M5 must be closed by the code, not by remembering not to deploy. M5 removes the interlock. |
| D12 | **Offline UUIDs match the vanilla derivation**: UUIDv3 (RFC 4122, MD5) over the UTF-8 bytes of `OfflinePlayer:<name>`. The UUID in Login Start is ignored. | Keeps player data, permissions, and world files interchangeable with other implementations, and gives a player a stable identity across restarts. The Login Start UUID is unauthenticated — a client may send any value — so trusting it would let a player assume another's identity for free. Derived from RFC 4122 plus a publicly documented prefix string; no proprietary source involved. |
| D13 | **`md-5` and `flate2` are permitted dependencies.** | Both are general-purpose infrastructure, not Minecraft-domain logic, so D4 permits them. Both are MIT/Apache-2.0. |

---

## 3. Verified protocol constants

Confirmed 2026-09-03 against
[Java Edition protocol / Packets](https://minecraft.wiki/w/Java_Edition_protocol/Packets)
for protocol 776. No proprietary source, decompiled bytecode, or private
mappings were consulted.

### 3.1 Login state packets

| Packet | Direction | ID | Payload |
|---|---|---|---|
| Disconnect | Clientbound | `0x00` | JSON text component *(already implemented in M1)* |
| Encryption Request | Clientbound | `0x01` | *(M5)* |
| Login Success | Clientbound | `0x02` | Game Profile, UUID session ID |
| Set Compression | Clientbound | `0x03` | VarInt threshold |
| Login Plugin Request | Clientbound | `0x04` | *(not implemented)* |
| Cookie Request | Clientbound | `0x05` | *(not implemented)* |
| Login Start | Serverbound | `0x00` | String(16) name, UUID |
| Encryption Response | Serverbound | `0x01` | *(M5)* |
| Login Plugin Response | Serverbound | `0x02` | *(not implemented)* |
| Login Acknowledged | Serverbound | `0x03` | *(empty)* — switches to Configuration |
| Cookie Response | Serverbound | `0x04` | *(not implemented)* |

**Game Profile:** UUID, String(16) username, prefixed array of properties.
**Property:** String(64) name, String(32767) value, prefixed optional
String(1024) signature.

Packets marked *not implemented* are never sent by Pyrite, so a compliant
client never sends their counterparts. Receiving one is an unknown-packet
error and closes the connection, which is the existing M1 behaviour.

### 3.2 Compressed frame format

Once Set Compression has been sent, every subsequent frame is:

```
VarInt  Packet Length    length of everything after this field
VarInt  Data Length      uncompressed length of the payload, or 0
Byte[]  Payload          zlib-compressed when Data Length != 0
```

`Data Length == 0` means the payload is a raw, uncompressed packet
(id VarInt + body). Otherwise the payload is zlib-compressed and `Data Length`
is the size it inflates to.

A packet whose uncompressed size is **at or above** the threshold is
compressed; one below it is sent with `Data Length = 0`. A threshold below
zero disables compression entirely.

---

## 4. Changes to `crates/protocol`

### 4.1 `buf.rs` — three new primitives

```rust
pub fn write_uuid<B: BufMut>(dst: &mut B, value: u128);
pub fn read_uuid<B: Buf>(src: &mut B) -> Result<u128, ProtocolError>;
```

A UUID is an unsigned 128-bit integer, big-endian, 16 bytes. Represented as
`u128` rather than pulling in a `uuid` crate: the protocol needs the 128 bits
and nothing else, and formatting is a presentation concern.

Prefixed arrays (VarInt count, then elements) and prefixed optionals (a
`bool`, then the value when true) are needed by Game Profile. Both are
expressed as generic helpers taking a closure per element, so no allocation
occurs for an absent optional or an empty array. Array decoding takes a
`max_len` argument and checks it against the declared count **before**
reserving — the same discipline `read_string` already applies, and the same
bug class as the Milestone 1 decoder finding.

### 4.2 `codec.rs` — compression

`PacketCodec` gains:

```rust
pub fn set_compression(&mut self, threshold: i32);
```

which sets `compression_threshold` to `Some(threshold)` for a non-negative
value and `None` otherwise.

**Encode**, when a threshold is set: write the packet id and body into the
existing reused scratch buffer. If its length is at or above the threshold,
zlib-compress it into a second reused buffer and emit
`VarInt(data_len_field_len + compressed_len) ++ VarInt(uncompressed_len) ++ compressed`.
Otherwise emit `VarInt(1 + scratch_len) ++ VarInt(0) ++ scratch`.

**Decode**, when a threshold is set: read the packet length as today, then
read the data length from the frame. When it is zero, the remainder is a raw
packet and decoding proceeds exactly as in the uncompressed path. Otherwise
inflate.

Four guards, all applied **before** any allocation or inflation:

1. `data_length` must not be negative.
2. `data_length` must not exceed `MAX_PACKET_SIZE`. A tiny compressed payload
   can otherwise declare a huge inflated size — the decompression analogue of
   the Milestone 1 amplification finding, and the reason this guard is
   specified explicitly rather than left to the implementer.
3. `0 < data_length < threshold` is a protocol violation: the peer should
   have sent it uncompressed. Rejecting it prevents an attacker spending our
   CPU on inflation that should never have been requested.
4. The decompressor is bounded with `take(data_length)` so a payload that
   inflates further than declared is truncated and rejected, rather than
   growing the output buffer past the declared size. After inflation the
   actual length must equal `data_length`.

New error variants: `ProtocolError::CompressedSizeMismatch { declared, actual }`
and `ProtocolError::CompressedBelowThreshold { data_length, threshold }`.
Zlib failures map through the existing `ProtocolError::Io`.

### 4.3 `packets/login.rs`

Adds `LoginStart`, `SetCompression`, `LoginSuccess`, and `LoginAcknowledged`
alongside M1's `LoginDisconnect`, plus `GameProfile` and `ProfileProperty`.
All implement both directions per D2.

---

## 5. Changes to `crates/net`

### 5.1 `offline.rs` — offline identity

```rust
pub fn offline_uuid(username: &str) -> u128;
pub fn validate_username(name: &str) -> Result<(), NetError>;
```

`offline_uuid` computes UUIDv3 per RFC 4122: MD5 over the UTF-8 bytes of
`OfflinePlayer:<username>`, then set the version nibble to 3 and the variant
bits to RFC 4122. This is the value other implementations compute for the
same name (D12).

`validate_username` enforces 1–16 characters of `[a-zA-Z0-9_]`. A name
outside that set is rejected with a `LoginDisconnect` rather than accepted,
because it will not round-trip through other tooling and is a cheap way to
smuggle odd data into logs and future world files.

### 5.2 `connection.rs` — the login flow

On `Handshake` with next state `Login` or `Transfer`, the connection now
enters Login rather than immediately disconnecting. Then:

1. Receive `LoginStart`. Validate the username; on failure send
   `LoginDisconnect` and close.
2. Derive the UUID with `offline_uuid`, ignoring the client-supplied one.
3. If compression is enabled, send `SetCompression`, **flush it**, and only
   then call `codec.set_compression()`.
4. Send `LoginSuccess` with a `GameProfile` carrying the derived UUID, the
   validated username, and an empty properties array.
5. Receive `LoginAcknowledged`; transition to Configuration.

**Step 3's ordering is the subtle part.** Set Compression is itself sent
uncompressed and the new format applies to the *next* packet. Flipping the
codec before the frame is flushed emits a compressed Set Compression, which
no client can parse. This gets a dedicated test asserting the byte-level
shape of both frames.

`ConnectionState` gains the `Login → Configuration` transition. `Status →
Login` remains rejected.

Once in Configuration the connection has nothing to send, so it idles until
the read timeout closes it. That is expected in M2 and is asserted as such
rather than treated as a bug.

### 5.3 `ServerConfig`

Gains `compression_threshold: Option<i32>`, default `Some(256)` — the
conventional default. `None` disables compression, in which case Set
Compression is never sent and the codec stays in M1's format.

---

## 6. Changes to `crates/server`

Adds `--compression-threshold` (default 256, `-1` disables) and
`--insecure-offline-mode`.

**The interlock (D11).** At startup, if the bind address is not a loopback
address and `--insecure-offline-mode` was not passed, the server prints an
error naming the specific risk — that anyone may join under any username,
including one with operator rights — and exits non-zero. When the flag is
passed, it logs a `warn` at startup and continues. Loopback binds are
unaffected, so local development needs no flag.

This is a startup-time check on the resolved bind address, not a per-
connection check: it must be impossible to reach a state where the server is
listening on a public interface in offline mode without the operator having
said so.

---

## 7. Testing

**Unit — `buf`:** UUID round-trips including all-zero and all-ones; a
prefixed array whose declared count exceeds `max_len` errors before
reserving; a prefixed optional round-trips in both present and absent forms.

**Unit — `codec`:** a packet below the threshold encodes with
`Data Length == 0`; one at or above encodes compressed; both round-trip to
identical bytes. A compressed frame fed one byte at a time yields exactly one
packet. Each of the four guards in §4.2 is exercised by a test that
constructs the malformed frame directly — in particular, a frame declaring a
`data_length` above `MAX_PACKET_SIZE` must be rejected without inflating.

**Unit — `offline`:** `offline_uuid` is stable across calls, differs between
usernames, and has the correct version and variant bits. Usernames of 0 and
17 characters and one containing `-` are rejected.

**Integration (`crates/net/tests/login.rs`):** the full
handshake → login start → set compression → login success →
login acknowledged sequence over `tokio::io::duplex()`, asserting the exact
packet order, that the client's UUID is not echoed back, that the derived
UUID matches `offline_uuid`, and that the connection ends in Configuration.
A second test asserts the compression switchover at byte level: Set
Compression's frame has no data-length field and Login Success's does. A
third asserts an invalid username receives `LoginDisconnect` and no
`LoginSuccess`.

**Integration — the interlock:** unit-testable by extracting the decision
into a function over `(SocketAddr, bool) -> Result<(), _>` rather than
inlining it in `main`, so both the refusal and the override are asserted
without binding a socket.

---

## 8. Known limitations

**Offline mode is impersonable by design.** Any client may claim any
username. The interlock (D11) confines that to loopback until M5 lands; it
does not fix it. This is the single most important thing to remember about
the state of the server between M2 and M5.

**No encryption.** All traffic is plaintext, including in the `Transfer`
path.

**Login `Transfer` is treated as `Login`.** A transferred client carries a
cookie in vanilla; cookies are not implemented, so a transfer is handled as
an ordinary login. Revisit when cookies land.

**The connection stalls in Configuration.** No Configuration packets exist
until M4. The client will time out. Expected for this milestone.
