# Roadmap to Cedarwood 1.0

Cedarwood 0.6 establishes the intended safety, portability, performance, and persistence
foundations. A 1.0 release should follow evidence from real downstream use rather than another
large speculative redesign.

## Stabilization gates

- **API bake time:** collect migration feedback from 0.6 users, especially byte-key users,
  tokenizer integrations, direct construction, entry iteration, and builder tuning. Resolve any
  naming or ownership issues before declaring the API stable.
- **Persistence compatibility:** keep the documented v1 reader/writer contract covered by fixtures
  across releases. Demonstrate that a later reader accepts valid 0.6 files and rejects unsupported
  major versions and layouts without exposing unvalidated data to query code.
- **Safety:** retain two-layout state-machine, Miri, and fuzz coverage for every change touching
  node indexing, relocation, erase, deserialization, or allocator lists. Complete a focused unsafe
  code audit before 1.0.
- **Performance:** preserve reproducible Criterion baselines and the checked-in comparison schema.
  Investigate material regressions before release, while treating machine-specific measurements
  as comparative evidence rather than universal claims.
- **Compatibility policy:** publish the supported Rust-version policy and test the declared MSRV,
  default layout, `reduced-trie`, `std`, and `no_std + alloc` combinations continuously.
- **Downstream validation:** test release candidates against important consumers and document any
  migration constraints not captured by the crate's own suite.
- **Release discipline:** keep the changelog, version, rustdoc, README examples, binary-format
  documentation, package contents, and benchmark evidence synchronized for each release.

## Deferred investigations

- Add the C++ cedar implementation to the shared comparison schema once a pinned `cedarpp.h` and a
  correctness-checked adapter are available.
- Evaluate memory-mapped or borrowed persistence only with a representation that preserves the
  current validation boundary; raw mapped bytes must never feed unchecked queries directly.
- Revisit a layout-policy abstraction only if it removes meaningful conditional duplication with
  no measurable performance or ergonomics cost.
- Revisit the default layout only when representative measurements show a consistent improvement,
  including allocated bytes rather than occupied-slot counts alone.

## Not required for 1.0

CodSpeed, memory mapping, a runtime-selectable layout, and a C++ result are useful enhancements but
are not release blockers. Correctness, a stable public contract, safe persistence, reproducible
evidence, and downstream confidence are the gates.
