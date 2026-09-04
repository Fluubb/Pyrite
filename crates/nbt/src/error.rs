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
