# Cedarwood benchmarks

The shared benchmark support module parses the first whitespace-delimited field from each nonempty
line of `macro-benchmark/dict.txt`, removes duplicate keys, sorts them by UTF-8 byte order, and
assigns stable nonnegative values by sorted position. Both Criterion and the standalone comparison
harness use this exact normalization, sampling, miss-generation, and tokenizer-text code. The
checked-in corpus currently provides 349,045 unique keys.

Run the complete suite for the default layout with:

```bash
cargo bench --bench cedarwood_benchmark
```

Run it for the reduced-trie layout with:

```bash
cargo bench --bench cedarwood_benchmark --features reduced-trie
```

For a quick compile-and-execute smoke test of every workload, append `-- --test` to either command.

## Tracking a local regression

Criterion baselines are machine-local, so record the before and after measurements on the same
otherwise-idle machine, with the same Rust toolchain and power settings. Separate target
directories keep the two layouts from overwriting each other's baselines:

```bash
CARGO_TARGET_DIR=target/criterion-default cargo bench --bench cedarwood_benchmark -- --save-baseline before
CARGO_TARGET_DIR=target/criterion-reduced cargo bench --bench cedarwood_benchmark --features reduced-trie -- --save-baseline before

# After applying the candidate change:
CARGO_TARGET_DIR=target/criterion-default cargo bench --bench cedarwood_benchmark -- --baseline before
CARGO_TARGET_DIR=target/criterion-reduced cargo bench --bench cedarwood_benchmark --features reduced-trie -- --baseline before
```

Preserve decision-relevant results under `benches/results/` with the date, CPU, OS, Rust version,
layout, command, confidence interval, and conclusion. CI compiles and smoke-runs every Criterion
workload in both layouts; it does not treat noisy shared-runner timings as a regression oracle.

## Workloads

- `build/incremental_sorted` constructs a new trie from every corpus key through `build`.
- `build/direct_sorted` constructs the same trie through `Cedar::from_sorted`, allocating complete
  sibling sets directly. Both build workloads report throughput in keys, and the benchmark prints
  their memory statistics before timing.
- `exact_match/hit_sample` queries 4,096 keys sampled evenly across the corpus.
- `exact_match/realistic_prefix_miss_sample` queries the same sampled keys with a suffix that is
  verified not to occur in the trie, preserving a complete real key as the miss's prefix.
- `prefix_scan/tokenizer_text` runs common-prefix iteration at every UTF-8 character boundary of a
  deterministic text of at least 64 KiB assembled from sampled corpus keys. Throughput is reported
  in input bytes.
- `churn/update_erase_reinsert_working_set` repeatedly updates, erases, and reinserts each key in a
  deterministic 2,048-key working set. Throughput counts all three mutations per key.
- `entries/dictionary` reconstructs and consumes every key/value entry in the full corpus.
  Throughput is reported in entries.
- `entries/single_deep_key/{8192,32768}` reconstructs one 8 KiB or 32 KiB key, including iterator
  exhaustion. These cases expose repeated prefix copying on long branches; throughput is in key
  bytes. The tries are constructed before timing.

Corpus loading, sampling, miss validation, trie setup for lookup workloads, and tokenizer offset
construction occur outside the timed loops. Criterion's `black_box` is applied to timed inputs and
results.

The Phase 5 default/reduced layout comparison and checked/unchecked `PrefixIter` A/B are preserved
in [`results/2026-07-10-phase5-apple-m4-pro.md`](results/2026-07-10-phase5-apple-m4-pro.md).
The update and entry-iteration audit records its same-machine before/after measurements in
[`results/2026-09-05-update-entries-apple-m4-pro.md`](results/2026-09-05-update-entries-apple-m4-pro.md).

The standalone [persistence I/O probe](tools/persistence_io.rs) compares buffered path helpers with
raw `File` stream I/O, verifies byte and entry equality, and keeps filesystem work out of the
Criterion suite. See [its recorded results and reproduction commands](results/2026-09-05-persistence-io.md).

For the separate, exact-pinned comparison against other Rust implementations, see
[`comparison/README.md`](comparison/README.md). The comparison harness verifies equivalent results
before timing and keeps all comparator dependencies outside cedarwood's published crate. Its
checked-in raw output and README table are mechanically cross-checked in CI.
