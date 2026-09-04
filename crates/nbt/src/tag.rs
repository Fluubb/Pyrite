//! The NBT value tree.

use crate::error::NbtError;
use bytes::Bytes;

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

/// A value in an NBT document.
///
/// `End` is absent by design: it is a structural marker in the encoding, not
/// a value a document can hold, and a variant for it would force every match
/// to carry an unreachable arm.
#[derive(Debug, Clone, PartialEq)]
pub enum NbtTag {
    /// Signed 8-bit integer.
    Byte(i8),
    /// Signed 16-bit integer.
    Short(i16),
    /// Signed 32-bit integer.
    Int(i32),
    /// Signed 64-bit integer.
    Long(i64),
    /// IEEE 754 binary32.
    Float(f32),
    /// IEEE 754 binary64.
    Double(f64),
    /// Raw bytes. Held as [`Bytes`] so bulk payloads are refcounted slices
    /// rather than copies.
    ByteArray(Bytes),
    /// A UTF-8 string.
    String(String),
    /// A homogeneous list.
    List(NbtList),
    /// A named collection.
    Compound(NbtCompound),
    /// A sequence of 32-bit integers. Owned rather than borrowed because the
    /// elements need byte-swapping out of the big-endian wire form.
    IntArray(Vec<i32>),
    /// A sequence of 64-bit integers.
    LongArray(Vec<i64>),
}

impl NbtTag {
    /// The wire identifier for this tag's type.
    pub fn id(&self) -> TagId {
        match self {
            Self::Byte(_) => TagId::Byte,
            Self::Short(_) => TagId::Short,
            Self::Int(_) => TagId::Int,
            Self::Long(_) => TagId::Long,
            Self::Float(_) => TagId::Float,
            Self::Double(_) => TagId::Double,
            Self::ByteArray(_) => TagId::ByteArray,
            Self::String(_) => TagId::String,
            Self::List(_) => TagId::List,
            Self::Compound(_) => TagId::Compound,
            Self::IntArray(_) => TagId::IntArray,
            Self::LongArray(_) => TagId::LongArray,
        }
    }
}

macro_rules! impl_from {
    ($($ty:ty => $variant:ident),* $(,)?) => {
        $(
            impl From<$ty> for NbtTag {
                fn from(value: $ty) -> Self {
                    Self::$variant(value)
                }
            }
        )*
    };
}

impl_from! {
    i8 => Byte,
    i16 => Short,
    i32 => Int,
    i64 => Long,
    f32 => Float,
    f64 => Double,
    Bytes => ByteArray,
    String => String,
    Vec<i32> => IntArray,
    Vec<i64> => LongArray,
    NbtList => List,
    NbtCompound => Compound,
}

impl From<&str> for NbtTag {
    fn from(value: &str) -> Self {
        Self::String(value.to_owned())
    }
}

/// An ordered collection of named tags.
///
/// Backed by a vector rather than a map: compound order carries no meaning in
/// the format, but preserving it makes encoded output deterministic and
/// byte-exact assertions possible. Compounds hold tens of keys, so linear
/// lookup is not a concern.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NbtCompound {
    entries: Vec<(String, NbtTag)>,
}

impl NbtCompound {
    /// Creates an empty compound.
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts a tag, replacing any existing entry with the same name in
    /// place so that ordering is stable across updates.
    ///
    /// NBT compounds do not permit duplicate names, so replacing rather than
    /// appending is what the format requires.
    pub fn insert(&mut self, name: impl Into<String>, value: impl Into<NbtTag>) {
        let name = name.into();
        let value = value.into();
        match self.entries.iter_mut().find(|(key, _)| *key == name) {
            Some(entry) => entry.1 = value,
            None => self.entries.push((name, value)),
        }
    }

    /// Returns the tag with this name, if present.
    pub fn get(&self, name: &str) -> Option<&NbtTag> {
        self.entries
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value)
    }

    /// The number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the compound has no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Iterates entries in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = &(String, NbtTag)> {
        self.entries.iter()
    }
}

/// A homogeneous sequence.
///
/// The declared element type is carried alongside the items so that an empty
/// list round-trips with the type it was written with.
#[derive(Debug, Clone, PartialEq)]
pub struct NbtList {
    element_type: TagId,
    items: Vec<NbtTag>,
}

impl NbtList {
    /// Creates an empty list, which by convention declares element type
    /// [`TagId::End`].
    pub fn empty() -> Self {
        Self {
            element_type: TagId::End,
            items: Vec::new(),
        }
    }

    /// Creates a list from items that must all share one type.
    ///
    /// Homogeneity is enforced here rather than at encode time, so that a
    /// list which exists is always encodable.
    pub fn new(items: Vec<NbtTag>) -> Result<Self, NbtError> {
        let Some(first) = items.first() else {
            return Ok(Self::empty());
        };
        let element_type = first.id();

        for item in &items {
            if item.id() != element_type {
                return Err(NbtError::HeterogeneousList {
                    expected: element_type,
                    found: item.id(),
                });
            }
        }

        Ok(Self {
            element_type,
            items,
        })
    }

    /// Builds a list whose homogeneity the caller has already established.
    ///
    /// Used by the decoder, which reads a declared element type and then
    /// reads exactly that many values of exactly that type, so the invariant
    /// holds structurally.
    #[allow(dead_code)]
    pub(crate) fn from_parts(element_type: TagId, items: Vec<NbtTag>) -> Self {
        Self {
            element_type,
            items,
        }
    }

    /// The declared element type.
    pub fn element_type(&self) -> TagId {
        self.element_type
    }

    /// The elements.
    pub fn items(&self) -> &[NbtTag] {
        &self.items
    }

    /// The number of elements.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether the list has no elements.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
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

    #[test]
    fn every_tag_reports_its_wire_id() {
        for (tag, expected) in [
            (NbtTag::Byte(1), TagId::Byte),
            (NbtTag::Short(1), TagId::Short),
            (NbtTag::Int(1), TagId::Int),
            (NbtTag::Long(1), TagId::Long),
            (NbtTag::Float(1.0), TagId::Float),
            (NbtTag::Double(1.0), TagId::Double),
            (NbtTag::ByteArray(Bytes::new()), TagId::ByteArray),
            (NbtTag::String(String::new()), TagId::String),
            (NbtTag::List(NbtList::empty()), TagId::List),
            (NbtTag::Compound(NbtCompound::new()), TagId::Compound),
            (NbtTag::IntArray(Vec::new()), TagId::IntArray),
            (NbtTag::LongArray(Vec::new()), TagId::LongArray),
        ] {
            assert_eq!(tag.id(), expected);
        }
    }

    #[test]
    fn compounds_preserve_insertion_order() {
        let mut compound = NbtCompound::new();
        compound.insert("zebra", 1i32);
        compound.insert("apple", 2i32);
        compound.insert("mango", 3i32);

        let keys: Vec<&str> = compound.iter().map(|(key, _)| key.as_str()).collect();
        assert_eq!(
            keys,
            ["zebra", "apple", "mango"],
            "order must be insertion order, not sorted or hashed"
        );
    }

    #[test]
    fn inserting_an_existing_key_replaces_it_in_place() {
        let mut compound = NbtCompound::new();
        compound.insert("a", 1i32);
        compound.insert("b", 2i32);
        compound.insert("a", 99i32);

        assert_eq!(compound.len(), 2, "a duplicate key must not be appended");
        assert_eq!(compound.get("a"), Some(&NbtTag::Int(99)));
        let keys: Vec<&str> = compound.iter().map(|(key, _)| key.as_str()).collect();
        assert_eq!(keys, ["a", "b"], "replacement keeps the original position");
    }

    #[test]
    fn compound_lookup_finds_and_misses() {
        let mut compound = NbtCompound::new();
        compound.insert("present", "yes");
        assert_eq!(
            compound.get("present"),
            Some(&NbtTag::String("yes".to_owned()))
        );
        assert_eq!(compound.get("absent"), None);
    }

    #[test]
    fn an_empty_list_declares_the_end_type() {
        let list = NbtList::empty();
        assert_eq!(list.element_type(), TagId::End);
        assert!(list.is_empty());
    }

    #[test]
    fn a_homogeneous_list_infers_its_element_type() {
        let list = NbtList::new(vec![NbtTag::Int(1), NbtTag::Int(2)]).unwrap();
        assert_eq!(list.element_type(), TagId::Int);
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn a_heterogeneous_list_cannot_be_constructed() {
        // Enforced here rather than at encode time, so that writing a
        // well-formed tree is infallible with respect to list typing.
        let result = NbtList::new(vec![NbtTag::Int(1), NbtTag::String("x".to_owned())]);
        assert!(matches!(
            result,
            Err(NbtError::HeterogeneousList {
                expected: TagId::Int,
                found: TagId::String
            })
        ));
    }

    #[test]
    fn conversions_produce_the_expected_variants() {
        assert_eq!(NbtTag::from(1i8), NbtTag::Byte(1));
        assert_eq!(NbtTag::from(1i16), NbtTag::Short(1));
        assert_eq!(NbtTag::from(1i32), NbtTag::Int(1));
        assert_eq!(NbtTag::from(1i64), NbtTag::Long(1));
        assert_eq!(NbtTag::from(1.5f32), NbtTag::Float(1.5));
        assert_eq!(NbtTag::from(1.5f64), NbtTag::Double(1.5));
        assert_eq!(NbtTag::from("x"), NbtTag::String("x".to_owned()));
        assert_eq!(NbtTag::from("x".to_owned()), NbtTag::String("x".to_owned()));
        assert_eq!(NbtTag::from(vec![1i32]), NbtTag::IntArray(vec![1]));
        assert_eq!(NbtTag::from(vec![1i64]), NbtTag::LongArray(vec![1]));
    }
}
