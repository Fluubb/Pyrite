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
pub fn read_prefixed_optional<B, T, F>(
    src: &mut B,
    read_value: F,
) -> Result<Option<T>, ProtocolError>
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
        assert!(
            read_prefixed_array(&mut src, 16, read_u16)
                .unwrap()
                .is_empty()
        );
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
}
