# cedarwood

Efficiently-updatable double-array trie in Rust, ported from C++ cedar.

## Build & Test

```bash
cargo build                              # debug build
cargo build --release                    # release build
cargo test                               # run all tests
cargo test --features reduced-trie       # test with reduced-trie feature
cargo test --no-default-features --lib   # no_std + alloc core
cargo test --no-default-features --features reduced-trie --lib
cargo clippy --all-targets --all-features -- -D warnings
cargo bench                              # run criterion benchmarks
cargo doc --open                         # generate and view rustdoc
```

## Project Structure

```
src/lib.rs          -- entire library: data structures, Cedar impl, tests
benches/
  cedarwood_benchmark.rs   -- criterion micro-benchmarks
  macro-benchmark/dict.txt -- shared real-dictionary benchmark corpus
  comparison/              -- standalone cross-library comparison harness
  cpp/                     -- C++ cedar benchmark for comparison
docs/                      -- architecture and design documentation
```

## Code Conventions

- Single-file library (`src/lib.rs`). All types, methods, and tests live here.
- Internal structs (`NInfo`, `Node`, `Block`) are private. The public surface includes `Cedar`,
  `CedarBuilder`, `CedarError`, `CedarPersistenceError`, query/entry iterators, `MemoryStats`, and
  the shared value and persistence-load bounds.
- Stored keys are nonempty byte strings without `0x00`; values are `0..=i32::MAX - 2` in both
  layouts. String APIs are thin wrappers around byte APIs.
- Feature flag `reduced-trie` enables a more compact trie representation via `#[cfg(feature = "reduced-trie")]` blocks throughout the code.
- The default `std` feature owns persistence and `std::error::Error`; disabling default features
  builds the query/mutation core with `core` plus `alloc`. Keep `reduced-trie` orthogonal to `std`.
- Uses `smallvec` for stack-allocated child collection during conflict resolution (avoids heap allocation for up to 256 children).
- Widening casts use `i32::from(u8_val)` instead of `as i32` (clippy compliance).
- Edition 2021, formatted with `rustfmt` (config in `rustfmt.toml`).
- MSRV is Rust 1.62.0. Keep `Cargo.lock` at format version 3 so the exact MSRV CI job can consume
  it, and test all four `std`/`reduced-trie` combinations before raising the declaration.
- Criterion regressions use same-machine saved baselines from `benches/README.md`; comparison table
  rows must pass the harness's mechanical README verifier.

## Key Implementation Notes

- The double-array stores `base` and `check` in `Node`. Negative values in `base`/`check` indicate free slots forming a doubly-linked free list.
- Blocks are 256-element chunks classified as Open (>1 free), Closed (1 free), or Full (0 free), managed as cyclic doubly-linked lists.
- Conflict resolution (`resolve`) relocates the smaller sibling set to minimize work.
- The `reject` heuristic prunes block searches: if a block's minimum rejection threshold exceeds the number of children needed, the block is skipped.
- `Cedar::from_sorted` and `from_sorted_bytes` validate strict byte order and uniqueness, then
  allocate each complete sibling set through the existing block/free-list machinery. They must not
  regress mutation-after-build invariants in either layout.
- Stream/file persistence and its wire DTOs are available only with `std`; the core trie must keep
  compiling against `core + alloc`. Deserialized state is never queryable until structural
  validation completes.
