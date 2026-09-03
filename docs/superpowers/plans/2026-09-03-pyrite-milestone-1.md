# Pyrite Milestone 1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a Rust Cargo workspace whose server binary listens on `0.0.0.0:25565` and answers a vanilla Minecraft client's Server List Ping with a correct MOTD, version, player counts, and latency ping/pong.

**Architecture:** Three crates. `pyrite-protocol` is a pure, runtime-free codec crate: VarInt primitives, a length-prefixed framing codec, and one struct per packet implementing a `Packet` trait that carries both encode and decode. `pyrite-net` owns connection lifecycle — a `Connection<S>` generic over `AsyncRead + AsyncWrite + Unpin` wrapping `Framed<S, PacketCodec>`, plus an explicit state machine. `pyrite-server` is the Tokio binary: CLI parsing, listener, one task per connection.

**Tech Stack:** Rust 1.94 (edition 2024), `tokio`, `tokio-util` (codec), `bytes`, `futures-util`, `serde`/`serde_json`, `thiserror`, `tracing`/`tracing-subscriber`, `clap`.

**Spec:** `docs/superpowers/specs/2026-09-03-pyrite-milestone-1-design.md`

## Global Constraints

- **Clean-room, absolute.** Never consult, reference, or reproduce decompiled Mojang bytecode, private mappings, or proprietary assets. Only open reverse-engineered documentation of wire formats.
- **No Minecraft-domain dependencies.** `valence_protocol`, `fastanvil`, `simdnbt`, Pumpkin and equivalents are forbidden (spec D4). Generic infrastructure crates are unrestricted.
- **Pinned protocol version:** `PROTOCOL_VERSION = 776`, `VERSION_NAME = "26.2"`. Verified 2026-09-03 against the Minecraft Wiki protocol pages.
- **Packet IDs:** Handshake `0x00` (Handshaking, serverbound). Status Request `0x00` / Ping Request `0x01` (Status, serverbound). Status Response `0x00` / Pong Response `0x01` (Status, clientbound). Login Disconnect `0x00` (Login, clientbound).
- **Next state values:** `1` = Status, `2` = Login, `3` = Transfer.
- **No `unwrap`/`expect`/`panic` on any path reachable from network input.** Enforced by `clippy::unwrap_used = "deny"` and `clippy::expect_used = "deny"` in workspace lints. Test modules opt out explicitly with `#![allow(clippy::unwrap_used, clippy::expect_used)]` as the first line inside `mod tests`.
- **No `todo!()` / `unimplemented!()`.** Future features are represented as `Option` fields that are `None`, never as panicking stubs.
- **Every public item needs a doc comment.** `missing_docs = "warn"` plus CI's `-D warnings` makes this a build failure.
- **`MAX_PACKET_SIZE = 2_097_151`** bytes (the largest value a 3-byte length VarInt can express).
- **Package names:** `pyrite-protocol`, `pyrite-net`, `pyrite-server`. Library targets: `pyrite_protocol`, `pyrite_net`.
- Commit after every task. Conventional Commit prefixes (`feat:`, `test:`, `chore:`, `docs:`).

---

## File Structure

| File | Responsibility |
|---|---|
| `Cargo.toml` | Workspace members, shared dependency versions, shared lint table |
| `rust-toolchain.toml` | Pin the toolchain so CI and local builds agree |
| `deny.toml` | Licence allowlist for the clean-room dependency audit |
| `LICENSE-MIT`, `LICENSE-APACHE` | Dual licence |
| `README.md` | Pre-alpha banner, is / is-not statement |
| `CONTRIBUTING.md` | Clean-room policy, dependency allowlist rules |
| `.github/workflows/ci.yml` | fmt, clippy, cross-platform test, licence audit |
| `.github/ISSUE_TEMPLATE/*` | Two strict templates; blank issues disabled |
| `crates/protocol/src/lib.rs` | Crate root, module wiring, re-exports |
| `crates/protocol/src/error.rs` | `ProtocolError` — every failure mode of the codec |
| `crates/protocol/src/version.rs` | The two pinned version constants |
| `crates/protocol/src/varint.rs` | VarInt/VarLong encode, decode, non-consuming decode, length |
| `crates/protocol/src/buf.rs` | String / `u16` / `i64` primitives with bounds checks |
| `crates/protocol/src/codec.rs` | `PacketCodec`, `RawPacket`, framing rules |
| `crates/protocol/src/packets/mod.rs` | `Packet` trait, `State`, `Direction` |
| `crates/protocol/src/packets/handshake.rs` | `Handshake`, `NextState` |
| `crates/protocol/src/packets/status.rs` | Four status packets + response JSON types |
| `crates/protocol/src/packets/login.rs` | Clientbound `LoginDisconnect` |
| `crates/protocol/src/text.rs` | `TextComponent` |
| `crates/net/src/lib.rs` | Crate root |
| `crates/net/src/error.rs` | `NetError` |
| `crates/net/src/config.rs` | `ServerConfig` |
| `crates/net/src/state.rs` | `ConnectionState` finite state machine |
| `crates/net/src/connection.rs` | `Connection<S>`, read loop, dispatch, timeout |
| `crates/net/tests/server_list_ping.rs` | End-to-end ping over `tokio::io::duplex()` |
| `crates/server/src/main.rs` | CLI, tracing init, listener, accept loop, shutdown |

---

## Task 1: Workspace scaffolding, governance, and CI

**Files:**
- Create: `Cargo.toml`, `rust-toolchain.toml`, `deny.toml`, `.gitattributes`
- Create: `LICENSE-MIT`, `LICENSE-APACHE`, `README.md`, `CONTRIBUTING.md`
- Create: `.github/workflows/ci.yml`, `.github/ISSUE_TEMPLATE/config.yml`, `.github/ISSUE_TEMPLATE/bug_report.md`, `.github/ISSUE_TEMPLATE/technical_rfc.md`
- Create: `crates/protocol/Cargo.toml`, `crates/protocol/src/lib.rs`
- Create: `crates/net/Cargo.toml`, `crates/net/src/lib.rs`
- Create: `crates/server/Cargo.toml`, `crates/server/src/main.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: the workspace dependency aliases every later crate uses (`bytes`, `tokio`, `tokio-util`, `futures-util`, `serde`, `serde_json`, `thiserror`, `tracing`, `tracing-subscriber`, `clap`), and the `[lints] workspace = true` convention.

- [ ] **Step 1: Create the workspace manifest**

`Cargo.toml`:

```toml
[workspace]
resolver = "3"
members = ["crates/protocol", "crates/net", "crates/server"]

[workspace.package]
version = "0.1.0"
edition = "2024"
rust-version = "1.94"
license = "MIT OR Apache-2.0"
authors = ["The Pyrite Contributors"]

[workspace.dependencies]
pyrite-protocol = { path = "crates/protocol", version = "0.1.0" }
pyrite-net = { path = "crates/net", version = "0.1.0" }

bytes = "1"
tokio = { version = "1", features = ["rt-multi-thread", "macros", "net", "io-util", "time", "signal"] }
tokio-util = { version = "0.7", features = ["codec"] }
futures-util = { version = "0.3", default-features = false, features = ["sink"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
clap = { version = "4", features = ["derive", "env"] }

[workspace.lints.rust]
unsafe_code = "forbid"
missing_docs = "warn"
missing_debug_implementations = "warn"

[workspace.lints.clippy]
unwrap_used = "deny"
expect_used = "deny"
panic = "deny"

[profile.release]
lto = "thin"
codegen-units = 1
panic = "abort"
```

`rust-toolchain.toml`:

```toml
[toolchain]
channel = "1.94.0"
components = ["rustfmt", "clippy"]
```

`.gitattributes` (the repo is developed on Windows; this keeps CI diffs stable):

```
* text=auto eol=lf
```

- [ ] **Step 2: Create the three crate manifests and minimal roots**

`crates/protocol/Cargo.toml`:

```toml
[package]
name = "pyrite-protocol"
description = "Clean-room Minecraft-compatible network protocol codecs for Pyrite."
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
authors.workspace = true

[lib]
name = "pyrite_protocol"

[dependencies]
bytes.workspace = true
tokio-util.workspace = true
serde.workspace = true
serde_json.workspace = true
thiserror.workspace = true

[lints]
workspace = true
```

`crates/protocol/src/lib.rs`:

```rust
//! Clean-room implementation of the Minecraft-compatible network protocol.
//!
//! This crate is deliberately runtime-free: it depends on no async executor so
//! that it can be used from blocking contexts (fuzz targets, the future
//! client's worker threads) as well as from Tokio.
//!
//! Everything here is derived solely from open, publicly documented
//! reverse-engineering of the wire format. No proprietary bytecode, decompiled
//! source, or private mapping was consulted.
```

`crates/net/Cargo.toml`:

```toml
[package]
name = "pyrite-net"
description = "Connection lifecycle and state machine for the Pyrite server engine."
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
authors.workspace = true

[lib]
name = "pyrite_net"

[dependencies]
pyrite-protocol.workspace = true
bytes.workspace = true
tokio.workspace = true
tokio-util.workspace = true
futures-util.workspace = true
serde_json.workspace = true
thiserror.workspace = true
tracing.workspace = true

[dev-dependencies]
tokio = { workspace = true, features = ["rt", "macros", "io-util", "time"] }

[lints]
workspace = true
```

`crates/net/src/lib.rs`:

```rust
//! Connection lifecycle management for the Pyrite server engine.
```

`crates/server/Cargo.toml`:

```toml
[package]
name = "pyrite-server"
description = "The Pyrite server executable."
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
authors.workspace = true

[[bin]]
name = "pyrite-server"
path = "src/main.rs"

[dependencies]
pyrite-net.workspace = true
pyrite-protocol.workspace = true
tokio.workspace = true
tracing.workspace = true
tracing-subscriber.workspace = true
clap.workspace = true

[lints]
workspace = true
```

`crates/server/src/main.rs` (replaced entirely in Task 12):

```rust
//! The Pyrite server executable.

fn main() {
    println!("pyrite-server: not yet wired up");
}
```

- [ ] **Step 3: Verify the workspace compiles**

Run: `cargo build --workspace`
Expected: three crates compile, zero warnings.

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 4: Write the licence files**

`LICENSE-MIT` — replace `<YEAR>` with `2026` and `<COPYRIGHT HOLDER>` with `The Pyrite Contributors`:

```
MIT License

Copyright (c) 2026 The Pyrite Contributors

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

`LICENSE-APACHE` — fetch the verbatim text rather than retyping it:

```bash
curl -fsSL https://www.apache.org/licenses/LICENSE-2.0.txt -o LICENSE-APACHE
```

Verify it is the full licence: `wc -l LICENSE-APACHE` should report roughly 200 lines.

- [ ] **Step 5: Write README.md**

```markdown
# Pyrite

**Project status: Pre-Alpha / Architecture R&D.** Nothing here is usable for
playing a game yet.

**What this is:** A clean-room, high-performance server engine written in Rust
that speaks the Minecraft network protocol, with a sandboxed WebAssembly mod
runtime.

**What this is NOT:** This is not Forge, Fabric, Paper, or Spigot. It cannot
run existing Java `.jar` mods or plugins, and it never will — mods target a
WebAssembly interface instead. It ships no game assets.

## Status

Milestone 1 (in progress): protocol foundations and Server List Ping.

## Building

Requires Rust 1.94 or newer.

```bash
cargo build --workspace
cargo run -p pyrite-server -- --bind 0.0.0.0:25565
```

## Legal

Pyrite is a clean-room implementation built only from open, publicly
documented reverse-engineering of the network protocol and file formats. It
contains no Mojang code, no decompiled bytecode, no private mappings, and no
proprietary assets. Pyrite is not affiliated with or endorsed by Mojang AB or
Microsoft.

## Licence

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your
option.
```

- [ ] **Step 6: Write CONTRIBUTING.md**

```markdown
# Contributing to Pyrite

## Clean-room policy — read this first

Pyrite is a clean-room project. This is not a stylistic preference; it is the
condition under which the project can exist.

**Submissions containing or derived from decompiled Mojang bytecode, private
mappings, or proprietary game assets will be immediately rejected and
deleted.** This applies to code, comments, test fixtures, and documentation.

If you have decompiled the game, you may still contribute, but you must not
transcribe, paraphrase, or work from what you saw. Work only from open,
publicly maintained documentation of the wire format and file formats, and
cite your source in the pull request.

By opening a pull request you affirm that your contribution meets this
standard.

## Dependency policy

Pyrite hand-writes everything Minecraft-specific: the network protocol, NBT,
and packet codecs. Third-party crates that implement Minecraft domain logic
are not accepted, because their provenance becomes ours.

- **Allowed:** general-purpose infrastructure — async runtimes, buffer and
  serialisation libraries, compression and cryptography primitives, ECS
  libraries, WebAssembly runtimes.
- **Not allowed:** crates implementing Minecraft protocol, NBT, Anvil/region
  IO, world generation, or game logic.
- **Licences:** every dependency must be MIT, Apache-2.0, BSD-2-Clause,
  BSD-3-Clause, ISC, or Unicode-3.0. CI enforces this with `cargo-deny`.

Adding a dependency requires a note in the pull request explaining why it is
infrastructure rather than domain logic.

## Before you open a pull request

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

All three must be clean. CI runs them on Linux, macOS, and Windows.

## Code standards

- No `unwrap()`, `expect()`, or `panic!()` on any path reachable from network
  input. Return a typed error instead. Clippy enforces this.
- No `todo!()` or `unimplemented!()` on merged code.
- Every public item carries a doc comment.
- Byte-level operations — bit shifts, masks, framing arithmetic — get a comment
  explaining what the bytes mean, not just what the code does.

## Licence

Contributions are dual-licensed under MIT or Apache-2.0, matching the project.
```

- [ ] **Step 7: Write the issue templates**

`.github/ISSUE_TEMPLATE/config.yml` — this is the file that actually disables free-form issues:

```yaml
blank_issues_enabled: false
contact_links:
  - name: Not a support forum
    url: https://github.com/pyrite-engine/pyrite/blob/main/README.md
    about: Pyrite is pre-alpha and cannot run Java mods. Please read the README before opening an issue.
```

`.github/ISSUE_TEMPLATE/bug_report.md`:

```markdown
---
name: Bug report
about: A reproducible defect in code that already exists
title: ''
labels: bug
---

**Affected crate**
pyrite-protocol / pyrite-net / pyrite-server

**Reproduction**
Exact steps, commands, and client version.

**Expected behaviour**

**Actual behaviour**
Include the full error and the relevant `RUST_LOG=debug` output.

**Environment**
- Pyrite commit:
- Rust version (`rustc -V`):
- OS:

**Clean-room confirmation**
- [ ] This report contains no decompiled code, private mappings, or proprietary assets.
```

`.github/ISSUE_TEMPLATE/technical_rfc.md`:

```markdown
---
name: Technical RFC
about: Propose a design change to an engine subsystem
title: 'RFC: '
labels: rfc
---

**Subsystem**
protocol / net / world / ecs / modloader / assets

**Problem**
What is wrong or missing today. Be concrete.

**Proposed design**
Include interfaces, data flow, and the failure modes you considered.

**Alternatives considered**
And why you rejected them.

**Performance impact**
Allocations per tick or per packet, memory footprint, effect on tick budget.

**Clean-room confirmation**
- [ ] This proposal is derived only from open documentation of the wire format.
```

- [ ] **Step 8: Write the licence audit config**

`deny.toml`:

```toml
[licenses]
version = 2
allow = [
    "MIT",
    "Apache-2.0",
    "Apache-2.0 WITH LLVM-exception",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "ISC",
    "Unicode-3.0",
    "Zlib",
]
confidence-threshold = 0.93

[bans]
multiple-versions = "warn"

[advisories]
version = 2
```

- [ ] **Step 9: Write the CI workflow**

`.github/workflows/ci.yml`:

```yaml
name: CI

on:
  push:
    branches: [main]
  pull_request:
    branches: [main]

env:
  CARGO_TERM_COLOR: always
  RUSTFLAGS: -D warnings

jobs:
  fmt:
    name: Format
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt
      - run: cargo fmt --all --check

  clippy:
    name: Clippy
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: clippy
      - uses: Swatinem/rust-cache@v2
      - run: cargo clippy --workspace --all-targets -- -D warnings

  test:
    name: Test (${{ matrix.os }})
    runs-on: ${{ matrix.os }}
    strategy:
      fail-fast: false
      matrix:
        os: [ubuntu-latest, macos-latest, windows-latest]
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: cargo test --workspace --all-targets

  licenses:
    name: Clean-room dependency audit
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: EmbarkStudios/cargo-deny-action@v2
        with:
          command: check licenses bans
```

- [ ] **Step 10: Verify everything still builds, then commit**

Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo build --workspace`
Expected: all clean.

```bash
git add -A
git commit -m "chore: scaffold cargo workspace, governance files, and CI"
```

---

## Task 2: Protocol error type and pinned version constants

**Files:**
- Create: `crates/protocol/src/error.rs`
- Create: `crates/protocol/src/version.rs`
- Modify: `crates/protocol/src/lib.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: `ProtocolError` (every later module returns it), `State`, `Direction` re-exported from `packets`, and `PROTOCOL_VERSION: i32 = 776`, `VERSION_NAME: &str = "26.2"`.

Note: `State` and `Direction` are defined here rather than in `packets/mod.rs`, because `ProtocolError::UnknownPacket` needs them and `error.rs` must not depend on `packets`. `packets/mod.rs` re-exports them in Task 6.

- [ ] **Step 1: Write `version.rs`**

```rust
//! The single pinned protocol version this build targets.
//!
//! Pyrite targets exactly one protocol version at a time (design decision D3).
//! Changing the version the engine speaks means changing these two constants
//! and auditing the packet ID tables in [`crate::packets`].
//!
//! Verified 2026-09-03 against public reverse-engineering documentation of the
//! Java Edition protocol.

/// The numeric protocol version sent in the handshake and advertised in the
/// status response. Java Edition 26.2.
pub const PROTOCOL_VERSION: i32 = 776;

/// The human-readable version name shown in a client's server list when the
/// client's own protocol version does not match [`PROTOCOL_VERSION`].
pub const VERSION_NAME: &str = "26.2";
```

- [ ] **Step 2: Write `error.rs`**

```rust
//! Typed errors for every failure mode in protocol encoding and decoding.

use std::fmt;

/// The connection state a packet belongs to.
///
/// A connection begins in [`State::Handshaking`] and moves to exactly one of
/// [`State::Status`] or [`State::Login`] based on the handshake's next-state
/// field. `Configuration` and `Play` are declared because the state machine and
/// error type name them; no packets target them in Milestone 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum State {
    /// The initial state of every connection.
    Handshaking,
    /// Server list ping.
    Status,
    /// Authentication and connection setup.
    Login,
    /// Post-login negotiation of registries and resource packs.
    Configuration,
    /// In-world gameplay.
    Play,
}

impl fmt::Display for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Handshaking => "handshaking",
            Self::Status => "status",
            Self::Login => "login",
            Self::Configuration => "configuration",
            Self::Play => "play",
        };
        f.write_str(name)
    }
}

/// Which way a packet travels on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Direction {
    /// Client to server.
    Serverbound,
    /// Server to client.
    Clientbound,
}

impl fmt::Display for Direction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Serverbound => "serverbound",
            Self::Clientbound => "clientbound",
        };
        f.write_str(name)
    }
}

/// Every way protocol encoding or decoding can fail.
#[derive(Debug, thiserror::Error)]
pub enum ProtocolError {
    /// A VarInt or VarLong ran past its maximum encoded length, which means the
    /// peer sent a value that cannot fit the target integer type.
    #[error("varint exceeded its maximum encoded length of {max} bytes")]
    VarIntTooLong {
        /// The maximum number of bytes permitted for this integer width.
        max: usize,
    },

    /// The buffer ended in the middle of a field that was already committed to.
    #[error("unexpected end of buffer while decoding")]
    UnexpectedEof,

    /// A length prefix was negative. Lengths are VarInts and so can carry a
    /// negative bit pattern, which is always a protocol violation.
    #[error("negative length prefix: {0}")]
    NegativeLength(i32),

    /// A frame declared a length above the maximum a length VarInt can express.
    #[error("frame length {len} exceeds maximum {max}")]
    FrameTooLarge {
        /// The declared length.
        len: usize,
        /// The maximum permitted length.
        max: usize,
    },

    /// A string's byte length exceeded the cap for its field.
    #[error("string length {len} bytes exceeds maximum {max} bytes")]
    StringTooLong {
        /// The declared byte length.
        len: usize,
        /// The maximum permitted byte length.
        max: usize,
    },

    /// A string field did not contain valid UTF-8.
    #[error("string field was not valid utf-8")]
    InvalidUtf8(#[from] std::str::Utf8Error),

    /// No packet is defined for this state, direction, and ID combination.
    #[error("unknown {direction} packet id {id:#04x} in state {state}")]
    UnknownPacket {
        /// The connection state the packet arrived in.
        state: State,
        /// The direction the packet travelled.
        direction: Direction,
        /// The packet ID that was not recognised.
        id: i32,
    },

    /// The handshake's next-state field was not 1, 2, or 3.
    #[error("invalid next state value {0}, expected 1 (status), 2 (login), or 3 (transfer)")]
    InvalidNextState(i32),

    /// A JSON payload failed to serialise or deserialise.
    #[error("json payload error")]
    Json(#[from] serde_json::Error),

    /// An underlying I/O error. Required because `tokio_util::codec::Decoder`
    /// mandates `From<std::io::Error>` on its error type.
    #[error("i/o error")]
    Io(#[from] std::io::Error),
}
```

- [ ] **Step 3: Wire the modules into `lib.rs`**

Append to `crates/protocol/src/lib.rs`:

```rust

pub mod error;
pub mod version;

pub use error::{Direction, ProtocolError, State};
pub use version::{PROTOCOL_VERSION, VERSION_NAME};
```

- [ ] **Step 4: Write a test that the constants and Display impls hold**

Create `crates/protocol/src/version.rs` test module at the bottom of that file:

```rust

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinned_version_matches_documented_constants() {
        // Guards against an accidental edit; these two must always change
        // together and must match what the status response advertises.
        assert_eq!(PROTOCOL_VERSION, 776);
        assert_eq!(VERSION_NAME, "26.2");
    }
}
```

- [ ] **Step 5: Run the tests**

Run: `cargo test -p pyrite-protocol`
Expected: PASS.

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add crates/protocol
git commit -m "feat(protocol): add typed errors, state/direction, and pinned version constants"
```

---

## Task 3: VarInt and VarLong primitives

**Files:**
- Create: `crates/protocol/src/varint.rs`
- Modify: `crates/protocol/src/lib.rs`

**Interfaces:**
- Consumes: `ProtocolError` from Task 2.
- Produces:
  - `pub fn write_varint<B: BufMut>(dst: &mut B, value: i32)`
  - `pub fn read_varint<B: Buf>(src: &mut B) -> Result<i32, ProtocolError>`
  - `pub fn read_varint_slice(src: &[u8]) -> Result<Option<(i32, usize)>, ProtocolError>`
  - `pub fn varint_len(value: i32) -> usize`
  - `pub fn write_varlong<B: BufMut>(dst: &mut B, value: i64)`
  - `pub fn read_varlong<B: Buf>(src: &mut B) -> Result<i64, ProtocolError>`
  - `pub const MAX_VARINT_LEN: usize = 5`, `pub const MAX_VARLONG_LEN: usize = 10`

- [ ] **Step 1: Write the failing tests**

Create `crates/protocol/src/varint.rs` containing *only* this test module for now:

```rust
//! Zero-allocation VarInt and VarLong codecs.

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use bytes::BytesMut;

    /// Encoded forms taken from public documentation of the wire format.
    const VARINT_VECTORS: &[(i32, &[u8])] = &[
        (0, &[0x00]),
        (1, &[0x01]),
        (2, &[0x02]),
        (127, &[0x7f]),
        (128, &[0x80, 0x01]),
        (255, &[0xff, 0x01]),
        (25565, &[0xdd, 0xc7, 0x01]),
        (2097151, &[0xff, 0xff, 0x7f]),
        (2147483647, &[0xff, 0xff, 0xff, 0xff, 0x07]),
        (-1, &[0xff, 0xff, 0xff, 0xff, 0x0f]),
        (-2147483648, &[0x80, 0x80, 0x80, 0x80, 0x08]),
    ];

    const VARLONG_VECTORS: &[(i64, &[u8])] = &[
        (0, &[0x00]),
        (127, &[0x7f]),
        (2147483647, &[0xff, 0xff, 0xff, 0xff, 0x07]),
        (
            9223372036854775807,
            &[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x7f],
        ),
        (
            -1,
            &[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x01],
        ),
        (
            -9223372036854775808,
            &[0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x01],
        ),
    ];

    #[test]
    fn varint_encodes_to_documented_bytes() {
        for (value, expected) in VARINT_VECTORS {
            let mut buf = BytesMut::new();
            write_varint(&mut buf, *value);
            assert_eq!(&buf[..], *expected, "encoding {value}");
        }
    }

    #[test]
    fn varint_decodes_documented_bytes() {
        for (expected, bytes) in VARINT_VECTORS {
            let mut src = *bytes;
            let decoded = read_varint(&mut src).unwrap();
            assert_eq!(decoded, *expected, "decoding {bytes:?}");
            assert!(src.is_empty(), "decoder must consume exactly the varint");
        }
    }

    #[test]
    fn varint_len_matches_encoded_length() {
        for (value, expected) in VARINT_VECTORS {
            assert_eq!(varint_len(*value), expected.len(), "length of {value}");
        }
    }

    #[test]
    fn varint_slice_decodes_and_reports_width() {
        for (expected, bytes) in VARINT_VECTORS {
            let decoded = read_varint_slice(bytes).unwrap();
            assert_eq!(decoded, Some((*expected, bytes.len())));
        }
    }

    #[test]
    fn varint_slice_returns_none_on_partial_input() {
        // Every proper prefix of a multi-byte varint is incomplete, never an
        // error: the framing codec relies on this to avoid consuming bytes it
        // cannot yet interpret.
        let full: &[u8] = &[0xff, 0xff, 0xff, 0xff, 0x0f];
        for split in 0..full.len() {
            assert_eq!(read_varint_slice(&full[..split]).unwrap(), None);
        }
    }

    #[test]
    fn varint_slice_ignores_trailing_bytes() {
        let buf: &[u8] = &[0x80, 0x01, 0xaa, 0xbb];
        assert_eq!(read_varint_slice(buf).unwrap(), Some((128, 2)));
    }

    #[test]
    fn varint_rejects_overlong_encoding() {
        // Six continuation bytes cannot fit an i32 and must be rejected rather
        // than silently wrapping.
        let mut src: &[u8] = &[0x80, 0x80, 0x80, 0x80, 0x80, 0x01];
        assert!(matches!(
            read_varint(&mut src),
            Err(ProtocolError::VarIntTooLong { max: 5 })
        ));
        assert!(matches!(
            read_varint_slice(&[0x80, 0x80, 0x80, 0x80, 0x80, 0x01]),
            Err(ProtocolError::VarIntTooLong { max: 5 })
        ));
    }

    #[test]
    fn varint_reports_eof_on_empty_buffer() {
        let mut src: &[u8] = &[];
        assert!(matches!(
            read_varint(&mut src),
            Err(ProtocolError::UnexpectedEof)
        ));
    }

    #[test]
    fn varlong_round_trips_documented_vectors() {
        for (value, expected) in VARLONG_VECTORS {
            let mut buf = BytesMut::new();
            write_varlong(&mut buf, *value);
            assert_eq!(&buf[..], *expected, "encoding {value}");

            let mut src = &buf[..];
            assert_eq!(read_varlong(&mut src).unwrap(), *value);
        }
    }

    #[test]
    fn varlong_rejects_overlong_encoding() {
        let mut src: &[u8] = &[0x80; 11];
        assert!(matches!(
            read_varlong(&mut src),
            Err(ProtocolError::VarIntTooLong { max: 10 })
        ));
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p pyrite-protocol --lib varint`
Expected: compile failure — `write_varint`, `read_varint`, `read_varint_slice`, `varint_len`, `write_varlong`, `read_varlong` are not defined.

Note: `varint.rs` is not yet declared in `lib.rs`, so add `pub mod varint;` to `lib.rs` first if the module is not being compiled at all.

- [ ] **Step 3: Write the implementation**

Insert above the test module in `crates/protocol/src/varint.rs`:

```rust
use bytes::{Buf, BufMut};

use crate::error::ProtocolError;

/// Maximum bytes a 32-bit VarInt can occupy.
///
/// Each byte carries 7 payload bits, so 32 bits needs `ceil(32 / 7) == 5`.
pub const MAX_VARINT_LEN: usize = 5;

/// Maximum bytes a 64-bit VarLong can occupy: `ceil(64 / 7) == 10`.
pub const MAX_VARLONG_LEN: usize = 10;

/// Bit 7 of each byte marks "another byte follows".
const CONTINUATION_BIT: u8 = 0x80;

/// Bits 0-6 of each byte carry payload.
const PAYLOAD_MASK: u8 = 0x7f;

/// Encodes `value` as a VarInt into `dst`.
///
/// The format is little-endian base-128: the low 7 bits of the value go into
/// the low 7 bits of the first byte, the next 7 bits into the second, and so
/// on. Every byte except the last sets the high continuation bit.
///
/// The value is reinterpreted as `u32` first so that the shift is logical
/// rather than arithmetic — an arithmetic right shift of a negative number
/// would keep feeding in sign bits and never terminate. This is why negative
/// values always occupy the full five bytes.
pub fn write_varint<B: BufMut>(dst: &mut B, value: i32) {
    let mut remaining = value as u32;
    loop {
        // If nothing outside the low 7 bits is left, this is the final byte.
        if remaining & !(PAYLOAD_MASK as u32) == 0 {
            dst.put_u8(remaining as u8);
            return;
        }
        dst.put_u8((remaining as u8 & PAYLOAD_MASK) | CONTINUATION_BIT);
        remaining >>= 7;
    }
}

/// Decodes a VarInt from `src`, advancing it past the bytes consumed.
///
/// Returns [`ProtocolError::UnexpectedEof`] if the buffer ends mid-value and
/// [`ProtocolError::VarIntTooLong`] if a sixth byte would be required.
pub fn read_varint<B: Buf>(src: &mut B) -> Result<i32, ProtocolError> {
    let mut result: u32 = 0;
    for index in 0..MAX_VARINT_LEN {
        if !src.has_remaining() {
            return Err(ProtocolError::UnexpectedEof);
        }
        let byte = src.get_u8();
        // Shift each 7-bit group into place. On the fifth byte the shift is 28,
        // so the top 4 payload bits land in bits 28-31 and any bits above that
        // are discarded — matching the reference encoding of negative values.
        result |= u32::from(byte & PAYLOAD_MASK) << (7 * index);
        if byte & CONTINUATION_BIT == 0 {
            return Ok(result as i32);
        }
    }
    Err(ProtocolError::VarIntTooLong {
        max: MAX_VARINT_LEN,
    })
}

/// Decodes a VarInt from the front of `src` **without consuming anything**,
/// returning the value and the number of bytes it occupies.
///
/// Returns `Ok(None)` when `src` holds only part of a VarInt. The framing codec
/// depends on this: a frame's length prefix arrives byte by byte over TCP, and
/// a consuming decoder would swallow bytes it cannot yet interpret, leaving the
/// stream permanently desynchronised.
pub fn read_varint_slice(src: &[u8]) -> Result<Option<(i32, usize)>, ProtocolError> {
    let mut result: u32 = 0;
    for index in 0..MAX_VARINT_LEN {
        let Some(&byte) = src.get(index) else {
            return Ok(None);
        };
        result |= u32::from(byte & PAYLOAD_MASK) << (7 * index);
        if byte & CONTINUATION_BIT == 0 {
            return Ok(Some((result as i32, index + 1)));
        }
    }
    Err(ProtocolError::VarIntTooLong {
        max: MAX_VARINT_LEN,
    })
}

/// Returns how many bytes `value` occupies when VarInt-encoded.
///
/// Used to size a length prefix without encoding twice.
pub fn varint_len(value: i32) -> usize {
    match value as u32 {
        0x0000_0000..=0x0000_007f => 1,
        0x0000_0080..=0x0000_3fff => 2,
        0x0000_4000..=0x001f_ffff => 3,
        0x0020_0000..=0x0fff_ffff => 4,
        _ => 5,
    }
}

/// Encodes `value` as a VarLong into `dst`.
///
/// Identical to [`write_varint`] but over 64 bits, so negative values occupy
/// the full ten bytes.
pub fn write_varlong<B: BufMut>(dst: &mut B, value: i64) {
    let mut remaining = value as u64;
    loop {
        if remaining & !(PAYLOAD_MASK as u64) == 0 {
            dst.put_u8(remaining as u8);
            return;
        }
        dst.put_u8((remaining as u8 & PAYLOAD_MASK) | CONTINUATION_BIT);
        remaining >>= 7;
    }
}

/// Decodes a VarLong from `src`, advancing it past the bytes consumed.
pub fn read_varlong<B: Buf>(src: &mut B) -> Result<i64, ProtocolError> {
    let mut result: u64 = 0;
    for index in 0..MAX_VARLONG_LEN {
        if !src.has_remaining() {
            return Err(ProtocolError::UnexpectedEof);
        }
        let byte = src.get_u8();
        result |= u64::from(byte & PAYLOAD_MASK) << (7 * index);
        if byte & CONTINUATION_BIT == 0 {
            return Ok(result as i64);
        }
    }
    Err(ProtocolError::VarIntTooLong {
        max: MAX_VARLONG_LEN,
    })
}
```

- [ ] **Step 4: Declare the module and run the tests**

Add to `crates/protocol/src/lib.rs`:

```rust
pub mod varint;
```

Run: `cargo test -p pyrite-protocol --lib varint`
Expected: all ten tests PASS.

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add crates/protocol
git commit -m "feat(protocol): add zero-allocation varint and varlong codecs"
```

---

## Task 4: Buffer primitives — strings, u16, i64

**Files:**
- Create: `crates/protocol/src/buf.rs`
- Modify: `crates/protocol/src/lib.rs`

**Interfaces:**
- Consumes: `write_varint`/`read_varint` from Task 3, `ProtocolError` from Task 2.
- Produces:
  - `pub fn write_string<B: BufMut>(dst: &mut B, value: &str)`
  - `pub fn read_string<B: Buf>(src: &mut B, max_chars: usize) -> Result<String, ProtocolError>`
  - `pub fn write_u16<B: BufMut>(dst: &mut B, value: u16)`
  - `pub fn read_u16<B: Buf>(src: &mut B) -> Result<u16, ProtocolError>`
  - `pub fn write_i64<B: BufMut>(dst: &mut B, value: i64)`
  - `pub fn read_i64<B: Buf>(src: &mut B) -> Result<i64, ProtocolError>`

- [ ] **Step 1: Write the failing tests**

Create `crates/protocol/src/buf.rs` with only this test module:

```rust
//! Primitive field codecs shared by every packet implementation.

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use bytes::BytesMut;

    #[test]
    fn string_round_trips() {
        for value in ["", "localhost", "a much longer server address", "ünïcödé ✔"] {
            let mut buf = BytesMut::new();
            write_string(&mut buf, value);
            let mut src = &buf[..];
            assert_eq!(read_string(&mut src, 255).unwrap(), value);
            assert!(src.is_empty());
        }
    }

    #[test]
    fn string_is_length_prefixed_in_bytes_not_chars() {
        // "é" is two UTF-8 bytes; the prefix counts bytes.
        let mut buf = BytesMut::new();
        write_string(&mut buf, "é");
        assert_eq!(&buf[..], &[0x02, 0xc3, 0xa9]);
    }

    #[test]
    fn string_rejects_declared_length_above_cap_before_allocating() {
        // Declares 300 bytes with a 4-char cap (12-byte limit). Must reject on
        // the prefix alone, without needing the body to be present.
        let mut buf = BytesMut::new();
        crate::varint::write_varint(&mut buf, 300);
        let mut src = &buf[..];
        assert!(matches!(
            read_string(&mut src, 4),
            Err(ProtocolError::StringTooLong { len: 300, max: 12 })
        ));
    }

    #[test]
    fn string_rejects_negative_length() {
        let mut buf = BytesMut::new();
        crate::varint::write_varint(&mut buf, -1);
        let mut src = &buf[..];
        assert!(matches!(
            read_string(&mut src, 255),
            Err(ProtocolError::NegativeLength(-1))
        ));
    }

    #[test]
    fn string_reports_eof_when_body_is_short() {
        let buf: &[u8] = &[0x05, b'a', b'b'];
        let mut src = buf;
        assert!(matches!(
            read_string(&mut src, 255),
            Err(ProtocolError::UnexpectedEof)
        ));
    }

    #[test]
    fn string_rejects_invalid_utf8() {
        let buf: &[u8] = &[0x02, 0xff, 0xfe];
        let mut src = buf;
        assert!(matches!(
            read_string(&mut src, 255),
            Err(ProtocolError::InvalidUtf8(_))
        ));
    }

    #[test]
    fn u16_is_big_endian() {
        let mut buf = BytesMut::new();
        write_u16(&mut buf, 25565);
        assert_eq!(&buf[..], &[0x63, 0xdd]);
        let mut src = &buf[..];
        assert_eq!(read_u16(&mut src).unwrap(), 25565);
    }

    #[test]
    fn i64_is_big_endian_and_round_trips() {
        for value in [0i64, -1, i64::MAX, i64::MIN, 0x0123_4567_89ab_cdef] {
            let mut buf = BytesMut::new();
            write_i64(&mut buf, value);
            assert_eq!(buf.len(), 8);
            let mut src = &buf[..];
            assert_eq!(read_i64(&mut src).unwrap(), value);
        }
    }

    #[test]
    fn fixed_width_reads_report_eof() {
        let mut short: &[u8] = &[0x00];
        assert!(matches!(
            read_u16(&mut short),
            Err(ProtocolError::UnexpectedEof)
        ));
        let mut short: &[u8] = &[0x00, 0x01, 0x02];
        assert!(matches!(
            read_i64(&mut short),
            Err(ProtocolError::UnexpectedEof)
        ));
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Add `pub mod buf;` to `crates/protocol/src/lib.rs`, then run:

Run: `cargo test -p pyrite-protocol --lib buf`
Expected: compile failure — the six functions are undefined.

- [ ] **Step 3: Write the implementation**

Insert above the test module in `crates/protocol/src/buf.rs`:

```rust
use bytes::{Buf, BufMut};

use crate::error::ProtocolError;
use crate::varint::{read_varint, write_varint};

/// Upper bound on UTF-8 bytes per declared character.
///
/// The protocol counts string length in UTF-16 code units. A character needing
/// two UTF-16 units (a surrogate pair) occupies four UTF-8 bytes, and one
/// needing a single unit occupies at most three, so three bytes per declared
/// character is a correct and tight upper bound.
const MAX_BYTES_PER_CHAR: usize = 3;

/// Writes a length-prefixed UTF-8 string.
///
/// The prefix is a VarInt counting **bytes**, not characters.
pub fn write_string<B: BufMut>(dst: &mut B, value: &str) {
    write_varint(dst, value.len() as i32);
    dst.put_slice(value.as_bytes());
}

/// Reads a length-prefixed UTF-8 string, rejecting anything longer than
/// `max_chars` characters.
///
/// The length cap is checked against the declared prefix *before* any
/// allocation, so a hostile peer cannot induce a large allocation by lying
/// about the length.
pub fn read_string<B: Buf>(src: &mut B, max_chars: usize) -> Result<String, ProtocolError> {
    let declared = read_varint(src)?;
    let len =
        usize::try_from(declared).map_err(|_| ProtocolError::NegativeLength(declared))?;

    let max = max_chars.saturating_mul(MAX_BYTES_PER_CHAR);
    if len > max {
        return Err(ProtocolError::StringTooLong { len, max });
    }
    if src.remaining() < len {
        return Err(ProtocolError::UnexpectedEof);
    }

    let mut bytes = vec![0u8; len];
    src.copy_to_slice(&mut bytes);
    String::from_utf8(bytes).map_err(|error| ProtocolError::InvalidUtf8(error.utf8_error()))
}

/// Writes an unsigned 16-bit integer in network byte order (big-endian).
pub fn write_u16<B: BufMut>(dst: &mut B, value: u16) {
    dst.put_u16(value);
}

/// Reads a big-endian unsigned 16-bit integer.
pub fn read_u16<B: Buf>(src: &mut B) -> Result<u16, ProtocolError> {
    if src.remaining() < 2 {
        return Err(ProtocolError::UnexpectedEof);
    }
    Ok(src.get_u16())
}

/// Writes a signed 64-bit integer in network byte order (big-endian).
pub fn write_i64<B: BufMut>(dst: &mut B, value: i64) {
    dst.put_i64(value);
}

/// Reads a big-endian signed 64-bit integer.
pub fn read_i64<B: Buf>(src: &mut B) -> Result<i64, ProtocolError> {
    if src.remaining() < 8 {
        return Err(ProtocolError::UnexpectedEof);
    }
    Ok(src.get_i64())
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p pyrite-protocol --lib buf`
Expected: all nine tests PASS.

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add crates/protocol
git commit -m "feat(protocol): add bounds-checked string and fixed-width field codecs"
```

---

## Task 5: The `Packet` trait

**Files:**
- Create: `crates/protocol/src/packets/mod.rs`
- Modify: `crates/protocol/src/lib.rs`

**Interfaces:**
- Consumes: `ProtocolError`, `State`, `Direction` from Task 2.
- Produces: `pub trait Packet` with `const ID: i32`, `const STATE: State`, `const DIRECTION: Direction`, `fn encode<B: BufMut>(&self, dst: &mut B) -> Result<(), ProtocolError>`, `fn decode<B: Buf>(src: &mut B) -> Result<Self, ProtocolError>`.

This task has no test of its own — a trait with no implementors cannot be exercised. It is verified by Task 6's tests. Keep it as its own commit because it is the interface every subsequent task depends on.

- [ ] **Step 1: Write the trait**

`crates/protocol/src/packets/mod.rs`:

```rust
//! Strongly typed packet definitions.
//!
//! Every packet is its own struct implementing [`Packet`]. There is
//! deliberately no per-state enum: an enum is sized to its largest variant,
//! which would make every small packet pay for the largest one once Play state
//! arrives. Dispatch is a hand-written match over `(state, id)` in the
//! networking layer.
//!
//! Both directions are implemented for every packet even when only one is used
//! by the server today. This is what lets the future Pyrite client share this
//! crate unchanged, and it makes every packet round-trip testable.

use bytes::{Buf, BufMut};

use crate::error::{Direction, ProtocolError, State};

pub mod handshake;
pub mod login;
pub mod status;

/// A single protocol packet.
///
/// Implementations encode and decode only the packet **body**. The length
/// prefix and packet ID VarInt are written by
/// [`crate::codec::PacketCodec`], which is the only place framing rules live.
pub trait Packet: Sized {
    /// The packet's ID within its state and direction.
    const ID: i32;

    /// The connection state this packet is valid in.
    const STATE: State;

    /// The direction this packet travels.
    const DIRECTION: Direction;

    /// Writes the packet body to `dst`.
    fn encode<B: BufMut>(&self, dst: &mut B) -> Result<(), ProtocolError>;

    /// Reads a packet body from `src`.
    ///
    /// Implementations must not assume `src` contains only this packet; they
    /// read exactly their own fields and leave the rest.
    fn decode<B: Buf>(src: &mut B) -> Result<Self, ProtocolError>;
}
```

- [ ] **Step 2: Create the three module files as empty stubs so the crate compiles**

These are filled in by Tasks 6, 7, and 8. Create each with only its module doc comment:

`crates/protocol/src/packets/handshake.rs`:

```rust
//! The handshake packet, sent once at the start of every connection.
```

`crates/protocol/src/packets/status.rs`:

```rust
//! Server list ping packets.
```

`crates/protocol/src/packets/login.rs`:

```rust
//! Login state packets.
```

- [ ] **Step 3: Wire into `lib.rs`**

Add to `crates/protocol/src/lib.rs`:

```rust
pub mod packets;

pub use packets::Packet;
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add crates/protocol
git commit -m "feat(protocol): add the Packet trait"
```

---

## Task 6: Handshake packet

**Files:**
- Modify: `crates/protocol/src/packets/handshake.rs`

**Interfaces:**
- Consumes: `Packet` trait (Task 5), `buf` helpers (Task 4), `varint` (Task 3).
- Produces:
  - `pub struct Handshake { pub protocol_version: i32, pub server_address: String, pub server_port: u16, pub next_state: NextState }`
  - `pub enum NextState { Status = 1, Login = 2, Transfer = 3 }`
  - `pub const MAX_SERVER_ADDRESS_CHARS: usize = 255`

- [ ] **Step 1: Write the failing tests**

Append to `crates/protocol/src/packets/handshake.rs`:

```rust

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use bytes::BytesMut;

    fn sample() -> Handshake {
        Handshake {
            protocol_version: 776,
            server_address: "localhost".to_owned(),
            server_port: 25565,
            next_state: NextState::Status,
        }
    }

    #[test]
    fn handshake_round_trips() {
        let original = sample();
        let mut buf = BytesMut::new();
        original.encode(&mut buf).unwrap();

        let mut src = &buf[..];
        let decoded = Handshake::decode(&mut src).unwrap();

        assert_eq!(decoded, original);
        assert!(src.is_empty(), "decode must consume the whole body");
    }

    #[test]
    fn handshake_encodes_expected_byte_layout() {
        let mut buf = BytesMut::new();
        sample().encode(&mut buf).unwrap();

        let expected: &[u8] = &[
            0x88, 0x06, // protocol version 776 as varint
            0x09, // server address length 9
            b'l', b'o', b'c', b'a', b'l', b'h', b'o', b's', b't', //
            0x63, 0xdd, // port 25565, big-endian
            0x01, // next state 1 (status)
        ];
        assert_eq!(&buf[..], expected);
    }

    #[test]
    fn handshake_constants_match_the_specification() {
        assert_eq!(Handshake::ID, 0x00);
        assert_eq!(Handshake::STATE, State::Handshaking);
        assert_eq!(Handshake::DIRECTION, Direction::Serverbound);
    }

    #[test]
    fn next_state_accepts_all_three_documented_values() {
        for (raw, expected) in [
            (1, NextState::Status),
            (2, NextState::Login),
            (3, NextState::Transfer),
        ] {
            assert_eq!(NextState::try_from(raw).unwrap(), expected);
        }
    }

    #[test]
    fn next_state_rejects_undefined_values() {
        for raw in [0, 4, -1, 999] {
            assert!(matches!(
                NextState::try_from(raw),
                Err(ProtocolError::InvalidNextState(value)) if value == raw
            ));
        }
    }

    #[test]
    fn handshake_rejects_oversized_server_address() {
        // Declares a 1000-byte address; the cap is 255 chars => 765 bytes.
        let mut buf = BytesMut::new();
        crate::varint::write_varint(&mut buf, 776);
        crate::varint::write_varint(&mut buf, 1000);
        let mut src = &buf[..];
        assert!(matches!(
            Handshake::decode(&mut src),
            Err(ProtocolError::StringTooLong { len: 1000, max: 765 })
        ));
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p pyrite-protocol --lib handshake`
Expected: compile failure — `Handshake` and `NextState` are undefined.

- [ ] **Step 3: Write the implementation**

Insert above the test module in `crates/protocol/src/packets/handshake.rs`:

```rust
use bytes::{Buf, BufMut};

use crate::buf::{read_string, read_u16, write_string, write_u16};
use crate::error::{Direction, ProtocolError, State};
use crate::packets::Packet;
use crate::varint::{read_varint, write_varint};

/// Maximum length of the server address field, in characters.
pub const MAX_SERVER_ADDRESS_CHARS: usize = 255;

/// The state a client asks to move into after the handshake.
///
/// Encoded as a VarInt. Any value other than 1, 2, or 3 is a protocol
/// violation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum NextState {
    /// Server list ping. The connection is closed afterwards.
    Status = 1,
    /// Begin authentication and join the server.
    Login = 2,
    /// The client was transferred here by another server.
    Transfer = 3,
}

impl TryFrom<i32> for NextState {
    type Error = ProtocolError;

    fn try_from(value: i32) -> Result<Self, ProtocolError> {
        match value {
            1 => Ok(Self::Status),
            2 => Ok(Self::Login),
            3 => Ok(Self::Transfer),
            other => Err(ProtocolError::InvalidNextState(other)),
        }
    }
}

/// The first packet of every connection.
///
/// It is the only packet in [`State::Handshaking`], and it determines which
/// state the connection moves into next.
///
/// `server_address` and `server_port` record what hostname the client dialled.
/// They are informational — clients routinely send values rewritten by proxies
/// or SRV lookups — and must never be trusted for authorisation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Handshake {
    /// The protocol version the client speaks. `-1` means the client is
    /// probing to discover the server's version.
    pub protocol_version: i32,
    /// The hostname or IP the client used to reach this server.
    pub server_address: String,
    /// The port the client connected to.
    pub server_port: u16,
    /// The state the client wants to enter next.
    pub next_state: NextState,
}

impl Packet for Handshake {
    const ID: i32 = 0x00;
    const STATE: State = State::Handshaking;
    const DIRECTION: Direction = Direction::Serverbound;

    fn encode<B: BufMut>(&self, dst: &mut B) -> Result<(), ProtocolError> {
        write_varint(dst, self.protocol_version);
        write_string(dst, &self.server_address);
        write_u16(dst, self.server_port);
        write_varint(dst, self.next_state as i32);
        Ok(())
    }

    fn decode<B: Buf>(src: &mut B) -> Result<Self, ProtocolError> {
        let protocol_version = read_varint(src)?;
        let server_address = read_string(src, MAX_SERVER_ADDRESS_CHARS)?;
        let server_port = read_u16(src)?;
        let next_state = NextState::try_from(read_varint(src)?)?;

        Ok(Self {
            protocol_version,
            server_address,
            server_port,
            next_state,
        })
    }
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p pyrite-protocol --lib handshake`
Expected: all six tests PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/protocol
git commit -m "feat(protocol): add handshake packet with next-state validation"
```

---

## Task 7: Text component and status response JSON types

**Files:**
- Create: `crates/protocol/src/text.rs`
- Modify: `crates/protocol/src/lib.rs`

**Interfaces:**
- Consumes: `serde`.
- Produces: `pub struct TextComponent` with `TextComponent::new(impl Into<String>)`, `Serialize`, `Deserialize`, `PartialEq`, `Clone`.

- [ ] **Step 1: Write the failing tests**

Create `crates/protocol/src/text.rs` with only this test module:

```rust
//! Typed chat/text components.

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn plain_component_serialises_to_just_text() {
        let component = TextComponent::new("A Pyrite Server");
        let json = serde_json::to_string(&component).unwrap();
        assert_eq!(json, r#"{"text":"A Pyrite Server"}"#);
    }

    #[test]
    fn optional_fields_are_omitted_when_unset() {
        let json = serde_json::to_value(TextComponent::new("hi")).unwrap();
        let object = json.as_object().unwrap();
        assert!(!object.contains_key("color"));
        assert!(!object.contains_key("bold"));
        assert!(!object.contains_key("extra"));
    }

    #[test]
    fn styled_component_round_trips() {
        let mut component = TextComponent::new("Pyrite");
        component.color = Some("gold".to_owned());
        component.bold = Some(true);
        component.extra = vec![TextComponent::new(" engine")];

        let json = serde_json::to_string(&component).unwrap();
        let decoded: TextComponent = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, component);
    }

    #[test]
    fn deserialises_a_component_with_only_text() {
        let decoded: TextComponent = serde_json::from_str(r#"{"text":"hello"}"#).unwrap();
        assert_eq!(decoded, TextComponent::new("hello"));
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Add `pub mod text;` to `crates/protocol/src/lib.rs`, then run:

Run: `cargo test -p pyrite-protocol --lib text`
Expected: compile failure — `TextComponent` is undefined.

- [ ] **Step 3: Write the implementation**

Insert above the test module in `crates/protocol/src/text.rs`:

```rust
use serde::{Deserialize, Serialize};

/// A chat/text component.
///
/// Deliberately minimal: this covers what the MOTD and disconnect reasons need
/// today. It grows when chat lands in a later milestone. Building the MOTD from
/// this struct rather than formatting a JSON string by hand makes malformed
/// output unrepresentable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextComponent {
    /// The literal text of this component.
    pub text: String,

    /// A named colour, for example `"gold"`, or a `"#rrggbb"` literal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,

    /// Renders bold when `Some(true)`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bold: Option<bool>,

    /// Renders italic when `Some(true)`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub italic: Option<bool>,

    /// Child components, appended after this one and inheriting its styling.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra: Vec<TextComponent>,
}

impl TextComponent {
    /// Creates an unstyled component carrying only literal text.
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            color: None,
            bold: None,
            italic: None,
            extra: Vec::new(),
        }
    }
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p pyrite-protocol --lib text`
Expected: all four tests PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/protocol
git commit -m "feat(protocol): add typed text component"
```

---

## Task 8: Status packets and response payload

**Files:**
- Modify: `crates/protocol/src/packets/status.rs`

**Interfaces:**
- Consumes: `Packet` (Task 5), `buf` (Task 4), `TextComponent` (Task 7), version constants (Task 2).
- Produces:
  - `pub struct StatusRequest`
  - `pub struct StatusResponse { pub json: String }`
  - `pub struct PingRequest { pub payload: i64 }`
  - `pub struct PongResponse { pub payload: i64 }`
  - `pub struct StatusResponseJson { pub version: VersionInfo, pub players: PlayersInfo, pub description: TextComponent, pub favicon: Option<String>, pub enforces_secure_chat: bool }`
  - `pub struct VersionInfo { pub name: String, pub protocol: i32 }`
  - `pub struct PlayersInfo { pub max: i32, pub online: i32, pub sample: Vec<SamplePlayer> }`
  - `pub struct SamplePlayer { pub name: String, pub id: String }`
  - `pub const MAX_STATUS_JSON_CHARS: usize = 32767`

- [ ] **Step 1: Write the failing tests**

Append to `crates/protocol/src/packets/status.rs`:

```rust

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use bytes::BytesMut;

    #[test]
    fn status_request_is_an_empty_body() {
        let mut buf = BytesMut::new();
        StatusRequest.encode(&mut buf).unwrap();
        assert!(buf.is_empty(), "status request carries no fields");

        let mut src = &buf[..];
        StatusRequest::decode(&mut src).unwrap();
    }

    #[test]
    fn packet_constants_match_the_specification() {
        assert_eq!(StatusRequest::ID, 0x00);
        assert_eq!(StatusRequest::DIRECTION, Direction::Serverbound);
        assert_eq!(StatusResponse::ID, 0x00);
        assert_eq!(StatusResponse::DIRECTION, Direction::Clientbound);
        assert_eq!(PingRequest::ID, 0x01);
        assert_eq!(PingRequest::DIRECTION, Direction::Serverbound);
        assert_eq!(PongResponse::ID, 0x01);
        assert_eq!(PongResponse::DIRECTION, Direction::Clientbound);

        for state in [
            StatusRequest::STATE,
            StatusResponse::STATE,
            PingRequest::STATE,
            PongResponse::STATE,
        ] {
            assert_eq!(state, State::Status);
        }
    }

    #[test]
    fn status_response_round_trips() {
        let original = StatusResponse {
            json: r#"{"version":{"name":"26.2","protocol":776}}"#.to_owned(),
        };
        let mut buf = BytesMut::new();
        original.encode(&mut buf).unwrap();
        let mut src = &buf[..];
        assert_eq!(StatusResponse::decode(&mut src).unwrap(), original);
        assert!(src.is_empty());
    }

    #[test]
    fn ping_and_pong_round_trip_every_payload_bit() {
        for payload in [0i64, 1, -1, i64::MAX, i64::MIN, 0x0123_4567_89ab_cdef] {
            let mut buf = BytesMut::new();
            PingRequest { payload }.encode(&mut buf).unwrap();
            assert_eq!(buf.len(), 8, "payload is a fixed-width long");

            let mut src = &buf[..];
            assert_eq!(PingRequest::decode(&mut src).unwrap().payload, payload);

            let mut buf = BytesMut::new();
            PongResponse { payload }.encode(&mut buf).unwrap();
            let mut src = &buf[..];
            assert_eq!(PongResponse::decode(&mut src).unwrap().payload, payload);
        }
    }

    #[test]
    fn status_json_serialises_the_documented_shape() {
        let status = StatusResponseJson::new(
            TextComponent::new("A Pyrite Server"),
            100,
            0,
        );
        let value = serde_json::to_value(&status).unwrap();

        assert_eq!(value["version"]["name"], "26.2");
        assert_eq!(value["version"]["protocol"], 776);
        assert_eq!(value["players"]["max"], 100);
        assert_eq!(value["players"]["online"], 0);
        assert_eq!(value["description"]["text"], "A Pyrite Server");
        assert_eq!(value["enforcesSecureChat"], false);
        assert!(
            value.get("favicon").is_none(),
            "favicon is omitted when unset"
        );
    }

    #[test]
    fn status_json_round_trips() {
        let original =
            StatusResponseJson::new(TextComponent::new("Pyrite"), 20, 3);
        let encoded = serde_json::to_string(&original).unwrap();
        let decoded: StatusResponseJson = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded.version.protocol, original.version.protocol);
        assert_eq!(decoded.players.max, 20);
        assert_eq!(decoded.players.online, 3);
        assert_eq!(decoded.description, original.description);
    }

    #[test]
    fn status_response_rejects_oversized_json_prefix() {
        let mut buf = BytesMut::new();
        // 32767 chars => 98301 byte cap; declare one byte more.
        crate::varint::write_varint(&mut buf, 98_302);
        let mut src = &buf[..];
        assert!(matches!(
            StatusResponse::decode(&mut src),
            Err(ProtocolError::StringTooLong { .. })
        ));
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p pyrite-protocol --lib status`
Expected: compile failure — the packet and JSON types are undefined.

- [ ] **Step 3: Write the implementation**

Insert above the test module in `crates/protocol/src/packets/status.rs`:

```rust
use bytes::{Buf, BufMut};
use serde::{Deserialize, Serialize};

use crate::buf::{read_i64, read_string, write_i64, write_string};
use crate::error::{Direction, ProtocolError, State};
use crate::packets::Packet;
use crate::text::TextComponent;
use crate::version::{PROTOCOL_VERSION, VERSION_NAME};

/// Maximum length of the status response JSON string, in characters.
pub const MAX_STATUS_JSON_CHARS: usize = 32767;

/// Sent by the client to ask for the server's status.
///
/// Carries no fields. A client is permitted to skip this packet entirely and
/// send [`PingRequest`] straight after the handshake, so the server must not
/// require it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatusRequest;

impl Packet for StatusRequest {
    const ID: i32 = 0x00;
    const STATE: State = State::Status;
    const DIRECTION: Direction = Direction::Serverbound;

    fn encode<B: BufMut>(&self, _dst: &mut B) -> Result<(), ProtocolError> {
        Ok(())
    }

    fn decode<B: Buf>(_src: &mut B) -> Result<Self, ProtocolError> {
        Ok(Self)
    }
}

/// The server's answer to [`StatusRequest`]: one JSON string.
///
/// Build the payload with [`StatusResponseJson`] rather than formatting the
/// string by hand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusResponse {
    /// The serialised [`StatusResponseJson`] document.
    pub json: String,
}

impl Packet for StatusResponse {
    const ID: i32 = 0x00;
    const STATE: State = State::Status;
    const DIRECTION: Direction = Direction::Clientbound;

    fn encode<B: BufMut>(&self, dst: &mut B) -> Result<(), ProtocolError> {
        write_string(dst, &self.json);
        Ok(())
    }

    fn decode<B: Buf>(src: &mut B) -> Result<Self, ProtocolError> {
        Ok(Self {
            json: read_string(src, MAX_STATUS_JSON_CHARS)?,
        })
    }
}

/// A latency probe. The payload is opaque and must be echoed verbatim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PingRequest {
    /// Arbitrary client-chosen value, conventionally a millisecond timestamp.
    pub payload: i64,
}

impl Packet for PingRequest {
    const ID: i32 = 0x01;
    const STATE: State = State::Status;
    const DIRECTION: Direction = Direction::Serverbound;

    fn encode<B: BufMut>(&self, dst: &mut B) -> Result<(), ProtocolError> {
        write_i64(dst, self.payload);
        Ok(())
    }

    fn decode<B: Buf>(src: &mut B) -> Result<Self, ProtocolError> {
        Ok(Self {
            payload: read_i64(src)?,
        })
    }
}

/// The echo of a [`PingRequest`].
///
/// The client subtracts its own send time from its receive time to display
/// latency, so the payload must be returned bit-for-bit and the response must
/// not be delayed by server-side work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PongResponse {
    /// The payload copied verbatim from the ping.
    pub payload: i64,
}

impl Packet for PongResponse {
    const ID: i32 = 0x01;
    const STATE: State = State::Status;
    const DIRECTION: Direction = Direction::Clientbound;

    fn encode<B: BufMut>(&self, dst: &mut B) -> Result<(), ProtocolError> {
        write_i64(dst, self.payload);
        Ok(())
    }

    fn decode<B: Buf>(src: &mut B) -> Result<Self, ProtocolError> {
        Ok(Self {
            payload: read_i64(src)?,
        })
    }
}

/// The JSON document carried inside [`StatusResponse`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusResponseJson {
    /// Version name and protocol number.
    pub version: VersionInfo,
    /// Player counts and the hover sample.
    pub players: PlayersInfo,
    /// The MOTD.
    pub description: TextComponent,
    /// Optional base64-encoded 64x64 PNG, prefixed `data:image/png;base64,`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub favicon: Option<String>,
    /// Whether the server requires cryptographically signed chat.
    #[serde(rename = "enforcesSecureChat")]
    pub enforces_secure_chat: bool,
}

impl StatusResponseJson {
    /// Builds a status document advertising this build's pinned version.
    pub fn new(description: TextComponent, max_players: i32, online_players: i32) -> Self {
        Self {
            version: VersionInfo {
                name: VERSION_NAME.to_owned(),
                protocol: PROTOCOL_VERSION,
            },
            players: PlayersInfo {
                max: max_players,
                online: online_players,
                sample: Vec::new(),
            },
            description,
            favicon: None,
            enforces_secure_chat: false,
        }
    }
}

/// The `version` object of the status document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionInfo {
    /// Displayed to the client when its protocol number does not match.
    pub name: String,
    /// The protocol number the server speaks.
    pub protocol: i32,
}

/// The `players` object of the status document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayersInfo {
    /// Slot count shown to the client.
    pub max: i32,
    /// Currently connected players.
    pub online: i32,
    /// Names shown when hovering the player count. May be empty.
    #[serde(default)]
    pub sample: Vec<SamplePlayer>,
}

/// One entry in the hover sample list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SamplePlayer {
    /// The displayed name.
    pub name: String,
    /// The player's UUID in hyphenated string form.
    pub id: String,
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p pyrite-protocol --lib status`
Expected: all seven tests PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/protocol
git commit -m "feat(protocol): add status, ping, and pong packets with typed json payload"
```

---

## Task 9: Login disconnect packet

**Files:**
- Modify: `crates/protocol/src/packets/login.rs`

**Interfaces:**
- Consumes: `Packet` (Task 5), `buf` (Task 4), `TextComponent` (Task 7).
- Produces: `pub struct LoginDisconnect { pub reason: String }` with `LoginDisconnect::from_component(&TextComponent) -> Result<Self, ProtocolError>`, and `pub const MAX_DISCONNECT_REASON_CHARS: usize = 262144`.

- [ ] **Step 1: Write the failing tests**

Append to `crates/protocol/src/packets/login.rs`:

```rust

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use bytes::BytesMut;

    #[test]
    fn login_disconnect_constants_match_the_specification() {
        assert_eq!(LoginDisconnect::ID, 0x00);
        assert_eq!(LoginDisconnect::STATE, State::Login);
        assert_eq!(LoginDisconnect::DIRECTION, Direction::Clientbound);
    }

    #[test]
    fn login_disconnect_round_trips() {
        let original = LoginDisconnect {
            reason: r#"{"text":"nope"}"#.to_owned(),
        };
        let mut buf = BytesMut::new();
        original.encode(&mut buf).unwrap();
        let mut src = &buf[..];
        assert_eq!(LoginDisconnect::decode(&mut src).unwrap(), original);
    }

    #[test]
    fn from_component_produces_valid_json() {
        let packet =
            LoginDisconnect::from_component(&TextComponent::new("Login not implemented"))
                .unwrap();
        let value: serde_json::Value = serde_json::from_str(&packet.reason).unwrap();
        assert_eq!(value["text"], "Login not implemented");
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p pyrite-protocol --lib login`
Expected: compile failure — `LoginDisconnect` is undefined.

- [ ] **Step 3: Write the implementation**

Insert above the test module in `crates/protocol/src/packets/login.rs`:

```rust
use bytes::{Buf, BufMut};

use crate::buf::{read_string, write_string};
use crate::error::{Direction, ProtocolError, State};
use crate::packets::Packet;
use crate::text::TextComponent;

/// Maximum length of the disconnect reason, in characters.
pub const MAX_DISCONNECT_REASON_CHARS: usize = 262_144;

/// Tells a client in [`State::Login`] why it is being refused, then the
/// connection closes.
///
/// The reason is a JSON text component encoded as a string. This differs from
/// the play-state disconnect packet, which carries a binary component instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginDisconnect {
    /// A JSON-encoded [`TextComponent`].
    pub reason: String,
}

impl LoginDisconnect {
    /// Builds a disconnect packet from a text component.
    pub fn from_component(reason: &TextComponent) -> Result<Self, ProtocolError> {
        Ok(Self {
            reason: serde_json::to_string(reason)?,
        })
    }
}

impl Packet for LoginDisconnect {
    const ID: i32 = 0x00;
    const STATE: State = State::Login;
    const DIRECTION: Direction = Direction::Clientbound;

    fn encode<B: BufMut>(&self, dst: &mut B) -> Result<(), ProtocolError> {
        write_string(dst, &self.reason);
        Ok(())
    }

    fn decode<B: Buf>(src: &mut B) -> Result<Self, ProtocolError> {
        Ok(Self {
            reason: read_string(src, MAX_DISCONNECT_REASON_CHARS)?,
        })
    }
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p pyrite-protocol --lib login`
Expected: all three tests PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/protocol
git commit -m "feat(protocol): add clientbound login disconnect packet"
```

---

## Task 10: The framing codec

**Files:**
- Create: `crates/protocol/src/codec.rs`
- Modify: `crates/protocol/src/lib.rs`

**Interfaces:**
- Consumes: `varint` (Task 3), `Packet` (Task 5), `ProtocolError` (Task 2).
- Produces:
  - `pub struct RawPacket { pub id: i32, pub body: Bytes }`
  - `pub struct PacketCodec` with `PacketCodec::new()`, `Default`
  - `pub const MAX_PACKET_SIZE: usize = 2_097_151`
  - `impl Decoder for PacketCodec { type Item = RawPacket; type Error = ProtocolError; }`
  - `impl<P: Packet> Encoder<P> for PacketCodec { type Error = ProtocolError; }`
  - `RawPacket::decode_as::<P: Packet>(&self) -> Result<P, ProtocolError>`

- [ ] **Step 1: Write the failing tests**

Create `crates/protocol/src/codec.rs` with only this test module:

```rust
//! Length-prefixed packet framing.

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use crate::packets::handshake::{Handshake, NextState};
    use crate::packets::status::{PingRequest, StatusResponse};

    fn encoded_handshake() -> BytesMut {
        let mut codec = PacketCodec::new();
        let mut buf = BytesMut::new();
        codec
            .encode(
                Handshake {
                    protocol_version: 776,
                    server_address: "localhost".to_owned(),
                    server_port: 25565,
                    next_state: NextState::Status,
                },
                &mut buf,
            )
            .unwrap();
        buf
    }

    #[test]
    fn encoded_frame_is_length_then_id_then_body() {
        let buf = encoded_handshake();
        // Body is 15 bytes (see the handshake byte-layout test) and the id
        // varint is 1 byte, so the length prefix is 16.
        assert_eq!(buf[0], 16);
        assert_eq!(buf[1], 0x00, "packet id");
        assert_eq!(buf.len(), 17);
    }

    #[test]
    fn decodes_a_complete_frame() {
        let mut codec = PacketCodec::new();
        let mut buf = encoded_handshake();
        let packet = codec.decode(&mut buf).unwrap().unwrap();
        assert_eq!(packet.id, 0x00);
        assert!(buf.is_empty(), "the frame is fully consumed");

        let handshake: Handshake = packet.decode_as().unwrap();
        assert_eq!(handshake.server_address, "localhost");
        assert_eq!(handshake.next_state, NextState::Status);
    }

    #[test]
    fn returns_none_for_every_partial_prefix() {
        let full = encoded_handshake();
        let mut codec = PacketCodec::new();

        for split in 0..full.len() {
            let mut partial = BytesMut::from(&full[..split]);
            assert_eq!(
                codec.decode(&mut partial).unwrap().map(|p| p.id),
                None,
                "a {split}-byte prefix must decode to None"
            );
            assert_eq!(
                partial.len(),
                split,
                "an incomplete frame must not consume bytes"
            );
        }
    }

    #[test]
    fn feeding_one_byte_at_a_time_yields_exactly_one_frame() {
        let full = encoded_handshake();
        let mut codec = PacketCodec::new();
        let mut buf = BytesMut::new();
        let mut frames = 0;

        for byte in full.iter() {
            buf.extend_from_slice(&[*byte]);
            if codec.decode(&mut buf).unwrap().is_some() {
                frames += 1;
            }
        }

        assert_eq!(frames, 1);
        assert!(buf.is_empty());
    }

    #[test]
    fn decodes_two_frames_from_one_buffer() {
        let mut codec = PacketCodec::new();
        let mut buf = encoded_handshake();
        let second = encoded_handshake();
        buf.extend_from_slice(&second);

        assert!(codec.decode(&mut buf).unwrap().is_some());
        assert!(codec.decode(&mut buf).unwrap().is_some());
        assert!(codec.decode(&mut buf).unwrap().is_none());
        assert!(buf.is_empty());
    }

    #[test]
    fn rejects_a_length_prefix_above_the_maximum() {
        let mut codec = PacketCodec::new();
        let mut buf = BytesMut::new();
        crate::varint::write_varint(&mut buf, (MAX_PACKET_SIZE + 1) as i32);
        assert!(matches!(
            codec.decode(&mut buf),
            Err(ProtocolError::FrameTooLarge { .. })
        ));
    }

    #[test]
    fn rejects_a_negative_length_prefix() {
        let mut codec = PacketCodec::new();
        let mut buf = BytesMut::new();
        crate::varint::write_varint(&mut buf, -1);
        assert!(matches!(
            codec.decode(&mut buf),
            Err(ProtocolError::NegativeLength(-1))
        ));
    }

    #[test]
    fn round_trips_a_clientbound_packet() {
        let mut codec = PacketCodec::new();
        let mut buf = BytesMut::new();
        codec
            .encode(
                StatusResponse {
                    json: r#"{"text":"x"}"#.to_owned(),
                },
                &mut buf,
            )
            .unwrap();

        let frame = codec.decode(&mut buf).unwrap().unwrap();
        assert_eq!(frame.id, StatusResponse::ID);
        let decoded: StatusResponse = frame.decode_as().unwrap();
        assert_eq!(decoded.json, r#"{"text":"x"}"#);
    }

    #[test]
    fn decode_as_rejects_trailing_bytes() {
        // A body longer than the packet's fields indicates a desynchronised
        // stream or a malformed peer; it must not be silently ignored.
        let mut codec = PacketCodec::new();
        let mut buf = BytesMut::new();
        codec.encode(PingRequest { payload: 7 }, &mut buf).unwrap();
        // Rewrite the frame with one extra body byte.
        let mut tampered = BytesMut::new();
        crate::varint::write_varint(&mut tampered, 10);
        tampered.extend_from_slice(&buf[1..]);
        tampered.extend_from_slice(&[0xaa]);

        let frame = codec.decode(&mut tampered).unwrap().unwrap();
        assert!(matches!(
            frame.decode_as::<PingRequest>(),
            Err(ProtocolError::TrailingBytes { remaining: 1 })
        ));
    }
}
```

Note: this introduces one new error variant, `TrailingBytes { remaining: usize }`. Step 3 adds it.

- [ ] **Step 2: Run the tests to verify they fail**

Add `pub mod codec;` to `crates/protocol/src/lib.rs`, then run:

Run: `cargo test -p pyrite-protocol --lib codec`
Expected: compile failure — `PacketCodec`, `RawPacket`, `MAX_PACKET_SIZE` are undefined.

- [ ] **Step 3: Add the `TrailingBytes` error variant**

Add to `ProtocolError` in `crates/protocol/src/error.rs`:

```rust
    /// A packet body contained more bytes than the packet's fields consume.
    #[error("packet body had {remaining} trailing bytes")]
    TrailingBytes {
        /// How many bytes were left over.
        remaining: usize,
    },
```

- [ ] **Step 4: Write the codec implementation**

Insert above the test module in `crates/protocol/src/codec.rs`:

```rust
use bytes::{Buf, Bytes, BytesMut};
use tokio_util::codec::{Decoder, Encoder};

use crate::error::ProtocolError;
use crate::packets::Packet;
use crate::varint::{read_varint, read_varint_slice, write_varint};

/// The largest packet body Pyrite will accept, in bytes.
///
/// A frame's length prefix is a VarInt; three bytes carry 21 payload bits, so
/// `2^21 - 1` is the largest length expressible in the conventional prefix
/// width. Anything larger is treated as a malformed or hostile peer.
pub const MAX_PACKET_SIZE: usize = 2_097_151;

/// A decoded frame: its packet ID and its still-encoded body.
///
/// `body` is a [`Bytes`] slice sharing the read buffer's allocation, so
/// framing does not copy the payload. Turn it into a typed packet with
/// [`RawPacket::decode_as`].
#[derive(Debug, Clone)]
pub struct RawPacket {
    /// The packet ID read from the front of the frame.
    pub id: i32,
    /// The remaining bytes of the frame, after the ID.
    pub body: Bytes,
}

impl RawPacket {
    /// Decodes this frame's body as packet type `P`.
    ///
    /// Returns [`ProtocolError::TrailingBytes`] if `P` does not consume the
    /// whole body, which indicates a malformed peer or a version mismatch
    /// rather than something safe to ignore.
    pub fn decode_as<P: Packet>(&self) -> Result<P, ProtocolError> {
        let mut body = self.body.clone();
        let packet = P::decode(&mut body)?;
        if body.has_remaining() {
            return Err(ProtocolError::TrailingBytes {
                remaining: body.remaining(),
            });
        }
        Ok(packet)
    }
}

/// Length-prefixed packet framing.
///
/// Uncompressed frame layout:
///
/// ```text
/// +------------------+------------------+------------------+
/// | VarInt length    | VarInt packet id | body             |
/// +------------------+------------------+------------------+
/// ```
///
/// `length` counts the packet ID plus the body, and does not count itself.
#[derive(Debug, Default)]
pub struct PacketCodec {
    /// Reused encode scratch buffer, so encoding a packet does not allocate
    /// once the connection has warmed up.
    scratch: BytesMut,

    /// Compression threshold, set once the server enables compression during
    /// login. `None` means every frame is sent uncompressed, which is the only
    /// mode Milestone 1 implements. This is a field rather than a stub so the
    /// codec is fully functional today.
    compression_threshold: Option<i32>,
}

impl PacketCodec {
    /// Creates a codec in uncompressed, unencrypted mode.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the active compression threshold, if compression is enabled.
    pub fn compression_threshold(&self) -> Option<i32> {
        self.compression_threshold
    }
}

impl Decoder for PacketCodec {
    type Item = RawPacket;
    type Error = ProtocolError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<RawPacket>, ProtocolError> {
        // Read the length prefix without consuming it: TCP delivers the prefix
        // in arbitrary fragments, and consuming a partial VarInt would
        // desynchronise the stream for good.
        let Some((declared, prefix_len)) = read_varint_slice(src)? else {
            return Ok(None);
        };

        let body_len =
            usize::try_from(declared).map_err(|_| ProtocolError::NegativeLength(declared))?;

        if body_len > MAX_PACKET_SIZE {
            return Err(ProtocolError::FrameTooLarge {
                len: body_len,
                max: MAX_PACKET_SIZE,
            });
        }

        let frame_len = prefix_len + body_len;
        if src.len() < frame_len {
            // Tell the buffer how much more we need so it grows once rather
            // than repeatedly as bytes trickle in.
            src.reserve(frame_len - src.len());
            return Ok(None);
        }

        let mut frame = src.split_to(frame_len).freeze();
        frame.advance(prefix_len);

        // `read_varint` advances `frame`, so what remains is exactly the body.
        let id = read_varint(&mut frame)?;

        Ok(Some(RawPacket { id, body: frame }))
    }
}

impl<P: Packet> Encoder<P> for PacketCodec {
    type Error = ProtocolError;

    fn encode(&mut self, item: P, dst: &mut BytesMut) -> Result<(), ProtocolError> {
        // The length prefix counts the ID and body, so both must be written
        // before the prefix can be known. The scratch buffer is reused across
        // calls to keep this allocation-free in steady state.
        self.scratch.clear();
        write_varint(&mut self.scratch, P::ID);
        item.encode(&mut self.scratch)?;

        let len = self.scratch.len();
        if len > MAX_PACKET_SIZE {
            return Err(ProtocolError::FrameTooLarge {
                len,
                max: MAX_PACKET_SIZE,
            });
        }

        dst.reserve(len + 3);
        write_varint(dst, len as i32);
        dst.extend_from_slice(&self.scratch);
        Ok(())
    }
}
```

- [ ] **Step 5: Add the test module's imports**

The test module references `BytesMut`, `PacketCodec`, `Decoder`, `Encoder`, `ProtocolError`, `MAX_PACKET_SIZE` — all reachable through `use super::*;` since the implementation imports them. Confirm the test module compiles; if `Packet` is not in scope for `StatusResponse::ID`, add `use crate::packets::Packet;` inside `mod tests`.

- [ ] **Step 6: Run the tests**

Run: `cargo test -p pyrite-protocol --lib codec`
Expected: all nine tests PASS.

Run: `cargo test -p pyrite-protocol`
Expected: every protocol test passes.

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 7: Commit**

```bash
git add crates/protocol
git commit -m "feat(protocol): add length-prefixed packet framing codec"
```

---

## Task 11: Connection state machine and server config

**Files:**
- Create: `crates/net/src/error.rs`
- Create: `crates/net/src/config.rs`
- Create: `crates/net/src/state.rs`
- Modify: `crates/net/src/lib.rs`

**Interfaces:**
- Consumes: `State`, `ProtocolError` from `pyrite_protocol`.
- Produces:
  - `pub enum NetError` with variants `Protocol(ProtocolError)`, `Io(std::io::Error)`, `IllegalTransition { from: State, to: State }`, `UnexpectedPacket { state: State, id: i32 }`, `Timeout`, `Closed`
  - `pub struct ServerConfig { pub motd: TextComponent, pub max_players: i32, pub read_timeout: Duration }` with `Default`
  - `pub struct ConnectionState` with `new()`, `current() -> State`, `transition(to: State) -> Result<(), NetError>`, `mark_status_request_seen() -> Result<(), NetError>`

- [ ] **Step 1: Write the failing tests**

Create `crates/net/src/state.rs` with only this test module:

```rust
//! The connection finite state machine.

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn connections_begin_in_handshaking() {
        assert_eq!(ConnectionState::new().current(), State::Handshaking);
    }

    #[test]
    fn handshaking_moves_to_status_or_login() {
        for target in [State::Status, State::Login] {
            let mut fsm = ConnectionState::new();
            fsm.transition(target).unwrap();
            assert_eq!(fsm.current(), target);
        }
    }

    #[test]
    fn status_cannot_escalate_to_login() {
        // A status connection is unauthenticated and must never be able to
        // walk itself into login; this is a protocol violation, not a no-op.
        let mut fsm = ConnectionState::new();
        fsm.transition(State::Status).unwrap();
        assert!(matches!(
            fsm.transition(State::Login),
            Err(NetError::IllegalTransition {
                from: State::Status,
                to: State::Login
            })
        ));
        assert_eq!(fsm.current(), State::Status, "a rejected transition must not mutate state");
    }

    #[test]
    fn handshaking_cannot_skip_to_play_or_configuration() {
        for target in [State::Play, State::Configuration, State::Handshaking] {
            let mut fsm = ConnectionState::new();
            assert!(matches!(
                fsm.transition(target),
                Err(NetError::IllegalTransition { .. })
            ));
        }
    }

    #[test]
    fn status_request_may_be_skipped_entirely() {
        // A client is allowed to send the ping straight after the handshake.
        // Nothing in the state machine may require a status request first.
        let mut fsm = ConnectionState::new();
        fsm.transition(State::Status).unwrap();
        assert!(!fsm.status_request_seen());
    }

    #[test]
    fn duplicate_status_requests_are_rejected() {
        let mut fsm = ConnectionState::new();
        fsm.transition(State::Status).unwrap();
        fsm.mark_status_request_seen().unwrap();
        assert!(fsm.status_request_seen());
        assert!(matches!(
            fsm.mark_status_request_seen(),
            Err(NetError::UnexpectedPacket { .. })
        ));
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Add to `crates/net/src/lib.rs`:

```rust

pub mod config;
pub mod error;
pub mod state;

pub use config::ServerConfig;
pub use error::NetError;
pub use state::ConnectionState;
```

Run: `cargo test -p pyrite-net --lib state`
Expected: compile failure — `ConnectionState` and `NetError` are undefined.

- [ ] **Step 3: Write `error.rs`**

```rust
//! Errors raised while servicing a connection.

use pyrite_protocol::{ProtocolError, State};

/// Everything that can go wrong on a single connection.
///
/// Every variant terminates only the connection that produced it. None of them
/// is fatal to the server.
#[derive(Debug, thiserror::Error)]
pub enum NetError {
    /// The peer sent something the codec could not decode.
    #[error("protocol error")]
    Protocol(#[from] ProtocolError),

    /// The underlying transport failed.
    #[error("i/o error")]
    Io(#[from] std::io::Error),

    /// The peer tried to move between states in a way the protocol forbids.
    #[error("illegal state transition from {from} to {to}")]
    IllegalTransition {
        /// The state the connection was in.
        from: State,
        /// The state the peer tried to reach.
        to: State,
    },

    /// A packet arrived that is not valid in the current state, or arrived
    /// more times than permitted.
    #[error("unexpected packet id {id:#04x} in state {state}")]
    UnexpectedPacket {
        /// The state the connection was in.
        state: State,
        /// The offending packet ID.
        id: i32,
    },

    /// The peer sent nothing for longer than the configured read timeout.
    #[error("connection timed out waiting for a packet")]
    Timeout,

    /// The peer closed the connection cleanly.
    #[error("connection closed by peer")]
    Closed,
}
```

- [ ] **Step 4: Write `config.rs`**

```rust
//! Runtime configuration for the listening server.

use std::time::Duration;

use pyrite_protocol::text::TextComponent;

/// Settings shared by every connection.
///
/// Held behind an `Arc` and never mutated after startup, so it costs one
/// pointer clone per connection rather than a copy.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// The message of the day shown in a client's server list.
    pub motd: TextComponent,
    /// The slot count advertised to clients.
    pub max_players: i32,
    /// How long a connection may sit idle before it is dropped.
    pub read_timeout: Duration,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            motd: TextComponent::new("A Pyrite Server"),
            max_players: 20,
            // A status ping completes in milliseconds. Anything idle for this
            // long is a stalled or hostile peer holding a task open, so it is
            // dropped rather than allowed to accumulate.
            read_timeout: Duration::from_secs(30),
        }
    }
}
```

- [ ] **Step 5: Write the state machine**

Insert above the test module in `crates/net/src/state.rs`:

```rust
use pyrite_protocol::State;

use crate::error::NetError;

/// Tracks which protocol state a connection is in and enforces legal moves.
///
/// Transitions are explicit rather than implied by which packet arrived, so an
/// illegal move is a typed error that closes the connection instead of a
/// silently ignored no-op.
#[derive(Debug)]
pub struct ConnectionState {
    current: State,
    status_request_seen: bool,
}

impl ConnectionState {
    /// Creates a state machine for a freshly accepted connection.
    pub fn new() -> Self {
        Self {
            current: State::Handshaking,
            status_request_seen: false,
        }
    }

    /// The state the connection is currently in.
    pub fn current(&self) -> State {
        self.current
    }

    /// Whether a status request has already been serviced.
    ///
    /// This is informational only. A client may legally skip the status
    /// request and send a ping immediately, so nothing may gate on this being
    /// true.
    pub fn status_request_seen(&self) -> bool {
        self.status_request_seen
    }

    /// Moves to `to`, or rejects the move.
    ///
    /// The only legal moves in Milestone 1 are out of `Handshaking` into
    /// `Status` or `Login`. In particular `Status -> Login` is refused: a
    /// status connection is unauthenticated and must not be able to promote
    /// itself.
    pub fn transition(&mut self, to: State) -> Result<(), NetError> {
        let permitted = matches!(
            (self.current, to),
            (State::Handshaking, State::Status) | (State::Handshaking, State::Login)
        );

        if !permitted {
            return Err(NetError::IllegalTransition {
                from: self.current,
                to,
            });
        }

        self.current = to;
        Ok(())
    }

    /// Records that a status request was received, rejecting duplicates.
    pub fn mark_status_request_seen(&mut self) -> Result<(), NetError> {
        if self.status_request_seen {
            return Err(NetError::UnexpectedPacket {
                state: self.current,
                id: 0x00,
            });
        }
        self.status_request_seen = true;
        Ok(())
    }
}

impl Default for ConnectionState {
    fn default() -> Self {
        Self::new()
    }
}
```

- [ ] **Step 6: Run the tests**

Run: `cargo test -p pyrite-net --lib`
Expected: all six tests PASS.

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 7: Commit**

```bash
git add crates/net
git commit -m "feat(net): add connection state machine, config, and error type"
```

---

## Task 12: Connection handler and end-to-end ping test

**Files:**
- Create: `crates/net/src/connection.rs`
- Create: `crates/net/tests/server_list_ping.rs`
- Modify: `crates/net/src/lib.rs`

**Interfaces:**
- Consumes: everything from Tasks 2-11.
- Produces: `pub struct Connection<S>` with `Connection::new(stream: S, config: Arc<ServerConfig>) -> Self` and `pub async fn run(self) -> Result<(), NetError>`.

- [ ] **Step 1: Write the failing integration test**

Create `crates/net/tests/server_list_ping.rs`:

```rust
//! End-to-end verification of the Milestone 1 success criterion.
//!
//! These tests drive a real `Connection` over an in-memory duplex pipe rather
//! than a socket, so they are deterministic on every platform, need no ports,
//! and never sleep.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;
use std::time::Duration;

use bytes::BytesMut;
use futures_util::{SinkExt, StreamExt};
use pyrite_net::{Connection, ServerConfig};
use pyrite_protocol::codec::PacketCodec;
use pyrite_protocol::packets::handshake::{Handshake, NextState};
use pyrite_protocol::packets::login::LoginDisconnect;
use pyrite_protocol::packets::status::{
    PingRequest, PongResponse, StatusRequest, StatusResponse, StatusResponseJson,
};
use pyrite_protocol::text::TextComponent;
use pyrite_protocol::{Packet, PROTOCOL_VERSION, VERSION_NAME};
use tokio_util::codec::Framed;

fn config() -> Arc<ServerConfig> {
    Arc::new(ServerConfig {
        motd: TextComponent::new("A Pyrite Server"),
        max_players: 100,
        read_timeout: Duration::from_secs(5),
    })
}

fn handshake(next_state: NextState) -> Handshake {
    Handshake {
        protocol_version: PROTOCOL_VERSION,
        server_address: "localhost".to_owned(),
        server_port: 25565,
        next_state,
    }
}

#[tokio::test]
async fn full_server_list_ping_exchange() {
    let (client, server) = tokio::io::duplex(4096);
    let task = tokio::spawn(Connection::new(server, config()).run());
    let mut client = Framed::new(client, PacketCodec::new());

    client.send(handshake(NextState::Status)).await.unwrap();
    client.send(StatusRequest).await.unwrap();

    let frame = client.next().await.unwrap().unwrap();
    assert_eq!(frame.id, StatusResponse::ID);
    let response: StatusResponse = frame.decode_as().unwrap();
    let status: StatusResponseJson = serde_json::from_str(&response.json).unwrap();

    assert_eq!(status.version.protocol, PROTOCOL_VERSION);
    assert_eq!(status.version.name, VERSION_NAME);
    assert_eq!(status.players.max, 100);
    assert_eq!(status.players.online, 0);
    assert_eq!(status.description, TextComponent::new("A Pyrite Server"));

    let payload = 0x0123_4567_89ab_cdefi64;
    client.send(PingRequest { payload }).await.unwrap();

    let frame = client.next().await.unwrap().unwrap();
    assert_eq!(frame.id, PongResponse::ID);
    let pong: PongResponse = frame.decode_as().unwrap();
    assert_eq!(pong.payload, payload, "the payload must be echoed verbatim");

    // The server closes the connection after the pong.
    assert!(client.next().await.is_none());
    task.await.unwrap().unwrap();
}

#[tokio::test]
async fn ping_without_a_status_request_is_accepted() {
    // Clients are permitted to skip the status request entirely.
    let (client, server) = tokio::io::duplex(4096);
    let task = tokio::spawn(Connection::new(server, config()).run());
    let mut client = Framed::new(client, PacketCodec::new());

    client.send(handshake(NextState::Status)).await.unwrap();
    client.send(PingRequest { payload: -1 }).await.unwrap();

    let frame = client.next().await.unwrap().unwrap();
    let pong: PongResponse = frame.decode_as().unwrap();
    assert_eq!(pong.payload, -1);
    task.await.unwrap().unwrap();
}

#[tokio::test]
async fn login_receives_a_graceful_disconnect() {
    let (client, server) = tokio::io::duplex(4096);
    let task = tokio::spawn(Connection::new(server, config()).run());
    let mut client = Framed::new(client, PacketCodec::new());

    client.send(handshake(NextState::Login)).await.unwrap();

    let frame = client.next().await.unwrap().unwrap();
    assert_eq!(frame.id, LoginDisconnect::ID);
    let disconnect: LoginDisconnect = frame.decode_as().unwrap();
    let reason: serde_json::Value = serde_json::from_str(&disconnect.reason).unwrap();
    assert!(
        reason["text"].as_str().unwrap().contains("not implemented"),
        "the reason must explain why, got {reason}"
    );
    task.await.unwrap().unwrap();
}

#[tokio::test]
async fn an_unknown_packet_id_closes_the_connection() {
    let (client, server) = tokio::io::duplex(4096);
    let task = tokio::spawn(Connection::new(server, config()).run());
    let mut client = Framed::new(client, PacketCodec::new());

    client.send(handshake(NextState::Status)).await.unwrap();

    // Hand-frame a status packet with an ID nothing defines.
    let mut raw = BytesMut::new();
    raw.extend_from_slice(&[0x01, 0x7f]); // length 1, packet id 0x7f
    client.get_mut().write_all_bytes(&raw).await;

    assert!(client.next().await.is_none());
    assert!(task.await.unwrap().is_err());
}

#[tokio::test]
async fn a_client_disconnecting_immediately_is_not_an_error() {
    let (client, server) = tokio::io::duplex(4096);
    let task = tokio::spawn(Connection::new(server, config()).run());
    drop(client);
    // A peer hanging up before saying anything is ordinary, not a failure.
    task.await.unwrap().unwrap();
}
```

Note: `write_all_bytes` in the unknown-ID test is not a real API. Replace that line with:

```rust
    use tokio::io::AsyncWriteExt;
    client.get_mut().write_all(&raw).await.unwrap();
```

and add `tokio = { workspace = true, features = ["io-util"] }` to `crates/net`'s dev-dependencies (already present from Task 1).

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p pyrite-net --test server_list_ping`
Expected: compile failure — `Connection` is undefined.

- [ ] **Step 3: Write the connection handler**

Create `crates/net/src/connection.rs`:

```rust
//! Per-connection lifecycle: read frames, dispatch, respond.

use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use pyrite_protocol::codec::{PacketCodec, RawPacket};
use pyrite_protocol::packets::handshake::{Handshake, NextState};
use pyrite_protocol::packets::login::LoginDisconnect;
use pyrite_protocol::packets::status::{
    PingRequest, PongResponse, StatusRequest, StatusResponse, StatusResponseJson,
};
use pyrite_protocol::text::TextComponent;
use pyrite_protocol::{Packet, State};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::time::timeout;
use tokio_util::codec::Framed;
use tracing::debug;

use crate::config::ServerConfig;
use crate::error::NetError;
use crate::state::ConnectionState;

/// The reason sent to clients that try to log in during Milestone 1.
const LOGIN_UNAVAILABLE: &str = "Login is not implemented yet — Pyrite is pre-alpha.";

/// One client connection.
///
/// Generic over the transport rather than tied to `TcpStream` so that the same
/// handler serves a socket, an in-memory duplex pipe in tests, and — once the
/// embedded singleplayer server exists — a loopback pipe with no kernel
/// involvement at all.
#[derive(Debug)]
pub struct Connection<S> {
    framed: Framed<S, PacketCodec>,
    state: ConnectionState,
    config: Arc<ServerConfig>,
}

impl<S> Connection<S>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    /// Wraps a transport in the packet codec and prepares its state machine.
    pub fn new(stream: S, config: Arc<ServerConfig>) -> Self {
        Self {
            framed: Framed::new(stream, PacketCodec::new()),
            state: ConnectionState::new(),
            config,
        }
    }

    /// Services the connection until it closes or errors.
    ///
    /// A peer hanging up cleanly is a normal outcome and returns `Ok(())`. Any
    /// protocol violation returns an error, which the caller logs; it never
    /// affects any other connection.
    pub async fn run(mut self) -> Result<(), NetError> {
        loop {
            let next = timeout(self.config.read_timeout, self.framed.next()).await;

            let frame = match next {
                Err(_elapsed) => return Err(NetError::Timeout),
                Ok(None) => {
                    debug!("peer closed the connection");
                    return Ok(());
                }
                Ok(Some(frame)) => frame?,
            };

            if self.handle(frame).await? == Flow::Close {
                return Ok(());
            }
        }
    }

    /// Dispatches one frame based on the current state and its packet ID.
    async fn handle(&mut self, frame: RawPacket) -> Result<Flow, NetError> {
        match (self.state.current(), frame.id) {
            (State::Handshaking, Handshake::ID) => self.handle_handshake(&frame).await,
            (State::Status, StatusRequest::ID) => self.handle_status_request().await,
            (State::Status, PingRequest::ID) => self.handle_ping(&frame).await,
            (state, id) => Err(NetError::UnexpectedPacket { state, id }),
        }
    }

    async fn handle_handshake(&mut self, frame: &RawPacket) -> Result<Flow, NetError> {
        let handshake: Handshake = frame.decode_as()?;
        debug!(
            protocol_version = handshake.protocol_version,
            address = %handshake.server_address,
            next_state = ?handshake.next_state,
            "handshake received"
        );

        match handshake.next_state {
            NextState::Status => {
                self.state.transition(State::Status)?;
                Ok(Flow::Continue)
            }
            // A transferred client is mid-login from our point of view, so it
            // takes the same path as a fresh login until login is implemented.
            NextState::Login | NextState::Transfer => {
                self.state.transition(State::Login)?;
                let packet =
                    LoginDisconnect::from_component(&TextComponent::new(LOGIN_UNAVAILABLE))?;
                self.framed.send(packet).await?;
                Ok(Flow::Close)
            }
        }
    }

    async fn handle_status_request(&mut self) -> Result<Flow, NetError> {
        self.state.mark_status_request_seen()?;

        let status = StatusResponseJson::new(
            self.config.motd.clone(),
            self.config.max_players,
            // Milestone 1 has no player list; every connection is transient.
            0,
        );
        let json = serde_json::to_string(&status).map_err(|error| {
            NetError::Protocol(pyrite_protocol::ProtocolError::Json(error))
        })?;

        self.framed.send(StatusResponse { json }).await?;
        Ok(Flow::Continue)
    }

    async fn handle_ping(&mut self, frame: &RawPacket) -> Result<Flow, NetError> {
        let ping: PingRequest = frame.decode_as()?;
        // Echo immediately and do no other work first: the client measures
        // latency as the round trip of this exact exchange.
        self.framed
            .send(PongResponse {
                payload: ping.payload,
            })
            .await?;
        // The status exchange ends here by convention.
        Ok(Flow::Close)
    }
}

/// Whether the read loop should continue or shut the connection down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Flow {
    /// Keep reading.
    Continue,
    /// Close the connection normally.
    Close,
}
```

- [ ] **Step 4: Export from `lib.rs`**

Add to `crates/net/src/lib.rs`:

```rust
pub mod connection;

pub use connection::Connection;
```

- [ ] **Step 5: Run the tests**

Run: `cargo test -p pyrite-net`
Expected: all five integration tests plus the six state tests PASS.

Run: `cargo test --workspace`
Expected: everything passes.

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add crates/net
git commit -m "feat(net): add connection handler with end-to-end ping tests"
```

---

## Task 13: The server binary

**Files:**
- Modify: `crates/server/src/main.rs`

**Interfaces:**
- Consumes: `Connection`, `ServerConfig` from `pyrite_net`; `TextComponent`, version constants from `pyrite_protocol`.
- Produces: the `pyrite-server` binary.

- [ ] **Step 1: Write `main.rs`**

Replace `crates/server/src/main.rs` entirely:

```rust
//! The Pyrite server executable.
//!
//! Binds a TCP listener, accepts connections, and hands each one to a
//! [`Connection`] on its own Tokio task.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use pyrite_net::{Connection, ServerConfig};
use pyrite_protocol::text::TextComponent;
use pyrite_protocol::{PROTOCOL_VERSION, VERSION_NAME};
use tokio::net::TcpListener;
use tracing::{Instrument, error, info, info_span, warn};
use tracing_subscriber::EnvFilter;

/// Command line options.
#[derive(Debug, Parser)]
#[command(name = "pyrite-server", version, about = "The Pyrite server engine")]
struct Args {
    /// Address to listen on.
    #[arg(long, env = "PYRITE_BIND", default_value = "0.0.0.0:25565")]
    bind: SocketAddr,

    /// Message of the day shown in the client's server list.
    #[arg(long, env = "PYRITE_MOTD", default_value = "A Pyrite Server")]
    motd: String,

    /// Slot count advertised to clients.
    #[arg(long, env = "PYRITE_MAX_PLAYERS", default_value_t = 20)]
    max_players: i32,

    /// Seconds a connection may sit idle before it is dropped.
    #[arg(long, env = "PYRITE_READ_TIMEOUT", default_value_t = 30)]
    read_timeout: u64,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    // Default to `info`; `RUST_LOG=pyrite_net=debug` turns on per-packet tracing.
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let config = Arc::new(ServerConfig {
        motd: TextComponent::new(args.motd),
        max_players: args.max_players,
        read_timeout: Duration::from_secs(args.read_timeout),
    });

    let listener = TcpListener::bind(args.bind).await?;
    info!(
        address = %args.bind,
        version = VERSION_NAME,
        protocol = PROTOCOL_VERSION,
        "pyrite-server listening"
    );

    loop {
        tokio::select! {
            result = listener.accept() => {
                match result {
                    Ok((stream, peer)) => {
                        let config = Arc::clone(&config);
                        let span = info_span!("connection", %peer);
                        tokio::spawn(
                            async move {
                                if let Err(error) = Connection::new(stream, config).run().await {
                                    warn!(%error, "connection closed with an error");
                                }
                            }
                            .instrument(span),
                        );
                    }
                    Err(error) => {
                        // A failed accept (a descriptor limit, for example) must
                        // never take the listener down with it.
                        error!(%error, "failed to accept a connection");
                    }
                }
            }
            result = tokio::signal::ctrl_c() => {
                match result {
                    Ok(()) => info!("shutdown signal received, stopping the listener"),
                    Err(error) => error!(%error, "failed to listen for the shutdown signal"),
                }
                break;
            }
        }
    }

    Ok(())
}
```

Note: `unwrap_or_else` on `EnvFilter::try_from_default_env` is not `unwrap()`; it supplies a fallback and does not trip the clippy lint.

- [ ] **Step 2: Build and lint**

Run: `cargo build --workspace`
Expected: success, no warnings.

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: clean.

Run: `cargo fmt --all --check`
Expected: clean.

- [ ] **Step 3: Verify the full test suite**

Run: `cargo test --workspace`
Expected: all tests pass.

- [ ] **Step 4: Manual verification against a real client**

Run: `cargo run -p pyrite-server -- --motd "Pyrite is alive"`

Expected log line: `pyrite-server listening address=0.0.0.0:25565 version=26.2 protocol=776`

Then, in a vanilla Minecraft client, add a server pointing at `localhost` and check the server list entry shows:

- the MOTD `Pyrite is alive`
- `0/20` players
- a green latency bar with a millisecond figure
- version `26.2` if the client's protocol differs from 776

Refresh the entry several times and confirm the server neither panics nor leaks tasks. Close the client mid-ping and confirm the server logs a debug line and keeps running.

If no client is available, verify with a raw probe instead:

```bash
RUST_LOG=debug cargo run -p pyrite-server
```

and confirm `cargo test --workspace` covers the same exchange — `full_server_list_ping_exchange` is the automated form of this check.

- [ ] **Step 5: Commit**

```bash
git add crates/server
git commit -m "feat(server): add tokio listener, cli, tracing, and graceful shutdown"
```

- [ ] **Step 6: Update the README status line**

Change the Status section of `README.md` to:

```markdown
## Status

Milestone 1 complete: the server answers the Server List Ping. Login, world
storage, and the WASM mod runtime are not implemented.
```

```bash
git add README.md
git commit -m "docs: mark milestone 1 complete"
```

---

## Self-Review

**Spec coverage.** Every spec section maps to a task: §4 layout → Task 1; §5.1 varint → Task 3; §5.2 buf → Task 4; §5.3 codec → Task 10; §5.4 Packet trait → Task 5; §5.5 handshake → Task 6; §5.6 status → Task 8; §5.7 text → Task 7; §5.8 error → Task 2 (extended in Task 10); §6.1 state → Task 11; §6.2 connection → Task 12; §6.3 error policy → workspace lints in Task 1; §7 server → Task 13; §8 testing → tests in Tasks 3-12 plus CI in Task 1. The login stub of §6.2 is Task 9 plus Task 12's dispatch.

**Ordering deviation from the spec's narrative.** The spec presents the codec (§5.3) before the packet trait (§5.4); the plan builds the trait first (Task 5) because `PacketCodec`'s `Encoder` impl is generic over `P: Packet` and cannot compile without it. Behaviour is unchanged.

**Two additions discovered while writing the plan**, neither in the spec:
- `ProtocolError::TrailingBytes` — needed so `RawPacket::decode_as` can reject a body longer than the packet's fields, rather than silently ignoring the excess.
- `ProtocolError::NegativeLength` — needed because a length prefix is a VarInt and can therefore carry a negative bit pattern.

Both are strictly additive to the error enum and are introduced with the tests that require them.

**Type consistency.** `read_string(src, max_chars)` is called with `MAX_SERVER_ADDRESS_CHARS` (Task 6), `MAX_STATUS_JSON_CHARS` (Task 8), and `MAX_DISCONNECT_REASON_CHARS` (Task 9), all `usize`, matching Task 4's signature. `ProtocolError::VarIntTooLong { max }` is constructed and matched with the same field name throughout Task 3. `State` and `Direction` are defined once in Task 2's `error.rs` and re-exported through `packets` in Task 5 — every later `use` resolves to the same types. `Connection::new(stream, Arc<ServerConfig>)` in Task 12 matches every call site in Tasks 12 and 13.
