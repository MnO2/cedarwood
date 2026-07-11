# Cedarwood Performance, Safety, and API Roadmap

Status: approved for sequential implementation

## Goal

Prepare cedarwood for a 0.6 release by replacing synthetic performance claims with reproducible measurements, protecting the trie's unsafe hot paths, cleaning up public API footguns, and adding bulk construction and persistence. Finish with documented compatibility, CI, and release gates that can support a later 1.0 release.

## Current baseline

- `benches/cedarwood_benchmark.rs` builds a 13-key trie.
- `benches/macro-benchmark/dict.txt` contains the real dictionary corpus.
- `benches/macro-benchmark/src/main.rs` measures wall-clock time with `Instant` and prints milliseconds.
- Query traversal uses unchecked indexing in `find`, `begin`, and `next`; `PrefixIter::next` uses checked indexing.
- CI runs `cargo check --all-features`, cross-platform tests with all features, rustfmt, and coverage. It does not run Miri, fuzz smoke tests, Clippy with denied warnings, a separate feature matrix, or MSRV validation.
- The public API accepts `&str`, panics on empty insertion keys, reserves negative and layout-specific `i32` values without a public contract, and returns `Option<Vec<_>>` from allocating prefix searches.
- `build` is repeated incremental insertion. The trie has no persistence API, entry iterator, entry count, builder, or public memory statistics.

## Execution rules for sub-agents

1. Execute one phase at a time. Do not start the next phase until the primary agent has reviewed the diff and rerun that phase's verification.
2. Before editing, run `git status --short --branch -uall`. Treat changes from completed phases as the new baseline and preserve unrelated user work.
3. Read `CLAUDE.md`, this plan, and the files named by the assigned phase before changing code.
4. Keep each phase scoped to its listed files and acceptance criteria. Do not perform unrelated refactors.
5. Add tests before or with behavior changes. Any unsafe block must have a local safety argument tied to validated invariants.
6. Do not commit, push, publish, tag, or modify GitHub state. The primary agent owns integration and release actions.
7. End each handoff with changed files, design decisions, commands run, results, and remaining risks. A phase is incomplete if its verification fails.

## Phase 1: Representative Cedar benchmarks

### Scope

- Replace the 13-key Criterion fixture with a shared loader for `benches/macro-benchmark/dict.txt`.
- Parse the first whitespace-delimited field from each nonempty dictionary line and assign stable nonnegative values.
- Benchmark these workloads separately:
  - full-trie construction;
  - exact-match hits sampled across the corpus;
  - exact-match misses that share realistic prefixes with dictionary keys;
  - tokenizer-style prefix scanning over a deterministic long text assembled from the corpus;
  - deterministic insert/update/erase churn over a working set.
- Set Criterion throughput in keys or bytes where it makes results easier to interpret. Keep fixture construction outside timed lookup loops and use `black_box` on inputs and results.
- Replace the macro benchmark's ad hoc timing. Keep any useful corpus-loading code only if Criterion uses it; otherwise remove the obsolete macro-benchmark executable while retaining the corpus.
- Document benchmark commands and workload definitions in `README.md` or `benches/README.md`.

### Acceptance criteria

- The benchmark corpus contains at least 300,000 unique nonempty keys after parsing.
- Every timed workload has a deterministic input set and a name that identifies hit, miss, scan, build, or churn behavior.
- A short Criterion smoke run completes for both layouts.
- Unit tests remain unchanged and green.

### Verification

```bash
cargo fmt --all -- --check
cargo test
cargo test --features reduced-trie
cargo bench --bench cedarwood_benchmark --no-run
cargo bench --bench cedarwood_benchmark -- --test
cargo bench --bench cedarwood_benchmark --features reduced-trie -- --test
```

## Phase 2: Reproducible comparison harness and published table

### Scope

- Add an isolated comparison harness so benchmark-only dependencies do not become runtime dependencies of cedarwood.
- Compare cedarwood with `fst`, `daachorse`, `crawdad`, `yada`, `std::collections::HashMap`, and the existing C++ cedar implementation when the local C++ dependency is available.
- Pin comparator versions and record the exact compiler versions, optimization flags, dataset hash, operating system, CPU, and date.
- Use equivalent semantics and the same corpus for all implementations. Separate static-only implementations from mutable ones instead of implying that every library supports the same update workload.
- Report construction time, exact-hit throughput, exact-miss throughput, tokenizer-style prefix throughput where supported, and resident or owned data-structure bytes using a documented measurement method.
- Add a README comparison table generated from an actual run. Label the machine and date, state that results are comparative rather than universal, and link to reproduction instructions and raw output.
- Do not fabricate unavailable C++ or library results. Mark unsupported operations as unsupported and unavailable toolchains as not measured.

### Acceptance criteria

- A single documented command builds and runs the Rust comparison harness.
- Every number in the README table is traceable to checked-in raw output and harness code.
- The harness verifies lookup results before timing them.
- Comparator dependencies remain outside `[dependencies]` for the published cedarwood library.

### Verification

```bash
cargo test
cargo test --features reduced-trie
cargo check --manifest-path benches/comparison/Cargo.toml
# Run the comparison command documented by the phase and preserve its raw output.
```

## Phase 3: Miri, fuzzing, and mutation state-machine tests

### Scope

- Add a deterministic property/state-machine test that interleaves insert, overwrite, erase, exact lookup, common-prefix lookup, and predictive lookup operations.
- Use a standard map-based reference model and compare values and result sets after every operation sequence. Cover shared prefixes, Unicode, long keys, repeated deletion, and reinsertion. Exercise empty strings only through nonmutating operations during this phase because the current insertion API panics on them; Phase 4 must add checked empty-insertion coverage. Use nonnegative values below the internal upper sentinels so the model is valid on both layouts before Phase 4 formalizes the public value contract.
- Add a `cargo-fuzz` target that decodes arbitrary bytes into the same operation model. Keep the target deterministic, bounded, and suitable for both the default and reduced layouts.
- Add a Miri CI job for default and `reduced-trie` configurations. Exclude benchmark and fuzz crates from Miri where required.
- Add a bounded fuzz smoke check to CI if it is stable within the project's time budget; otherwise document the local and scheduled commands and keep continuous fuzzing separate from pull-request CI.
- Run Miri and the fuzz target against the existing unsafe query paths before considering any new unchecked access.
- Pin Miri to `nightly-2026-07-10` and cargo-fuzz to `0.13.2`. Review both pins quarterly and when either upstream publishes a relevant compatibility or security advisory. Update `.github/workflows/CI.yml` and the Phase 3/Phase 7 commands in this roadmap together, only after rerunning both layouts locally.

### Acceptance criteria

- The state-machine test exercises update/erase/reinsert sequences and checks the reference model after each sequence.
- Miri passes for both layouts.
- The fuzz target builds and completes a bounded smoke run without crashes or mismatches.
- All unchecked indexing helpers retain precise safety comments, and no failing corpus is ignored.

### Verification

```bash
cargo fmt --all -- --check
cargo fmt --manifest-path test-support/Cargo.toml -- --check
cargo fmt --manifest-path fuzz/Cargo.toml -- --check
cargo test
cargo test --features reduced-trie
cargo +nightly-2026-07-10 miri test
cargo +nightly-2026-07-10 miri test --features reduced-trie
cargo install cargo-fuzz --version 0.13.2 --locked
cargo +nightly-2026-07-10 fuzz build stateful_operations
cargo +nightly-2026-07-10 fuzz build stateful_operations --features reduced-trie
cargo +nightly-2026-07-10 fuzz run stateful_operations -- -runs=10000
cargo +nightly-2026-07-10 fuzz run stateful_operations --features reduced-trie -- -runs=10000
cargo package --list --allow-dirty
cargo package --allow-dirty
```

## Phase 4: 0.6 API cleanup and observability

### Scope

- Introduce a public error type and checked mutation APIs. Empty keys must return an error rather than panic.
- Define one value contract that works in both layouts. Reserve internal sentinels and reject values outside the supported range with a typed error. Add boundary tests for `-1`, other negative values, the maximum accepted value, and each reserved upper value.
- Change `build` and `update` to return `Result`; update examples, rustdoc, README, and internal call sites.
- Change allocating `common_prefix_search` and `common_prefix_predict` to return `Vec` directly. Keep iterator APIs as the zero-allocation option.
- Add byte-slice counterparts for construction, update, erase, exact match, prefix search, and predictive search. Implement `&str` methods as thin wrappers around the byte APIs so the byte-oriented implementation has one source of truth.
- Add `len`, `is_empty`, an entry iterator that reconstructs keys as bytes, and a UTF-8 convenience adapter that reports invalid UTF-8 rather than assuming it cannot occur.
- Add a `CedarBuilder` exposing `ordered` and `max_trial` with validated settings and documented defaults.
- Add memory observability for logical entries, used node slots, allocated node capacity, allocated bytes, and load factor. Define every metric in rustdoc and use the same definitions in benchmarks.
- Update crate version and documentation for the breaking 0.6 API, but do not publish it.

### Acceptance criteria

- Public mutation APIs do not panic for caller-controlled empty keys or invalid values.
- Both layouts accept and reject the same documented value range.
- String and byte APIs return identical results for valid UTF-8 keys.
- Iterating all entries after mixed updates and erases reconstructs the reference map exactly.
- `len` changes only when a key is added or removed, not when a value is overwritten or a missing key is erased.
- Builder defaults preserve current behavior.

### Verification

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
cargo test --features reduced-trie
cargo test --doc
cargo doc --no-deps --all-features
```

## Phase 5: Bulk construction and layout/hot-path decisions

### Scope

- Add `Cedar::from_sorted_bytes` and a UTF-8 convenience constructor for strictly sorted unique input.
- Validate sort order, duplicates, empty keys, and values before mutating output. Return typed errors that identify invalid ordering and duplicate keys.
- Implement a direct bulk builder rather than forwarding each item to incremental `update`. Build sibling sets recursively or iteratively, allocate each set together, and preserve all block/free-list invariants required by later incremental updates and erases.
- Add equivalence tests against incremental construction for exact match, prefix search, predictive search, iteration, update-after-build, and erase-after-build.
- Benchmark incremental sorted construction against direct bulk construction for time and memory.
- Benchmark default and `reduced-trie` layouts using Phase 1 workloads. Change the default layout only if the measurements show a consistent win and the migration cost is documented.
- Use Phase 3 safety coverage to evaluate checked versus unchecked indexing in `PrefixIter::next`. Adopt unchecked access only when it produces a measurable improvement and every derived index has a documented invariant. Otherwise keep checked access.
- Evaluate a layout-policy abstraction with a small prototype or written code-level assessment. Adopt it only if it reduces conditional duplication without degrading public ergonomics or measured performance.

### Acceptance criteria

- Bulk construction does not call public or private incremental update once per key.
- Bulk-built tries support later update and erase operations in both layouts.
- Benchmarks and raw results justify the final decisions on the default layout and `PrefixIter` indexing.
- Rejected architectural changes are recorded in the plan's decision log with evidence, so the phase can finish without forcing a speculative abstraction.

### Verification

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
cargo test --features reduced-trie
cargo +nightly-2026-07-10 miri test
cargo +nightly-2026-07-10 miri test --features reduced-trie
cargo bench --bench cedarwood_benchmark -- --test
cargo bench --bench cedarwood_benchmark --features reduced-trie -- --test
```

## Phase 6: Versioned serialization

### Scope

- Add a stable, versioned binary format with a magic header, format version, layout identifier, byte order, lengths, configuration, and all state required for later updates and erases.
- Encode fields explicitly. Do not serialize Rust struct memory or depend on compiler layout.
- Provide stream-based save/load APIs behind the appropriate `std` feature boundary. File-path helpers may wrap the stream APIs.
- Validate all lengths, conversions, indices, block lists, free lists, parent checks, sibling links, sentinel values, and layout compatibility before constructing a queryable `Cedar`. This validation is mandatory because query methods use unchecked indexing.
- Reject truncated, oversized, corrupted, unsupported-version, wrong-layout, and trailing-data inputs with typed errors. Put allocation limits or checked arithmetic in front of attacker-controlled lengths.
- Add deterministic round-trip fixtures, mutation-after-load tests, and corruption tests for both layouts.
- Document compatibility guarantees: readers must reject unknown major format versions, and format evolution must not silently reinterpret a layout.
- Treat memory mapping as a later optimization unless the owned format and validation design can support it without exposing unvalidated bytes to unsafe query paths.

### Acceptance criteria

- A trie round-trips with identical entries, configuration, metrics, and query results.
- A loaded trie supports update and erase.
- Corrupt input cannot reach unchecked query code as a constructed `Cedar`.
- Default-layout and reduced-layout files cannot be confused.
- The format and compatibility policy are documented.

### Verification

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
cargo test --features reduced-trie
cargo +nightly-2026-07-10 miri test
cargo +nightly-2026-07-10 miri test --features reduced-trie
cargo test --doc
```

## Phase 7: CI matrix, portability, and release readiness

### Scope

- Split CI so default and `reduced-trie` configurations are explicit rather than relying only on `--all-features`.
- Add Clippy with `-D warnings`, rustdoc, Miri, and an MSRV job. Declare the MSRV in `Cargo.toml` and README after verifying it.
- Add benchmark regression tracking with CodSpeed or a documented Criterion comparison workflow. Keep PR permissions minimal and do not expose secrets to forked workflows.
- Retain and CI-check the `no_std` plus `alloc` support introduced by the Phase 6 architecture
  review. Keep filesystem and stream serialization under the default `std` feature.
- Add `CHANGELOG.md` covering the 0.6 breaking changes and a short 1.0 roadmap that lists the remaining stability requirements.
- Update `CLAUDE.md`, API documentation, architecture documentation, README examples, benchmark instructions, and feature descriptions to match the implemented code.
- Run the full test, lint, docs, Miri, fuzz-smoke, benchmark-smoke, comparison-harness, and package checks from a clean worktree diff.

### Acceptance criteria

- CI visibly covers both layouts and the declared MSRV.
- Clippy warnings fail CI.
- The README contains reproducible performance results rather than unqualified speed claims.
- `cargo package --list` contains every required source, document, fixture, and generated artifact, while excluding development-only corpora where intended.
- The changelog identifies all breaking API changes and migration steps.
- No release, tag, push, or crates.io publish occurs without a separate user request.

### Verification

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
cargo test --features reduced-trie
cargo test --doc
cargo doc --no-deps --all-features
cargo +nightly-2026-07-10 miri test
cargo +nightly-2026-07-10 miri test --features reduced-trie
cargo install cargo-fuzz --version 0.13.2 --locked
cargo +nightly-2026-07-10 fuzz run stateful_operations -- -runs=10000
cargo +nightly-2026-07-10 fuzz run stateful_operations --features reduced-trie -- -runs=10000
cargo bench --bench cedarwood_benchmark -- --test
cargo bench --bench cedarwood_benchmark --features reduced-trie -- --test
cargo package --list
# Run the checked-in comparison command and confirm README/raw-output consistency.
```

## Decision log

Agents append evidence-backed decisions here as phases complete. Record benchmark environment and raw-result paths for performance decisions. Record any deferred item with its blocker, owner, and the smallest next action.

| Phase | Decision | Evidence |
|---|---|---|
| 0 | Execute phases sequentially so benchmark, safety, API, and persistence work build on a verified baseline. | User-approved implementation order and shared-worktree constraints. |
| 1 | Use Criterion as the sole Cedar benchmark runner, with deterministic build, hit, miss, tokenizer scan, and mutation-churn workloads. Retain the 349,045-key corpus and remove the obsolete macro-benchmark executable. | `benches/cedarwood_benchmark.rs`, `benches/README.md`, and successful default/reduced Criterion smoke runs on 2026-07-10. |
| 2 | Keep comparator dependencies in a standalone exact-pinned crate; publish only one correctness-checked local run using the workload construction shared with Criterion. Treat static/mutable capability differences explicitly and emulate daachorse exact lookup as an anchored full-span match. | `benches/support/mod.rs`, `benches/comparison/`, `benches/comparison/results/2026-07-11-apple-m4-pro.txt`, and the mechanically verified README comparison table. |
| 2 deferred | C++ cedar remains not measured. Blocker: `cedarpp.h` is unavailable locally, and the legacy C++ executable does not implement the shared workloads, correctness checks, memory method, or result schema. Owner: a future comparison phase. Smallest next action: install or vendor a pinned C++ cedar header, then add a shared-schema adapter before recording any C++ number. | `benches/cpp/bench_cedar.cc`, the local header probe recorded in the raw output, and `benches/comparison/README.md`. |
| 3 | Use one bounded Cedar-independent reference-model crate for deterministic proptest and cargo-fuzz coverage, with fixed nonnegative values, a 256-operation fuzz limit, exhaustive query checks after every mutation, and thin local Cedar adapters. Keep the stable 10,000-run fuzz smoke in pull-request CI alongside a two-layout Miri matrix. | `test-support/`, `tests/stateful_operations.rs`, `fuzz/fuzz_targets/stateful_operations.rs`, and `.github/workflows/CI.yml`. |
| 3 fix | Distinguish an unstarted predictive iterator from an exhausted iterator explicitly. Empty-prefix prediction previously returned to `(from, p) == (0, 0)` at exhaustion and restarted forever; empty read coverage exposed the bug. Reduce legacy randomized test sizes only under Miri, while retaining their 1,000-case normal runs and the dedicated mutation model. | `PrefixPredictIter::started` in `src/lib.rs`; default/reduced state-machine tests and Miri runs now terminate and pass. |
| 3 correction | Pin the safety toolchain to Rust `nightly-2026-07-10` and cargo-fuzz `0.13.2`, review the pair quarterly or on relevant upstream advisories, and update CI plus this roadmap together only after the full local two-layout verification. Move shared fuzz/test logic into the declared, non-published `cedarwood-test-support` path crate so neither package imports source across package boundaries and cedarwood exposes no test-only runtime API. Exclude all Phase 3 development-only packages/tests from the published crate; Cargo accepts and strips the path-only dev-dependency without requiring a registry version. | Rust `1.99.0-nightly (af3d95584 2026-07-09)` installed through the dated `nightly-2026-07-10` alias; cargo-fuzz `0.13.2` built with that toolchain; successful default/reduced Miri and 10,000-run fuzz checks; `cargo package --list --allow-dirty`; and successful `cargo package --allow-dirty` verification. |
| 4 | Use one layout-independent stored-key contract: nonempty bytes without `0x00`, because zero is the structural terminal label. Use one value contract, `0..=i32::MAX - 2`, reserving both larger values so default and reduced layouts accept identical inputs. Validate an entire incremental `build` before mutating, return `CedarError` from checked construction/update, and treat unrepresentable lookup/erase keys as misses. | Boundary, atomic-build, UTF-8/byte parity, terminal-byte, and default/reduced tests in `src/lib.rs`; successful Clippy, rustdoc, default/reduced tests, Miri, and 10,000-run fuzz smoke on 2026-07-10. |
| 4 observability | Track logical entries explicitly; expose lazy byte-key reconstruction plus a UTF-8 adapter that returns `FromUtf8Error`. Define used slots as occupied double-array slots including the root, node capacity as the node vector capacity, allocated bytes as the sum of owned vector capacities times element sizes, and load factor as used slots divided by node capacity. Preserve builder defaults (`ordered = true`, `max_trial = 1`) and reject nonpositive trials. | The Phase 3 state model compares `len` and the complete reconstructed entry map after every operation; `benches/comparison` asserts its allocator total equals `Cedar::allocated_bytes()`; both layouts pass Miri and fuzz smoke. |
| 4 compatibility | Keep the checked-in 2026-07-11 comparison result as an explicitly labeled historical 0.5.0 baseline rather than regenerating it during the API phase. Compile the current 0.6 harness without changing the historical raw result or table values. | `README.md`, unchanged `benches/comparison/results/2026-07-11-apple-m4-pro.txt`, and successful `cargo check --manifest-path benches/comparison/Cargo.toml`. |
| 4 review | Keep the new public error and metric types extensible with `#[non_exhaustive]`, remove the raw double-array slot from exact-match results, and traverse child/sibling links for entry iteration so its work follows live trie structure rather than retained array capacity. | Deep architecture review; default/reduced unordered-sibling entry tests; strict Clippy and rustdoc verification. |
| 5 | Add direct `from_sorted`/`from_sorted_bytes` construction by iteratively partitioning validated prefix ranges and allocating each complete sibling set through the existing block/free-list allocator. Preserve semantic equivalence and later update/erase support; aggregate occupancy matched incremental construction on the measured corpus, while slot identity is not promised. | Default/reduced equivalence and mutation tests; 349,045-key smoke; `benches/results/2026-07-10-phase5-apple-m4-pro.md`. Direct construction measured 4.7% faster in default and 10.9% faster in reduced-trie. |
| 5 layout | Retain the default layout. Reduced-trie occupied 19.2% fewer slots but allocated the same bytes after capacity rounding, was slower for exact hits/misses, and only faster in some build/scan/churn workloads. | Full two-layout Criterion results in `benches/results/2026-07-10-phase5-apple-m4-pro.md`. |
| 5 indexing | Keep checked indexing in `PrefixIter::next`. An unchecked candidate produced no statistically significant prefix-scan change in either layout, so expanding the unsafe surface is unjustified. | Default A/B: p=0.21; reduced A/B: p=0.16. Raw confidence ranges and commands are in `benches/results/2026-07-10-phase5-apple-m4-pro.md`. |
| 5 architecture | Defer a layout-policy abstraction. The 32 conditional attributes encode substantial behavioral differences, not just base encoding; a private policy would move rather than remove most branches, while a public generic would degrade ergonomics without measured benefit. | Code-level assessment in `benches/results/2026-07-10-phase5-apple-m4-pro.md`; no speculative abstraction added. |
| 5 review | Route direct construction through `CedarBuilder` as well as `Cedar` so later mutations retain configured `ordered` and `max_trial` behavior. | Deep architecture review and configured post-build mutation test in both layouts. |
| 6 | Use an owned version 1.0 format with an 88-byte magic/version/layout/configuration header and explicitly little-endian node, node-info, block, and reject records. Apply a configurable 512 MiB default peak-load ceiling before allocation, and validate allocator lists plus the complete reachable trie before exposing unchecked queries. Reject newer versions, wrong layouts, truncation, corruption, and trailing bytes with `CedarPersistenceError`. Defer mmap until a separate representation can preserve validation and ownership boundaries. | Default/reduced deterministic round-trip, mutation-after-load, header/limit/truncation/trailing-data, and allocator/trie corruption tests in `src/lib.rs`; byte-level contract and compatibility policy in `docs/serialization.md`. |
| 6 review | Remove attacker-controlled spare-capacity fields from the unpublished v1 format; reserve only validated logical lengths with `try_reserve_exact`, account for DTO/live/validation peak memory, and return typed allocation failure. Separate `persistence::v1` wire DTOs from live trie structs, reject reachable empty structural nodes, and document `save_to_path` as non-atomic. Preserve logical occupancy metrics while allowing allocator-dependent `allocated_bytes` to change after load. Leave only a mechanical `std` feature gate for Phase 7. | Security/architecture review findings; targeted empty-structural-node corruption regression; updated 88-byte format contract and std-boundary notes in `docs/serialization.md`. |
| 6 std boundary | Pull the `no_std + alloc` boundary forward rather than leaving unconditional `std` dependencies for Phase 7. Enable `std` by default, keep `reduced-trie` orthogonal, and gate the persistence DTO/codec, APIs, errors, and standard error implementations together. | Successful no-default-features checks and 25-test library runs in both layouts; default/reduced std suites and rustdoc remain green. |
| 7 MSRV | Declare Rust 1.62.0 and retain Cargo.lock format v3. An isolated consumer compiled the normal dependency graph with exact Rust/Cargo 1.62.0 in all four `std`/`reduced-trie` combinations; Rust 1.61.0 failed at the two `bool::then_some` calls, establishing the source floor. | Exact `cargo +1.62.0 check` consumer probes for all four combinations and the corresponding 1.61.0 negative probe on 2026-07-10; `Cargo.toml` and the CI MSRV matrix. |
| 7 CI | Use explicit four-way stable test and strict-Clippy matrices, two-way rustdoc, exact four-way MSRV checks, dated two-layout Miri, bounded two-layout fuzzing, two-layout benchmark smoke, comparison-table verification, and package-content inspection. Keep workflow permissions read-only and run tokened coverage only on pushes to the canonical repository. | `.github/workflows/CI.yml`; local Phase 7 verification commands. |
| 7 benchmark tracking | Keep regression decisions local and same-machine through Criterion saved baselines; CI compiles and executes every workload but does not interpret noisy shared-runner timings. Refresh the comparison table with a correctness-checked 0.6 run while preserving the historical 0.5 raw output separately. | `benches/README.md`, `benches/comparison/results/2026-07-11-apple-m4-pro-cedarwood-0.6.0.txt`, and the mechanical README verifier. |
| 7 regression fix | Remove the eager whole-query NUL scan from common-prefix iteration. Tokenizer workloads call the iterator once per suffix, so that validation made a nominally linear scan quadratic and reduced measured throughput to 3.72 MiB/s. Stop at a NUL during traversal instead, allowing already-completed valid prefixes; exact, predictive, and erase NUL queries remain misses. | Before/final 0.6 comparison runs on the same Apple M4 Pro: 3.72 MiB/s before and 246.76 MiB/s after; default/reduced NUL regression tests and current raw output. |
| 7 release readiness | Add complete 0.6 migration notes and explicit 1.0 stabilization gates. Keep persistence under `std`, retain `no_std + alloc`, and resolve the yanked `crossbeam-channel 0.5.6` lock entry by updating to 0.5.15 without changing public dependencies. | `CHANGELOG.md`, `docs/roadmap-to-1.0.md`, synchronized public docs, and `Cargo.lock`. |
| 7 review | Make the current-result verifier bind README rows to the current benchmark contract, not merely to a self-consistent stale raw file. The comparison manifest explicitly disables inherited defaults and enables only `std`; recording and verification validate that non-reduced layout contract and share package-version, root-manifest/source hashes, comparison-manifest/harness hashes, canonical input paths, and remaining hash computation. The historical 0.5 file remains exempt. | Comparison verifier unit tests, including stale harness, root-manifest, and comparison-manifest hash cases; successful current 0.6 provenance and README verification. |
