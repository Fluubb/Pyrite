//! Primitive field codecs shared by every packet implementation.

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
    let len = usize::try_from(declared).map_err(|_| ProtocolError::NegativeLength(declared))?;

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
