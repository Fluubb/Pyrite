# Pyrite — Milestone 3 Design: NBT

**Date:** 2026-09-04
**Status:** Approved
**Scope:** Second slice of Master Plan Phase 1's remainder, after Milestone 2.

---

## 1. Objective

Implement NBT — the binary tree format Minecraft uses for structured data — as
a standalone crate, hand-written per D4.

NBT is a prerequisite, not a feature. Nothing about the running server changes
in this milestone. Milestone 4 cannot begin without it: the registry data the
client requires during Configuration is a large NBT document, and Play-state
packets carry NBT throughout.

Non-goals: serde integration (see D16), Anvil region files, compression of NBT
documents, and any protocol packet that carries NBT — those arrive in M4.

### 1.1 Success criteria

1. `cargo build --workspace` succeeds with no warnings.
2. `cargo clippy --workspace --all-targets -- -D warnings` is clean.
3. `cargo test --workspace` passes on Linux, macOS, and Windows.
4. Every one of the 13 tag types round-trips: encode then decode yields an
   equal value, and decode then encode yields identical bytes.
5. A document encoded by Pyrite matches the documented byte layout exactly,
   asserted against hand-written byte vectors rather than only against
   Pyrite's own decoder.
6. A deeply nested document is rejected with a typed error rather than
   overflowing the stack.
7. A tag declaring an array or list longer than the remaining input is
   rejected before any allocation proportional to the declared length.

**What this milestone does not produce:** any visible change. The server
behaves exactly as it did at the end of M2. Success is established entirely by
tests.

---

## 2. Decision log

Continues D1–D13. Those remain in force.

| # | Decision | Rationale |
|---|---|---|
| D14 | **NBT lives in its own crate, `crates/nbt` (`pyrite-nbt`)**, depending only on `bytes` and `thiserror`. It has no dependency on `pyrite-protocol`. | NBT is a standalone data format used by two unrelated domains: the network protocol and, at Phase 2, Anvil save files. A module inside `pyrite-protocol` would force the future world-storage crate to depend on the entire network protocol to read a file off disk. Getting that edge right is cheapest now, before anything depends on it. This is not a D7 placeholder — the crate ships complete content immediately. |
| D15 | **Values are an owned tree** (`NbtTag` with owned `String`/`Vec`), not a borrowed view or a streaming visitor. | The server predominantly *builds* NBT (registry data now, chunk data later) rather than parsing it, and the NBT it does parse is small. A borrowed `NbtRef<'a>` would leak lifetimes into every packet struct carrying NBT and is awkward for construction — optimising the direction of travel we use least. A streaming API turns every consumer into a state machine for a payload that comfortably fits in memory. A borrowed or streaming path can be added later behind the same public type if profiling ever demands it. |
| D16 | **No serde integration.** Documents are hand-built with a `compound!` macro. | A correct serde data-format implementation is roughly the size of this entire milestone, and serde's model does not map cleanly onto NBT's typed arrays and homogeneous lists — which is precisely where its bugs would live. M4 builds a handful of documents. Revisit at Phase 2 alongside Anvil, where it would pay for itself twice. Follows D6's precedent of deferring machinery until the count justifies it. |
| D17 | **Compounds preserve insertion order** (`Vec<(String, NbtTag)>`, not `HashMap`). | Compound order is not semantically meaningful, but ordered output makes byte-exact round-trip assertions possible and makes generated documents deterministic — which the asset pipeline needs at Phase 6, where content is hashed. Compounds hold tens of keys, so linear lookup is not a performance concern. |
| D18 | **The network and file root forms are separate entry points** over a shared payload codec, not a mode flag on one function. | Since 1.20.2 the network root omits its name while the file root keeps it (§3). Two named functions make each call site state which format it means; a boolean parameter would read as `read_root(src, true)` at the call site and be silently wrong half the time. |

---

## 3. Verified format

Confirmed 2026-09-04 against public reverse-engineering references:
[NBT format](https://minecraft.wiki/w/NBT_format),
[Java Edition protocol / Data types](https://minecraft.wiki/w/Java_Edition_protocol/Data_types),
[wiki.vg NBT](https://wiki.vg/Nbt),
[Minecraft Wiki wiki.vg merge / NBT](https://minecraft.wiki/w/Minecraft_Wiki:Projects/wiki.vg_merge/NBT).
No proprietary source, decompiled bytecode, or private mappings were consulted.

### 3.1 Tag types

All multi-byte numbers are big-endian and signed unless stated.

| ID | Tag | Payload |
|---|---|---|
| 0 | End | none — terminates a compound |
| 1 | Byte | `i8` |
| 2 | Short | `i16` |
| 3 | Int | `i32` |
| 4 | Long | `i64` |
| 5 | Float | `f32`, IEEE 754 binary32 |
| 6 | Double | `f64`, IEEE 754 binary64 |
| 7 | ByteArray | `i32` length, then that many bytes |
| 8 | String | `u16` length in bytes, then UTF-8 |
| 9 | List | `u8` element type, `i32` length, then that many bare payloads |
| 10 | Compound | repeating `u8` type + `u16`-prefixed name + payload, terminated by an End tag |
| 11 | IntArray | `i32` length, then that many `i32` |
| 12 | LongArray | `i32` length, then that many `i64` |

### 3.2 The root, and why it has two forms

**Since 1.20.2 (protocol 764), NBT sent over the network omits the root
compound's name.** A network document is a type byte followed directly by the
payload. File NBT — save data, and Anvil at Phase 2 — retains the name, so a
file document is a type byte, a `u16`-prefixed name, then the payload.

This is the single detail most likely to break everything silently: get it
wrong and every document is misaligned by the two-byte name length, with no
error at the point of the mistake.

### 3.3 String encoding

**Strings are regular UTF-8, not Java's modified UTF-8.** The protocol
documentation states this explicitly. This is worth recording because NBT's
origins in Java's `DataOutput.writeUTF` make the opposite assumption natural,
and the difference is invisible until a document contains a NUL byte or a
character outside the basic multilingual plane.

`String::from_utf8` is therefore correct, and invalid bytes are a typed error
rather than something to replace or coerce.

### 3.4 List element typing

A list declares one element type for all its members, and elements carry no
individual type tags. An empty list conventionally declares element type `End`
(0). A non-empty list declaring `End` is malformed. A list whose declared type
is above 12 is malformed.

---

## 4. `crates/nbt`

```
crates/nbt/
├── Cargo.toml
└── src/
    ├── lib.rs        # crate root, re-exports
    ├── tag.rs        # NbtTag, NbtCompound, TagId
    ├── read.rs       # decoding, and every bound described in §5
    ├── write.rs      # encoding
    ├── macros.rs     # compound! and list! construction helpers
    └── error.rs      # NbtError
```

### 4.1 `tag.rs`

```rust
pub enum NbtTag {
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    ByteArray(Bytes),
    String(String),
    List(NbtList),
    Compound(NbtCompound),
    IntArray(Vec<i32>),
    LongArray(Vec<i64>),
}
```

`End` is deliberately not a variant of `NbtTag`. It is a structural marker in
the encoding, not a value a document can hold, and giving it a variant would
make every `match` carry an arm that cannot occur. `TagId` is a separate enum
covering all 13 wire identifiers including `End`.

`ByteArray` holds `Bytes` so bulk payloads are refcounted slices rather than
copies. `IntArray` and `LongArray` hold `Vec` because their elements need
byte-swapping out of the big-endian wire form and cannot be borrowed directly.

`NbtCompound` wraps `Vec<(String, NbtTag)>` and exposes `get(&str)`,
`insert`, `len`, `is_empty`, and iteration (D17). It also has a crate-private
`push`, which appends without checking for an existing key; the decoder is
its only caller (§5), and it is not `pub` because a document holding
duplicate names lets `get` and iteration disagree about the same bytes,
which is exactly the invariant `insert` exists to enforce.

`NbtList` carries its element `TagId` alongside `Vec<NbtTag>`, so a list's
declared type survives a round trip even when empty. A list built through the
public API validates that every element matches the declared type; a
heterogeneous list is unrepresentable rather than caught at encode time.

### 4.2 `read.rs`

```rust
pub fn read_network_root<B: Buf>(src: &mut B) -> Result<NbtCompound, NbtError>;
pub fn read_named_root<B: Buf>(src: &mut B) -> Result<(String, NbtCompound), NbtError>;
```

Both delegate to a private `read_payload(src, id, depth)`. A root that is not
a compound is `NbtError::RootNotCompound`: both forms are documented as
beginning with a compound.

**The absent-document case.** Some packets encode an optional NBT field as a
bare `0x00` — a lone End tag meaning "nothing here" — rather than as a present
but empty compound. `read_network_root` treats that as `RootNotCompound`,
which is correct for a field that is required to be a document. A third entry
point covers the optional case:

```rust
pub fn read_optional_network_root<B: Buf>(
    src: &mut B,
) -> Result<Option<NbtCompound>, NbtError>;
```

which returns `Ok(None)` on a leading End and otherwise defers to
`read_network_root`. This is specified here rather than discovered in M4,
where the symptom would be a packet that fails to decode for no visible
reason.

### 4.3 `write.rs`

```rust
pub fn write_network_root<B: BufMut>(dst: &mut B, value: &NbtCompound);
pub fn write_named_root<B: BufMut>(dst: &mut B, name: &str, value: &NbtCompound);
```

**Amended after implementation.** This section originally said writing cannot
fail and returns no `Result`. That premise was false: an NBT string is
length-prefixed with a `u16`, so a `String` over 65,535 bytes is constructible
but not encodable, and an array above `i32::MAX` elements likewise. Truncating
would silently corrupt the document and panicking is forbidden on any path
reachable from input, so both writers return `Result<(), NbtError>`. The
list-homogeneity invariant *is* enforced at construction as originally
described, so it needs no writer check.

**Amended again after an independent review.** The writer had no depth guard
of its own, so a hand-built tree deeper than `MAX_DEPTH` — the same limit the
reader enforces — would overflow the stack on write rather than fail cleanly,
fatal under `panic = "abort"`. The writer now threads a depth counter through
`write_compound_body` and `write_payload` exactly as the reader does, and
returns `NbtError::DepthExceeded` past the limit; both public writers already
returned `Result`, so this needed no signature change. Milestone 4's registry
generation is exactly the code that would trip this.

### 4.4 `macros.rs`

```rust
let dimension = compound! {
    "name" => "minecraft:overworld",
    "id" => 0i32,
    "element" => compound! {
        "has_skylight" => 1i8,
        "height" => 384i32,
    },
};
```

`compound!` and `list!` exist so M4's registry documents are readable at the
call site. Every field stays visible where it is written — which matters
because a wrong field name in registry data produces a client that fails to
join with no server-side error.

---

## 5. Bounds

This is the third time this project has met the same bug class. The Milestone 1
final review found a peer-declared frame length driving a 699,000× memory
amplification; the Milestone 2 spec caught the same shape in decompression
before it shipped. NBT presents it again in two new forms, so the bounds are
specified here rather than left to the implementer.

**Depth limit — `MAX_DEPTH = 512`.** Compounds and lists decode by recursive
descent. A document consisting of 100,000 nested list openings is a few hundred
kilobytes on the wire and overflows the stack. With `panic = "abort"` in the
release profile that terminates the entire server, not one connection. Depth
is threaded through `read_payload` and exceeding it is
`NbtError::DepthExceeded`.

**Declared lengths are checked against bytes actually remaining, before any
allocation proportional to them.** A `ByteArray`, `IntArray`, `LongArray` or
`List` header can declare two billion elements inside a twenty-byte input. The
containing frame is already bounded by `MAX_PACKET_SIZE`, but that bounds the
frame, not what the frame claims about itself. Each array reader checks
`declared * element_size <= src.remaining()` before reserving, using checked
multiplication so the product itself cannot overflow. The initial reservation
is additionally capped at `MAX_PREALLOC_ELEMENTS` (64, matching the existing
convention in `protocol/src/buf.rs`), so the vector grows with real data
rather than with the claim.

**Node budget — `MAX_TOTAL_NODES = 65_536`.** Added after the final review,
then re-sized after an independent review of that fix. Depth bounds a
document's *nesting*; nothing bounded its *size*. The budget must be sized
against the memory-densest shape a document can take, not the cheapest one:
a list of empty compounds costs one wire byte per node, but a compound
*entry* — a name and a value, which is what a flat document is actually made
of — costs as little as 4 wire bytes on the wire while occupying
`size_of::<(String, NbtTag)>()` = 64 bytes once decoded (`size_of::<NbtTag>()`
alone is 40). A budget sized against the list shape binds two nodes *after*
`MAX_PACKET_SIZE` does on the entry shape, so it can never fire on the input
that costs the most memory. At `1 << 16` the entry shape caps a decoded
document at roughly 4 MiB of tree, still some twentyfold above the few
thousand tags a real registry document contains. The budget is threaded
through decoding exactly as `depth` is.

**Compounds decode by appending, not by inserting, and a duplicate name is
rejected once the compound is complete.** Added after the final review, then
corrected after an independent review of that fix. `NbtCompound::insert`
scans existing entries to replace duplicates in place, which is right for
hand-built documents and catastrophic in a decoder: calling it per entry made
decoding an N-entry compound cost O(N²) string comparisons, measured at
roughly ninety seconds of blocked CPU for one maximum-size document. The
decoder uses an append-only path (`NbtCompound::push`, crate-private) to keep
that cost linear.

Appending alone, though, lets a document with duplicate names decode
successfully into a compound whose own API then disagrees with itself:
`get` returns the first match, iteration (or collecting into a map) yields
the last. Two consumers of the same bytes — say, a handler that validates by
iterating and one that reads a field with `get` — would then see different
values for the same key from the same document, which is the shape of a
request-smuggling bug once Milestone 4 makes this reachable from an
unauthenticated connection. NBT forbids duplicate names, so keeping either
reading is wrong; the decoder instead scans the finished compound for
duplicates once, after every entry has been read (an O(N log N)
sort-and-compare, not a per-entry scan), and rejects the document with
`NbtError::DuplicateKey` if it finds one.

These two guards are a different shape from the rest of this section, and the
difference is worth naming. Everything above bounds *allocation proportional
to a declared length*. These bound *work and memory proportional to an
undeclared, unbounded count* — which is why neither was anticipated here, and
why the quadratic one was invisible to every reviewer who read the code rather
than measuring it.

**Negative lengths are rejected.** Array and list lengths are signed 32-bit, so
a negative value is representable on the wire and must be a typed error rather
than a cast into a huge `usize`.

**Unknown tag identifiers are rejected.** Any type byte above 12 is
`NbtError::UnknownTag`, not a skipped field.

**Malformed lists are rejected.** A non-empty list declaring element type `End`
is an error, as is any list whose declared type exceeds 12.

---

## 6. Errors

```rust
pub enum NbtError {
    UnexpectedEof,
    UnknownTag(u8),
    RootNotCompound(TagId),
    DepthExceeded { max: usize },
    NegativeLength(i32),
    LengthExceedsInput { declared: usize, remaining: usize },
    InvalidListElementType(TagId),
    HeterogeneousList { expected: TagId, found: TagId },
    StringTooLong { len: usize },
    ArrayTooLong { len: usize },
    TooManyNodes { max: usize },
    DuplicateKey { name: String },
    InvalidUtf8(std::str::Utf8Error),
}
```

`HeterogeneousList` and `StringTooLong` arrived with the writer's `Result`
return (§4.3); `ArrayTooLong` and `TooManyNodes` with the bounds added after
the final review (§5); `DuplicateKey` with the correction to that fix after
an independent review (§5).

`NbtError` is the crate's own type. `pyrite-protocol` gains
`ProtocolError::Nbt(#[from] NbtError)`, so a malformed document reaching the
networking layer is classified as a protocol violation and logged at `warn` —
not as transport noise. That distinction is deliberate: the Milestone 1 review
found a corrupt compressed payload being filed as a hung-up socket precisely
because an error type laundered through a generic variant.

---

## 7. Integration with `crates/protocol`

`buf.rs` gains `read_nbt` and `write_nbt` so packets consume NBT exactly as
they consume strings and UUIDs. `pyrite-protocol` takes a dependency on
`pyrite-nbt`; the reverse edge does not exist.

No packet uses NBT in this milestone. The helpers exist so that M4's
Configuration and Play packets do not each invent their own call.

---

## 8. Testing

**Round trips.** Every tag type, in both directions: value → bytes → value
must be equal, and bytes → value → bytes must be identical. The second
direction is what catches an encoder and decoder that are wrong in the same
way and therefore agree with each other.

**Byte-exact vectors.** Hand-written expected bytes for a small document,
asserted independently of Pyrite's own decoder. A round trip alone cannot
detect a consistently wrong layout; only a vector written from the
documentation can.

**Root forms.** A network root and a named root of the same document differ by
exactly the encoded name, and each rejects the other's layout.

**Bounds, one test per guard in §5.** A depth bomb asserting `DepthExceeded`
rather than a crash; a length bomb asserting rejection without allocation; a
negative length; an unknown tag id; a non-empty list declaring `End`; invalid
UTF-8; and input truncated at each field boundary.

**Empty edge cases.** An empty compound, an empty list, a zero-length array —
each round-trips and each produces the documented bytes.

---

## 9. Known limitations

**No serde support.** Documents are hand-built (D16). This is a real
ergonomics cost at M4, accepted deliberately and scheduled for reconsideration
at Phase 2.

**No NBT-level compression.** Anvil stores NBT gzip- or zlib-compressed. That
belongs with the region-file reader at Phase 2, not here; network NBT is
compressed by the packet codec, which already exists.

**`MAX_DEPTH = 512` is a policy, not a wire constraint.** No documented limit
exists. 512 is far beyond any legitimate document and far below anything that
threatens the stack. It is a constant in one place, adjustable if a real
document ever approaches it.

**No `long_array` element-count sanity beyond the input bound.** A document
may legitimately contain a large `LongArray` — chunk block states are exactly
that — so the only bound is the containing frame, which the packet codec
already caps.
