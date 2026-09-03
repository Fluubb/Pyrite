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
