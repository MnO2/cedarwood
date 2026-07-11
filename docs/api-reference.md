# API Reference

This document summarizes cedarwood 0.6. For the complete signatures and per-item details, build
the rustdoc with `cargo doc --open`.

Cedarwood 0.6 supports Rust 1.62.0 and newer. The default `std` feature provides persistence and
standard error integration. With `default-features = false`, the mutation and query core requires
`alloc` but not `std`; `reduced-trie` can be enabled independently in either configuration.

## Key and value contract

Cedar stores nonempty byte keys. Byte `0x00` is reserved as the terminal label, so stored keys
cannot contain it. The `&str` APIs are thin UTF-8 wrappers around the byte APIs.

Values must be between `MIN_VALUE` (`0`) and `MAX_VALUE` (`i32::MAX - 2`), inclusive. This range is
identical in the default and `reduced-trie` layouts. Empty keys, terminal bytes, out-of-range
values, and invalid builder settings return `CedarError`; they do not mutate the trie.

Exact, predictive, and erase queries containing `0x00` behave as misses. Common-prefix iteration
stops at the first `0x00` and can still return valid stored prefixes before it; it does not scan the
complete query before traversal. Empty exact, common-prefix, and erase queries behave as misses
because the trie cannot store an empty key. An empty predictive prefix matches every stored key.

## Construction and configuration

`Cedar::new()` creates an empty trie with ordered sibling chains and `max_trial = 1`.
`Cedar::builder()` returns a `CedarBuilder`:

```rust
use cedarwood::Cedar;

let cedar = Cedar::builder()
    .ordered(false)
    .max_trial(3)?
    .build();
assert!(cedar.is_empty());
# Ok::<(), cedarwood::CedarError>(())
```

`ordered(false)` avoids maintaining sorted sibling chains during insertion. `max_trial` controls
how many failed free-block probes are allowed before a block is moved out of the open list and must
be greater than zero. The defaults preserve cedarwood 0.5 behavior.

For a static, already sorted dataset, use the direct constructor:

```rust
use cedarwood::Cedar;

let cedar = Cedar::from_sorted(&[("alpha", 1), ("beta", 2)])?;
assert_eq!(cedar.len(), 2);
# Ok::<(), cedarwood::CedarError>(())
```

`from_sorted` and `from_sorted_bytes` require strictly byte-sorted, unique input. They validate the
entire input and return `CedarError::DuplicateKey` or `CedarError::KeysNotSorted` with the offending
input index. They allocate sibling sets directly and return a trie that still supports `update` and
`erase`. Use incremental `build` when input is unsorted or duplicate keys should overwrite.

`CedarBuilder::from_sorted` and `CedarBuilder::from_sorted_bytes` provide the same direct
construction while preserving configured `ordered` and `max_trial` behavior for later mutations.

## Mutation

```rust
use cedarwood::Cedar;

let mut cedar = Cedar::new();
cedar.build(&[("alpha", 1), ("beta", 2)])?;
cedar.update("alpha", 3)?;
assert!(cedar.erase("beta"));
assert!(!cedar.erase("missing"));
# Ok::<(), cedarwood::CedarError>(())
```

- `build(&[(&str, i32)]) -> Result<(), CedarError>` validates the entire input before inserting
  any item. Duplicate keys use the last supplied value.
- `build_bytes(&[(&[u8], i32)]) -> Result<(), CedarError>` is the byte-key counterpart.
- `update(&str, i32)` and `update_bytes(&[u8], i32)` insert or overwrite one entry.
- `erase(&str) -> bool` and `erase_bytes(&[u8]) -> bool` report whether an entry was removed.

`build` remains the incremental construction API in 0.6.

## Exact lookup

`exact_match_search` and `exact_match_search_bytes` return
`Option<(value, matched_byte_length)>`:

```rust
use cedarwood::Cedar;

let mut cedar = Cedar::new();
cedar.update("abc", 10)?;
assert_eq!(
    cedar.exact_match_search("abc"),
    Some((10, 3)),
);
assert_eq!(cedar.exact_match_search_bytes(b"missing"), None);
# Ok::<(), cedarwood::CedarError>(())
```

## Common-prefix search

`common_prefix_search` finds stored keys that are prefixes of a query. Its byte counterpart is
`common_prefix_search_bytes`. Both return an empty `Vec` when there are no matches. Each item is
`(value, last_matched_byte_position)`.

```rust
use cedarwood::Cedar;

let mut cedar = Cedar::new();
cedar.build(&[("a", 0), ("ab", 1), ("abc", 2)])?;
assert_eq!(
    cedar.common_prefix_search("abcdef"),
    vec![(0, 0), (1, 1), (2, 2)],
);
assert!(cedar.common_prefix_search("xyz").is_empty());
# Ok::<(), cedarwood::CedarError>(())
```

`common_prefix_iter` and `common_prefix_iter_bytes` provide the same results without allocating a
result vector. `PrefixIter` implements `Iterator<Item = (i32, usize)>` and `Clone`.

## Predictive search

`common_prefix_predict` finds stored keys that begin with a query prefix. Its byte counterpart is
`common_prefix_predict_bytes`. Both return an empty `Vec` when there are no matches. Each item is
`(value, depth_below_the_prefix_in_bytes)`.

```rust
use cedarwood::Cedar;

let mut cedar = Cedar::new();
cedar.build(&[("app", 0), ("apple", 1), ("application", 2)])?;
let values: Vec<_> = cedar
    .common_prefix_predict("app")
    .into_iter()
    .map(|item| item.0)
    .collect();
assert_eq!(values, vec![0, 1, 2]);
# Ok::<(), cedarwood::CedarError>(())
```

`common_prefix_predict_iter` and `common_prefix_predict_iter_bytes` are lazy counterparts.
`PrefixPredictIter` implements `Iterator<Item = (i32, usize)>` and `Clone`. Prediction order follows
sibling order; do not depend on it when `ordered(false)` is used.

## Entry iteration

`entries()` reconstructs every stored key and yields `(Vec<u8>, i32)`. Its order reflects current
double-array placement and is not stable across mutations or versions.

`entries_str()` converts each reconstructed key to `String` and yields
`Result<(String, i32), FromUtf8Error>`. Byte keys are never assumed to be valid UTF-8:

```rust
use cedarwood::Cedar;

let mut cedar = Cedar::new();
cedar.update_bytes(&[0xff, b'x'], 7)?;
assert!(cedar.entries_str().next().unwrap().is_err());
assert_eq!(cedar.entries().next(), Some((vec![0xff, b'x'], 7)));
# Ok::<(), cedarwood::CedarError>(())
```

## Logical and memory metrics

- `len()` is the number of stored entries. Overwrite does not change it.
- `is_empty()` is equivalent to `len() == 0`.
- `used_node_slots()` counts occupied double-array slots, including the root and default-layout
  terminal slots.
- `node_capacity()` is the allocated element capacity of the node vector.
- `allocated_bytes()` sums the reserved capacity of the node, node-info, block, and reject vectors
  multiplied by their element sizes. It excludes the inline `Cedar` value and allocator metadata.
- `load_factor()` is `used_node_slots / node_capacity`.
- `memory_stats()` returns those values together in `MemoryStats`.

These definitions are intended for reproducible diagnostics and benchmarks. They describe owned
capacity, not process resident memory.

## Persistence

With the default `std` feature enabled, `save_to_writer` and `load_from_reader` use cedarwood's
stable, explicitly encoded little-endian binary format. `save_to_path` and `load_from_path` are
filesystem conveniences. Loading validates the allocator metadata and complete trie graph before
returning a queryable `Cedar`. The persistence APIs and error type are absent when default features
are disabled.

`load_from_reader` limits peak owned and validation storage to `DEFAULT_LOAD_MEMORY_LIMIT` (512 MiB).
Applications can choose another ceiling with `load_from_reader_with_limit`. The format preserves
`ordered`, `max_trial`, entry state, and allocator state, so the loaded trie supports later
`update` and `erase` operations. Files from the default and `reduced-trie` layouts are intentionally
incompatible.

Serialized lengths never request spare vector capacity, and all attacker-sized reservations are
fallible. Logical occupancy metrics round-trip; `allocated_bytes()` may change because it reports
the receiving allocator rather than serialized allocator growth history. `save_to_path` truncates
its destination and is not atomic; use `save_to_writer` with an application-managed temporary-file
and rename policy when atomic replacement is required.

See [Binary Serialization Format](serialization.md) for the byte-level format, validation boundary,
and compatibility policy.

## Public error type

`CedarError` implements `Debug`, `Display`, `Clone`, `Eq`, and `PartialEq`, plus
`std::error::Error` when the `std` feature is enabled.
Its variants are:

- `EmptyKey`
- `NulByte { position }`
- `InvalidValue { value }`
- `InvalidMaxTrial { value }`
- `DuplicateKey { index }`
- `KeysNotSorted { index }`

Persistence APIs return the separate `CedarPersistenceError`, whose `Io` variant retains the
underlying `std::io::Error`. Other variants distinguish truncation, unsupported versions and byte
order, wrong layout, invalid headers, allocation limits or failures, corrupt state, and trailing
bytes.

## Feature flags

| Flag | Default | Description |
|---|:---:|---|
| `std` | yes | Enables stream/file persistence and standard error integration. Disable default features for `no_std + alloc`. |
| `reduced-trie` | no | Stores leaf values directly in branch nodes and uses terminal nodes only when needed for prefix keys. The public key and value contract is unchanged. |
