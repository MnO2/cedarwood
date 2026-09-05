# Changelog

All notable changes to cedarwood are documented here. The project follows semantic versioning.

## Unreleased

### Fixed

- Deleting and reinserting keys after all 255 root-block byte labels are occupied no longer
  corrupts the root block's free list. Block 0's reserved-root counting convention is preserved.
- Persistence rejects reachable leaf nodes containing an internal "no value" sentinel, preventing
  a loaded trie from returning reserved values or following a sentinel as a node index.
- Loading retries interrupted reads while checking for trailing data instead of reporting a
  transient `Interrupted` I/O error.
- The comparison harness rejects debug measurements instead of labeling their timings as a
  release-profile run.

### Performance

- Updating a key determines whether it is new during insertion, removing the separate exact
  lookup that previously traversed the key again.
- Entry iteration reuses a traversal key buffer instead of cloning each intermediate prefix,
  avoiding quadratic prefix copying on long branches.
- Path persistence helpers buffer file I/O and report buffered write errors on flush, avoiding
  one system call per encoded integer field.
- Added entry-iteration benchmarks covering the real dictionary and 8 KiB/32 KiB single keys.

### Documentation and validation

- Clarified byte positions, Unicode matching, binary-key restrictions, incremental `build`
  semantics, retained capacity after deletion, buffered stream usage, and actual allocator and
  complexity behavior.
- Expanded byte-key and persistence regression coverage. CI now runs every integration test,
  rustdoc example, and applicable README/API Markdown example in all four feature configurations,
  plus comparison harness format and provenance tests.

## [0.6.0] - 2026-07-12

Version 0.6 is a breaking release. It makes invalid input explicit, adds a byte-oriented API and
safe persistence, and replaces synthetic performance claims with reproducible measurements.

### Added

- Direct construction from strictly sorted unique input with `Cedar::from_sorted`,
  `Cedar::from_sorted_bytes`, and matching `CedarBuilder` methods.
- Byte-slice construction, mutation, lookup, common-prefix, and predictive-search APIs.
- `CedarBuilder` configuration for sibling ordering and `max_trial`.
- `len`, `is_empty`, byte-key and checked UTF-8 entry iterators, and public memory statistics.
- A versioned, explicitly encoded persistence format. Stream and file APIs fully validate loaded
  allocator and trie state before returning a queryable `Cedar`.
- `no_std + alloc` support through `default-features = false`; persistence remains behind the
  default `std` feature.
- Stateful property tests, two-layout Miri coverage, bounded fuzz smoke tests, representative
  Criterion workloads, and a pinned cross-implementation comparison harness.

### Changed

- `build` and `update` now return `Result<_, CedarError>` instead of accepting invalid input or
  panicking. `build` validates its complete input before mutating the trie.
- Stored keys must be nonempty and cannot contain byte `0x00`. Values must be in
  `MIN_VALUE..=MAX_VALUE` (`0..=i32::MAX - 2`) in both layouts.
- Allocating `common_prefix_search` and `common_prefix_predict` return an empty `Vec` when there are
  no matches instead of `None`. Their iterator counterparts remain allocation-free.
- Exact lookup returns only `(value, matched_byte_length)`; internal double-array slots are no
  longer exposed.
- `erase` and `erase_bytes` return `bool` to report whether an entry existed.
- The crate now declares and verifies Rust 1.62.0 as its minimum supported Rust version (MSRV).
- `reduced-trie` remains optional and orthogonal to `std`; measurements did not justify changing
  the default layout.

### Fixed

- Negative and reserved values can no longer be silently confused with internal sentinels.
- Empty insertion keys return a typed error instead of panicking.
- Exhausting predictive iteration from an empty prefix no longer restarts traversal indefinitely.
- Persistence rejects incompatible layouts, unknown versions, oversized or truncated input,
  trailing bytes, and malformed node, sibling, free-list, or block-list relationships.

### Migration from 0.5

1. Propagate or handle `CedarError` from `build`, `update`, and their byte variants.
2. Replace `if let Some(matches) = cedar.common_prefix_search(...)` with direct iteration over the
   returned `Vec`; no matches are represented by an empty vector.
3. Destructure exact matches as `(value, byte_length)` and stop depending on internal node slots.
4. Use the boolean returned by `erase` when callers need to distinguish deletion from a miss.
5. Validate or remap existing negative values before insertion. `-1` and all other negative values
   are outside the 0.6 contract.
6. Enable the default `std` feature when using persistence. Embedded users can disable default
   features and provide an allocator.

[0.6.0]: https://github.com/MnO2/cedarwood/compare/0.5.0...v0.6.0
