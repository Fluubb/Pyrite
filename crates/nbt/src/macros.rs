//! Construction helpers for building documents by hand.

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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use crate::tag::{NbtCompound, NbtList, NbtTag};

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
