//! Encoding NBT documents.

use bytes::BufMut;

use crate::error::NbtError;
use crate::tag::{NbtCompound, NbtTag, TagId};

/// Writes a document in the network form: a type byte, then the payload.
///
/// The root's name is omitted. This has been the network form since protocol
/// 764; use [`write_named_root`] for save files.
pub fn write_network_root<B: BufMut>(dst: &mut B, value: &NbtCompound) -> Result<(), NbtError> {
    dst.put_u8(TagId::Compound as u8);
    write_compound_body(dst, value)
}

/// Writes a document in the file form: a type byte, a name, then the payload.
pub fn write_named_root<B: BufMut>(
    dst: &mut B,
    name: &str,
    value: &NbtCompound,
) -> Result<(), NbtError> {
    dst.put_u8(TagId::Compound as u8);
    write_nbt_string(dst, name)?;
    write_compound_body(dst, value)
}

/// Writes a length-prefixed UTF-8 string.
///
/// The prefix is an unsigned 16-bit length, so a longer string cannot be
/// represented at all. Reporting that is the only correct option: truncating
/// would corrupt the document silently, and panicking is forbidden on any
/// path reachable from input.
fn write_nbt_string<B: BufMut>(dst: &mut B, value: &str) -> Result<(), NbtError> {
    let len = value.len();
    let len = u16::try_from(len).map_err(|_| NbtError::StringTooLong { len })?;
    dst.put_u16(len);
    dst.put_slice(value.as_bytes());
    Ok(())
}

/// Converts a collection length to the `i32` the format uses.
///
/// A bare `as i32` would wrap a collection at or above `i32::MAX` into a
/// negative length, which the decoder then rejects -- silent corruption on
/// write, loud failure on read. Unreachable in practice, but strings already
/// check their `u16` prefix two functions above, and an unchecked cast beside
/// a checked one reads as though the difference was intended.
fn array_len(len: usize) -> Result<i32, NbtError> {
    i32::try_from(len).map_err(|_| NbtError::ArrayTooLong { len })
}

/// Writes a compound's entries followed by the terminating `End`.
fn write_compound_body<B: BufMut>(dst: &mut B, value: &NbtCompound) -> Result<(), NbtError> {
    for (name, tag) in value.iter() {
        dst.put_u8(tag.id() as u8);
        write_nbt_string(dst, name)?;
        write_payload(dst, tag)?;
    }
    dst.put_u8(TagId::End as u8);
    Ok(())
}

/// Writes a tag's payload, without its type byte or name.
fn write_payload<B: BufMut>(dst: &mut B, tag: &NbtTag) -> Result<(), NbtError> {
    match tag {
        NbtTag::Byte(value) => dst.put_i8(*value),
        NbtTag::Short(value) => dst.put_i16(*value),
        NbtTag::Int(value) => dst.put_i32(*value),
        NbtTag::Long(value) => dst.put_i64(*value),
        NbtTag::Float(value) => dst.put_f32(*value),
        NbtTag::Double(value) => dst.put_f64(*value),
        NbtTag::ByteArray(value) => {
            dst.put_i32(array_len(value.len())?);
            dst.put_slice(value);
        }
        NbtTag::String(value) => write_nbt_string(dst, value)?,
        NbtTag::List(list) => {
            // The element type is written once, and elements follow as bare
            // payloads with no per-element tag.
            dst.put_u8(list.element_type() as u8);
            dst.put_i32(array_len(list.len())?);
            for item in list.items() {
                write_payload(dst, item)?;
            }
        }
        NbtTag::Compound(compound) => write_compound_body(dst, compound)?,
        NbtTag::IntArray(values) => {
            dst.put_i32(array_len(values.len())?);
            for value in values {
                dst.put_i32(*value);
            }
        }
        NbtTag::LongArray(values) => {
            dst.put_i32(array_len(values.len())?);
            for value in values {
                dst.put_i64(*value);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use crate::error::NbtError;
    use crate::tag::{NbtCompound, NbtList, NbtTag};
    use bytes::BytesMut;

    #[test]
    fn a_network_root_omits_the_name() {
        // Since protocol 764 the network form is a type byte followed
        // directly by the payload. An empty compound is therefore two bytes:
        // the compound tag, then the End that closes it.
        let mut buf = BytesMut::new();
        write_network_root(&mut buf, &NbtCompound::new()).unwrap();
        assert_eq!(&buf[..], &[0x0a, 0x00]);
    }

    #[test]
    fn a_named_root_includes_the_name() {
        // The file form inserts a u16-prefixed name between the type byte and
        // the payload. This is the difference that silently misaligns a whole
        // document if the wrong form is used.
        let mut buf = BytesMut::new();
        write_named_root(&mut buf, "hi", &NbtCompound::new()).unwrap();
        assert_eq!(&buf[..], &[0x0a, 0x00, 0x02, b'h', b'i', 0x00]);
    }

    #[test]
    fn a_compound_with_one_int_matches_the_documented_layout() {
        // Byte for byte from the format documentation:
        //   0a           compound
        //   0003 "num"   name, u16-prefixed
        //   03           int tag
        //   0001 "a"     name
        //   0000007b     123
        //   00           end of the inner compound
        //   00           end of the root
        let mut inner = NbtCompound::new();
        inner.insert("a", 123i32);
        let mut root = NbtCompound::new();
        root.insert("num", inner);

        let mut buf = BytesMut::new();
        write_network_root(&mut buf, &root).unwrap();

        let expected: &[u8] = &[
            0x0a, // root compound, network form: no name
            0x0a, 0x00, 0x03, b'n', b'u', b'm', // compound "num"
            0x03, 0x00, 0x01, b'a', 0x00, 0x00, 0x00, 0x7b, // int "a" = 123
            0x00, // end of "num"
            0x00, // end of root
        ];
        assert_eq!(&buf[..], expected);
    }

    #[test]
    fn scalars_are_big_endian() {
        let mut root = NbtCompound::new();
        root.insert("s", 0x0102i16);
        root.insert("l", 0x0102_0304_0506_0708i64);

        let mut buf = BytesMut::new();
        write_network_root(&mut buf, &root).unwrap();

        // Locate the short's payload: root tag, then tag+name(1+2+1) for "s".
        assert_eq!(&buf[5..7], &[0x01, 0x02]);
        assert_eq!(
            &buf[11..19],
            &[0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08]
        );
    }

    #[test]
    fn an_empty_list_writes_the_end_type_and_a_zero_length() {
        let mut root = NbtCompound::new();
        root.insert("l", NbtList::empty());

        let mut buf = BytesMut::new();
        write_network_root(&mut buf, &root).unwrap();

        let expected: &[u8] = &[
            0x0a, // root
            0x09, 0x00, 0x01, b'l', // list "l"
            0x00, // element type End
            0x00, 0x00, 0x00, 0x00, // length 0
            0x00, // end of root
        ];
        assert_eq!(&buf[..], expected);
    }

    #[test]
    fn a_list_writes_its_type_once_and_bare_payloads() {
        let mut root = NbtCompound::new();
        root.insert(
            "l",
            NbtList::new(vec![NbtTag::Byte(7), NbtTag::Byte(8)]).unwrap(),
        );

        let mut buf = BytesMut::new();
        write_network_root(&mut buf, &root).unwrap();

        let expected: &[u8] = &[
            0x0a, //
            0x09, 0x00, 0x01, b'l', //
            0x01, // element type Byte
            0x00, 0x00, 0x00, 0x02, // length 2
            0x07, 0x08, // bare payloads, no per-element tags
            0x00,
        ];
        assert_eq!(&buf[..], expected);
    }

    #[test]
    fn arrays_write_a_signed_length_then_elements() {
        let mut root = NbtCompound::new();
        root.insert("i", vec![1i32, -1i32]);

        let mut buf = BytesMut::new();
        write_network_root(&mut buf, &root).unwrap();

        let expected: &[u8] = &[
            0x0a, //
            0x0b, 0x00, 0x01, b'i', // int array "i"
            0x00, 0x00, 0x00, 0x02, // length 2
            0x00, 0x00, 0x00, 0x01, // 1
            0xff, 0xff, 0xff, 0xff, // -1
            0x00,
        ];
        assert_eq!(&buf[..], expected);
    }

    #[test]
    fn a_string_longer_than_the_length_prefix_allows_is_rejected() {
        // The prefix is a u16, so 65536 bytes cannot be represented. Writing
        // must report that rather than truncating, which would silently
        // corrupt the document.
        let mut root = NbtCompound::new();
        root.insert("s", "x".repeat(65_536));

        let mut buf = BytesMut::new();
        assert!(matches!(
            write_network_root(&mut buf, &root),
            Err(NbtError::StringTooLong { len: 65_536 })
        ));
    }

    #[test]
    fn a_name_longer_than_the_length_prefix_allows_is_rejected() {
        let mut root = NbtCompound::new();
        root.insert("x".repeat(70_000), 1i32);

        let mut buf = BytesMut::new();
        assert!(matches!(
            write_network_root(&mut buf, &root),
            Err(NbtError::StringTooLong { .. })
        ));
    }
}
