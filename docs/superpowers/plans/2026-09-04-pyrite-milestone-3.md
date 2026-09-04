# Pyrite Milestone 3 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement NBT as a standalone `pyrite-nbt` crate, so Milestone 4 can build the registry documents the Configuration state requires.

**Architecture:** A new crate depending only on `bytes` and `thiserror`, with no dependency on `pyrite-protocol` — NBT is a data format used by both the wire and, later, Anvil save files. An owned value tree (`NbtTag`), ordered compounds, and separate entry points for the network root (no name, since 1.20.2) and the file root (named). Every peer-declared length is checked against bytes actually remaining before allocating, and recursion is depth-capped.

**Tech Stack:** Rust 1.94 (edition 2024), `bytes`, `thiserror`.

**Spec:** `docs/superpowers/specs/2026-09-04-pyrite-milestone-3-design.md`

## Global Constraints

- **Clean-room, absolute.** Never consult or reproduce decompiled Mojang bytecode, private mappings, or proprietary assets. Only open reverse-engineered documentation of wire formats.
- **No Minecraft-domain dependencies.** `pyrite-nbt` depends on `bytes` and `thiserror` only. Adding `simdnbt`, `fastnbt`, `hematite-nbt` or equivalents is forbidden (spec D4/D14).
- **`pyrite-nbt` must NOT depend on `pyrite-protocol`.** The edge runs `protocol → nbt`, never the reverse (spec D14).
- **All multi-byte numbers are big-endian and signed** unless stated. String lengths are `u16` (unsigned); array and list lengths are `i32` (signed).
- **Tag IDs:** End 0, Byte 1, Short 2, Int 3, Long 4, Float 5, Double 6, ByteArray 7, String 8, List 9, Compound 10, IntArray 11, LongArray 12.
- **Network root omits the compound's name; file root includes it.** Two separate entry points, never a boolean parameter (spec D18).
- **NBT strings are regular UTF-8, not Java's modified UTF-8.** `String::from_utf8` is correct; invalid bytes are a typed error.
- **`MAX_DEPTH = 512`**, `MAX_PREALLOC_ELEMENTS = 64`.
- **Compounds preserve insertion order** — `Vec<(String, NbtTag)>`, never a `HashMap` (spec D17).
- **No `unwrap`/`expect`/`panic` on any path reachable from input.** Enforced by `clippy::unwrap_used`, `clippy::expect_used`, `clippy::panic` = deny. Test modules opt out with `#![allow(clippy::unwrap_used, clippy::expect_used)]` as their first inner line.
- **No `todo!()` / `unimplemented!()`.**
- **Every public item needs a doc comment.** `missing_docs = "warn"` plus CI's `-D warnings` makes an omission a build failure.
- Gate before every commit: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all --check`.
- Commit after every task. Conventional Commit prefixes.

**Baseline:** 112 tests pass at the start of this plan.

---

## Spec correction, decided at plan time

**Spec §4.3 says writing cannot fail and returns no `Result`. That is wrong, and this plan overrides it.**

An NBT string is length-prefixed with a `u16`, so it cannot encode more than 65 535 bytes. `NbtTag::String` holds an ordinary `String`, which can be longer. An infallible writer would have to either truncate — silently corrupting the document — or panic, which the constraints forbid.

Therefore `write_network_root` and `write_named_root` return `Result<(), NbtError>`, and `NbtError` gains a `StringTooLong { len: usize }` variant. The spec's underlying intent — that a well-formed value tree always encodes — still holds; the escape hatch exists for the one case where the type system cannot express the wire's limit.

The list-homogeneity invariant *is* enforced at construction as the spec describes, so it needs no writer check.

---

## File Structure

| File | Change | Responsibility |
|---|---|---|
| `Cargo.toml` | modify | Add `crates/nbt` to workspace members and `pyrite-nbt` to `[workspace.dependencies]` |
| `crates/nbt/Cargo.toml` | create | Crate manifest, `bytes` + `thiserror`, workspace lints |
| `crates/nbt/src/lib.rs` | create | Crate root, module wiring, re-exports |
| `crates/nbt/src/error.rs` | create | `NbtError` — every failure mode |
| `crates/nbt/src/tag.rs` | create | `TagId`, `NbtTag`, `NbtCompound`, `NbtList`, `From` conversions |
| `crates/nbt/src/write.rs` | create | Encoding, both root forms |
| `crates/nbt/src/read.rs` | create | Decoding, all three root forms, every bound |
| `crates/nbt/src/macros.rs` | create | `compound!` and `list!` |
| `crates/protocol/Cargo.toml` | modify | Depend on `pyrite-nbt` |
| `crates/protocol/src/error.rs` | modify | `ProtocolError::Nbt` |
| `crates/protocol/src/buf.rs` | modify | `read_nbt` / `write_nbt` |

**Task order matters:** the writer (Task 3) comes before the reader (Task 4) so the reader can be tested against byte vectors the writer produced *and* against hand-written vectors, rather than only against itself.

---

## Task 1: Crate scaffolding, `NbtError`, and `TagId`

**Files:**
- Modify: `Cargo.toml`
- Create: `crates/nbt/Cargo.toml`, `crates/nbt/src/lib.rs`, `crates/nbt/src/error.rs`, `crates/nbt/src/tag.rs`

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `pub enum TagId` with variants `End`, `Byte`, `Short`, `Int`, `Long`, `Float`, `Double`, `ByteArray`, `String`, `List`, `Compound`, `IntArray`, `LongArray`, `#[repr(u8)]` with explicit discriminants 0–12
  - `impl TryFrom<u8> for TagId { type Error = NbtError; }`
  - `pub enum NbtError` with variants `UnexpectedEof`, `UnknownTag(u8)`, `RootNotCompound(TagId)`, `DepthExceeded { max: usize }`, `NegativeLength(i32)`, `LengthExceedsInput { declared: usize, remaining: usize }`, `InvalidListElementType(TagId)`, `HeterogeneousList { expected: TagId, found: TagId }`, `StringTooLong { len: usize }`, `InvalidUtf8(std::str::Utf8Error)`
  - `pub const MAX_DEPTH: usize = 512`

- [ ] **Step 1: Add the crate to the workspace**

In the root `Cargo.toml`, add `"crates/nbt"` to `[workspace] members` (before `"crates/protocol"`), and add to `[workspace.dependencies]`:

```toml
pyrite-nbt = { path = "crates/nbt", version = "0.1.0" }
```

- [ ] **Step 2: Create the crate manifest**

`crates/nbt/Cargo.toml`:

```toml
[package]
name = "pyrite-nbt"
description = "Clean-room implementation of the NBT binary format."
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
authors.workspace = true

[lib]
name = "pyrite_nbt"

[dependencies]
bytes.workspace = true
thiserror.workspace = true

[lints]
workspace = true
```

- [ ] **Step 3: Create the crate root**

`crates/nbt/src/lib.rs`:

```rust
//! Clean-room implementation of NBT, the binary tree format Minecraft uses
//! for structured data.
//!
//! This crate deliberately knows nothing about the network protocol. NBT is
//! used both on the wire and in save files, which are unrelated domains, so
//! the dependency runs `pyrite-protocol -> pyrite-nbt` and never the reverse.
//!
//! Two root forms exist and they are not interchangeable. Since protocol 764
//! the network form omits the root compound's name; the file form keeps it.
//! Reading one as the other misaligns the whole document by the length of the
//! name, with no error at the point of the mistake, so the entry points are
//! separate functions rather than a flag.

pub mod error;
pub mod tag;

pub use error::{MAX_DEPTH, NbtError};
pub use tag::{NbtCompound, NbtList, NbtTag, TagId};
```

- [ ] **Step 4: Write the error type**

`crates/nbt/src/error.rs`:

```rust
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
```

- [ ] **Step 5: Write the failing `TagId` tests**

Create `crates/nbt/src/tag.rs` containing only:

```rust
//! The NBT value tree.

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
```

- [ ] **Step 6: Run the tests to verify they fail**

Add `pub mod tag;` and `pub mod error;` to `crates/nbt/src/lib.rs` if not already present, then run:

Run: `cargo test -p pyrite-nbt`
Expected: compile failure — `TagId` is not defined.

- [ ] **Step 7: Write `TagId`**

Insert into `crates/nbt/src/tag.rs`, above the test module:

```rust
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
```

- [ ] **Step 8: Run the tests**

Run: `cargo test -p pyrite-nbt`
Expected: 2 tests PASS.

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 9: Commit**

```bash
git add Cargo.toml Cargo.lock crates/nbt
git commit -m "feat(nbt): add crate scaffolding, error type, and tag identifiers"
```

---

## Task 2: The value tree

**Files:**
- Modify: `crates/nbt/src/tag.rs`
- Modify: `crates/nbt/src/lib.rs`

**Interfaces:**
- Consumes: `TagId`, `NbtError` from Task 1.
- Produces:
  - `pub enum NbtTag` with variants `Byte(i8)`, `Short(i16)`, `Int(i32)`, `Long(i64)`, `Float(f32)`, `Double(f64)`, `ByteArray(Bytes)`, `String(String)`, `List(NbtList)`, `Compound(NbtCompound)`, `IntArray(Vec<i32>)`, `LongArray(Vec<i64>)`
  - `NbtTag::id(&self) -> TagId`
  - `pub struct NbtCompound` with `new()`, `insert(impl Into<String>, impl Into<NbtTag>)`, `get(&str) -> Option<&NbtTag>`, `len()`, `is_empty()`, `iter()`, `Default`
  - `pub struct NbtList` with `empty()`, `new(Vec<NbtTag>) -> Result<Self, NbtError>`, `from_parts(TagId, Vec<NbtTag>)` (crate-private), `element_type()`, `items()`, `len()`, `is_empty()`
  - `From` impls for `NbtTag`: `i8`, `i16`, `i32`, `i64`, `f32`, `f64`, `&str`, `String`, `Bytes`, `Vec<i32>`, `Vec<i64>`, `NbtCompound`, `NbtList`

- [ ] **Step 1: Write the failing tests**

Append to `mod tests` in `crates/nbt/src/tag.rs`:

```rust
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
        assert_eq!(
            NbtTag::from("x".to_owned()),
            NbtTag::String("x".to_owned())
        );
        assert_eq!(NbtTag::from(vec![1i32]), NbtTag::IntArray(vec![1]));
        assert_eq!(NbtTag::from(vec![1i64]), NbtTag::LongArray(vec![1]));
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p pyrite-nbt`
Expected: compile failure — `NbtTag`, `NbtCompound`, `NbtList` are not defined.

- [ ] **Step 3: Write the value types**

Insert into `crates/nbt/src/tag.rs`, above the test module. Add `use bytes::Bytes;` to the file's imports.

```rust
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
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p pyrite-nbt`
Expected: 10 tests PASS.

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add crates/nbt
git commit -m "feat(nbt): add the value tree with ordered compounds and typed lists"
```

---

## Task 3: The writer

**Files:**
- Create: `crates/nbt/src/write.rs`
- Modify: `crates/nbt/src/lib.rs`

**Interfaces:**
- Consumes: `NbtTag`, `NbtCompound`, `NbtList`, `TagId`, `NbtError`.
- Produces:
  - `pub fn write_network_root<B: BufMut>(dst: &mut B, value: &NbtCompound) -> Result<(), NbtError>`
  - `pub fn write_named_root<B: BufMut>(dst: &mut B, name: &str, value: &NbtCompound) -> Result<(), NbtError>`

- [ ] **Step 1: Write the failing tests**

Create `crates/nbt/src/write.rs` containing only:

```rust
//! Encoding NBT documents.

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
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
```

- [ ] **Step 2: Run the tests to verify they fail**

Add `pub mod write;` to `crates/nbt/src/lib.rs` and `pub use write::{write_named_root, write_network_root};`, then run:

Run: `cargo test -p pyrite-nbt --lib write`
Expected: compile failure — `write_network_root` and `write_named_root` are not defined.

- [ ] **Step 3: Write the implementation**

Insert into `crates/nbt/src/write.rs`, above the test module:

```rust
use bytes::BufMut;

use crate::error::NbtError;
use crate::tag::{NbtCompound, NbtTag, TagId};

/// Writes a document in the network form: a type byte, then the payload.
///
/// The root's name is omitted. This has been the network form since protocol
/// 764; use [`write_named_root`] for save files.
pub fn write_network_root<B: BufMut>(
    dst: &mut B,
    value: &NbtCompound,
) -> Result<(), NbtError> {
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

/// Writes a compound's entries followed by the terminating `End`.
fn write_compound_body<B: BufMut>(
    dst: &mut B,
    value: &NbtCompound,
) -> Result<(), NbtError> {
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
            dst.put_i32(value.len() as i32);
            dst.put_slice(value);
        }
        NbtTag::String(value) => write_nbt_string(dst, value)?,
        NbtTag::List(list) => {
            // The element type is written once, and elements follow as bare
            // payloads with no per-element tag.
            dst.put_u8(list.element_type() as u8);
            dst.put_i32(list.len() as i32);
            for item in list.items() {
                write_payload(dst, item)?;
            }
        }
        NbtTag::Compound(compound) => write_compound_body(dst, compound)?,
        NbtTag::IntArray(values) => {
            dst.put_i32(values.len() as i32);
            for value in values {
                dst.put_i32(*value);
            }
        }
        NbtTag::LongArray(values) => {
            dst.put_i32(values.len() as i32);
            for value in values {
                dst.put_i64(*value);
            }
        }
    }
    Ok(())
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p pyrite-nbt`
Expected: the 9 new writer tests plus the 10 tag tests PASS.

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add crates/nbt
git commit -m "feat(nbt): add the encoder with both root forms"
```

---

## Task 4: The reader

**Files:**
- Create: `crates/nbt/src/read.rs`
- Modify: `crates/nbt/src/lib.rs`

**Interfaces:**
- Consumes: everything from Tasks 1–3.
- Produces:
  - `pub fn read_network_root<B: Buf>(src: &mut B) -> Result<NbtCompound, NbtError>`
  - `pub fn read_named_root<B: Buf>(src: &mut B) -> Result<(String, NbtCompound), NbtError>`
  - `pub fn read_optional_network_root<B: Buf>(src: &mut B) -> Result<Option<NbtCompound>, NbtError>`

- [ ] **Step 1: Write the failing tests**

Create `crates/nbt/src/read.rs` containing only:

```rust
//! Decoding NBT documents.

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use crate::tag::{NbtCompound, NbtList, NbtTag};
    use crate::write::{write_named_root, write_network_root};
    use bytes::BytesMut;

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
    fn the_two_root_forms_are_not_interchangeable() {
        // Reading a named document as a network one misaligns everything from
        // the name onwards. It must fail rather than silently produce
        // nonsense.
        let mut buf = BytesMut::new();
        write_named_root(&mut buf, "root", &sample()).unwrap();
        let mut src = &buf[..];
        assert!(read_network_root(&mut src).is_err());
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
    fn an_array_longer_than_the_input_is_rejected_without_allocating() {
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
    fn a_list_longer_than_the_input_is_rejected_without_allocating() {
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
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Add `pub mod read;` to `crates/nbt/src/lib.rs` and `pub use read::{read_named_root, read_network_root, read_optional_network_root};`, then run:

Run: `cargo test -p pyrite-nbt --lib read`
Expected: compile failure — the three read functions are not defined.

Note: the test module uses `bytes::{Buf, BufMut, Bytes}` and `MAX_DEPTH`, `NbtError`, `TagId`; these arrive through `use super::*;` once the implementation imports them.

- [ ] **Step 3: Write the implementation**

Insert into `crates/nbt/src/read.rs`, above the test module:

```rust
use bytes::{Buf, BufMut, Bytes};

use crate::error::{MAX_DEPTH, NbtError};
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
    read_compound_body(src, 1)
}

/// Reads a document in the file form: a type byte, a name, then the payload.
pub fn read_named_root<B: Buf>(src: &mut B) -> Result<(String, NbtCompound), NbtError> {
    let id = read_tag_id(src)?;
    if id != TagId::Compound {
        return Err(NbtError::RootNotCompound(id));
    }
    let name = read_nbt_string(src)?;
    let compound = read_compound_body(src, 1)?;
    Ok((name, compound))
}

/// Reads a network-form document that may be absent.
///
/// Some packets encode "no document here" as a lone `End` tag rather than as
/// an empty compound, so a bare `0x00` yields `Ok(None)`.
pub fn read_optional_network_root<B: Buf>(
    src: &mut B,
) -> Result<Option<NbtCompound>, NbtError> {
    let id = read_tag_id(src)?;
    match id {
        TagId::End => Ok(None),
        TagId::Compound => Ok(Some(read_compound_body(src, 1)?)),
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

    let required = len
        .checked_mul(min_element_size)
        .ok_or(NbtError::LengthExceedsInput {
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
fn read_compound_body<B: Buf>(src: &mut B, depth: usize) -> Result<NbtCompound, NbtError> {
    if depth > MAX_DEPTH {
        return Err(NbtError::DepthExceeded { max: MAX_DEPTH });
    }

    let mut compound = NbtCompound::new();
    loop {
        let id = read_tag_id(src)?;
        if id == TagId::End {
            return Ok(compound);
        }
        let name = read_nbt_string(src)?;
        let value = read_payload(src, id, depth + 1)?;
        compound.insert(name, value);
    }
}

/// Reads one tag's payload, given its already-decoded type.
fn read_payload<B: Buf>(src: &mut B, id: TagId, depth: usize) -> Result<NbtTag, NbtError> {
    if depth > MAX_DEPTH {
        return Err(NbtError::DepthExceeded { max: MAX_DEPTH });
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
                items.push(read_payload(src, element_type, depth + 1)?);
            }
            NbtTag::List(NbtList::from_parts(element_type, items))
        }
        TagId::Compound => NbtTag::Compound(read_compound_body(src, depth + 1)?),
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
```

Note on `Bytes` and `BufMut`: the implementation imports them because
`copy_to_bytes` needs the former and the test module's hand-built documents
need the latter. If clippy reports either as unused in the implementation,
move that import into the test module rather than deleting it.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p pyrite-nbt`
Expected: the 16 reader tests plus all earlier tests PASS.

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 5: Verify the depth guard actually guards**

Temporarily raise `MAX_DEPTH` in `crates/nbt/src/error.rs` to `10_000_000` and re-run only the depth test:

Run: `cargo test -p pyrite-nbt --lib nesting_beyond_the_depth_limit`
Expected: the test crashes or aborts with a stack overflow rather than failing an assertion — which is the behaviour the limit exists to prevent.

Restore `MAX_DEPTH = 512` and confirm the test passes again. Record what you observed in the report; this is the evidence that the guard is load-bearing rather than decorative.

- [ ] **Step 6: Commit**

```bash
git add crates/nbt
git commit -m "feat(nbt): add the decoder with depth and length bounds"
```

---

## Task 5: Construction macros

**Files:**
- Create: `crates/nbt/src/macros.rs`
- Modify: `crates/nbt/src/lib.rs`

**Interfaces:**
- Consumes: `NbtCompound`, `NbtList`, `NbtTag`, `NbtError`.
- Produces: `compound!` and `list!`, both `#[macro_export]`.

- [ ] **Step 1: Write the failing tests**

Create `crates/nbt/src/macros.rs` containing only:

```rust
//! Construction helpers for building documents by hand.

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use crate::tag::{NbtCompound, NbtList, NbtTag};
    use crate::{compound, list};

    #[test]
    fn compound_builds_an_equivalent_value() {
        let built = compound! {
            "name" => "minecraft:overworld",
            "id" => 0i32,
        };

        let mut expected = NbtCompound::new();
        expected.insert("name", "minecraft:overworld");
        expected.insert("id", 0i32);

        assert_eq!(built, expected);
    }

    #[test]
    fn compound_nests() {
        let built = compound! {
            "element" => compound! {
                "has_skylight" => 1i8,
                "height" => 384i32,
            },
        };

        let element = built.get("element").unwrap();
        let NbtTag::Compound(inner) = element else {
            panic!("expected a compound");
        };
        assert_eq!(inner.get("height"), Some(&NbtTag::Int(384)));
    }

    #[test]
    fn an_empty_compound_is_valid() {
        assert_eq!(compound! {}, NbtCompound::new());
    }

    #[test]
    fn compound_accepts_a_trailing_comma() {
        let built = compound! {
            "a" => 1i32,
        };
        assert_eq!(built.len(), 1);
    }

    #[test]
    fn list_builds_a_homogeneous_list() {
        let built = list![1i32, 2i32, 3i32].unwrap();
        assert_eq!(built.len(), 3);
        assert_eq!(built.items()[0], NbtTag::Int(1));
    }

    #[test]
    fn an_empty_list_macro_is_valid() {
        let built: NbtList = list![].unwrap();
        assert!(built.is_empty());
    }

    #[test]
    fn list_rejects_mixed_types() {
        assert!(list![1i32, "two"].is_err());
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Add `pub mod macros;` to `crates/nbt/src/lib.rs`, then run:

Run: `cargo test -p pyrite-nbt --lib macros`
Expected: compile failure — `compound!` and `list!` are not defined.

- [ ] **Step 3: Write the macros**

Insert into `crates/nbt/src/macros.rs`, above the test module:

```rust
/// Builds an [`NbtCompound`](crate::NbtCompound) from name/value pairs.
///
/// Values are converted with `Into<NbtTag>`, so integer literals need their
/// suffix — `0i32` is an `Int`, `0i8` a `Byte` — and getting that wrong is a
/// type error rather than a silently different document.
///
/// ```
/// use pyrite_nbt::compound;
///
/// let dimension = compound! {
///     "name" => "minecraft:overworld",
///     "id" => 0i32,
/// };
/// assert_eq!(dimension.len(), 2);
/// ```
#[macro_export]
macro_rules! compound {
    ($($name:expr => $value:expr),* $(,)?) => {{
        #[allow(unused_mut)]
        let mut compound = $crate::NbtCompound::new();
        $(
            compound.insert($name, $value);
        )*
        compound
    }};
}

/// Builds an [`NbtList`](crate::NbtList) from elements that must share a type.
///
/// Returns `Result` because homogeneity is checked at construction, so a list
/// that exists is always encodable.
///
/// ```
/// use pyrite_nbt::list;
///
/// let ids = list![1i32, 2i32].unwrap();
/// assert_eq!(ids.len(), 2);
/// ```
#[macro_export]
macro_rules! list {
    () => {
        $crate::NbtList::new(::std::vec::Vec::new())
    };
    ($($value:expr),+ $(,)?) => {
        $crate::NbtList::new(::std::vec![
            $($crate::NbtTag::from($value)),+
        ])
    };
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p pyrite-nbt`
Expected: the 7 macro tests plus all earlier tests PASS. The doc examples run too, so `cargo test` must be run without `--all-targets` at least once to exercise them.

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add crates/nbt
git commit -m "feat(nbt): add compound! and list! construction macros"
```

---

## Task 6: Protocol integration

**Files:**
- Modify: `crates/protocol/Cargo.toml`
- Modify: `crates/protocol/src/error.rs`
- Modify: `crates/protocol/src/buf.rs`

**Interfaces:**
- Consumes: `read_network_root`, `write_network_root`, `NbtCompound`, `NbtError`.
- Produces:
  - `ProtocolError::Nbt(NbtError)`
  - `pub fn write_nbt<B: BufMut>(dst: &mut B, value: &NbtCompound) -> Result<(), ProtocolError>`
  - `pub fn read_nbt<B: Buf>(src: &mut B) -> Result<NbtCompound, ProtocolError>`

- [ ] **Step 1: Add the dependency**

In `crates/protocol/Cargo.toml`, add to `[dependencies]`:

```toml
pyrite-nbt.workspace = true
```

- [ ] **Step 2: Add the error variant**

Insert into `ProtocolError` in `crates/protocol/src/error.rs`, after the `Decompression` variant:

```rust
    /// An NBT document was malformed.
    ///
    /// Its own variant rather than folded into [`ProtocolError::Io`]: a
    /// malformed document is a protocol violation by the peer and must be
    /// logged as one. Filing it under a transport variant would classify it
    /// as routine noise and hide it at the default log level.
    #[error("nbt error: {0}")]
    Nbt(#[from] pyrite_nbt::NbtError),
```

- [ ] **Step 3: Write the failing tests**

Append to the existing `mod tests` in `crates/protocol/src/buf.rs`:

```rust
    #[test]
    fn nbt_round_trips_through_the_buf_helpers() {
        let mut document = pyrite_nbt::NbtCompound::new();
        document.insert("id", 7i32);
        document.insert("name", "minecraft:overworld");

        let mut buf = BytesMut::new();
        write_nbt(&mut buf, &document).unwrap();

        let mut src = &buf[..];
        assert_eq!(read_nbt(&mut src).unwrap(), document);
        assert!(src.is_empty());
    }

    #[test]
    fn nbt_uses_the_network_root_form() {
        // No name between the compound tag and its body.
        let mut buf = BytesMut::new();
        write_nbt(&mut buf, &pyrite_nbt::NbtCompound::new()).unwrap();
        assert_eq!(&buf[..], &[0x0a, 0x00]);
    }

    #[test]
    fn a_malformed_document_surfaces_as_a_protocol_error() {
        let buf: &[u8] = &[0x03, 0x00, 0x00, 0x00, 0x01];
        let mut src = buf;
        assert!(matches!(read_nbt(&mut src), Err(ProtocolError::Nbt(_))));
    }
```

- [ ] **Step 4: Run the tests to verify they fail**

Run: `cargo test -p pyrite-protocol --lib buf`
Expected: compile failure — `read_nbt` and `write_nbt` are not defined.

- [ ] **Step 5: Write the helpers**

Append to `crates/protocol/src/buf.rs`, above the test module:

```rust
/// Writes an NBT document in the network root form.
///
/// Packets carry NBT in the network form, which omits the root compound's
/// name. The file form belongs to save data and is not reachable from here.
pub fn write_nbt<B: BufMut>(
    dst: &mut B,
    value: &pyrite_nbt::NbtCompound,
) -> Result<(), ProtocolError> {
    pyrite_nbt::write_network_root(dst, value)?;
    Ok(())
}

/// Reads an NBT document in the network root form.
pub fn read_nbt<B: Buf>(src: &mut B) -> Result<pyrite_nbt::NbtCompound, ProtocolError> {
    Ok(pyrite_nbt::read_network_root(src)?)
}
```

- [ ] **Step 6: Run the full gate**

Run: `cargo test --workspace`
Expected: all tests PASS — 112 baseline plus the new NBT and integration tests.

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: clean.

Run: `cargo fmt --all --check`
Expected: clean.

- [ ] **Step 7: Confirm the dependency edge runs one way**

Run: `cargo tree -p pyrite-nbt -e normal --depth 1`
Expected: `bytes` and `thiserror` only. `pyrite-protocol` must NOT appear — the edge is `protocol → nbt`, never the reverse (spec D14).

- [ ] **Step 8: Commit**

```bash
git add Cargo.toml Cargo.lock crates/protocol
git commit -m "feat(protocol): wire nbt into the packet buffer helpers"
```

---

## Self-Review

**Spec coverage.** §4 crate layout → Tasks 1–5 (`macros.rs` in Task 5, the rest across 1–4). §4.1 value types → Task 2. §4.2 readers including `read_optional_network_root` → Task 4. §4.3 writers → Task 3. §4.4 macros → Task 5. §5 bounds → Task 4, one test per guard: depth (`nesting_beyond_the_depth_limit_is_rejected`), declared-vs-remaining (`an_array_longer_than_the_input_is_rejected_without_allocating`, `a_list_longer_than_the_input_is_rejected_without_allocating`), negative length (`a_negative_array_length_is_rejected`), unknown tag (`an_unknown_tag_id_is_rejected`), malformed list (`a_non_empty_list_of_end_is_rejected`). §6 errors → Task 1, extended in the correction below. §7 protocol integration → Task 6. §8 testing → tests throughout; the byte-exact vectors §8 demands are in Task 3, written from the documentation rather than from Pyrite's own decoder.

**Deviations from the spec, both deliberate:**
1. **Writing returns `Result`.** Spec §4.3 says it cannot fail. It can: a `String` longer than 65 535 bytes has no `u16` length prefix to describe it, and the alternatives are silent truncation or a forbidden panic. Documented at the top of this plan; adds `NbtError::StringTooLong`.
2. **`NbtError::HeterogeneousList` is not in the spec's §6 list.** It is required by §4.1's statement that a heterogeneous list must be unrepresentable rather than caught at encode time — enforcing that at construction needs an error to return.

**Type consistency.** `TagId` is defined in Task 1 and used by Tasks 2–4 and by `NbtError::RootNotCompound`/`InvalidListElementType`. `NbtList::from_parts` is declared `pub(crate)` in Task 2 and called only from Task 4's decoder, which is inside the crate. `NbtCompound::insert(impl Into<String>, impl Into<NbtTag>)` in Task 2 matches every call site in Tasks 3–6 and both macros. `MAX_DEPTH` lives in `error.rs` (Task 1) and is consumed by `read.rs` (Task 4) and by Task 4's depth test. `write_network_root`/`read_network_root` signatures match between Tasks 3, 4 and 6.

**One ordering constraint.** Task 3 (writer) precedes Task 4 (reader) deliberately: the reader's round-trip tests need an encoder, and the writer's byte-exact vectors are written from the format documentation, so the two are not merely checking each other.

**A test that would hang rather than fail.** Task 4 Step 5 deliberately raises `MAX_DEPTH` to confirm the guard is load-bearing, and the expected outcome is a stack overflow. Run it last, expect a crash, and restore the constant immediately — it is a one-off verification, not something to leave in the suite.
