# Reproducible implementation comparison

This standalone crate keeps comparator libraries out of cedarwood's runtime dependencies. It uses
the same sorted, deduplicated 349,045-key corpus for every implementation, assigns values by sorted
key position, and validates all sampled hits, misses, and supported prefix-scan results before
reporting timings.

Run it from the repository root:

```bash
(cd benches/comparison && cargo run --release --locked)
```

Measurement mode rejects debug builds so they cannot emit timings labeled with the release
profile. The README verification command below can run in debug mode.

To preserve a new run, redirect the complete output to a dated file under `results/`, then update
the root README only from that checked-in output. Do not compare results produced on different
machines as though they were from one experiment.

The current README table measures the unreleased worktree, whose package version is still 0.6.0,
and comes from
[`results/2026-09-05-apple-m4-pro-unreleased.txt`](results/2026-09-05-apple-m4-pro-unreleased.txt).
The earlier
[`0.6 output`](results/2026-07-11-apple-m4-pro-cedarwood-0.6.0.txt) and
[`0.5 output`](results/2026-07-11-apple-m4-pro.txt) remain unchanged as historical evidence; do not
mix rows from different runs.

Verify mechanically that the raw file still describes the current 0.6 benchmark inputs and that
the README table matches its CSV rows:

```bash
(cd benches/comparison && cargo run --locked -- --verify-readme results/2026-09-05-apple-m4-pro-unreleased.txt ../../README.md)
```

Verification rejects a stale package version or layout, root `Cargo.toml`, cedarwood source,
comparison `Cargo.toml`, comparison harness, shared workload support, comparison lockfile, dataset
path, or dataset hash. It also requires the cedarwood CSV row version to match the version parsed
from the root manifest. The command targets the current worktree result; the preserved historical
files record older sources and are not expected to verify against the current worktree.

## Measurement contract

- The comparison manifest explicitly disables cedarwood's inherited default features and enables
  only `std`, selecting the non-reduced/default layout. Recording and verification validate that
  manifest contract before reporting `cedarwood_layout=default`.
- Construction is the median of three fresh full-corpus builds.
- Exact-hit and exact-miss throughput use the same 4,096 evenly sampled keys. Misses append a
  Unicode noncharacter so they retain an entire real key as a prefix.
- The tokenizer workload uses the same deterministic text as the Criterion suite and reports all
  dictionary matches in it. Cedarwood, crawdad, and yada run common-prefix search at every UTF-8
  character boundary; crawdad decodes each text suffix through `.chars()` inside its timed scan, so
  it starts with the same UTF-8 text contract rather than a pre-decoded character buffer.
  Daachorse runs its equivalent native overlapping Aho-Corasick scan. `fst` and `HashMap` do not
  expose a native tokenizer/prefix-match operation and are marked unsupported.
- Daachorse has no dictionary-style exact lookup, so its exact result is an anchored,
  full-input-span match through the Aho-Corasick API; the output labels this explicitly.
- Owned heap memory is measured in a separate build by a tracking wrapper around Rust's system
  allocator. It is the net live byte count requested for the constructed data structure after
  temporary allocations have been released. The corpus and benchmark input buffers are excluded;
  allocator metadata and executable/code size are not included. For cedarwood 0.6 and later, the
  harness asserts that this value equals `Cedar::allocated_bytes()`, whose documented
  vector-capacity definition is the public source of truth.
- `fst`, daachorse, crawdad, and yada are static structures. Cedarwood and `HashMap` support
  mutation, so the table keeps that capability visible rather than implying feature parity.
- `benches/cpp/bench_cedar.cc` is a legacy benchmark and does not implement this harness's shared
  workloads, correctness checks, memory method, or output schema. The published run also lacked
  `cedarpp.h`, so C++ cedar remains explicitly not measured rather than estimated.

Comparator versions are exact-pinned in `Cargo.toml`, and `Cargo.lock` records their transitive
dependencies. Displayed versions are derived from the checked-in lockfile rather than duplicated in
the harness. Raw output records the compiler, release profile, OS, CPU, date, Git revision/worktree
state, cedarwood layout, and SHA-256 hashes for cedarwood's root manifest and source, the comparison
manifest and harness, shared workload support, the comparison lockfile, and the original corpus.
