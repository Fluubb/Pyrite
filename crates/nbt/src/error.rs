//! Typed errors for NBT encoding and decoding.

use crate::tag::TagId;

/// Maximum nesting depth accepted when decoding.
///
/// Compounds and lists decode by recursive descent, so a document consisting
/// of deeply nested list openings is a few hundred kilobytes on the wire and
/// overflows the stack. With `panic = "abort"` in the release profile that
/// terminates the whole process rather than one connection, so the limit is a
/// hard bound rather than a nicety. No documented wire limit exists; 512 is
/// far beyond any legitimate document and far below anything that threatens
/// the stack.
pub const MAX_DEPTH: usize = 512;

/// Maximum number of tags a single document may decode to.
///
/// Bounds the *tree*, where [`MAX_DEPTH`] bounds only its *nesting*. The two
/// are independent: a flat document one level deep can still be enormous.
///
/// Sized from the memory-densest shape, not the cheapest one. A list of empty
/// compounds costs one wire byte per node; a compound *entry* -- a name and a
/// value, which is what a flat document is actually made of -- costs as
/// little as 4 wire bytes (a type byte, a zero-length `u16` name, one payload
/// byte) but occupies `size_of::<(String, NbtTag)>()` = 64 bytes once
/// decoded, since `size_of::<NbtTag>()` alone is 40. A budget sized against
/// the list shape would let a maximum-size frame smuggle through roughly two
/// nodes' worth more of the entry shape than the budget was meant to permit,
/// which defeats the point of having one.
///
/// At `1 << 16` (65,536) nodes, the worst-case (compound-entry) shape caps a
/// decoded document at 65,536 * 64 bytes = 4 MiB of tree, plus the transient
/// doubling of the backing `Vec` during the last few reallocations. Real
/// registry documents contain a few thousand tags, so this still leaves
/// roughly twentyfold margin over real usage.
pub const MAX_TOTAL_NODES: usize = 1 << 16;

/// Every way NBT encoding or decoding can fail.
#[derive(Debug, thiserror::Error)]
pub enum NbtError {
    /// The input ended in the middle of a field.
    #[error("unexpected end of input while decoding nbt")]
    UnexpectedEof,

    /// A type byte did not name a known tag.
    #[error("unknown nbt tag id {0}")]
    UnknownTag(u8),

    /// A document's root was not a compound. Both root forms are documented
    /// as beginning with one.
    #[error("nbt root must be a compound, found {0:?}")]
    RootNotCompound(TagId),

    /// The document nested deeper than [`MAX_DEPTH`].
    #[error("nbt nesting exceeded the maximum depth of {max}")]
    DepthExceeded {
        /// The depth limit that was exceeded.
        max: usize,
    },

    /// An array or list declared a negative length. Lengths are signed 32-bit
    /// on the wire, so a negative value is representable and must be rejected
    /// rather than cast into a huge unsigned value.
    #[error("negative nbt length: {0}")]
    NegativeLength(i32),

    /// An array or list declared more elements than the input could hold.
    ///
    /// Checked before allocating, because the declared length is
    /// peer-controlled: a four-byte field can claim two billion elements
    /// inside a twenty-byte document.
    #[error("nbt length declares {declared} bytes but only {remaining} remain")]
    LengthExceedsInput {
        /// Bytes the declared length would require.
        declared: usize,
        /// Bytes actually left in the input.
        remaining: usize,
    },

    /// A list declared an element type it may not have: `End` for a non-empty
    /// list, or a value above 12.
    #[error("invalid nbt list element type {0:?}")]
    InvalidListElementType(TagId),

    /// A list was constructed with elements of differing types.
    #[error("heterogeneous nbt list: expected {expected:?}, found {found:?}")]
    HeterogeneousList {
        /// The type the list's first element established.
        expected: TagId,
        /// The type of the offending element.
        found: TagId,
    },

    /// The document decoded to more tags than [`MAX_TOTAL_NODES`] permits.
    #[error("nbt document exceeded the maximum of {max} tags")]
    TooManyNodes {
        /// The node budget that was exhausted.
        max: usize,
    },

    /// A compound contained two entries with the same name.
    ///
    /// NBT forbids duplicate names in a compound. Admitting them anyway would
    /// leave the compound's own API disagreeing with itself about what the
    /// document says: [`crate::NbtCompound::get`] answers with the first
    /// match, while iterating (or collecting into a map) yields the last. Two
    /// consumers of the same bytes could then observe different values for
    /// the same key, so a duplicate is rejected rather than admitted under
    /// either reading.
    #[error("nbt compound contains a duplicate key: {name:?}")]
    DuplicateKey {
        /// The name that appeared more than once.
        name: String,
    },

    /// An array or list held more elements than its `i32` length can express.
    #[error("nbt array of {len} elements exceeds the i32 length prefix")]
    ArrayTooLong {
        /// The collection's length.
        len: usize,
    },

    /// A string exceeded the `u16` length prefix the format allows.
    #[error("nbt string of {len} bytes exceeds the 65535 byte maximum")]
    StringTooLong {
        /// The string's length in bytes.
        len: usize,
    },

    /// A string field was not valid UTF-8. NBT on the wire uses regular
    /// UTF-8, not Java's modified variant, so this is a genuine error rather
    /// than an encoding to translate.
    #[error("nbt string was not valid utf-8")]
    InvalidUtf8(#[from] std::str::Utf8Error),
}
