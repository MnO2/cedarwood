# Binary Serialization Format

Cedarwood 0.6 can persist a mutable trie with `Cedar::save_to_writer` and restore it with
`Cedar::load_from_reader`. File helpers are available as `save_to_path` and `load_from_path`.
Persistence failures use `CedarPersistenceError`, separately from `CedarError` because I/O errors
are neither cloneable nor comparable.

```rust
use cedarwood::Cedar;

let mut cedar = Cedar::builder().ordered(false).max_trial(3)?.build();
cedar.update("alpha", 1)?;

let mut bytes = Vec::new();
cedar.save_to_writer(&mut bytes)?;
let mut loaded = Cedar::load_from_reader(bytes.as_slice())?;

assert_eq!(loaded.exact_match_search("alpha"), Some((1, 5)));
loaded.update("beta", 2)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Version 1.0 encoding

All integers are encoded explicitly in little-endian byte order. Rust struct bytes, enum
discriminants, padding, pointer widths, and compiler layout are never serialized. Private versioned
wire records are decoded separately and converted to live trie structs only after bounded,
fallible allocation. The fixed header is 88 bytes:

| Offset | Width | Field |
|---:|---:|---|
| 0 | 8 | Magic `CEDARW\0\0` |
| 8 | 2 | Format major version (`1`) |
| 10 | 2 | Format minor version (`0`) |
| 12 | 1 | Byte-order marker (`1` means little-endian) |
| 13 | 1 | Layout (`0` default, `1` reduced-trie) |
| 14 | 2 | Flags (bit 0 is ordered sibling chains; all other bits are reserved) |
| 16 | 4 | Signed `max_trial` configuration |
| 20 | 4 | Full-block list head |
| 24 | 4 | Closed-block list head |
| 28 | 4 | Open-block list head |
| 32 | 8 | Node count |
| 40 | 8 | `NInfo` count |
| 48 | 8 | Block count |
| 56 | 8 | Reject-table count |
| 64 | 8 | Internal node capacity |
| 72 | 8 | Active node count |
| 80 | 8 | Logical entry count |

The body immediately follows the header, in this order:

1. Nodes: signed `base_` and `check` (`4 + 4` bytes each).
2. Node information: `sibling` and `child` (`1 + 1` bytes each).
3. Blocks: signed `prev`, `next`, `num`, `reject`, `trial`, and `e_head`
   (`4 + 4 + 2 + 2 + 4 + 4` bytes each).
4. Reject-table values (`2` bytes each).

No trailing bytes are permitted. The format never stores or honors spare vector capacity: each
vector is fallibly reserved from its validated logical length. Entry count, used slots, active node
capacity, and load factor round-trip. `allocated_bytes` describes the receiving allocator's actual
reservations and can differ after loading because allocator growth history is intentionally not
trusted or persisted.

## Validation and resource limits

Loading is an untrusted-data boundary. Before allocating, the reader checks integer conversions,
vector length relationships, active block alignment, the fixed reject-table size, and a peak-memory
ceiling covering wire DTOs, live conversion, and worst-case validation scratch space. Every
attacker-sized reservation uses `try_reserve_exact`; allocation failure is returned as
`CedarPersistenceError::AllocationFailed` rather than panicking.
`load_from_reader` uses `DEFAULT_LOAD_MEMORY_LIMIT` (512 MiB);
`load_from_reader_with_limit` accepts an application-specific ceiling.

After decoding, cedarwood validates the root and layout sentinels, active and inactive storage,
per-block free counts, every cyclic free-list link, all three block-category lists, heuristic
ranges, parent ownership links, sibling ordering/cycles, terminal values, reachability, and the
logical entry count. A `Cedar` is returned only after this validation succeeds. This is required
because performance-sensitive query paths use unchecked indexing under those invariants.

Truncation, unsupported versions or byte order, layout mismatch, oversized allocation requests,
corruption, and trailing data produce typed `CedarPersistenceError` variants. An error never
returns a partially initialized trie.

## Compatibility policy

- Major version `1` readers reject unknown major versions. They also reject a minor version newer
  than they implement.
- A future compatible minor version may only add semantics that an older reader can reject or
  safely ignore according to a revised header contract. Existing fields will not be silently
  reinterpreted.
- Default-layout and reduced-trie files are deliberately incompatible. The layout byte is checked
  before body allocation, so compiling with a different feature cannot reinterpret stored nodes.
- Byte order is part of the format. Version 1 writes and accepts only the explicit little-endian
  marker on every host.
- The format preserves owned mutable state, not Rust ABI. Patch releases may improve validation
  while continuing to read valid version 1.0 files.

Memory-mapped loading is deferred. The owned format includes mutable allocator-list state and
requires full structural validation; a future mmap representation must keep unvalidated bytes
isolated from unchecked query code and will need its own alignment and ownership design.

`save_to_path` is a small, non-atomic convenience wrapper that truncates or creates its destination;
an I/O failure can leave a partial file. Applications needing atomic replacement should use
`save_to_writer` with a securely created temporary file, apply their required flush/sync policy,
and rename it according to platform semantics.

The private `persistence::v1` DTO/codec module contains all wire records and primitive I/O. These
APIs, `CedarPersistenceError`, and `DEFAULT_LOAD_MEMORY_LIMIT` require the default `std` feature.
Building with `default-features = false` removes the complete persistence surface and keeps query,
mutation, and live trie structs available through `core` and `alloc`.
