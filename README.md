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

Milestone 1 complete: the server answers the Server List Ping. Login, world
storage, and the WASM mod runtime are not implemented.

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
