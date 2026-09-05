# Buffered persistence I/O comparison

Measured on 2026-09-05 with macOS arm64, Apple M4 Pro, and
`rustc 1.97.0 (2d8144b78 2026-07-07)`, using the default trie layout and a release build.
The base commit was `36149ad9ca5e6f2f4846781f49ef3bacc90f3511` with the remaining audit changes applied.
SHA-256 of the measured sources:

```text
5f3c9ab37acfdc39bbec388e0d412fb1004f512d71521cc3202225c1e746af15  src/lib.rs
223ec994daf9f9ebcbab83268f8917d48411a9abb180b0a18f9ec3a48bc7a69a  benches/tools/persistence_io.rs
```

## Reproduce

From the repository root:

```bash
set -e
benchmark_dir="$(mktemp -d)"
cargo build --release --locked
rustc --edition=2021 -O benches/tools/persistence_io.rs \
  --extern cedarwood=target/release/libcedarwood.rlib \
  -L dependency=target/release/deps \
  -o "$benchmark_dir/persistence-io"
"$benchmark_dir/persistence-io"
rustc --edition=2021 -O --test benches/tools/persistence_io.rs \
  --extern cedarwood=target/release/libcedarwood.rlib \
  -L dependency=target/release/deps \
  -o "$benchmark_dir/persistence-io-tests"
"$benchmark_dir/persistence-io-tests"
rm "$benchmark_dir/persistence-io" "$benchmark_dir/persistence-io-tests"
rmdir "$benchmark_dir"
```

The [probe](../tools/persistence_io.rs) builds 10,000 sorted keys, from `key-00000000` through
`key-00009999`, once before timing. Each round compares raw `File` stream I/O, which reproduces
the former path helpers, with the current buffered `save_to_path` and `load_from_path` helpers.
The implementation order alternates between rounds. Both use the same trie and the same
330,842-byte representation; every save is checked for byte equality and every load for complete
entry equality outside the timed sections.

The timings include file creation/opening, encoding or decoding, loading validation, and the
buffered writer's explicit flush. They use the operating system's file cache and do not include
`sync_all`; these are local I/O measurements, not durable-storage throughput. Three short rounds
demonstrate the cost of small unbuffered reads/writes, rather than establishing a portable speedup
or a CI threshold. Before timing, the probe exclusively creates its own temporary directory,
uses mode `0700` on Unix, and retries name collisions without reusing existing files or symlinks.
It removes only its own data file and directory after verification. The standalone tests check
collision handling, preservation of existing files/symlinks, and Unix directory permissions.

## Recorded output

```text
entries=10000 file_bytes=330842
round=0 mode=raw save_ms=87.887 load_ms=40.841
round=0 mode=buffered save_ms=0.448 load_ms=0.432
round=1 mode=buffered save_ms=0.373 load_ms=0.440
round=1 mode=raw save_ms=86.359 load_ms=42.066
round=2 mode=raw save_ms=86.135 load_ms=42.726
round=2 mode=buffered save_ms=0.514 load_ms=0.415
```

| Operation | Raw median | Buffered median | Local ratio |
|---|---:|---:|---:|
| Save | 86.359 ms | 0.448 ms | 192.8× |
| Load and validate | 42.066 ms | 0.432 ms | 97.4× |

The wire format and validation work are unchanged. The path helpers now group the codec's small
field operations through `BufWriter` and `BufReader`; saving explicitly flushes the writer so an
I/O failure is returned to the caller.
