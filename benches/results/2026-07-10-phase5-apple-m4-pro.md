# Phase 5 bulk/layout benchmark evidence

Date: 2026-07-10

Environment:

- Apple M4 Pro, arm64
- macOS 26.1 (25B78)
- rustc 1.94.1 (e408947bf 2026-03-25), LLVM 21.1.8
- cargo 1.94.1 (29ea6fb6a 2026-03-24)
- Corpus SHA-256:
  `139519822fe8ab9e10d9d07e68ea0451045380aedaf54ecc51e2a28c6b42a13f`
- 349,045 sorted unique keys

Commands:

```bash
cargo bench --bench cedarwood_benchmark -- --noplot
cargo bench --bench cedarwood_benchmark --features reduced-trie -- --noplot
```

Recorded Criterion output (low, point estimate, and high from each reported confidence interval):

| Layout | Workload | Time confidence interval | Point throughput |
|---|---|---:|---:|
| default | build/incremental_sorted | [32.803, 32.929, 32.993] ms | 10.600 M keys/s |
| default | build/direct_sorted | [31.136, 31.368, 31.643] ms | 11.127 M keys/s |
| default | exact hit sample | [62.296, 62.867, 63.922] us | 65.153 M queries/s |
| default | exact miss sample | [62.934, 63.079, 63.227] us | 64.935 M queries/s |
| default | tokenizer prefix scan | [18.980, 19.059, 19.139] ms | 3.2794 MiB/s |
| default | mutation churn | [305.14, 305.55, 305.95] us | 20.108 M operations/s |
| reduced-trie | build/incremental_sorted | [33.716, 33.903, 33.999] ms | 10.295 M keys/s |
| reduced-trie | build/direct_sorted | [30.099, 30.202, 30.310] ms | 11.557 M keys/s |
| reduced-trie | exact hit sample | [66.718, 66.915, 67.099] us | 61.212 M queries/s |
| reduced-trie | exact miss sample | [66.585, 66.809, 67.049] us | 61.309 M queries/s |
| reduced-trie | tokenizer prefix scan | [18.760, 18.852, 18.949] ms | 3.3154 MiB/s |
| reduced-trie | mutation churn | [284.99, 287.46, 292.02] us | 21.373 M operations/s |

Construction memory emitted by the benchmark before timing:

| Layout | Method | Used slots | Node capacity | Allocated bytes | Load factor |
|---|---|---:|---:|---:|---:|
| default | incremental sorted | 1,548,541 | 2,097,152 | 21,135,874 | 0.738402 |
| default | direct sorted | 1,548,541 | 2,097,152 | 21,135,874 | 0.738402 |
| reduced-trie | incremental sorted | 1,251,497 | 2,097,152 | 21,135,874 | 0.596760 |
| reduced-trie | direct sorted | 1,251,497 | 2,097,152 | 21,135,874 | 0.596760 |

Direct construction was 4.7% faster than incremental sorted construction in the default layout
and 10.9% faster in reduced-trie. The implementations were semantically equivalent and had equal
aggregate occupancy and allocation within each layout on this corpus. Slot-for-slot identity is
not part of the constructor contract.

Reduced-trie occupied 19.2% fewer slots, but power-of-two vector growth left allocated bytes equal
on this corpus. It was 6.4% slower for exact hits and 5.9% slower for exact misses, about 1.1%
faster in prefix scan (within Criterion's noise threshold), and 5.9% faster for churn. Because the
results are workload-dependent rather than a consistent win, the default layout remains unchanged.

## PrefixIter checked/unchecked A/B

The unchecked candidate replaced the array accesses in `PrefixIter::next` with the existing
`node_unchecked` helper and documented the derived-index invariant. It was benchmarked against the
checked implementation, then removed because neither layout showed a statistically significant
change.

Commands:

```bash
cargo bench --bench cedarwood_benchmark prefix_scan/tokenizer_text -- --noplot
cargo bench --bench cedarwood_benchmark --features reduced-trie prefix_scan/tokenizer_text -- --noplot
```

| Layout | Checked point estimate | Unchecked point estimate | Criterion comparison |
|---|---:|---:|---|
| default | 19.059 ms | 18.934 ms | -0.24%..+1.11%, p=0.21; no change detected |
| reduced-trie | 18.852 ms | 18.838 ms | -1.22%..+0.20%, p=0.16; no change detected |

The checked implementation is retained to avoid expanding the unsafe surface without measurable
benefit.

The two layouts were run sequentially, so cross-layout percentages are descriptive rather than an
interleaved statistical comparison. The checked/unchecked A/B above uses Criterion's same-layout
baseline comparison and reports its confidence interval and p-value.

## Layout-policy assessment

`src/lib.rs` contains 32 layout-conditional attributes. A policy can hide base encoding and leaf
sentinel operations, but most conditionals are behavioral: leaf-to-branch promotion, terminal
placement, lookup completion, erase origin, predictive traversal, and child relocation. A generic
policy would either make `Cedar` generic in the public API or preserve compile-time aliases while
moving the same branches behind a private trait. The latter reduces only local syntax and adds an
abstraction boundary to hot paths; the former changes public ergonomics. No prototype was adopted
because the current evidence does not justify either cost. Revisit only with a design that removes
the behavioral duplication and demonstrates unchanged generated code and benchmark results.
