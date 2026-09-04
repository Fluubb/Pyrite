//! Decoding NBT documents.

use bytes::Buf;

use crate::error::{MAX_DEPTH, MAX_TOTAL_NODES, NbtError};
use crate::tag::{NbtCompound, NbtList, NbtTag, TagId};

/// Caps how many elements are pre-allocated for an array or list.
///
/// The declared count is peer-controlled, so the vector grows with real data
/// rather than with the claim. Matches the convention in
/// `pyrite-protocol`'s `buf` module.
const MAX_PREALLOC_ELEMENTS: usize = 64;

/// Reads a document in the network form: a type byte, then the payload.
///
/// The root carries no name. Reading a file-form document with this will fail
/// or produce nonsense, which is why the two forms are separate functions.
pub fn read_network_root<B: Buf>(src: &mut B) -> Result<NbtCompound, NbtError> {
    let id = read_tag_id(src)?;
    if id != TagId::Compound {
        return Err(NbtError::RootNotCompound(id));
    }
    read_compound_body(src, 1, &mut 0)
}

/// Reads a document in the file form: a type byte, a name, then the payload.
pub fn read_named_root<B: Buf>(src: &mut B) -> Result<(String, NbtCompound), NbtError> {
    let id = read_tag_id(src)?;
    if id != TagId::Compound {
        return Err(NbtError::RootNotCompound(id));
    }
    let name = read_nbt_string(src)?;
    let compound = read_compound_body(src, 1, &mut 0)?;
    Ok((name, compound))
}

/// Reads a network-form document that may be absent.
///
/// Some packets encode "no document here" as a lone `End` tag rather than as
/// an empty compound, so a bare `0x00` yields `Ok(None)`.
pub fn read_optional_network_root<B: Buf>(src: &mut B) -> Result<Option<NbtCompound>, NbtError> {
    let id = read_tag_id(src)?;
    match id {
        TagId::End => Ok(None),
        TagId::Compound => Ok(Some(read_compound_body(src, 1, &mut 0)?)),
        other => Err(NbtError::RootNotCompound(other)),
    }
}

/// Reads one type byte.
fn read_tag_id<B: Buf>(src: &mut B) -> Result<TagId, NbtError> {
    if !src.has_remaining() {
        return Err(NbtError::UnexpectedEof);
    }
    TagId::try_from(src.get_u8())
}

/// Reads a length-prefixed UTF-8 string.
///
/// The prefix is unsigned 16-bit, so it cannot exceed the input by more than
/// 64 KiB, but the remaining-bytes check still runs before allocating.
fn read_nbt_string<B: Buf>(src: &mut B) -> Result<String, NbtError> {
    if src.remaining() < 2 {
        return Err(NbtError::UnexpectedEof);
    }
    let len = usize::from(src.get_u16());

    if src.remaining() < len {
        return Err(NbtError::LengthExceedsInput {
            declared: len,
            remaining: src.remaining(),
        });
    }

    let mut bytes = vec![0u8; len];
    src.copy_to_slice(&mut bytes);
    String::from_utf8(bytes).map_err(|error| NbtError::InvalidUtf8(error.utf8_error()))
}

/// Reads a signed 32-bit element count and checks it against the input.
///
/// `min_element_size` is the smallest number of bytes one element can occupy.
/// For fixed-width elements that is exact; for compounds and lists it is a
/// lower bound, which is all that is needed to reject a count that could not
/// possibly fit.
fn read_length<B: Buf>(src: &mut B, min_element_size: usize) -> Result<usize, NbtError> {
    if src.remaining() < 4 {
        return Err(NbtError::UnexpectedEof);
    }
    let declared = src.get_i32();
    let len = usize::try_from(declared).map_err(|_| NbtError::NegativeLength(declared))?;

    let required =
        len.checked_mul(min_element_size)
            .ok_or_else(|| NbtError::LengthExceedsInput {
                // Unreachable on 64-bit; kept for 32-bit targets, where the
                // product can overflow `usize`. Report the raw element count
                // rather than a saturated byte figure: a saturated
                // `usize::MAX` would read as a bug in the error rather than
                // what actually happened, which is that the multiplication
                // overflowed.
                declared: len,
                remaining: src.remaining(),
            })?;

    if required > src.remaining() {
        return Err(NbtError::LengthExceedsInput {
            declared: required,
            remaining: src.remaining(),
        });
    }

    Ok(len)
}

/// Reads a compound's entries up to its terminating `End`.
///
/// `depth` counts wire nesting levels, not stack frames: it is passed
/// unchanged to [`read_payload`] for each entry, which is the one place that
/// increments it. One nesting level therefore costs one depth unit for both
/// compounds and lists, to within a single level -- a list sits at the same
/// depth as its containing compound, so a chain of lists reaches one level
/// deeper than a chain of compounds before the limit fires. Immaterial to
/// stack safety; noted so the constant is not read as an exact promise.
///
/// `nodes` is the running total of tags decoded for this document, bounding
/// the tree where `depth` bounds only its nesting.
///
/// This guard covers the recursion at the `TagId::Compound` arm of
/// [`read_payload`]. It is not redundant with that function's own guard,
/// which covers a path this one never sees: list elements.
fn read_compound_body<B: Buf>(
    src: &mut B,
    depth: usize,
    nodes: &mut usize,
) -> Result<NbtCompound, NbtError> {
    if depth > MAX_DEPTH {
        return Err(NbtError::DepthExceeded { max: MAX_DEPTH });
    }

    let mut compound = NbtCompound::new();
    loop {
        let id = read_tag_id(src)?;
        if id == TagId::End {
            reject_duplicate_keys(&compound)?;
            return Ok(compound);
        }
        let name = read_nbt_string(src)?;
        let value = read_payload(src, id, depth, nodes)?;
        // `push`, not `insert`: insert scans every existing entry to replace
        // duplicates in place, which makes decoding an N-entry compound cost
        // O(N^2) string comparisons. A peer controls N, so a single
        // maximum-size document measured at roughly ninety seconds of blocked
        // cpu before this changed. `push` can let duplicates survive, which
        // is why the whole compound is checked once above, after the loop.
        compound.push(name, value);
    }
}

/// Rejects a compound holding two entries with the same name.
///
/// Runs once per compound, after every entry has been read, rather than as a
/// per-entry scan -- an O(N log N) sort-and-compare over the finished
/// compound, not the O(N^2) pairwise scan `NbtCompound::insert` would cost if
/// called per entry. At the largest permitted compound this is tens of
/// milliseconds, against the roughly ninety seconds the quadratic path cost.
///
/// A compound admitting duplicates would leave its own API disagreeing about
/// what the document says -- see [`NbtError::DuplicateKey`] -- so this must
/// reject rather than silently keep one copy.
fn reject_duplicate_keys(compound: &NbtCompound) -> Result<(), NbtError> {
    if compound.len() < 2 {
        return Ok(());
    }

    let mut names: Vec<&str> = compound.iter().map(|(name, _)| name.as_str()).collect();
    names.sort_unstable();

    for pair in names.windows(2) {
        if pair[0] == pair[1] {
            return Err(NbtError::DuplicateKey {
                name: pair[0].to_owned(),
            });
        }
    }

    Ok(())
}

/// Reads one tag's payload, given its already-decoded type.
///
/// `depth` is the wire nesting level of `id` itself; recursing into a nested
/// compound or list element increments it by one (see [`read_compound_body`]).
///
/// This guard covers entry from the list-element loop below, where no other
/// check has run. It is not redundant with [`read_compound_body`]'s guard,
/// which covers a path this one never sees: a compound's own recursion.
fn read_payload<B: Buf>(
    src: &mut B,
    id: TagId,
    depth: usize,
    nodes: &mut usize,
) -> Result<NbtTag, NbtError> {
    if depth > MAX_DEPTH {
        return Err(NbtError::DepthExceeded { max: MAX_DEPTH });
    }

    *nodes += 1;
    if *nodes > MAX_TOTAL_NODES {
        return Err(NbtError::TooManyNodes {
            max: MAX_TOTAL_NODES,
        });
    }

    /// Reads a fixed-width value after checking the input holds it.
    macro_rules! fixed {
        ($size:expr, $get:ident, $variant:ident) => {{
            if src.remaining() < $size {
                return Err(NbtError::UnexpectedEof);
            }
            NbtTag::$variant(src.$get())
        }};
    }

    Ok(match id {
        // `End` never reaches here: compound bodies handle it as a terminator
        // and a list declaring it is rejected before elements are read.
        TagId::End => return Err(NbtError::InvalidListElementType(TagId::End)),
        TagId::Byte => fixed!(1, get_i8, Byte),
        TagId::Short => fixed!(2, get_i16, Short),
        TagId::Int => fixed!(4, get_i32, Int),
        TagId::Long => fixed!(8, get_i64, Long),
        TagId::Float => fixed!(4, get_f32, Float),
        TagId::Double => fixed!(8, get_f64, Double),
        TagId::ByteArray => {
            let len = read_length(src, 1)?;
            NbtTag::ByteArray(src.copy_to_bytes(len))
        }
        TagId::String => NbtTag::String(read_nbt_string(src)?),
        TagId::List => {
            let element_type = read_tag_id(src)?;

            // A list of End may only be empty; the element size below assumes
            // a real type, and a non-empty list of End is malformed anyway.
            let min_element_size = match element_type {
                TagId::End => 0,
                TagId::Byte => 1,
                TagId::Short => 2,
                TagId::Int | TagId::Float => 4,
                TagId::Long | TagId::Double => 8,
                // Variable-width: one byte is the smallest an element can be
                // (an empty compound is its lone End terminator).
                _ => 1,
            };

            let len = read_length(src, min_element_size)?;
            if element_type == TagId::End && len != 0 {
                return Err(NbtError::InvalidListElementType(TagId::End));
            }

            let mut items = Vec::with_capacity(len.min(MAX_PREALLOC_ELEMENTS));
            for _ in 0..len {
                items.push(read_payload(src, element_type, depth + 1, nodes)?);
            }
            NbtTag::List(NbtList::from_parts(element_type, items))
        }
        TagId::Compound => NbtTag::Compound(read_compound_body(src, depth + 1, nodes)?),
        TagId::IntArray => {
            let len = read_length(src, 4)?;
            let mut values = Vec::with_capacity(len.min(MAX_PREALLOC_ELEMENTS));
            for _ in 0..len {
                values.push(src.get_i32());
            }
            NbtTag::IntArray(values)
        }
        TagId::LongArray => {
            let len = read_length(src, 8)?;
            let mut values = Vec::with_capacity(len.min(MAX_PREALLOC_ELEMENTS));
            for _ in 0..len {
                values.push(src.get_i64());
            }
            NbtTag::LongArray(values)
        }
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use crate::tag::{NbtCompound, NbtList, NbtTag};
    use crate::write::{write_named_root, write_network_root};
    use bytes::{BufMut, Bytes, BytesMut};

    /// A document exercising every tag type.
    fn sample() -> NbtCompound {
        let mut inner = NbtCompound::new();
        inner.insert("nested", 1i8);

        let mut root = NbtCompound::new();
        root.insert("byte", 1i8);
        root.insert("short", -2i16);
        root.insert("int", 3i32);
        root.insert("long", -4i64);
        root.insert("float", 1.5f32);
        root.insert("double", -2.5f64);
        root.insert("bytes", Bytes::from_static(&[1, 2, 3]));
        root.insert("string", "hello");
        root.insert(
            "list",
            NbtList::new(vec![NbtTag::Int(1), NbtTag::Int(2)]).unwrap(),
        );
        root.insert("empty_list", NbtList::empty());
        root.insert("compound", inner);
        root.insert("ints", vec![1i32, -1]);
        root.insert("longs", vec![1i64, -1]);
        root
    }

    #[test]
    fn every_tag_type_round_trips_as_a_value() {
        let original = sample();
        let mut buf = BytesMut::new();
        write_network_root(&mut buf, &original).unwrap();

        let mut src = &buf[..];
        let decoded = read_network_root(&mut src).unwrap();

        assert_eq!(decoded, original);
        assert!(src.is_empty(), "the whole document must be consumed");
    }

    #[test]
    fn every_tag_type_round_trips_as_bytes() {
        // The value round trip alone cannot catch an encoder and decoder that
        // are wrong in the same way; re-encoding and comparing bytes can.
        let mut buf = BytesMut::new();
        write_network_root(&mut buf, &sample()).unwrap();

        let mut src = &buf[..];
        let decoded = read_network_root(&mut src).unwrap();

        let mut reencoded = BytesMut::new();
        write_network_root(&mut reencoded, &decoded).unwrap();
        assert_eq!(reencoded, buf);
    }

    #[test]
    fn a_named_root_round_trips_with_its_name() {
        let mut buf = BytesMut::new();
        write_named_root(&mut buf, "root", &sample()).unwrap();

        let mut src = &buf[..];
        let (name, decoded) = read_named_root(&mut src).unwrap();
        assert_eq!(name, "root");
        assert_eq!(decoded, sample());
    }

    #[test]
    fn reading_a_named_document_as_a_network_one_silently_misreads_it() {
        // The forms differ by the root's name, so a network read of a named
        // document consumes the name's length prefix as a tag id. For any
        // name under 256 bytes that high byte is 0x00 -- an End -- so the
        // read "succeeds" and yields an empty compound instead of failing.
        //
        // This is precisely why the two forms are separate entry points
        // rather than one function with a flag: nothing at the type level or
        // on the wire will catch the confusion for you, so the call site has
        // to say which format it means.
        let mut buf = BytesMut::new();
        write_named_root(&mut buf, "root", &sample()).unwrap();

        let mut src = &buf[..];
        let misread = read_network_root(&mut src).unwrap();

        assert_ne!(
            misread,
            sample(),
            "the document must not survive the confusion"
        );
        assert!(
            misread.is_empty(),
            "the name's high length byte reads as End"
        );
    }

    #[test]
    fn an_empty_compound_round_trips() {
        let mut buf = BytesMut::new();
        write_network_root(&mut buf, &NbtCompound::new()).unwrap();
        let mut src = &buf[..];
        assert_eq!(read_network_root(&mut src).unwrap(), NbtCompound::new());
    }

    #[test]
    fn an_absent_optional_document_is_none() {
        // Some packets encode "no nbt here" as a lone End tag rather than an
        // empty compound.
        let buf: &[u8] = &[0x00];
        let mut src = buf;
        assert_eq!(read_optional_network_root(&mut src).unwrap(), None);
    }

    #[test]
    fn a_present_optional_document_is_some() {
        let mut buf = BytesMut::new();
        write_network_root(&mut buf, &sample()).unwrap();
        let mut src = &buf[..];
        assert_eq!(
            read_optional_network_root(&mut src).unwrap(),
            Some(sample())
        );
    }

    #[test]
    fn a_non_compound_root_is_rejected() {
        let buf: &[u8] = &[0x03, 0x00, 0x00, 0x00, 0x01];
        let mut src = buf;
        assert!(matches!(
            read_network_root(&mut src),
            Err(NbtError::RootNotCompound(TagId::Int))
        ));
    }

    #[test]
    fn an_unknown_tag_id_is_rejected() {
        // Root compound, then a field with type byte 0x7f.
        let buf: &[u8] = &[0x0a, 0x7f, 0x00, 0x01, b'x'];
        let mut src = buf;
        assert!(matches!(
            read_network_root(&mut src),
            Err(NbtError::UnknownTag(0x7f))
        ));
    }

    #[test]
    fn nesting_beyond_the_depth_limit_is_rejected() {
        // Compounds nested past MAX_DEPTH. Without a limit this recurses
        // until the stack overflows, which with panic = "abort" takes the
        // whole process down rather than one connection.
        let mut buf = BytesMut::new();
        buf.put_u8(0x0a); // root
        for _ in 0..(MAX_DEPTH + 10) {
            buf.put_u8(0x0a); // nested compound
            buf.put_u16(0); // empty name
        }

        let mut src = &buf[..];
        assert!(matches!(
            read_network_root(&mut src),
            Err(NbtError::DepthExceeded { max: MAX_DEPTH })
        ));
    }

    #[test]
    fn a_negative_array_length_is_rejected() {
        let mut buf = BytesMut::new();
        buf.put_u8(0x0a); // root
        buf.put_u8(0x0b); // int array
        buf.put_u16(1);
        buf.put_slice(b"a");
        buf.put_i32(-1);

        let mut src = &buf[..];
        assert!(matches!(
            read_network_root(&mut src),
            Err(NbtError::NegativeLength(-1))
        ));
    }

    #[test]
    fn an_array_longer_than_the_input_is_rejected() {
        // Two billion ints declared inside a fifteen-byte document. The
        // containing frame is bounded elsewhere, but that bounds the frame,
        // not what the frame claims about itself.
        let mut buf = BytesMut::new();
        buf.put_u8(0x0a);
        buf.put_u8(0x0b); // int array
        buf.put_u16(1);
        buf.put_slice(b"a");
        buf.put_i32(2_000_000_000);

        let mut src = &buf[..];
        assert!(matches!(
            read_network_root(&mut src),
            Err(NbtError::LengthExceedsInput { .. })
        ));
    }

    #[test]
    fn a_list_longer_than_the_input_is_rejected() {
        let mut buf = BytesMut::new();
        buf.put_u8(0x0a);
        buf.put_u8(0x09); // list
        buf.put_u16(1);
        buf.put_slice(b"a");
        buf.put_u8(0x0a); // of compounds
        buf.put_i32(2_000_000_000);

        let mut src = &buf[..];
        assert!(matches!(
            read_network_root(&mut src),
            Err(NbtError::LengthExceedsInput { .. })
        ));
    }

    #[test]
    fn a_non_empty_list_of_end_is_rejected() {
        let mut buf = BytesMut::new();
        buf.put_u8(0x0a);
        buf.put_u8(0x09);
        buf.put_u16(1);
        buf.put_slice(b"a");
        buf.put_u8(0x00); // element type End
        buf.put_i32(3); // but three elements

        let mut src = &buf[..];
        assert!(matches!(
            read_network_root(&mut src),
            Err(NbtError::InvalidListElementType(TagId::End))
        ));
    }

    #[test]
    fn invalid_utf8_in_a_string_is_rejected() {
        let mut buf = BytesMut::new();
        buf.put_u8(0x0a);
        buf.put_u8(0x08); // string
        buf.put_u16(1);
        buf.put_slice(b"a");
        buf.put_u16(2);
        buf.put_slice(&[0xff, 0xfe]);

        let mut src = &buf[..];
        assert!(matches!(
            read_network_root(&mut src),
            Err(NbtError::InvalidUtf8(_))
        ));
    }

    #[test]
    fn a_document_may_be_followed_by_further_fields() {
        // Milestone 4's packets carry nbt followed by other fields, so the
        // reader must consume exactly one document and leave the rest for the
        // next field's decoder. The "exactly one packet body" guarantee lives
        // one layer up in RawPacket::decode_as, which is the only layer that
        // can know whether a buffer holds one packet or a shared cursor.
        let mut buf = BytesMut::new();
        write_network_root(&mut buf, &sample()).unwrap();
        buf.extend_from_slice(&[0xAB, 0xCD]);

        let mut src = &buf[..];
        let decoded = read_network_root(&mut src).unwrap();

        assert_eq!(decoded, sample());
        assert_eq!(src, &[0xAB, 0xCD], "following fields must survive intact");
    }

    #[test]
    fn truncation_at_any_point_is_rejected() {
        let mut buf = BytesMut::new();
        write_network_root(&mut buf, &sample()).unwrap();

        // Every proper prefix is incomplete and must error rather than
        // producing a partial document.
        for cut in 1..buf.len() {
            let mut src = &buf[..cut];
            assert!(
                read_network_root(&mut src).is_err(),
                "a {cut}-byte prefix must not decode"
            );
        }
    }

    #[test]
    fn duplicate_keys_are_rejected_not_silently_collapsed() {
        // Deterministic regression for the quadratic-decode fix and its
        // fallout. `NbtCompound::insert` scanned every existing entry to
        // replace duplicates in place -- O(N^2) over an N-entry compound, a
        // peer-controlled N, roughly ninety seconds of blocked cpu for one
        // maximum-size document. The decoder now appends (`push`) instead,
        // which is linear, but `push` by itself lets duplicate names survive
        // into a compound whose own API then disagrees with itself: `get`
        // answers with the first match, iteration yields the last. `insert`
        // could never produce this assertion's failure mode because it
        // silently collapsed duplicates down to one entry; `push` alone would
        // pass this document through with all of them. The decoder must
        // reject it instead.
        //
        // This is deterministic and instant, unlike a wall-clock bound: it
        // fails the same way on every machine, in debug or release, on a
        // loaded CI runner or an idle one.
        const ENTRIES: usize = 40_000;

        let mut buf = BytesMut::new();
        buf.put_u8(0x0a);
        for _ in 0..ENTRIES {
            buf.put_u8(0x01); // byte
            buf.put_u16(1);
            buf.put_slice(b"k");
            buf.put_i8(1);
        }
        buf.put_u8(0x00);

        let mut src = &buf[..];
        assert!(matches!(
            read_network_root(&mut src),
            Err(NbtError::DuplicateKey { name }) if name == "k"
        ));
    }

    #[test]
    fn a_large_flat_compound_with_distinct_keys_decodes_quickly() {
        // Secondary signal alongside the deterministic duplicate-key
        // assertion above: distinct names never trip the duplicate check, so
        // this instead measures throughput on the healthy decode path.
        //
        // The bound is tight, not generous: the healthy path takes
        // single-digit milliseconds for this input, so two seconds already
        // leaves roughly two orders of magnitude of headroom. A prior version
        // of this test allowed ten seconds and passed at 9.54s after the
        // quadratic bug it existed to catch was deliberately reintroduced --
        // headroom that wide made the bound worthless as a regression guard
        // and flaky against a merely slow runner. The duplicate-key test is
        // the primary guard against the regression; this one only confirms
        // the fix didn't also cost throughput.
        const ENTRIES: usize = 40_000;

        let mut buf = BytesMut::new();
        buf.put_u8(0x0a);
        for i in 0..ENTRIES {
            let name = format!("{i:04x}");
            buf.put_u8(0x01);
            buf.put_u16(name.len() as u16);
            buf.put_slice(name.as_bytes());
            buf.put_i8(1);
        }
        buf.put_u8(0x00);

        let started = std::time::Instant::now();
        let mut src = &buf[..];
        let decoded = read_network_root(&mut src).unwrap();
        let elapsed = started.elapsed();

        assert_eq!(decoded.len(), ENTRIES);
        assert!(
            elapsed < std::time::Duration::from_secs(2),
            "decoding {ENTRIES} distinct entries took {elapsed:?}, which suggests the quadratic path is back"
        );
    }

    #[test]
    fn a_document_exceeding_the_node_budget_is_rejected() {
        // A list of empty compounds costs one wire byte per element but a
        // whole NbtTag per element, so without a node budget a small document
        // expands roughly fortyfold in memory.
        let mut buf = BytesMut::new();
        buf.put_u8(0x0a);
        buf.put_u8(0x09); // list
        buf.put_u16(1);
        buf.put_slice(b"l");
        buf.put_u8(0x0a); // of compounds
        buf.put_i32((MAX_TOTAL_NODES + 10) as i32);
        for _ in 0..(MAX_TOTAL_NODES + 10) {
            buf.put_u8(0x00); // each an immediately-terminated empty compound
        }

        let mut src = &buf[..];
        assert!(matches!(
            read_network_root(&mut src),
            Err(NbtError::TooManyNodes {
                max: MAX_TOTAL_NODES
            })
        ));
    }

    #[test]
    fn a_document_within_the_node_budget_still_decodes() {
        let mut buf = BytesMut::new();
        buf.put_u8(0x0a);
        buf.put_u8(0x09);
        buf.put_u16(1);
        buf.put_slice(b"l");
        buf.put_u8(0x0a);
        buf.put_i32(1_000);
        for _ in 0..1_000 {
            buf.put_u8(0x00);
        }
        buf.put_u8(0x00); // terminates the root compound

        let mut src = &buf[..];
        let decoded = read_network_root(&mut src).unwrap();
        assert!(
            matches!(decoded.get("l"), Some(NbtTag::List(list)) if list.len() == 1_000),
            "expected a list of 1000 elements, got {:?}",
            decoded.get("l").map(NbtTag::id)
        );
    }

    #[test]
    fn a_document_at_exactly_the_node_budget_decodes() {
        // Boundary check on the budget sized against the memory-densest
        // shape (see MAX_TOTAL_NODES's doc comment): the limit rejects
        // anything *over* budget, not the budget count itself. One node here
        // is the list tag; the rest are its elements.
        let element_count = MAX_TOTAL_NODES - 1;

        let mut buf = BytesMut::new();
        buf.put_u8(0x0a);
        buf.put_u8(0x09); // list
        buf.put_u16(1);
        buf.put_slice(b"l");
        buf.put_u8(0x0a); // of compounds
        buf.put_i32(element_count as i32);
        for _ in 0..element_count {
            buf.put_u8(0x00);
        }
        buf.put_u8(0x00); // terminates the root compound

        let mut src = &buf[..];
        let decoded = read_network_root(&mut src).unwrap();
        assert!(
            matches!(decoded.get("l"), Some(NbtTag::List(list)) if list.len() == element_count),
            "a document totalling exactly MAX_TOTAL_NODES nodes must still decode"
        );
    }

    #[test]
    fn a_document_one_node_over_the_budget_is_rejected() {
        // The other side of the same boundary: one more node than the budget
        // must fail, not merely "enough more to notice".
        let mut buf = BytesMut::new();
        buf.put_u8(0x0a);
        buf.put_u8(0x09); // list
        buf.put_u16(1);
        buf.put_slice(b"l");
        buf.put_u8(0x0a); // of compounds
        buf.put_i32(MAX_TOTAL_NODES as i32);
        for _ in 0..MAX_TOTAL_NODES {
            buf.put_u8(0x00);
        }

        let mut src = &buf[..];
        assert!(matches!(
            read_network_root(&mut src),
            Err(NbtError::TooManyNodes {
                max: MAX_TOTAL_NODES
            })
        ));
    }
}
