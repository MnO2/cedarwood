# cedarwood

Efficiently-updatable double-array trie in Rust, ported from the C++ [cedar](http://www.tkl.iis.u-tokyo.ac.jp/~ynaga/cedar/) library by Naoki Yoshinaga.

[![CI](https://github.com/MnO2/cedarwood/actions/workflows/CI.yml/badge.svg)](https://github.com/MnO2/cedarwood/actions/workflows/CI.yml)
[![Crates.io](https://img.shields.io/crates/v/cedarwood.svg)](https://crates.io/crates/cedarwood)
[![docs.rs](https://docs.rs/cedarwood/badge.svg)](https://docs.rs/cedarwood/)

The minimum supported Rust version is 1.62.0. CI verifies that exact toolchain for every combination
of the `std` and `reduced-trie` features.

## Features

- **Fast lookups** -- double-array tries offer O(k) lookup time where k is the length of the key, with excellent cache locality compared to pointer-based tries.
- **Dynamic updates** -- keys can be inserted and deleted after the initial build, unlike many static trie implementations.
- **Common-prefix search** -- find all keys that are prefixes of a given query string, useful for tokenization and morphological analysis.
- **Predictive search** -- find all keys that share a given prefix, useful for autocomplete.
- **Full Unicode support** -- works with any valid UTF-8 string, including CJK characters, supplementary planes (SIP), and combining characters.
- **Reduced-trie mode** -- optional `reduced-trie` feature flag for a more compact representation at the cost of some flexibility.
- **Versioned persistence** -- save and fully validate mutable tries with a stable little-endian binary format.
- **`no_std + alloc` support** -- disable default features when stream/file persistence is not needed.

## Installation

Add it to your `Cargo.toml`:

```toml
[dependencies]
cedarwood = "0.6"
```

To enable the reduced-trie optimization:

```toml
[dependencies]
cedarwood = { version = "0.6", features = ["reduced-trie"] }
```

For `no_std + alloc` without persistence:

```toml
[dependencies]
cedarwood = { version = "0.6", default-features = false }
```

## Quick Start

```rust
use cedarwood::Cedar;

fn main() -> Result<(), cedarwood::CedarError> {
    let dict = vec![
        "a", "ab", "abc",
        "网", "网球", "网球拍",
        "中", "中华", "中华人民", "中华人民共和国",
    ];
    let key_values: Vec<(&str, i32)> = dict
        .into_iter()
        .enumerate()
        .map(|(k, s)| (s, k as i32))
        .collect();

    let mut cedar = Cedar::new();
    cedar.build(&key_values)?;

    // Exact match
    let result = cedar.exact_match_search("中华人民");
    assert!(result.is_some());

    // Common prefix search: finds "网", "网球", "网球拍"
    let result: Vec<i32> = cedar
        .common_prefix_search("网球拍卖会")
        .iter()
        .map(|x| x.0)
        .collect();
    assert_eq!(vec![3, 4, 5], result);

    // Predictive search: finds all keys starting with "中"
    let result: Vec<i32> = cedar
        .common_prefix_predict("中")
        .iter()
        .map(|x| x.0)
        .collect();
    assert_eq!(vec![6, 7, 8, 9], result);
    Ok(())
}
```

## API Overview

| Method | Description |
|--------|-------------|
| `Cedar::new()` | Create an empty trie |
| `Cedar::from_sorted(...)` / `from_sorted_bytes(...)` | Directly build a mutable trie from sorted unique input |
| `build(&key_values)` / `build_bytes(...)` | Validate and insert key-value pairs |
| `update(key, value)` / `update_bytes(...)` | Insert or update one key, returning a typed error for invalid input |
| `erase(key)` / `erase_bytes(...)` | Delete a key and report whether it existed |
| `exact_match_search(key)` | Look up an exact key, returns `Option<(value, length)>` |
| `common_prefix_search(key)` | Find all dictionary keys that are prefixes of `key` |
| `common_prefix_iter(key)` | Iterator version of `common_prefix_search` |
| `common_prefix_predict(key)` | Find all dictionary keys that start with `key` |
| `common_prefix_predict_iter(key)` | Iterator version of `common_prefix_predict` |
| `entries()` / `entries_str()` | Iterate reconstructed byte keys or checked UTF-8 keys |
| `len()` / `is_empty()` | Report the logical entry count |
| `memory_stats()` | Report occupied slots, node capacity, reserved bytes, and load factor |
| `save_to_writer(...)` / `load_from_reader(...)` | Persist and validate a mutable trie |

The `std` feature is enabled by default and provides persistence plus standard error integration.
Stream and path persistence APIs are not available when default features are disabled.

The byte APIs accept arbitrary nonempty byte strings except those containing `0x00`, which cedar
reserves as its terminal label. Values have one layout-independent contract:
`0..=cedarwood::MAX_VALUE` (`i32::MAX - 2`). Invalid keys and values return `CedarError`; exact and
predictive queries containing `0x00` behave as misses, while common-prefix traversal stops at the
reserved byte. `Cedar::builder()` exposes sibling ordering and the
validated `max_trial` tuning parameter while preserving the historical defaults.
For large static inputs that are already strictly byte-sorted and unique, `Cedar::from_sorted` (or
`from_sorted_bytes`) allocates complete sibling sets directly and avoids incremental conflict
resolution. The returned trie remains fully mutable.
Use the corresponding `CedarBuilder` methods when later mutations need nondefault `ordered` or
`max_trial` settings.

For detailed API documentation, see [docs.rs](https://docs.rs/cedarwood/) or the [docs/](docs/) folder.

## Use Cases

- **Text segmentation / tokenization** -- common-prefix search is the core operation for dictionary-based Chinese/Japanese word segmentation.
- **Autocomplete / suggest** -- predictive search returns all completions for a typed prefix.
- **Morphological analysis** -- fast dictionary lookup for NLP pipelines.
- **IP routing tables** -- longest-prefix matching on serialized address bytes.
- **Keyword filtering** -- scan text for occurrences of any keyword in a large dictionary.

## Benchmarks

```bash
cargo bench
```

The Criterion suite uses the sorted, deduplicated 349,045-entry dictionary in
`benches/macro-benchmark/dict.txt`.
See [benches/README.md](benches/README.md) for workload definitions, shorter smoke-test
commands, reduced-trie instructions, and the local Criterion baseline workflow used to investigate
regressions. A legacy C++ cedar benchmark is retained in
`benches/cpp/`, but it does not yet implement the shared comparison schema below.

### Implementation comparison

These results are comparative, not universal. They are from one run on 2026-07-11 UTC using an
Apple M4 Pro, macOS 26.1, and rustc 1.94.1. Lower is better for construction time and owned heap;
higher is better for throughput. The historical cedarwood 0.5.0 run remains checked in separately
for provenance but is not mixed into this table.

| Implementation | Mutable | Build (ms) | Exact hit (M/s) | Exact miss (M/s) | Prefix scan (MiB/s) | Owned heap (MiB) |
|---|:---:|---:|---:|---:|---:|---:|
| cedarwood 0.6.0 | yes | 33.105 | 57.733 | 57.979 | 246.76 | 20.157 |
| fst 0.4.7 | no | 50.112 | 10.671 | 10.651 | unsupported | 5.000 |
| daachorse 3.0.2 | no | 169.108 | 38.817 | 19.242 | 324.70 | 19.729 |
| crawdad 0.4.0 | no | 1131.762 | 91.358 | 104.355 | 309.10 | 8.250 |
| yada 0.7.0 | no | 308.049 | 65.034 | 47.672 | 389.22 | 5.908 |
| `std::collections::HashMap` | yes | 6.748 | 63.304 | 105.877 | unsupported | 19.407 |
| C++ cedar | yes | not measured | not measured | not measured | not measured | not measured |

The static implementations do not provide cedarwood's incremental update/erase capability.
Daachorse exact lookup is emulated as an anchored full-span Aho-Corasick match, while its prefix
throughput uses its native overlapping scan. `fst` and `HashMap` have no native tokenizer-style prefix
operation and are marked unsupported. C++ cedar was not measured because the shared-schema adapter
is not implemented and `cedarpp.h` was not installed locally.

Every measured number is preserved in the
[current 0.6 raw output](benches/comparison/results/2026-07-11-apple-m4-pro-cedarwood-0.6.0.txt).
The [historical 0.5 raw output](benches/comparison/results/2026-07-11-apple-m4-pro.txt) is retained
unchanged. See the
[comparison harness documentation](benches/comparison/README.md) for the exact workload, memory
method, pinned dependencies, and single-command reproduction instructions.

## Documentation

- [Architecture & Design](docs/architecture.md) -- how the double-array trie works
- [API Reference](docs/api-reference.md) -- detailed method documentation
- [Internal Implementation](docs/internals.md) -- block management, conflict resolution, and memory layout
- [Binary Serialization Format](docs/serialization.md) -- stable encoding, validation, limits, and compatibility
- [Changelog](CHANGELOG.md) -- breaking changes and 0.5-to-0.6 migration notes
- [Roadmap to 1.0](docs/roadmap-to-1.0.md) -- remaining stabilization gates and deferred work

## License

This work is released under the BSD-2-Clause license, following the original license of C++ cedar. A copy of the license is provided in the [LICENSE](LICENSE) file.

## Reference

- [cedar -- C++ implementation of efficiently-updatable double-array trie](http://www.tkl.iis.u-tokyo.ac.jp/~ynaga/cedar/) by Naoki Yoshinaga
- Aoe, J. (1989). [An efficient implementation of trie structures](https://dl.acm.org/citation.cfm?id=146691). *Software: Practice and Experience*.
