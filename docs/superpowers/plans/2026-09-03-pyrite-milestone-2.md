# Pyrite Milestone 2 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add packet compression and the Login state, so a client completes the login handshake in offline mode and transitions to Configuration.

**Architecture:** Compression branches inside the existing `PacketCodec` rather than wrapping it — the compressed and uncompressed forms differ *inside* the frame, so a wrapping layer would have to pass length information across a boundary that does not exist on the wire. The connection sends `Set Compression`, flushes it, and only then flips the codec, because that packet is itself sent uncompressed. Offline identity is derived, never trusted from the client.

**Tech Stack:** Rust 1.94 (edition 2024), `flate2` (zlib), `md-5` (UUIDv3), plus the existing `tokio`, `tokio-util`, `bytes`, `serde`, `thiserror`, `tracing`, `clap`.

**Spec:** `docs/superpowers/specs/2026-09-03-pyrite-milestone-2-design.md`

## Global Constraints

- **Clean-room, absolute.** Never consult or reproduce decompiled Mojang bytecode, private mappings, or proprietary assets. Only open reverse-engineered documentation of wire formats.
- **No Minecraft-domain dependencies.** `flate2` and `md-5` are general-purpose infrastructure and are permitted (spec D13). Minecraft-domain crates remain forbidden.
- **Protocol version stays 776 / "26.2".** Do not change `PROTOCOL_VERSION` or `VERSION_NAME`.
- **Login packet IDs.** Serverbound: Login Start `0x00`, Login Acknowledged `0x03`. Clientbound: Disconnect `0x00` (already exists), Login Success `0x02`, Set Compression `0x03`.
- **Compressed frame:** `VarInt(packet_length) ++ VarInt(data_length) ++ payload`. `data_length == 0` means the payload is uncompressed; otherwise it is the *inflated* size of a zlib payload. `packet_length` counts the data-length field plus the payload, not itself.
- **Threshold semantics:** a packet whose uncompressed size (packet id VarInt + body) is **at or above** the threshold is compressed; below it is sent with `data_length = 0`. A negative threshold disables compression. Threshold `0` compresses everything.
- **`MAX_PACKET_SIZE = 2_097_151`**, `MAX_SPECULATIVE_RESERVE = 8 * 1024`. Both already exist in `crates/protocol/src/codec.rs`.
- **No `unwrap`/`expect`/`panic` on any path reachable from network input.** Enforced by `clippy::unwrap_used`, `clippy::expect_used`, `clippy::panic` = deny. Test modules opt out with `#![allow(clippy::unwrap_used, clippy::expect_used)]` as their first inner line.
- **No `todo!()` / `unimplemented!()`.**
- **Every public item needs a doc comment.** `missing_docs = "warn"` plus CI's `-D warnings` makes an omission a build failure.
- Gate before every commit: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all --check`.
- Commit after every task. Conventional Commit prefixes.

**Baseline:** 64 tests pass at the start of this plan.

---

## File Structure

| File | Change | Responsibility |
|---|---|---|
| `crates/protocol/src/error.rs` | modify | Four new variants: `ArrayTooLong`, `InvalidBoolean`, `CompressedSizeMismatch`, `CompressedBelowThreshold` |
| `crates/protocol/src/buf.rs` | modify | UUID, prefixed array, prefixed optional primitives |
| `crates/protocol/src/codec.rs` | modify | Compression in `Decoder`/`Encoder`, `set_compression` |
| `crates/protocol/src/packets/login.rs` | modify | `LoginStart`, `SetCompression`, `LoginSuccess`, `LoginAcknowledged`, `GameProfile`, `ProfileProperty` |
| `crates/protocol/Cargo.toml` | modify | Add `flate2` |
| `crates/net/src/offline.rs` | create | `offline_uuid`, `validate_username` |
| `crates/net/src/error.rs` | modify | `InvalidUsername` variant |
| `crates/net/src/state.rs` | modify | Permit `Login -> Configuration` |
| `crates/net/src/config.rs` | modify | `compression_threshold` field |
| `crates/net/src/connection.rs` | modify | Login flow, compression switchover |
| `crates/net/Cargo.toml` | modify | Add `md-5` |
| `crates/net/tests/login.rs` | create | End-to-end login sequence |
| `crates/net/tests/server_list_ping.rs` | modify | Update the now-obsolete login-disconnect test |
| `crates/server/src/main.rs` | modify | `--compression-threshold`, `--insecure-offline-mode`, the binding interlock |
| `Cargo.toml` | modify | Workspace dependency entries for `flate2`, `md-5` |

**Cross-task hazard, read before Task 5:** M1's integration test `login_receives_a_graceful_disconnect` in `crates/net/tests/server_list_ping.rs` asserts that a handshake with next state Login produces a `LoginDisconnect`. Task 5 deliberately changes that behaviour, so that test **must be updated in Task 5**, not left to fail. It is called out again in Task 5's steps.

---

## Task 1: Buffer primitives — UUID, prefixed array, prefixed optional

**Files:**
- Modify: `crates/protocol/src/error.rs`
- Modify: `crates/protocol/src/buf.rs`

**Interfaces:**
- Consumes: `write_varint`/`read_varint` from `crate::varint`, `ProtocolError`.
- Produces:
  - `pub fn write_uuid<B: BufMut>(dst: &mut B, value: u128)`
  - `pub fn read_uuid<B: Buf>(src: &mut B) -> Result<u128, ProtocolError>`
  - `pub fn write_prefixed_array<B, T, F>(dst: &mut B, items: &[T], write_item: F)` where `B: BufMut, F: FnMut(&mut B, &T)`
  - `pub fn read_prefixed_array<B, T, F>(src: &mut B, max_len: usize, read_item: F) -> Result<Vec<T>, ProtocolError>` where `B: Buf, F: FnMut(&mut B) -> Result<T, ProtocolError>`
  - `pub fn write_prefixed_optional<B, T, F>(dst: &mut B, value: Option<&T>, write_value: F)` where `B: BufMut, F: FnOnce(&mut B, &T)`
  - `pub fn read_prefixed_optional<B, T, F>(src: &mut B, read_value: F) -> Result<Option<T>, ProtocolError>` where `B: Buf, F: FnOnce(&mut B) -> Result<T, ProtocolError>`
  - `ProtocolError::ArrayTooLong { len: usize, max: usize }`, `ProtocolError::InvalidBoolean(u8)`

- [ ] **Step 1: Add the two error variants**

Insert into `ProtocolError` in `crates/protocol/src/error.rs`, immediately after the `StringTooLong` variant:

```rust
    /// A prefixed array declared more elements than its field permits.
    #[error("array length {len} exceeds maximum {max}")]
    ArrayTooLong {
        /// The declared element count.
        len: usize,
        /// The maximum permitted count.
        max: usize,
    },

    /// A boolean field held a byte other than 0 or 1.
    #[error("invalid boolean byte {0:#04x}, expected 0 or 1")]
    InvalidBoolean(u8),
```

- [ ] **Step 2: Write the failing tests**

Append to the existing `mod tests` in `crates/protocol/src/buf.rs` (it already opens with `#![allow(clippy::unwrap_used, clippy::expect_used)]`; do not add a second one):

```rust
    #[test]
    fn uuid_round_trips() {
        for value in [
            0u128,
            u128::MAX,
            0x0123_4567_89ab_cdef_0123_4567_89ab_cdefu128,
        ] {
            let mut buf = BytesMut::new();
            write_uuid(&mut buf, value);
            assert_eq!(buf.len(), 16, "a uuid is exactly 16 bytes");
            let mut src = &buf[..];
            assert_eq!(read_uuid(&mut src).unwrap(), value);
            assert!(src.is_empty());
        }
    }

    #[test]
    fn uuid_is_big_endian() {
        let mut buf = BytesMut::new();
        write_uuid(&mut buf, 1);
        assert_eq!(buf[15], 0x01, "the low byte is last");
        assert_eq!(buf[0], 0x00);
    }

    #[test]
    fn uuid_reports_eof_when_short() {
        let mut short: &[u8] = &[0u8; 15];
        assert!(matches!(
            read_uuid(&mut short),
            Err(ProtocolError::UnexpectedEof)
        ));
    }

    #[test]
    fn prefixed_array_round_trips() {
        let items = vec![1u16, 2, 3, 65535];
        let mut buf = BytesMut::new();
        write_prefixed_array(&mut buf, &items, |dst, item| write_u16(dst, *item));

        let mut src = &buf[..];
        let decoded = read_prefixed_array(&mut src, 16, read_u16).unwrap();
        assert_eq!(decoded, items);
        assert!(src.is_empty());
    }

    #[test]
    fn empty_prefixed_array_round_trips() {
        let items: Vec<u16> = Vec::new();
        let mut buf = BytesMut::new();
        write_prefixed_array(&mut buf, &items, |dst, item| write_u16(dst, *item));
        assert_eq!(&buf[..], &[0x00], "an empty array is just a zero count");

        let mut src = &buf[..];
        assert!(read_prefixed_array(&mut src, 16, read_u16).unwrap().is_empty());
    }

    #[test]
    fn prefixed_array_rejects_a_count_above_the_cap_before_reserving() {
        // Declares 5000 elements with a cap of 8. Must reject on the count
        // alone, without the elements being present.
        let mut buf = BytesMut::new();
        crate::varint::write_varint(&mut buf, 5000);
        let mut src = &buf[..];
        assert!(matches!(
            read_prefixed_array(&mut src, 8, read_u16),
            Err(ProtocolError::ArrayTooLong { len: 5000, max: 8 })
        ));
    }

    #[test]
    fn prefixed_array_rejects_a_negative_count() {
        let mut buf = BytesMut::new();
        crate::varint::write_varint(&mut buf, -1);
        let mut src = &buf[..];
        assert!(matches!(
            read_prefixed_array(&mut src, 8, read_u16),
            Err(ProtocolError::NegativeLength(-1))
        ));
    }

    #[test]
    fn prefixed_optional_round_trips_present_and_absent() {
        let mut buf = BytesMut::new();
        write_prefixed_optional(&mut buf, Some(&7u16), |dst, v| write_u16(dst, *v));
        assert_eq!(&buf[..], &[0x01, 0x00, 0x07]);
        let mut src = &buf[..];
        assert_eq!(read_prefixed_optional(&mut src, read_u16).unwrap(), Some(7));

        let mut buf = BytesMut::new();
        write_prefixed_optional::<_, u16, _>(&mut buf, None, |dst, v| write_u16(dst, *v));
        assert_eq!(&buf[..], &[0x00], "an absent optional is one zero byte");
        let mut src = &buf[..];
        assert_eq!(read_prefixed_optional(&mut src, read_u16).unwrap(), None);
    }

    #[test]
    fn prefixed_optional_rejects_a_non_boolean_tag() {
        let buf: &[u8] = &[0x02];
        let mut src = buf;
        assert!(matches!(
            read_prefixed_optional(&mut src, read_u16),
            Err(ProtocolError::InvalidBoolean(0x02))
        ));
    }
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p pyrite-protocol --lib buf`
Expected: compile failure — `write_uuid`, `read_uuid`, `write_prefixed_array`, `read_prefixed_array`, `write_prefixed_optional`, `read_prefixed_optional` are not defined.

- [ ] **Step 4: Write the implementation**

Append to `crates/protocol/src/buf.rs`, above the test module:

```rust
/// Caps how many elements are pre-allocated for a prefixed array.
///
/// The declared count is peer-controlled. Reserving for the full declared
/// count would let a small frame commit a large allocation, the same failure
/// mode the framing decoder guards against. The vector still grows as real
/// elements decode, so a legitimate long array is unaffected.
const MAX_PREALLOC_ELEMENTS: usize = 64;

/// Writes a UUID as an unsigned 128-bit integer, big-endian, 16 bytes.
pub fn write_uuid<B: BufMut>(dst: &mut B, value: u128) {
    dst.put_u128(value);
}

/// Reads a big-endian unsigned 128-bit UUID.
pub fn read_uuid<B: Buf>(src: &mut B) -> Result<u128, ProtocolError> {
    if src.remaining() < 16 {
        return Err(ProtocolError::UnexpectedEof);
    }
    Ok(src.get_u128())
}

/// Writes a VarInt element count followed by each element.
pub fn write_prefixed_array<B, T, F>(dst: &mut B, items: &[T], mut write_item: F)
where
    B: BufMut,
    F: FnMut(&mut B, &T),
{
    write_varint(dst, items.len() as i32);
    for item in items {
        write_item(dst, item);
    }
}

/// Reads a VarInt element count followed by that many elements.
///
/// The count is checked against `max_len` before anything is allocated, and
/// the initial reservation is capped independently, so a hostile count cannot
/// induce a large allocation.
pub fn read_prefixed_array<B, T, F>(
    src: &mut B,
    max_len: usize,
    mut read_item: F,
) -> Result<Vec<T>, ProtocolError>
where
    B: Buf,
    F: FnMut(&mut B) -> Result<T, ProtocolError>,
{
    let declared = read_varint(src)?;
    let len = usize::try_from(declared).map_err(|_| ProtocolError::NegativeLength(declared))?;
    if len > max_len {
        return Err(ProtocolError::ArrayTooLong { len, max: max_len });
    }

    let mut items = Vec::with_capacity(len.min(MAX_PREALLOC_ELEMENTS));
    for _ in 0..len {
        items.push(read_item(src)?);
    }
    Ok(items)
}

/// Writes a boolean presence tag followed by the value when present.
pub fn write_prefixed_optional<B, T, F>(dst: &mut B, value: Option<&T>, write_value: F)
where
    B: BufMut,
    F: FnOnce(&mut B, &T),
{
    match value {
        Some(value) => {
            dst.put_u8(1);
            write_value(dst, value);
        }
        None => dst.put_u8(0),
    }
}

/// Reads a boolean presence tag and, when set, the value after it.
///
/// A tag byte other than 0 or 1 is a protocol violation rather than something
/// to coerce: accepting it would let two peers disagree about the frame's
/// length and desynchronise silently.
pub fn read_prefixed_optional<B, T, F>(src: &mut B, read_value: F) -> Result<Option<T>, ProtocolError>
where
    B: Buf,
    F: FnOnce(&mut B) -> Result<T, ProtocolError>,
{
    if !src.has_remaining() {
        return Err(ProtocolError::UnexpectedEof);
    }
    match src.get_u8() {
        0 => Ok(None),
        1 => Ok(Some(read_value(src)?)),
        other => Err(ProtocolError::InvalidBoolean(other)),
    }
}
```

- [ ] **Step 5: Run the tests**

Run: `cargo test -p pyrite-protocol --lib buf`
Expected: the 8 new tests plus the 9 existing ones PASS.

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add crates/protocol
git commit -m "feat(protocol): add uuid, prefixed array, and prefixed optional codecs"
```

---

## Task 2: Compression in the framing codec

**Files:**
- Modify: `Cargo.toml` (workspace dependency)
- Modify: `crates/protocol/Cargo.toml`
- Modify: `crates/protocol/src/error.rs`
- Modify: `crates/protocol/src/codec.rs`

**Interfaces:**
- Consumes: `varint_len`, `write_varint`, `read_varint`, `read_varint_slice`, `MAX_PACKET_SIZE`, `MAX_SPECULATIVE_RESERVE`.
- Produces:
  - `PacketCodec::set_compression(&mut self, threshold: i32)`
  - `ProtocolError::CompressedSizeMismatch { declared: usize, actual: usize }`
  - `ProtocolError::CompressedBelowThreshold { data_length: usize, threshold: i32 }`

- [ ] **Step 1: Add the `flate2` dependency**

In the root `Cargo.toml`, add to `[workspace.dependencies]`:

```toml
flate2 = "1"
```

In `crates/protocol/Cargo.toml`, add to `[dependencies]`:

```toml
flate2.workspace = true
```

- [ ] **Step 2: Add the two error variants**

Insert into `ProtocolError` in `crates/protocol/src/error.rs`, after the `ArrayTooLong` variant added in Task 1:

```rust
    /// A compressed packet inflated to a different size than it declared.
    #[error("compressed packet declared {declared} bytes but inflated to {actual}")]
    CompressedSizeMismatch {
        /// The size the peer said the payload would inflate to.
        declared: usize,
        /// The size it actually inflated to.
        actual: usize,
    },

    /// A packet was compressed even though it is below the threshold at which
    /// compression is permitted.
    #[error("compressed packet of {data_length} bytes is below the {threshold} byte threshold")]
    CompressedBelowThreshold {
        /// The declared uncompressed size.
        data_length: usize,
        /// The active compression threshold.
        threshold: i32,
    },
```

- [ ] **Step 3: Write the failing tests**

Append to the existing `mod tests` in `crates/protocol/src/codec.rs`:

```rust
    use crate::packets::status::PongResponse;

    /// Builds a codec with compression active at `threshold`.
    fn compressed_codec(threshold: i32) -> PacketCodec {
        let mut codec = PacketCodec::new();
        codec.set_compression(threshold);
        codec
    }

    #[test]
    fn set_compression_treats_a_negative_threshold_as_disabled() {
        let mut codec = PacketCodec::new();
        codec.set_compression(256);
        assert_eq!(codec.compression_threshold(), Some(256));
        codec.set_compression(-1);
        assert_eq!(codec.compression_threshold(), None);
    }

    #[test]
    fn a_small_packet_is_sent_uncompressed_with_a_zero_data_length() {
        // PongResponse is 9 bytes (1 id + 8 payload), well under the threshold.
        let mut codec = compressed_codec(256);
        let mut buf = BytesMut::new();
        codec.encode(PongResponse { payload: 7 }, &mut buf).unwrap();

        // length prefix, then a zero data-length, then the raw packet.
        assert_eq!(buf[0], 10, "packet length = 1 data-length byte + 9 payload");
        assert_eq!(buf[1], 0x00, "data length 0 means uncompressed");
        assert_eq!(buf[2], 0x01, "the raw packet id follows immediately");
        assert_eq!(buf.len(), 11);
    }

    #[test]
    fn a_large_packet_is_compressed_and_round_trips() {
        // A long, highly compressible json string comfortably exceeds the
        // threshold and shrinks a lot, so we can assert compression happened.
        let json = "x".repeat(4096);
        let mut codec = compressed_codec(256);
        let mut buf = BytesMut::new();
        codec
            .encode(StatusResponse { json: json.clone() }, &mut buf)
            .unwrap();

        assert!(
            buf.len() < 512,
            "a 4 KiB repetitive payload must compress well, got {} bytes",
            buf.len()
        );

        let frame = codec.decode(&mut buf).unwrap().unwrap();
        assert_eq!(frame.id, StatusResponse::ID);
        let decoded: StatusResponse = frame.decode_as().unwrap();
        assert_eq!(decoded.json, json);
        assert!(buf.is_empty());
    }

    #[test]
    fn a_small_packet_round_trips_through_the_uncompressed_path() {
        let mut codec = compressed_codec(256);
        let mut buf = BytesMut::new();
        codec.encode(PongResponse { payload: -1 }, &mut buf).unwrap();

        let frame = codec.decode(&mut buf).unwrap().unwrap();
        let decoded: PongResponse = frame.decode_as().unwrap();
        assert_eq!(decoded.payload, -1);
    }

    #[test]
    fn a_compressed_frame_fed_one_byte_at_a_time_yields_one_packet() {
        let json = "y".repeat(4096);
        let mut encoder = compressed_codec(256);
        let mut full = BytesMut::new();
        encoder
            .encode(StatusResponse { json: json.clone() }, &mut full)
            .unwrap();

        let mut decoder = compressed_codec(256);
        let mut buf = BytesMut::new();
        let mut frames = 0;
        for byte in full.iter() {
            buf.extend_from_slice(&[*byte]);
            if let Some(frame) = decoder.decode(&mut buf).unwrap() {
                let decoded: StatusResponse = frame.decode_as().unwrap();
                assert_eq!(decoded.json, json);
                frames += 1;
            }
        }
        assert_eq!(frames, 1);
        assert!(buf.is_empty());
    }

    #[test]
    fn a_declared_inflated_size_above_the_maximum_is_rejected() {
        // Guard 2. The payload is three bytes of garbage; the frame claims it
        // inflates to more than MAX_PACKET_SIZE. This must be refused from the
        // header alone, before any inflation is attempted.
        let mut codec = compressed_codec(256);
        let mut body = BytesMut::new();
        crate::varint::write_varint(&mut body, (MAX_PACKET_SIZE + 1) as i32);
        body.extend_from_slice(&[0x78, 0x9c, 0x00]);

        let mut buf = BytesMut::new();
        crate::varint::write_varint(&mut buf, body.len() as i32);
        buf.extend_from_slice(&body);

        assert!(matches!(
            codec.decode(&mut buf),
            Err(ProtocolError::FrameTooLarge { .. })
        ));
    }

    #[test]
    fn a_compressed_packet_below_the_threshold_is_rejected() {
        // Guard 3. Declaring an inflated size under the threshold means the
        // peer should have sent it uncompressed; honouring it would spend our
        // cpu on inflation that was never permitted.
        let mut codec = compressed_codec(256);
        let mut body = BytesMut::new();
        crate::varint::write_varint(&mut body, 10);
        body.extend_from_slice(&[0x78, 0x9c, 0x00]);

        let mut buf = BytesMut::new();
        crate::varint::write_varint(&mut buf, body.len() as i32);
        buf.extend_from_slice(&body);

        assert!(matches!(
            codec.decode(&mut buf),
            Err(ProtocolError::CompressedBelowThreshold {
                data_length: 10,
                threshold: 256
            })
        ));
    }

    #[test]
    fn a_payload_that_inflates_to_the_wrong_size_is_rejected() {
        // Guard 4. Compress 1000 bytes but claim 900. The mismatch must be
        // caught rather than silently handing a short buffer to the decoder.
        use flate2::Compression;
        use flate2::write::ZlibEncoder;
        use std::io::Write;

        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&vec![0x41; 1000]).unwrap();
        let compressed = encoder.finish().unwrap();

        let mut body = BytesMut::new();
        crate::varint::write_varint(&mut body, 900);
        body.extend_from_slice(&compressed);

        let mut buf = BytesMut::new();
        crate::varint::write_varint(&mut buf, body.len() as i32);
        buf.extend_from_slice(&body);

        let mut codec = compressed_codec(256);
        assert!(matches!(
            codec.decode(&mut buf),
            Err(ProtocolError::CompressedSizeMismatch {
                declared: 900,
                actual: _
            })
        ));
    }

    #[test]
    fn a_negative_data_length_is_rejected() {
        // Guard 1.
        let mut body = BytesMut::new();
        crate::varint::write_varint(&mut body, -1);
        body.extend_from_slice(&[0x78, 0x9c, 0x00]);

        let mut buf = BytesMut::new();
        crate::varint::write_varint(&mut buf, body.len() as i32);
        buf.extend_from_slice(&body);

        let mut codec = compressed_codec(256);
        assert!(matches!(
            codec.decode(&mut buf),
            Err(ProtocolError::NegativeLength(-1))
        ));
    }

    #[test]
    fn a_zero_threshold_compresses_everything() {
        let mut codec = compressed_codec(0);
        let mut buf = BytesMut::new();
        codec.encode(PongResponse { payload: 1 }, &mut buf).unwrap();

        let frame = codec.decode(&mut buf).unwrap().unwrap();
        let decoded: PongResponse = frame.decode_as().unwrap();
        assert_eq!(decoded.payload, 1);
    }
```

- [ ] **Step 4: Run the tests to verify they fail**

Run: `cargo test -p pyrite-protocol --lib codec`
Expected: compile failure — `set_compression` is not defined, and the new error variants are missing.

- [ ] **Step 5: Add the compressed scratch buffer and `set_compression`**

In `crates/protocol/src/codec.rs`, add to the `PacketCodec` struct after the `compression_threshold` field:

```rust
    /// Reused compression scratch buffer, so an encode that compresses does
    /// not allocate a fresh output vector each time.
    compressed: Vec<u8>,
```

And add to the `impl PacketCodec` block, after `compression_threshold()`:

```rust
    /// Enables or disables compression.
    ///
    /// A negative threshold disables compression; zero or above enables it and
    /// sets the size at or above which packets are compressed.
    ///
    /// The caller must flush the `Set Compression` packet **before** calling
    /// this. That packet is itself sent uncompressed and the new format
    /// applies only to what follows it, so flipping first would emit a
    /// compressed `Set Compression` that no peer can parse.
    pub fn set_compression(&mut self, threshold: i32) {
        self.compression_threshold = if threshold < 0 { None } else { Some(threshold) };
    }
```

Add these imports at the top of the file:

```rust
use std::io::{Read, Write};

use flate2::Compression;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
```

Also update the struct's doc comment to document the compressed layout, appending after the existing uncompressed diagram:

```rust
/// Once compression is enabled the layout becomes:
///
/// ```text
/// +------------------+------------------+---------------------------+
/// | VarInt length    | VarInt data len  | payload                   |
/// +------------------+------------------+---------------------------+
/// ```
///
/// `data len` is `0` when the payload is a raw uncompressed packet, and
/// otherwise the size the zlib-compressed payload inflates to. `length` counts
/// the data-length field plus the payload, and still does not count itself.
```

- [ ] **Step 6: Replace the `Encoder` implementation**

Replace the whole `impl<P: Packet> Encoder<P> for PacketCodec` block in `crates/protocol/src/codec.rs` with:

```rust
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

        let Some(threshold) = self.compression_threshold else {
            dst.reserve(len + 3);
            write_varint(dst, len as i32);
            dst.extend_from_slice(&self.scratch);
            return Ok(());
        };

        if len < threshold as usize {
            // Below the threshold: a zero data-length marks the payload as a
            // raw packet, so the peer skips inflation entirely.
            let payload_len = 1 + len;
            dst.reserve(payload_len + 3);
            write_varint(dst, payload_len as i32);
            write_varint(dst, 0);
            dst.extend_from_slice(&self.scratch);
            return Ok(());
        }

        self.compressed.clear();
        let mut encoder = ZlibEncoder::new(
            std::mem::take(&mut self.compressed),
            Compression::default(),
        );
        encoder.write_all(&self.scratch)?;
        self.compressed = encoder.finish()?;

        // The data-length field carries the *uncompressed* size, so the peer
        // knows how much to expect after inflating.
        let payload_len = varint_len(len as i32) + self.compressed.len();
        dst.reserve(payload_len + 3);
        write_varint(dst, payload_len as i32);
        write_varint(dst, len as i32);
        dst.extend_from_slice(&self.compressed);
        Ok(())
    }
}
```

Add `varint_len` to the existing `use crate::varint::{...}` import line.

- [ ] **Step 7: Add the decode branch**

In `Decoder::decode` in `crates/protocol/src/codec.rs`, replace these three lines:

```rust
        let mut frame = src.split_to(frame_len).freeze();
        frame.advance(prefix_len);

        // `read_varint` advances `frame`, so what remains is exactly the body.
        let id = read_varint(&mut frame)?;

        Ok(Some(RawPacket { id, body: frame }))
```

with:

```rust
        let mut frame = src.split_to(frame_len).freeze();
        frame.advance(prefix_len);

        let mut body = match self.compression_threshold {
            None => frame,
            Some(threshold) => self.decompress(frame, threshold)?,
        };

        // `read_varint` advances `body`, so what remains is exactly the packet
        // body.
        let id = read_varint(&mut body)?;

        Ok(Some(RawPacket { id, body }))
```

Then add this method to the `impl PacketCodec` block:

```rust
    /// Turns a compressed-mode frame body into a raw packet.
    ///
    /// Every bound here is checked before any allocation or inflation happens.
    /// The declared inflated size is peer-controlled, so a tiny payload can
    /// otherwise claim to expand to megabytes — the decompression form of the
    /// amplification the framing decoder already guards against.
    fn decompress(&self, mut frame: Bytes, threshold: i32) -> Result<Bytes, ProtocolError> {
        let declared = read_varint(&mut frame)?;

        // Guard 1: a negative length is always a violation.
        let data_length =
            usize::try_from(declared).map_err(|_| ProtocolError::NegativeLength(declared))?;

        // Zero means the payload is a raw packet and no inflation is needed.
        if data_length == 0 {
            return Ok(frame);
        }

        // Guard 2: refuse an inflated size we would never accept as a frame.
        if data_length > MAX_PACKET_SIZE {
            return Err(ProtocolError::FrameTooLarge {
                len: data_length,
                max: MAX_PACKET_SIZE,
            });
        }

        // Guard 3: below the threshold it should have been sent uncompressed.
        if data_length < threshold as usize {
            return Err(ProtocolError::CompressedBelowThreshold {
                data_length,
                threshold,
            });
        }

        // Guard 4: read at most one byte more than declared. If the payload
        // inflates further, the extra byte makes the length check below fail
        // instead of the output growing unbounded.
        let mut out = Vec::with_capacity(data_length.min(MAX_SPECULATIVE_RESERVE));
        let mut decoder = ZlibDecoder::new(frame.as_ref()).take(data_length as u64 + 1);
        decoder.read_to_end(&mut out)?;

        if out.len() != data_length {
            return Err(ProtocolError::CompressedSizeMismatch {
                declared: data_length,
                actual: out.len(),
            });
        }

        Ok(Bytes::from(out))
    }
```

- [ ] **Step 8: Run the tests**

Run: `cargo test -p pyrite-protocol`
Expected: the 10 new codec tests plus all existing protocol tests PASS.

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 9: Commit**

```bash
git add Cargo.toml Cargo.lock crates/protocol
git commit -m "feat(protocol): add zlib packet compression with bounded inflation"
```

---

## Task 3: Login state packets

**Files:**
- Modify: `crates/protocol/src/packets/login.rs`

**Interfaces:**
- Consumes: `Packet` trait, `buf` helpers from Task 1, `varint`.
- Produces:
  - `pub struct LoginStart { pub name: String, pub uuid: u128 }` — ID `0x00`, Login, Serverbound
  - `pub struct SetCompression { pub threshold: i32 }` — ID `0x03`, Login, Clientbound
  - `pub struct LoginSuccess { pub profile: GameProfile, pub session_id: u128 }` — ID `0x02`, Login, Clientbound
  - `pub struct LoginAcknowledged` — ID `0x03`, Login, Serverbound
  - `pub struct GameProfile { pub uuid: u128, pub username: String, pub properties: Vec<ProfileProperty> }`
  - `pub struct ProfileProperty { pub name: String, pub value: String, pub signature: Option<String> }`
  - `pub const MAX_USERNAME_CHARS: usize = 16`

- [ ] **Step 1: Write the failing tests**

Append to the existing `mod tests` in `crates/protocol/src/packets/login.rs`:

```rust
    fn sample_profile() -> GameProfile {
        GameProfile {
            uuid: 0x0123_4567_89ab_cdef_0123_4567_89ab_cdefu128,
            username: "Notch".to_owned(),
            properties: vec![ProfileProperty {
                name: "textures".to_owned(),
                value: "base64data".to_owned(),
                signature: Some("sig".to_owned()),
            }],
        }
    }

    #[test]
    fn login_packet_constants_match_the_specification() {
        assert_eq!(LoginStart::ID, 0x00);
        assert_eq!(LoginStart::DIRECTION, Direction::Serverbound);
        assert_eq!(LoginSuccess::ID, 0x02);
        assert_eq!(LoginSuccess::DIRECTION, Direction::Clientbound);
        assert_eq!(SetCompression::ID, 0x03);
        assert_eq!(SetCompression::DIRECTION, Direction::Clientbound);
        assert_eq!(LoginAcknowledged::ID, 0x03);
        assert_eq!(LoginAcknowledged::DIRECTION, Direction::Serverbound);

        for state in [
            LoginStart::STATE,
            LoginSuccess::STATE,
            SetCompression::STATE,
            LoginAcknowledged::STATE,
        ] {
            assert_eq!(state, State::Login);
        }
    }

    #[test]
    fn login_start_round_trips() {
        let original = LoginStart {
            name: "Notch".to_owned(),
            uuid: u128::MAX,
        };
        let mut buf = BytesMut::new();
        original.encode(&mut buf).unwrap();
        let mut src = &buf[..];
        assert_eq!(LoginStart::decode(&mut src).unwrap(), original);
        assert!(src.is_empty());
    }

    #[test]
    fn login_start_encodes_name_then_uuid() {
        let mut buf = BytesMut::new();
        LoginStart {
            name: "ab".to_owned(),
            uuid: 1,
        }
        .encode(&mut buf)
        .unwrap();
        assert_eq!(buf[0], 0x02, "string byte length");
        assert_eq!(&buf[1..3], b"ab");
        assert_eq!(buf.len(), 3 + 16, "name then a 16-byte uuid");
        assert_eq!(buf[buf.len() - 1], 0x01, "uuid is big-endian");
    }

    #[test]
    fn login_start_rejects_an_oversized_name() {
        // 16 chars => 48 byte cap; declare more.
        let mut buf = BytesMut::new();
        crate::varint::write_varint(&mut buf, 49);
        let mut src = &buf[..];
        assert!(matches!(
            LoginStart::decode(&mut src),
            Err(ProtocolError::StringTooLong { len: 49, max: 48 })
        ));
    }

    #[test]
    fn set_compression_round_trips_including_negative() {
        for threshold in [-1i32, 0, 256, 2_097_151] {
            let mut buf = BytesMut::new();
            SetCompression { threshold }.encode(&mut buf).unwrap();
            let mut src = &buf[..];
            assert_eq!(
                SetCompression::decode(&mut src).unwrap(),
                SetCompression { threshold }
            );
        }
    }

    #[test]
    fn login_success_round_trips() {
        let original = LoginSuccess {
            profile: sample_profile(),
            session_id: 42,
        };
        let mut buf = BytesMut::new();
        original.encode(&mut buf).unwrap();
        let mut src = &buf[..];
        assert_eq!(LoginSuccess::decode(&mut src).unwrap(), original);
        assert!(src.is_empty());
    }

    #[test]
    fn login_success_round_trips_with_no_properties_and_no_signature() {
        let original = LoginSuccess {
            profile: GameProfile {
                uuid: 7,
                username: "Steve".to_owned(),
                properties: Vec::new(),
            },
            session_id: 0,
        };
        let mut buf = BytesMut::new();
        original.encode(&mut buf).unwrap();
        let mut src = &buf[..];
        assert_eq!(LoginSuccess::decode(&mut src).unwrap(), original);
    }

    #[test]
    fn login_acknowledged_is_an_empty_body() {
        let mut buf = BytesMut::new();
        LoginAcknowledged.encode(&mut buf).unwrap();
        assert!(buf.is_empty());
        let mut src = &buf[..];
        LoginAcknowledged::decode(&mut src).unwrap();
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p pyrite-protocol --lib login`
Expected: compile failure — `LoginStart`, `SetCompression`, `LoginSuccess`, `LoginAcknowledged`, `GameProfile`, `ProfileProperty` are not defined.

- [ ] **Step 3: Write the implementation**

Insert into `crates/protocol/src/packets/login.rs`, above the test module. Extend the existing `use crate::buf::{...}` line to include the new helpers rather than adding a second import.

```rust
/// Maximum length of a username, in characters.
pub const MAX_USERNAME_CHARS: usize = 16;

/// Maximum length of a profile property's name, in characters.
const MAX_PROPERTY_NAME_CHARS: usize = 64;

/// Maximum length of a profile property's value, in characters.
const MAX_PROPERTY_VALUE_CHARS: usize = 32767;

/// Maximum length of a profile property's signature, in characters.
const MAX_SIGNATURE_CHARS: usize = 1024;

/// Maximum number of properties a game profile may carry.
///
/// Vanilla profiles carry at most a handful; the cap exists so a peer cannot
/// use the count to drive allocation.
const MAX_PROFILE_PROPERTIES: usize = 16;

/// One signed key/value pair attached to a player's profile, such as their
/// skin texture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileProperty {
    /// The property's name.
    pub name: String,
    /// The property's value.
    pub value: String,
    /// The Mojang signature over the value, when the property is signed.
    pub signature: Option<String>,
}

impl ProfileProperty {
    /// Writes this property's fields.
    fn encode_to<B: BufMut>(&self, dst: &mut B) {
        write_string(dst, &self.name);
        write_string(dst, &self.value);
        write_prefixed_optional(dst, self.signature.as_ref(), |dst, value| {
            write_string(dst, value)
        });
    }

    /// Reads one property's fields.
    fn decode_from<B: Buf>(src: &mut B) -> Result<Self, ProtocolError> {
        Ok(Self {
            name: read_string(src, MAX_PROPERTY_NAME_CHARS)?,
            value: read_string(src, MAX_PROPERTY_VALUE_CHARS)?,
            signature: read_prefixed_optional(src, |src| read_string(src, MAX_SIGNATURE_CHARS))?,
        })
    }
}

/// A player's identity as the server reports it back to them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameProfile {
    /// The player's UUID.
    pub uuid: u128,
    /// The player's username.
    pub username: String,
    /// Signed profile properties. Empty in offline mode.
    pub properties: Vec<ProfileProperty>,
}

impl GameProfile {
    /// Writes this profile's fields.
    fn encode_to<B: BufMut>(&self, dst: &mut B) {
        write_uuid(dst, self.uuid);
        write_string(dst, &self.username);
        write_prefixed_array(dst, &self.properties, |dst, property| {
            property.encode_to(dst)
        });
    }

    /// Reads a profile's fields.
    fn decode_from<B: Buf>(src: &mut B) -> Result<Self, ProtocolError> {
        Ok(Self {
            uuid: read_uuid(src)?,
            username: read_string(src, MAX_USERNAME_CHARS)?,
            properties: read_prefixed_array(
                src,
                MAX_PROFILE_PROPERTIES,
                ProfileProperty::decode_from,
            )?,
        })
    }
}

/// The first packet of the login sequence.
///
/// The UUID is supplied by the client and is **not** authenticated — a client
/// may send any value — so a server must never treat it as identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginStart {
    /// The username the client wishes to log in as.
    pub name: String,
    /// The client's claimed UUID. Unauthenticated.
    pub uuid: u128,
}

impl Packet for LoginStart {
    const ID: i32 = 0x00;
    const STATE: State = State::Login;
    const DIRECTION: Direction = Direction::Serverbound;

    fn encode<B: BufMut>(&self, dst: &mut B) -> Result<(), ProtocolError> {
        write_string(dst, &self.name);
        write_uuid(dst, self.uuid);
        Ok(())
    }

    fn decode<B: Buf>(src: &mut B) -> Result<Self, ProtocolError> {
        Ok(Self {
            name: read_string(src, MAX_USERNAME_CHARS)?,
            uuid: read_uuid(src)?,
        })
    }
}

/// Switches the connection to the compressed frame format.
///
/// This packet is itself sent uncompressed; the new format applies to
/// everything after it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetCompression {
    /// Size at or above which packets are compressed. Negative disables.
    pub threshold: i32,
}

impl Packet for SetCompression {
    const ID: i32 = 0x03;
    const STATE: State = State::Login;
    const DIRECTION: Direction = Direction::Clientbound;

    fn encode<B: BufMut>(&self, dst: &mut B) -> Result<(), ProtocolError> {
        write_varint(dst, self.threshold);
        Ok(())
    }

    fn decode<B: Buf>(src: &mut B) -> Result<Self, ProtocolError> {
        Ok(Self {
            threshold: read_varint(src)?,
        })
    }
}

/// Confirms login and tells the client the identity it was granted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginSuccess {
    /// The profile the server assigned.
    pub profile: GameProfile,
    /// Session identifier for this login.
    pub session_id: u128,
}

impl Packet for LoginSuccess {
    const ID: i32 = 0x02;
    const STATE: State = State::Login;
    const DIRECTION: Direction = Direction::Clientbound;

    fn encode<B: BufMut>(&self, dst: &mut B) -> Result<(), ProtocolError> {
        self.profile.encode_to(dst);
        write_uuid(dst, self.session_id);
        Ok(())
    }

    fn decode<B: Buf>(src: &mut B) -> Result<Self, ProtocolError> {
        Ok(Self {
            profile: GameProfile::decode_from(src)?,
            session_id: read_uuid(src)?,
        })
    }
}

/// The client's acknowledgement of [`LoginSuccess`].
///
/// Receiving it moves the connection into the configuration state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoginAcknowledged;

impl Packet for LoginAcknowledged {
    const ID: i32 = 0x03;
    const STATE: State = State::Login;
    const DIRECTION: Direction = Direction::Serverbound;

    fn encode<B: BufMut>(&self, _dst: &mut B) -> Result<(), ProtocolError> {
        Ok(())
    }

    fn decode<B: Buf>(_src: &mut B) -> Result<Self, ProtocolError> {
        Ok(Self)
    }
}
```

Update the file's `use` lines so `write_string`, `read_string`, `write_uuid`, `read_uuid`, `write_prefixed_array`, `read_prefixed_array`, `write_prefixed_optional`, `read_prefixed_optional` come from `crate::buf`, and `write_varint`, `read_varint` from `crate::varint`.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p pyrite-protocol --lib login`
Expected: the 8 new tests plus the 3 existing ones PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/protocol
git commit -m "feat(protocol): add login start, set compression, login success, and acknowledged"
```

---

## Task 4: Offline identity

**Files:**
- Modify: `Cargo.toml` (workspace dependency)
- Modify: `crates/net/Cargo.toml`
- Modify: `crates/net/src/error.rs`
- Create: `crates/net/src/offline.rs`
- Modify: `crates/net/src/lib.rs`

**Interfaces:**
- Consumes: `NetError`.
- Produces:
  - `pub fn offline_uuid(username: &str) -> u128`
  - `pub fn validate_username(name: &str) -> Result<(), NetError>`
  - `NetError::InvalidUsername { name: String }`

- [ ] **Step 1: Add the `md-5` dependency**

In the root `Cargo.toml`, add to `[workspace.dependencies]`:

```toml
md-5 = "0.10"
```

In `crates/net/Cargo.toml`, add to `[dependencies]`:

```toml
md-5.workspace = true
```

- [ ] **Step 2: Add the error variant**

Insert into `NetError` in `crates/net/src/error.rs`, after the `DuplicateStatusRequest` variant:

```rust
    /// The client asked to log in under a name the server will not accept.
    #[error("invalid username {name:?}")]
    InvalidUsername {
        /// The rejected name.
        name: String,
    },
```

- [ ] **Step 3: Write the failing tests**

Create `crates/net/src/offline.rs` containing only this:

```rust
//! Offline-mode player identity.

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn offline_uuid_is_stable_for_a_name() {
        assert_eq!(offline_uuid("Notch"), offline_uuid("Notch"));
    }

    #[test]
    fn offline_uuid_differs_between_names() {
        assert_ne!(offline_uuid("Notch"), offline_uuid("notch"));
        assert_ne!(offline_uuid("Notch"), offline_uuid("Steve"));
    }

    #[test]
    fn offline_uuid_has_version_3_and_rfc_4122_variant() {
        // A UUIDv3 must report version 3 in the high nibble of byte 6 and the
        // RFC 4122 variant in the top bits of byte 8. Clients and other server
        // implementations both check this.
        for name in ["Notch", "Steve", "a", "_______________x"] {
            let bytes = offline_uuid(name).to_be_bytes();
            assert_eq!(bytes[6] >> 4, 0x3, "version nibble for {name}");
            assert_eq!(bytes[8] >> 6, 0b10, "variant bits for {name}");
        }
    }

    #[test]
    fn valid_usernames_are_accepted() {
        for name in ["a", "Notch", "Steve_123", "________________"] {
            assert!(validate_username(name).is_ok(), "{name} should be valid");
        }
    }

    #[test]
    fn invalid_usernames_are_rejected() {
        for name in ["", "seventeen_chars_x", "has space", "hy-phen", "é"] {
            assert!(
                matches!(
                    validate_username(name),
                    Err(NetError::InvalidUsername { .. })
                ),
                "{name:?} should be rejected"
            );
        }
    }
}
```

- [ ] **Step 4: Run the tests to verify they fail**

Add `pub mod offline;` plus `pub use offline::{offline_uuid, validate_username};` to `crates/net/src/lib.rs`, then run:

Run: `cargo test -p pyrite-net --lib offline`
Expected: compile failure — `offline_uuid` and `validate_username` are not defined.

- [ ] **Step 5: Write the implementation**

Insert into `crates/net/src/offline.rs`, above the test module:

```rust
use md5::{Digest, Md5};

use crate::error::NetError;

/// Maximum username length, in characters.
const MAX_USERNAME_CHARS: usize = 16;

/// Derives the offline-mode UUID for a username.
///
/// This is a version 3 UUID per RFC 4122: an MD5 digest over the UTF-8 bytes
/// of `OfflinePlayer:<username>`, with the version nibble and variant bits
/// overwritten. Every implementation that follows the same convention derives
/// the same value, which is what keeps player data, permissions, and world
/// files interchangeable between servers.
///
/// The UUID a client sends in Login Start is deliberately not used: it is
/// unauthenticated, so honouring it would let anyone assume another player's
/// identity by asking.
pub fn offline_uuid(username: &str) -> u128 {
    let mut hasher = Md5::new();
    hasher.update(b"OfflinePlayer:");
    hasher.update(username.as_bytes());
    let mut bytes: [u8; 16] = hasher.finalize().into();

    // Byte 6's high nibble carries the version; set it to 3.
    bytes[6] = (bytes[6] & 0x0f) | 0x30;
    // Byte 8's top two bits carry the variant; set them to 0b10 (RFC 4122).
    bytes[8] = (bytes[8] & 0x3f) | 0x80;

    u128::from_be_bytes(bytes)
}

/// Checks that a username is one the server will accept.
///
/// One to sixteen characters of `[a-zA-Z0-9_]`. Names outside that set do not
/// round-trip through other tooling and are a cheap way to smuggle odd data
/// into logs and, later, world files — so they are refused at the door rather
/// than sanitised afterwards.
pub fn validate_username(name: &str) -> Result<(), NetError> {
    let valid = !name.is_empty()
        && name.chars().count() <= MAX_USERNAME_CHARS
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_');

    if valid {
        Ok(())
    } else {
        Err(NetError::InvalidUsername {
            name: name.to_owned(),
        })
    }
}
```

Note: the crate is `md-5` but its Rust name is `md5`.

- [ ] **Step 6: Run the tests**

Run: `cargo test -p pyrite-net --lib offline`
Expected: all 5 tests PASS.

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml Cargo.lock crates/net
git commit -m "feat(net): derive vanilla-compatible offline uuids and validate usernames"
```

---

## Task 5: The login flow

**Files:**
- Modify: `crates/net/src/config.rs`
- Modify: `crates/net/src/state.rs`
- Modify: `crates/net/src/connection.rs`
- Modify: `crates/net/tests/server_list_ping.rs`
- Create: `crates/net/tests/login.rs`

**Interfaces:**
- Consumes: login packets (Task 3), `offline_uuid`/`validate_username` (Task 4), `PacketCodec::set_compression` (Task 2).
- Produces: `ServerConfig.compression_threshold: Option<i32>`; a connection that reaches `State::Configuration`.

- [ ] **Step 1: Add the config field**

In `crates/net/src/config.rs`, add to `ServerConfig`:

```rust
    /// Size at or above which packets are compressed, or `None` to disable
    /// compression entirely.
    pub compression_threshold: Option<i32>,
```

and to its `Default` impl:

```rust
            // The conventional default. Small packets stay uncompressed, so
            // the ping path pays nothing, while chunk data later will.
            compression_threshold: Some(256),
```

- [ ] **Step 2: Permit the Login to Configuration transition**

In `crates/net/src/state.rs`, change the `permitted` expression in `transition` to:

```rust
        let permitted = matches!(
            (self.current, to),
            (State::Handshaking, State::Status)
                | (State::Handshaking, State::Login)
                | (State::Login, State::Configuration)
        );
```

and update the method's doc comment, replacing the sentence beginning "The only legal moves in Milestone 1" with:

```rust
    /// Legal moves are out of `Handshaking` into `Status` or `Login`, and from
    /// `Login` into `Configuration` once the client acknowledges login. In
    /// particular `Status -> Login` is refused: a status connection is
    /// unauthenticated and must not be able to promote itself.
```

- [ ] **Step 3: Write the failing state test**

Append to `mod tests` in `crates/net/src/state.rs`:

```rust
    #[test]
    fn login_may_advance_to_configuration() {
        let mut fsm = ConnectionState::new();
        fsm.transition(State::Login).unwrap();
        fsm.transition(State::Configuration).unwrap();
        assert_eq!(fsm.current(), State::Configuration);
    }

    #[test]
    fn configuration_is_terminal_in_this_milestone() {
        let mut fsm = ConnectionState::new();
        fsm.transition(State::Login).unwrap();
        fsm.transition(State::Configuration).unwrap();
        assert!(matches!(
            fsm.transition(State::Play),
            Err(NetError::IllegalTransition { .. })
        ));
    }
```

- [ ] **Step 4: Write the failing integration tests**

Create `crates/net/tests/login.rs`:

```rust
//! End-to-end verification of the Milestone 2 login sequence.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use pyrite_net::{Connection, NetError, ServerConfig, offline_uuid};
use pyrite_protocol::codec::PacketCodec;
use pyrite_protocol::packets::handshake::{Handshake, NextState};
use pyrite_protocol::packets::login::{
    LoginAcknowledged, LoginDisconnect, LoginStart, LoginSuccess, SetCompression,
};
use pyrite_protocol::text::TextComponent;
use pyrite_protocol::{PROTOCOL_VERSION, Packet};
use tokio_util::codec::Framed;

fn config(compression_threshold: Option<i32>) -> Arc<ServerConfig> {
    Arc::new(ServerConfig {
        motd: TextComponent::new("A Pyrite Server"),
        max_players: 100,
        read_timeout: Duration::from_secs(5),
        compression_threshold,
    })
}

fn handshake() -> Handshake {
    Handshake {
        protocol_version: PROTOCOL_VERSION,
        server_address: "localhost".to_owned(),
        server_port: 25565,
        next_state: NextState::Login,
    }
}

#[tokio::test]
async fn a_full_login_reaches_configuration() {
    let (client, server) = tokio::io::duplex(8192);
    let task = tokio::spawn(Connection::new(server, config(Some(256))).run());
    let mut client = Framed::new(client, PacketCodec::new());

    client.send(handshake()).await.unwrap();
    client
        .send(LoginStart {
            name: "Notch".to_owned(),
            // A deliberately bogus claimed uuid; the server must ignore it.
            uuid: u128::MAX,
        })
        .await
        .unwrap();

    // Set Compression arrives first, still in the uncompressed format.
    let frame = client.next().await.unwrap().unwrap();
    assert_eq!(frame.id, SetCompression::ID);
    let set: SetCompression = frame.decode_as().unwrap();
    assert_eq!(set.threshold, 256);

    // From here the client must speak the compressed format too.
    client.codec_mut().set_compression(set.threshold);

    let frame = client.next().await.unwrap().unwrap();
    assert_eq!(frame.id, LoginSuccess::ID);
    let success: LoginSuccess = frame.decode_as().unwrap();

    assert_eq!(success.profile.username, "Notch");
    assert_eq!(
        success.profile.uuid,
        offline_uuid("Notch"),
        "the server must derive the uuid, not echo the client's"
    );
    assert_ne!(success.profile.uuid, u128::MAX);
    assert!(success.profile.properties.is_empty());

    client.send(LoginAcknowledged).await.unwrap();

    // Nothing is sent in Configuration this milestone, so the connection
    // idles until the read timeout closes it. That is the expected end state.
    let result = task.await.unwrap();
    assert!(
        matches!(result, Err(NetError::Timeout)),
        "expected the connection to idle in configuration, got {result:?}"
    );
}

#[tokio::test]
async fn login_without_compression_skips_set_compression() {
    let (client, server) = tokio::io::duplex(8192);
    let task = tokio::spawn(Connection::new(server, config(None)).run());
    let mut client = Framed::new(client, PacketCodec::new());

    client.send(handshake()).await.unwrap();
    client
        .send(LoginStart {
            name: "Steve".to_owned(),
            uuid: 0,
        })
        .await
        .unwrap();

    let frame = client.next().await.unwrap().unwrap();
    assert_eq!(
        frame.id,
        LoginSuccess::ID,
        "with compression disabled, login success comes first"
    );

    client.send(LoginAcknowledged).await.unwrap();
    assert!(matches!(task.await.unwrap(), Err(NetError::Timeout)));
}

#[tokio::test]
async fn an_invalid_username_is_disconnected_without_a_login_success() {
    let (client, server) = tokio::io::duplex(8192);
    let task = tokio::spawn(Connection::new(server, config(Some(256))).run());
    let mut client = Framed::new(client, PacketCodec::new());

    client.send(handshake()).await.unwrap();
    client
        .send(LoginStart {
            name: "has space".to_owned(),
            uuid: 0,
        })
        .await
        .unwrap();

    let frame = client.next().await.unwrap().unwrap();
    assert_eq!(frame.id, LoginDisconnect::ID);
    let disconnect: LoginDisconnect = frame.decode_as().unwrap();
    let reason: serde_json::Value = serde_json::from_str(&disconnect.reason).unwrap();
    assert!(
        reason["text"].as_str().unwrap().contains("username"),
        "the reason should name the problem, got {reason}"
    );

    assert!(client.next().await.is_none(), "the connection closes");
    task.await.unwrap().unwrap();
}

#[tokio::test]
async fn set_compression_is_itself_sent_uncompressed() {
    // The switchover is the subtle part: Set Compression uses the old format
    // and everything after it uses the new one. A client that flips too early
    // cannot parse the packet that told it to flip.
    let (client, server) = tokio::io::duplex(8192);
    let task = tokio::spawn(Connection::new(server, config(Some(256))).run());
    let mut client = Framed::new(client, PacketCodec::new());

    client.send(handshake()).await.unwrap();
    client
        .send(LoginStart {
            name: "Notch".to_owned(),
            uuid: 0,
        })
        .await
        .unwrap();

    // Decoded with a codec that has NOT been switched: this only succeeds if
    // the server sent it in the uncompressed format.
    let frame = client.next().await.unwrap().unwrap();
    assert_eq!(frame.id, SetCompression::ID);

    // And Login Success must NOT be readable without switching, proving the
    // server did change format immediately afterwards.
    client.codec_mut().set_compression(256);
    let frame = client.next().await.unwrap().unwrap();
    assert_eq!(frame.id, LoginSuccess::ID);

    drop(client);
    let _ = task.await.unwrap();
}
```

- [ ] **Step 5: Update the obsolete Milestone 1 test**

`crates/net/tests/server_list_ping.rs` contains `login_receives_a_graceful_disconnect`, which asserts that a handshake with next state Login produces a `LoginDisconnect` saying login is not implemented. Task 5 makes login work, so that assertion is now wrong. **Delete that test** — `crates/net/tests/login.rs` covers the login path in full, and `an_invalid_username_is_disconnected_without_a_login_success` covers the disconnect path.

Also update `config()` in `server_list_ping.rs` and in `hostile_peer.rs` to include the new field:

```rust
        compression_threshold: None,
```

Status-state connections never enable compression, so `None` keeps those tests exercising exactly what they did before.

- [ ] **Step 6: Run the tests to verify they fail**

Run: `cargo test -p pyrite-net`
Expected: compile failure — `codec_mut` usage aside, the connection does not yet handle `LoginStart`, so the login tests fail.

Note: `Framed::codec_mut` is a real `tokio_util` method; no work is needed to provide it.

- [ ] **Step 7: Rewrite the handshake handler and add the login handlers**

In `crates/net/src/connection.rs`, replace the `NextState::Login | NextState::Transfer` arm of `handle_handshake` with:

```rust
            // A transferred client is mid-login from our point of view, so it
            // takes the same path as a fresh login. Cookies, which a real
            // transfer also carries, are not implemented.
            NextState::Login | NextState::Transfer => {
                self.state.transition(State::Login)?;
                Ok(Flow::Continue)
            }
```

Add these two handlers to the same `impl` block:

```rust
    async fn handle_login_start(&mut self, frame: &RawPacket) -> Result<Flow, NetError> {
        let start: LoginStart = frame.decode_as()?;

        if let Err(error) = validate_username(&start.name) {
            debug!(name = %start.name, "rejecting invalid username");
            let packet = LoginDisconnect::from_component(&TextComponent::new(
                "Invalid username. Use 1-16 characters of A-Z, a-z, 0-9 or _.",
            ))?;
            self.framed.send(packet).await?;
            return Err(error);
        }

        // The uuid in Login Start is unauthenticated, so it is discarded and
        // the offline uuid derived from the name instead.
        let uuid = offline_uuid(&start.name);

        if let Some(threshold) = self.config.compression_threshold {
            // Send and flush Set Compression in the OLD format, then switch.
            // Switching first would compress the very packet that announces
            // compression, which no client can read.
            self.framed.send(SetCompression { threshold }).await?;
            self.framed.flush().await?;
            self.framed.codec_mut().set_compression(threshold);
        }

        self.framed
            .send(LoginSuccess {
                profile: GameProfile {
                    uuid,
                    username: start.name.clone(),
                    properties: Vec::new(),
                },
                session_id: uuid,
            })
            .await?;

        debug!(name = %start.name, %uuid, "login succeeded");
        Ok(Flow::Continue)
    }

    async fn handle_login_acknowledged(&mut self) -> Result<Flow, NetError> {
        self.state.transition(State::Configuration)?;
        debug!("entering configuration");
        // Configuration packets arrive in a later milestone. Until then the
        // connection idles here and the read timeout eventually closes it.
        Ok(Flow::Continue)
    }
```

Add these dispatch arms to `handle`, after the existing Status arms:

```rust
            (State::Login, LoginStart::ID) => self.handle_login_start(&frame).await,
            (State::Login, LoginAcknowledged::ID) => self.handle_login_acknowledged().await,
```

Update the imports at the top of `connection.rs`: add `GameProfile`, `LoginAcknowledged`, `LoginStart`, `LoginSuccess`, `SetCompression` to the `pyrite_protocol::packets::login::{...}` line, add `use crate::offline::{offline_uuid, validate_username};`, and add `SinkExt`'s `flush` (already available via the existing `futures_util::SinkExt` import). Delete the now-unused `LOGIN_UNAVAILABLE` constant.

- [ ] **Step 8: Run the tests**

Run: `cargo test --workspace`
Expected: all tests PASS, including the 4 new login integration tests and the 2 new state tests.

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 9: Commit**

```bash
git add crates/net
git commit -m "feat(net): implement the offline-mode login flow with compression switchover"
```

---

## Task 6: CLI flags and the binding interlock

**Files:**
- Modify: `crates/server/src/main.rs`

**Interfaces:**
- Consumes: `ServerConfig.compression_threshold`.
- Produces: `--compression-threshold`, `--insecure-offline-mode`, and `fn check_bind_safety(addr: SocketAddr, insecure_offline_mode: bool) -> Result<(), String>`.

- [ ] **Step 1: Write the failing tests**

Append to `crates/server/src/main.rs`:

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    fn addr(text: &str) -> SocketAddr {
        text.parse().unwrap()
    }

    #[test]
    fn loopback_binds_need_no_flag() {
        assert!(check_bind_safety(addr("127.0.0.1:25565"), false).is_ok());
        assert!(check_bind_safety(addr("[::1]:25565"), false).is_ok());
    }

    #[test]
    fn public_binds_are_refused_without_the_flag() {
        for text in ["0.0.0.0:25565", "192.168.1.10:25565", "[::]:25565"] {
            let result = check_bind_safety(addr(text), false);
            assert!(result.is_err(), "{text} must be refused");
            let message = result.unwrap_err();
            assert!(
                message.contains("--insecure-offline-mode"),
                "the error must name the flag that overrides it, got {message}"
            );
            assert!(
                message.contains("any username"),
                "the error must say what the risk actually is, got {message}"
            );
        }
    }

    #[test]
    fn public_binds_are_permitted_with_the_flag() {
        assert!(check_bind_safety(addr("0.0.0.0:25565"), true).is_ok());
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p pyrite-server`
Expected: compile failure — `check_bind_safety` is not defined.

- [ ] **Step 3: Add the CLI flags**

In `crates/server/src/main.rs`, add to `Args`:

```rust
    /// Size at or above which packets are compressed. Negative disables
    /// compression entirely.
    #[arg(long, env = "PYRITE_COMPRESSION_THRESHOLD", default_value_t = 256)]
    compression_threshold: i32,

    /// Permit binding a non-loopback address while authentication is
    /// unimplemented.
    ///
    /// Without this, the server refuses to listen anywhere but loopback,
    /// because offline mode lets any client join under any username.
    #[arg(long, env = "PYRITE_INSECURE_OFFLINE_MODE", default_value_t = false)]
    insecure_offline_mode: bool,
```

- [ ] **Step 4: Write the interlock**

Add to `crates/server/src/main.rs`, above `main`:

```rust
/// Refuses a non-loopback bind while the server can only run in offline mode.
///
/// Offline mode authenticates nobody: any client may join under any username,
/// including one that has been granted operator rights. Until authentication
/// lands, exposing the server on a public interface has to be a deliberate act
/// rather than the default, so the check lives at startup where it cannot be
/// reached around.
fn check_bind_safety(addr: SocketAddr, insecure_offline_mode: bool) -> Result<(), String> {
    if insecure_offline_mode || addr.ip().is_loopback() {
        return Ok(());
    }

    Err(format!(
        "refusing to bind {addr}: this server runs in offline mode, so any client could \
         join under any username, including one holding operator rights. Bind a loopback \
         address such as 127.0.0.1:{port} instead, or pass --insecure-offline-mode if you \
         genuinely intend to expose it.",
        port = addr.port()
    ))
}
```

- [ ] **Step 5: Wire it into `main`**

In `crates/server/src/main.rs`, immediately after the `tracing_subscriber` initialisation and before the `ServerConfig` is built, insert:

```rust
    if let Err(message) = check_bind_safety(args.bind, args.insecure_offline_mode) {
        error!("{message}");
        std::process::exit(1);
    }

    if args.insecure_offline_mode && !args.bind.ip().is_loopback() {
        warn!(
            address = %args.bind,
            "listening publicly in offline mode: any client can join under any username"
        );
    }
```

Then add the threshold to the `ServerConfig` construction:

```rust
        compression_threshold: (args.compression_threshold >= 0)
            .then_some(args.compression_threshold),
```

- [ ] **Step 6: Run the tests**

Run: `cargo test --workspace`
Expected: all tests PASS, including the 3 new interlock tests.

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: clean.

Run: `cargo fmt --all --check`
Expected: clean.

- [ ] **Step 7: Verify the interlock by hand**

Run: `cargo run -p pyrite-server -- --bind 0.0.0.0:25565`
Expected: an error naming the risk and the flag, and a non-zero exit. Confirm with `echo $?` that the exit code is 1.

Run: `cargo run -p pyrite-server -- --bind 127.0.0.1:25565`
Expected: starts normally and logs the listening line. Stop it with Ctrl-C.

- [ ] **Step 8: Commit**

```bash
git add crates/server
git commit -m "feat(server): add compression flag and refuse public binds in offline mode"
```

---

## Self-Review

**Spec coverage.** §4.1 buf primitives → Task 1. §4.2 compression and its four guards → Task 2 (guard 1 `a_negative_data_length_is_rejected`, guard 2 `a_declared_inflated_size_above_the_maximum_is_rejected`, guard 3 `a_compressed_packet_below_the_threshold_is_rejected`, guard 4 `a_payload_that_inflates_to_the_wrong_size_is_rejected`). §4.3 login packets → Task 3. §5.1 offline identity → Task 4. §5.2 login flow and the switchover ordering → Task 5. §5.3 `ServerConfig` → Task 5 Step 1. §6 CLI and interlock → Task 6. §7 testing → tests throughout, with §7's interlock requirement ("extract the decision into a function over `(SocketAddr, bool)`") satisfied by Task 6's `check_bind_safety`.

**Type consistency.** `set_compression(&mut self, threshold: i32)` is defined in Task 2 and called in Task 5 Step 7 and in Task 5's integration tests with the same signature. `offline_uuid(&str) -> u128` is defined in Task 4 and used in Task 5's handler and assertions. `GameProfile { uuid, username, properties }` is defined in Task 3 and constructed in Task 5 with those exact field names. `ServerConfig.compression_threshold: Option<i32>` is added in Task 5 Step 1 and consumed in Task 5 Step 7 and Task 6 Step 5. `MAX_USERNAME_CHARS` exists twice deliberately — `pyrite_protocol::packets::login::MAX_USERNAME_CHARS` bounds decoding, and a private copy in `offline.rs` bounds policy; they are the same value but different concerns, and the protocol crate must not depend on net.

**Two hazards restated for executors.**
1. Task 5 Step 5 deletes M1's `login_receives_a_graceful_disconnect` test, because Task 5 deliberately changes the behaviour it asserts. Removing it is correct, not test-suppression; the login path gains four replacement tests.
2. Task 5 Step 1 adds a field to `ServerConfig`, which breaks every existing struct-literal construction of it. Task 5 Step 5 names the two test files that must be updated (`server_list_ping.rs`, `hostile_peer.rs`); `main.rs` is updated in Task 6 Step 5.

**Ordering note.** Task 2 (compression) must precede Task 5, since the login flow calls `set_compression`. Tasks 1, 2, 3, 4 are otherwise independent of each other except that Task 3 consumes Task 1's primitives.
