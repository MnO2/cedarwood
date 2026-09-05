# Update and entry iteration measurements

Measured on 2026-09-05 UTC on an Apple M4 Pro, macOS 26.1 (25B78), with
`rustc 1.97.0 (2d8144b78 2026-07-07)`, `aarch64-apple-darwin`, Cargo's optimized bench profile.
These are local before/after measurements, not portable performance guarantees.

The baseline uses the update and entry iteration implementations from `71cdec2`, with the new
entry workloads added to the benchmark harness. The independent root-block allocator fix was
already present in the reduced-layout baseline; the dictionary workloads do not fill the root's
255 nonzero labels. The candidate is the unreleased working tree based on that commit. Candidate
`src/lib.rs` SHA-256 after formatting: `5f3c9ab37acfdc39bbec388e0d412fb1004f512d71521cc3202225c1e746af15`.

Construction uses the shared 349,045-entry dictionary. Churn runs the existing 2,048-key
update/erase/reinsert workload. Entry iteration consumes all reconstructed keys, either from the
full dictionary or a trie containing one repeated-byte key of the indicated length.

## Results

Times are Criterion estimates with 95% confidence interval bounds in parentheses. Lower is better.

| Layout | Workload | Before | After |
|---|---|---:|---:|
| Default | Incremental sorted build | 36.817 ms (36.038–37.250) | 33.627 ms (33.276–33.992) |
| Default | Churn | 307.32 µs (306.76–307.86) | 289.80 µs (288.40–291.69) |
| Default | Dictionary entries | 47.131 ms (46.925–47.338) | 21.532 ms (21.501–21.561) |
| Default | One 8,192-byte key | 1.1923 ms (1.1862–1.2000) | 94.095 µs (94.000–94.186) |
| Default | One 32,768-byte key | 21.197 ms (21.026–21.425) | 376.23 µs (375.68–376.84) |
| Reduced | Incremental sorted build | 35.557 ms (35.419–35.672) | 32.933 ms (32.813–33.004) |
| Reduced | Churn | 286.70 µs (286.23–287.19) | 268.38 µs (267.60–269.22) |
| Reduced | Dictionary entries | 46.463 ms (46.209–46.791) | 20.863 ms (20.733–21.052) |
| Reduced | One 8,192-byte key | 1.1740 ms (1.1718–1.1762) | 97.826 µs (93.581–105.93) |
| Reduced | One 32,768-byte key | 21.229 ms (21.081–21.396) | 377.05 µs (373.40–382.41) |

Dictionary enumeration takes about 54–55% less time. Increasing the single key's length fourfold
now takes approximately four times as long, consistent with eliminating the old quadratic prefix
copying. Sorted incremental construction improves by about 8% in Criterion's change estimates.
The default churn comparison reports "Change within noise threshold" despite its lower point
estimate, so that result alone does not establish a stable improvement. Reduced churn improves
by about 7% in this run. Node occupancy and allocated bytes are unchanged in these workloads.

## Reproduction

Run the same harness before and after the update/entry changes, keeping the toolchain and machine
conditions fixed. The distinct baseline names keep layout measurements separate. Default baseline
build/churn and entries were measured in separate invocations; the following combined filter runs
the same workloads:

```bash
cargo bench --bench cedarwood_benchmark --locked -- 'build/incremental_sorted|churn/|entries/' --warm-up-time 1 --measurement-time 3 --sample-size 20 --save-baseline audit-before
cargo bench --bench cedarwood_benchmark --features reduced-trie --locked -- 'build/incremental_sorted|churn/|entries/' --warm-up-time 1 --measurement-time 3 --sample-size 20 --save-baseline audit-reduced-before

# After applying the candidate:
cargo bench --bench cedarwood_benchmark --locked -- 'build/incremental_sorted|churn/|entries/' --warm-up-time 1 --measurement-time 3 --sample-size 20 --baseline audit-before
cargo bench --bench cedarwood_benchmark --features reduced-trie --locked -- 'build/incremental_sorted|churn/|entries/' --warm-up-time 1 --measurement-time 3 --sample-size 20 --baseline audit-reduced-before
```

The build group overrides the requested sample count to 10. Other groups use 20 samples; Criterion
extends collection beyond three seconds when necessary. Some runs contain outliers, particularly
default churn and the reduced 8 KiB case. No other benchmark measurement ran concurrently.

File I/O buffering has a separate [same-source measurement](2026-09-05-persistence-io.md).
