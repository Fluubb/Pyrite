//! Zero-allocation VarInt and VarLong codecs.

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
