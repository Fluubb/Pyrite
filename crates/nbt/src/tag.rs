//! The NBT value tree.

use crate::error::NbtError;

/// A tag's wire identifier.
///
/// Includes `End`, which is a structural marker in the encoding rather than a
/// value a document can hold — hence its absence from [`NbtTag`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum TagId {
    /// Terminates a compound. Carries no payload.
    End = 0,
    /// Signed 8-bit integer.
    Byte = 1,
    /// Signed 16-bit integer.
    Short = 2,
    /// Signed 32-bit integer.
    Int = 3,
    /// Signed 64-bit integer.
    Long = 4,
    /// IEEE 754 binary32.
    Float = 5,
    /// IEEE 754 binary64.
    Double = 6,
    /// Length-prefixed byte sequence.
    ByteArray = 7,
    /// Length-prefixed UTF-8 string.
    String = 8,
    /// Homogeneous sequence with one declared element type.
    List = 9,
    /// Named tags terminated by an `End`.
    Compound = 10,
    /// Length-prefixed sequence of 32-bit integers.
    IntArray = 11,
    /// Length-prefixed sequence of 64-bit integers.
    LongArray = 12,
}

impl TryFrom<u8> for TagId {
    type Error = NbtError;

    fn try_from(value: u8) -> Result<Self, NbtError> {
        Ok(match value {
            0 => Self::End,
            1 => Self::Byte,
            2 => Self::Short,
            3 => Self::Int,
            4 => Self::Long,
            5 => Self::Float,
            6 => Self::Double,
            7 => Self::ByteArray,
            8 => Self::String,
            9 => Self::List,
            10 => Self::Compound,
            11 => Self::IntArray,
            12 => Self::LongArray,
            other => return Err(NbtError::UnknownTag(other)),
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn tag_ids_match_the_documented_numbering() {
        for (id, expected) in [
            (0u8, TagId::End),
            (1, TagId::Byte),
            (2, TagId::Short),
            (3, TagId::Int),
            (4, TagId::Long),
            (5, TagId::Float),
            (6, TagId::Double),
            (7, TagId::ByteArray),
            (8, TagId::String),
            (9, TagId::List),
            (10, TagId::Compound),
            (11, TagId::IntArray),
            (12, TagId::LongArray),
        ] {
            assert_eq!(TagId::try_from(id).unwrap(), expected, "id {id}");
            assert_eq!(expected as u8, id, "discriminant for {expected:?}");
        }
    }

    #[test]
    fn unknown_tag_ids_are_rejected() {
        for id in [13u8, 14, 200, 255] {
            assert!(matches!(
                TagId::try_from(id),
                Err(NbtError::UnknownTag(got)) if got == id
            ));
        }
    }
}
