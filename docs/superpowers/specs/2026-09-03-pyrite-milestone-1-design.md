# Pyrite — Milestone 1 Design: Protocol Foundations & Server List Ping

**Date:** 2026-09-03
**Status:** Approved
**Scope:** Sprint 1 / Phase 1 of the Master Plan roadmap.

---

## 1. Objective

Stand up the Pyrite Cargo workspace and implement enough of the Minecraft
network protocol that an unmodified vanilla client, pointed at
`0.0.0.0:25565`, renders a correct multiplayer-list entry — MOTD, version
string, player counts — and reports a real latency figure from the ping/pong
round trip. The server must not panic or leak connections while doing so.

Non-goals for this milestone: NBT, world storage, ECS, the WASM runtime,
encryption, and compression. Login is stubbed with a graceful disconnect.

### Success criteria

1. `cargo build --workspace` succeeds with no warnings.
2. `cargo clippy --workspace --all-targets -- -D warnings` is clean.
3. `cargo test --workspace` passes on Linux, macOS, and Windows.
4. `cargo run -p pyrite-server` binds `0.0.0.0:25565` and logs a startup line.
5. A vanilla client adding `localhost` to its server list shows the configured
   MOTD, the version name, `0/N` players, and a latency bar.
6. Repeated pings and abrupt client disconnects do not crash or hang the server.

---

## 2. Decision log

These decisions reconcile four planning documents that disagreed with each
other. They are recorded here so they are not re-litigated mid-implementation.

| # | Decision | Rationale |
|---|---|---|
| D1 | **Crate naming follows `crates/{protocol,net,server}`.** MasterPlan §6's `engine-*` scheme is superseded. | InitializationPlan §3 and the Sprint 1 brief agree; MasterPlan §6 is the outlier and is stale. |
| D2 | **`crates/protocol` is bidirectional from day one.** Every packet is typed by direction and implements both encode and decode. | The future Pyrite client shares this exact crate rather than forking it. Also yields free round-trip tests, the cheapest proof a codec is correct. |
| D3 | **One pinned protocol version**, a single const pair in `protocol/src/version.rs`. | Multi-version dispatch is overhead against a four-packet sprint. MasterPlan's protocol-churn risk is answered by the crate boundary, not by runtime version tables. |
| D4 | **Hand-write the wire protocol and NBT. No Minecraft-domain crates.** Generic infrastructure crates are unrestricted. Anvil region IO is explicitly left open, to be decided at Phase 2 against the CONTRIBUTING allowlist. | Non-negotiable #1. This overrides PyriteClientPlan's recommendation of `valence_protocol` / `simdnbt`; that document's dependency table does not apply to the server engine. `bevy_ecs`/`hecs` remain permissible — a generic ECS is not Minecraft-domain. |
| D5 | **`Connection` is generic over `AsyncRead + AsyncWrite + Unpin`.** | Costs one type parameter. Buys socket-free end-to-end tests over `tokio::io::duplex()` today, and makes PyriteClientPlan's embedded-server loopback a byte pipe that already fits. A packet-level `Transport` trait is deferred: designing one against four packets bakes in wrong assumptions, and its `async-trait` + `Box<dyn Packet>` shape allocates per packet, against non-negotiable #2. |
| D6 | **Trait-per-packet, one struct per packet.** No large per-state enums, no derive macro yet. | Monomorphized, no dyn dispatch, no per-packet allocation. A per-state enum is sized to its largest variant, which Play-state would make expensive. Revisit a `#[derive(Packet)]` macro when packet count exceeds ~30; the trait signature is chosen so a macro can generate exactly it without call-site churn. |
| D7 | **No empty placeholder crates** for `world`, `ecs`, `modloader`, `assets`, `sdk`. | Each is added when its phase begins. Empty crates cost build time and imply progress that does not exist. |

---

## 3. Verified protocol constants

Confirmed 2026-09-03 against public reverse-engineering references
([Minecraft Wiki: Protocol version](https://minecraft.wiki/w/Protocol_version),
[Java Edition protocol / Server List Ping](https://minecraft.wiki/w/Java_Edition_protocol/Server_List_Ping)).
No proprietary source, decompiled bytecode, or private mappings were consulted.

Java Edition has moved to calendar versioning. The current release is **26.2**,
protocol **776**.

| Packet | State | Direction | ID | Payload |
|---|---|---|---|---|
| Handshake | Handshaking | Serverbound | `0x00` | VarInt protocol version, String server address, `u16` port, VarInt next state |
| Status Request | Status | Serverbound | `0x00` | *(empty)* |
| Status Response | Status | Clientbound | `0x00` | String (JSON) |
| Ping Request | Status | Serverbound | `0x01` | `i64` payload |
| Pong Response | Status | Clientbound | `0x01` | `i64` payload (echoed verbatim) |

Next-state values: `1` = Status, `2` = Login, `3` = Transfer.

**Robustness note:** a client is permitted to skip Status Request entirely and
send Ping Request immediately after the handshake. The state machine must
accept either ordering rather than requiring Status Request first.

**Convention:** the server closes the connection after sending Pong Response.

---

## 4. Repository layout

```
Pyrite/
├── Cargo.toml                     # workspace: members, shared deps, shared lints
├── rust-toolchain.toml            # pin stable 1.94.0
├── deny.toml                      # cargo-deny licence allowlist
├── .gitignore
├── LICENSE-MIT
├── LICENSE-APACHE
├── README.md                      # pre-alpha banner, is/is-not statement
├── CONTRIBUTING.md                # clean-room policy + dependency allowlist
├── .github/
│   ├── workflows/ci.yml
│   └── ISSUE_TEMPLATE/
│       ├── config.yml             # blank_issues_enabled: false
│       ├── bug_report.md
│       └── technical_rfc.md
├── docs/superpowers/specs/
└── crates/
    ├── protocol/
    │   ├── Cargo.toml
    │   └── src/
    │       ├── lib.rs
    │       ├── varint.rs
    │       ├── codec.rs
    │       ├── error.rs
    │       ├── version.rs
    │       ├── text.rs
    │       ├── buf.rs             # String/u16/i64 read+write helpers over Buf/BufMut
    │       └── packets/
    │           ├── mod.rs         # Packet trait, State, Direction, RawPacket
    │           ├── handshake.rs
    │           ├── status.rs
    │           └── login.rs       # clientbound Disconnect only
    ├── net/
    │   ├── Cargo.toml
    │   └── src/
    │       ├── lib.rs
    │       ├── config.rs
    │       ├── state.rs
    │       ├── connection.rs
    │       └── error.rs
    └── server/
        ├── Cargo.toml
        └── src/main.rs
```

Crate package names are `pyrite-protocol`, `pyrite-net`, `pyrite-server`;
library targets are `pyrite_protocol` and `pyrite_net`.

`config.yml` with `blank_issues_enabled: false` is what actually enforces
InitializationPlan §2's "no generic feature requests" rule — the two templates
alone do not.

---

## 5. `crates/protocol`

Depends only on `bytes`, `tokio-util`, `serde`, `serde_json`, `thiserror`.
No Tokio runtime dependency: this crate is pure codec and must stay usable
from a non-async context (the future client's meshing threads, fuzz targets).

### 5.1 `varint.rs`

Three functions, not two. The third is not optional.

```rust
pub fn write_varint<B: BufMut>(dst: &mut B, value: i32);
pub fn read_varint<B: Buf>(src: &mut B) -> Result<i32, ProtocolError>;
pub fn read_varint_slice(src: &[u8]) -> Result<Option<(i32, usize)>, ProtocolError>;
```

`read_varint_slice` decodes without consuming and returns `Ok(None)` when the
buffer holds a partial VarInt. The framing codec needs this because a frame's
length prefix arrives byte-by-byte over TCP; a consuming decoder would eat
bytes it cannot yet interpret and desynchronise the stream permanently.

VarLong equivalents (`write_varlong`, `read_varlong`) are included now — Play
state needs them and they are fifteen lines.

Encoding is little-endian base-128 with a continuation bit in the MSB. Negative
`i32` values are transmitted as their two's-complement bit pattern, so `-1`
occupies the full five bytes. Overflow guards reject VarInts longer than 5
bytes and VarLongs longer than 10, returning `ProtocolError::VarIntTooLong`
rather than silently wrapping.

Doc comments explain each shift and mask operation at byte level.

### 5.2 `buf.rs`

Shared primitive read/write helpers so packet implementations do not repeat
bounds checks: `read_string`/`write_string`, `read_u16`/`write_u16`,
`read_i64`/`write_i64`.

A protocol String is a VarInt **byte** length followed by UTF-8. Length caps in
the specification are expressed in characters, with the byte cap being
`3n + 3`; `read_string` takes a `max_chars` argument and enforces the derived
byte cap before allocating, so a hostile length prefix cannot trigger a large
allocation. Invalid UTF-8 is a typed error, never a panic.

### 5.3 `codec.rs`

`PacketCodec` implements `tokio_util::codec::Decoder` and
`Encoder<P: Packet>`.

Frame layout (uncompressed): `VarInt(length) ++ VarInt(packet_id) ++ body`,
where `length` counts the id plus body but not itself.

Decoder algorithm:

1. `read_varint_slice` over the current buffer. `Ok(None)` → return `Ok(None)`,
   consuming nothing.
2. Reject `length` that is negative or exceeds `MAX_PACKET_SIZE`
   (2 097 151 bytes — the maximum a three-byte length VarInt can express) with
   `ProtocolError::FrameTooLarge`.
3. If fewer than `header_len + length` bytes are buffered, `reserve` the
   shortfall and return `Ok(None)`.
4. Split off exactly one frame, read the packet id VarInt from its front, and
   yield `RawPacket { id, body: Bytes }`.

`RawPacket.body` is a `Bytes` slice over the same refcounted allocation as the
read buffer, so framing does not memcpy the payload.

The codec carries two fields, both `None` this milestone and both documented as
Milestone 2 seams: `compression_threshold: Option<i32>` and
`encryption: Option<Cipher>`. They are *fields*, never `todo!()` — the code
compiles and runs correctly with them unset.

### 5.4 `packets/mod.rs`

```rust
pub enum State { Handshaking, Status, Login, Configuration, Play }
pub enum Direction { Serverbound, Clientbound }

pub trait Packet: Sized {
    const ID: i32;
    const STATE: State;
    const DIRECTION: Direction;
    fn encode<B: BufMut>(&self, dst: &mut B) -> Result<(), ProtocolError>;
    fn decode<B: Buf>(src: &mut B) -> Result<Self, ProtocolError>;
}
```

`Configuration` and `Play` are declared in `State` now because the FSM and the
error type reference them; no packets are defined for them yet.

### 5.5 `packets/handshake.rs`

`Handshake { protocol_version: i32, server_address: String, server_port: u16,
next_state: NextState }`, where `NextState` is
`Status | Login | Transfer`, decoded from `1 | 2 | 3`; anything else is
`ProtocolError::InvalidNextState`. `server_address` is capped at 255 characters.

### 5.6 `packets/status.rs`

Packet types `StatusRequest`, `StatusResponse { json: String }`,
`PingRequest { payload: i64 }`, `PongResponse { payload: i64 }`.

Alongside them, serde types for the response body — `StatusResponseJson`,
`VersionInfo { name, protocol }`, `PlayersInfo { max, online, sample }`,
`SamplePlayer { name, id }` — with `favicon: Option<String>` and
`enforces_secure_chat: bool` renamed to `enforcesSecureChat` on the wire.
Building the response through serde rather than string formatting makes
malformed JSON unrepresentable.

### 5.7 `text.rs`

A minimal typed `TextComponent` (`text`, optional `color`, `bold`, `italic`,
plus `extra` children) so the MOTD is a struct rather than a raw JSON blob.
This is deliberately small; it grows when chat lands in a later phase.

### 5.8 `error.rs`

```rust
#[derive(Debug, thiserror::Error)]
pub enum ProtocolError {
    VarIntTooLong,
    UnexpectedEof,
    FrameTooLarge { len: usize, max: usize },
    StringTooLong { len: usize, max: usize },
    InvalidUtf8(#[from] std::str::Utf8Error),
    UnknownPacket { state: State, direction: Direction, id: i32 },
    InvalidNextState(i32),
    Json(#[from] serde_json::Error),
    Io(#[from] std::io::Error),
}
```

`Io` is required because `tokio_util::codec::Decoder::Error` must implement
`From<std::io::Error>`.

---

## 6. `crates/net`

### 6.1 `state.rs`

```rust
pub struct ConnectionState { current: State, status_request_seen: bool }
```

`transition(&mut self, to: State) -> Result<(), NetError>` permits exactly:
`Handshaking → Status`, `Handshaking → Login`, and terminal transitions.
`Status → Login` is rejected with a typed error rather than ignored — an
illegal transition is a protocol violation and closes the connection.

Per §3, Status accepts Ping Request whether or not Status Request preceded it;
`status_request_seen` exists for logging and duplicate-request rejection, not
to gate the ping.

### 6.2 `connection.rs`

```rust
pub struct Connection<S: AsyncRead + AsyncWrite + Unpin> {
    framed: Framed<S, PacketCodec>,
    state: ConnectionState,
    config: Arc<ServerConfig>,
}
```

`run()` loops: read a `RawPacket`, dispatch on `(state, id)`, write any
response. Dispatch is a hand-written match; an unmatched pair yields
`ProtocolError::UnknownPacket` and closes the connection.

Two additions the source plans omit, both required for a socket exposed on a
public port:

- **Read idle timeout**, default 30 s, via `tokio::time::timeout`. Without it a
  half-open connection occupies a task indefinitely and the accept loop can be
  exhausted by trivially cheap client behaviour.
- **Per-connection `tracing` span** carrying the peer address, so every log line
  from a connection is attributable.

Login state responds with a real clientbound `Disconnect` carrying a
`TextComponent` reason, then closes — friendlier than dropping the socket, and
it exercises the clientbound encode path in this milestone.

After `PongResponse` is flushed the connection closes normally, per §3.

### 6.3 Error policy

No `unwrap`, `expect`, or panic on any path reachable from network input.
Connection-level errors are logged at `warn` (protocol violations) or `debug`
(ordinary disconnects) and terminate only that connection's task. The workspace
lint table denies `clippy::unwrap_used` and `clippy::expect_used` in these
crates to enforce this mechanically rather than by review.

---

## 7. `crates/server`

`main.rs`:

1. `tracing_subscriber` with `EnvFilter`, defaulting to `info`, overridable via
   `RUST_LOG`.
2. `clap` derive CLI: `--bind` (default `0.0.0.0:25565`), `--motd`,
   `--max-players`, all with environment-variable fallbacks.
3. Bind `TcpListener`; log the bound address and the advertised version.
4. Accept loop: `tokio::spawn` per connection, `ServerConfig` shared as `Arc`,
   per-task errors logged and never propagated into the accept loop.
5. `tokio::signal::ctrl_c` triggers graceful shutdown; the accept loop stops
   and in-flight connections are allowed to finish.

An accept error is logged and the loop continues — a single failed accept
(EMFILE, for example) must not terminate the server.

---

## 8. Testing strategy

**Unit — `varint`:** the publicly documented vectors, both directions:
`0 → [0x00]`, `1 → [0x01]`, `127 → [0x7f]`, `128 → [0x80, 0x01]`,
`255 → [0xff, 0x01]`, `2147483647 → [0xff, 0xff, 0xff, 0xff, 0x07]`,
`-1 → [0xff, 0xff, 0xff, 0xff, 0x0f]`,
`-2147483648 → [0x80, 0x80, 0x80, 0x80, 0x08]`, plus VarLong equivalents.
A six-byte continuation sequence must return `VarIntTooLong`, not wrap.

**Unit — `buf`:** a String whose declared length exceeds the cap must error
before allocating; invalid UTF-8 must return `InvalidUtf8`.

**Unit — `codec`:** a frame fed **one byte at a time** must return `Ok(None)`
for every prefix and exactly one frame on the final byte. Two frames in one
buffer must decode as two. A length prefix above `MAX_PACKET_SIZE` must error
without allocating.

**Round-trip — every packet:** encode then decode, assert equality. D2 makes
this possible for all four packets, and it is the primary defence against
field-order mistakes.

**Integration — the milestone criterion:** the full
handshake → status request → status response → ping → pong exchange driven over
`tokio::io::duplex()`, asserting the response JSON deserialises with the
configured MOTD and counts, and that the pong payload equals the ping payload
bit-for-bit. No listener, no port, no sleep — deterministic in CI on every
platform. A second test drives handshake → ping (skipping status request) per
§3, and a third asserts `Handshaking → Status → Login` is rejected.

**CI** (`.github/workflows/ci.yml`), on push and PR to `main`:
`cargo fmt --check`; `cargo clippy --workspace --all-targets -- -D warnings`;
`cargo test --workspace` on `ubuntu-latest`, `macos-latest`,
`windows-latest`; and `cargo-deny check licenses` against the `deny.toml`
allowlist (MIT, Apache-2.0, BSD-2/3-Clause, ISC, Unicode-3.0), satisfying
InitializationPlan §4's clean-room dependency audit.

---

## 9. Known limitations

**"Zero-copy" is qualified, deliberately.** `tokio_util::codec::Decoder` cannot
yield items borrowing from its input buffer, so packet `String` fields
allocate. What this design actually achieves is: refcounted `Bytes` slices for
frame bodies with no memcpy, one reused read buffer per connection, no
`Box<dyn Packet>` and no per-packet heap allocation for dispatch, and
monomorphised encode/decode. The handful of `String` allocations in Status are
irrelevant at ping frequency. The distinction is recorded here so that Play
state — where chunk payloads are large and frequent — is designed against
`Bytes` slices from the outset rather than inheriting a false assumption.

**Deferred to Milestone 2:** encryption (AES-128-CFB8), compression (zlib with
threshold), Login and Configuration state implementations, NBT.

**Deferred to Phase 2:** the Anvil region-IO dependency decision (D4).

**Single pinned protocol version.** Clients on other versions receive a correct
status response advertising protocol 776 and are shown as incompatible by their
own client, which is the intended behaviour, not a failure.
