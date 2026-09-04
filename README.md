# Pyrite

A clean-room, high-performance server engine written in Rust that speaks the
Minecraft network protocol, with a sandboxed WebAssembly mod runtime.

> **Project status: Pre-Alpha / Architecture R&D.**
> Pyrite answers the server list ping and completes login. There is no world,
> no entities, and no mod runtime yet. You cannot play on it.

**What this is NOT:** not Forge, Fabric, Paper, or Spigot. It cannot run
existing Java `.jar` mods or plugins, and it never will — mods target a
WebAssembly interface instead. It ships no game assets.

## Status

| Milestone | Scope | State |
|---|---|---|
| M1 | Protocol foundations, Server List Ping | **Done** |
| M2 | Packet compression, Login state, offline identity | **Done** |
| M3 | NBT | Not started |
| M4 | Configuration + Play states — join an empty world | Not started |
| M5 | Encryption (AES-128-CFB8), Mojang authentication | Not started |

Targets protocol **776** (Java Edition 26.2), pinned in one place
(`crates/protocol/src/version.rs`).

105 tests. CI runs formatting, `clippy -D warnings`, the test suite on Linux,
macOS and Windows, and a licence audit of every dependency.

## Building and running

Requires Rust 1.94 or newer.

```bash
cargo build --workspace
cargo run -p pyrite-server -- --bind 127.0.0.1:25565 --motd "Pyrite"
```

Add `localhost` to a client's server list and you will see the MOTD, player
counts, and a real latency figure. Logging in gets you as far as the
configuration stage, where the client will then wait for registry data that
does not exist yet (that is M4) and time out. This is expected.

`RUST_LOG=pyrite_net=debug` traces the handshake packet by packet.

### Why `--bind 0.0.0.0` refuses to start

Pyrite has no authentication yet, so it runs in offline mode: **any client can
connect under any username**, including one you have granted operator rights.
Binding a non-loopback address therefore requires an explicit
`--insecure-offline-mode`, and logs a warning when you use it. The guard is
removed in M5, when real authentication lands.

## Architecture

A Cargo workspace of decoupled crates:

| Crate | Responsibility |
|---|---|
| `pyrite-protocol` | Packet types, VarInt/VarLong, framing, compression. Executor-free, so it is usable from blocking code and from the future client. |
| `pyrite-net` | Connection lifecycle, state machine, offline identity. Generic over the transport, so the same handler serves a socket, an in-memory pipe in tests, and later an in-process loopback server. |
| `pyrite-server` | The binary: CLI, listener, bounded concurrency, graceful shutdown. |

Design decisions and their reasoning live in
[`docs/superpowers/specs/`](docs/superpowers/specs/); each milestone has a
design document recording what was decided and why.

## Clean-room

Pyrite is a clean-room implementation built **only** from open, publicly
documented reverse-engineering of the network protocol and file formats. It
contains no Mojang code, no decompiled bytecode, no private mappings, and no
proprietary assets. Every protocol constant is cited to a public source in the
milestone design documents.

Third-party crates implementing Minecraft domain logic are not accepted, since
their provenance would become ours. See [CONTRIBUTING.md](CONTRIBUTING.md).

Pyrite is not affiliated with or endorsed by Mojang AB or Microsoft.
Minecraft is a trademark of Mojang AB.

## Contributing

Read [CONTRIBUTING.md](CONTRIBUTING.md) first — the clean-room policy is a
condition of the project existing, not a style preference.

This is pre-alpha and the architecture is still moving. Technical RFCs are
welcome; bug reports need a reproduction. Please do not open issues asking for
Java mod support.

## Licence

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your
option.
