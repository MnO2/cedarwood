mod support;

use cedarwood::Cedar;
use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use support::{
    entries, evenly_sampled_keys, exact_misses, load_dictionary, tokenizer_text, CHURN_WORKING_SET_SIZE,
    LOOKUP_SAMPLE_SIZE, SCAN_TEXT_MIN_BYTES,
};

fn benchmark_build(c: &mut Criterion, corpus_entries: &[(&str, i32)]) {
    let mut incremental = Cedar::new();
    incremental.build(corpus_entries).unwrap();
    let direct = Cedar::from_sorted(corpus_entries).unwrap();
    assert_eq!(incremental.len(), direct.len());
    eprintln!(
        "phase5_build_memory,layout={},method=incremental_sorted,entries={},used_node_slots={},node_capacity={},allocated_bytes={},load_factor={:.6}",
        if cfg!(feature = "reduced-trie") {
            "reduced-trie"
        } else {
            "default"
        },
        incremental.len(),
        incremental.used_node_slots(),
        incremental.node_capacity(),
        incremental.allocated_bytes(),
        incremental.load_factor()
    );
    eprintln!(
        "phase5_build_memory,layout={},method=direct_sorted,entries={},used_node_slots={},node_capacity={},allocated_bytes={},load_factor={:.6}",
        if cfg!(feature = "reduced-trie") {
            "reduced-trie"
        } else {
            "default"
        },
        direct.len(),
        direct.used_node_slots(),
        direct.node_capacity(),
        direct.allocated_bytes(),
        direct.load_factor()
    );

    let mut group = c.benchmark_group("build");
    // Full-corpus construction is intentionally expensive; ten samples still exercise the
    // complete 349k-key workload while keeping local and CI smoke runs practical.
    group.sample_size(10);
    group.throughput(Throughput::Elements(corpus_entries.len() as u64));
    group.bench_function("incremental_sorted", |b| {
        b.iter(|| {
            let mut cedar = Cedar::new();
            cedar.build(black_box(corpus_entries)).unwrap();
            black_box(cedar)
        });
    });
    group.bench_function("direct_sorted", |b| {
        b.iter(|| black_box(Cedar::from_sorted(black_box(corpus_entries)).unwrap()));
    });
    group.finish();
}

fn benchmark_exact_hits(c: &mut Criterion, cedar: &Cedar, hit_keys: &[&str]) {
    let mut group = c.benchmark_group("exact_match");
    group.throughput(Throughput::Elements(hit_keys.len() as u64));
    group.bench_function("hit_sample", |b| {
        b.iter(|| {
            let checksum = hit_keys.iter().fold(0_i64, |checksum, key| {
                let value = cedar
                    .exact_match_search(black_box(key))
                    .expect("sampled corpus key must be present")
                    .0;
                checksum.wrapping_add(i64::from(value))
            });
            black_box(checksum)
        });
    });
    group.finish();
}

fn benchmark_exact_misses(c: &mut Criterion, cedar: &Cedar, miss_keys: &[String]) {
    assert!(miss_keys.iter().all(|key| cedar.exact_match_search(key).is_none()));

    let mut group = c.benchmark_group("exact_match");
    group.throughput(Throughput::Elements(miss_keys.len() as u64));
    group.bench_function("realistic_prefix_miss_sample", |b| {
        b.iter(|| {
            let misses = miss_keys
                .iter()
                .filter(|key| cedar.exact_match_search(black_box(key)).is_none())
                .count();
            black_box(misses)
        });
    });
    group.finish();
}

fn benchmark_tokenizer_scan(c: &mut Criterion, cedar: &Cedar, text: &str) {
    let character_offsets: Vec<usize> = text.char_indices().map(|(offset, _)| offset).collect();

    let mut group = c.benchmark_group("prefix_scan");
    group.throughput(Throughput::Bytes(text.len() as u64));
    group.bench_function("tokenizer_text", |b| {
        b.iter(|| {
            let checksum = character_offsets.iter().fold(0_i64, |checksum, offset| {
                cedar
                    .common_prefix_iter(black_box(&text[*offset..]))
                    .fold(checksum, |checksum, (value, length)| {
                        checksum.wrapping_add(i64::from(value)).wrapping_add(length as i64)
                    })
            });
            black_box(checksum)
        });
    });
    group.finish();
}

fn benchmark_churn(c: &mut Criterion, working_entries: &[(&str, i32)]) {
    let mut cedar = Cedar::new();
    cedar.build(working_entries).unwrap();

    let mut group = c.benchmark_group("churn");
    group.throughput(Throughput::Elements((working_entries.len() * 3) as u64));
    group.bench_function("update_erase_reinsert_working_set", |b| {
        b.iter(|| {
            for (key, value) in working_entries {
                cedar.update(black_box(key), black_box(value + 1)).unwrap();
                black_box(cedar.erase(black_box(key)));
                cedar.update(black_box(key), black_box(*value)).unwrap();
            }
            black_box(working_entries.len() * 3)
        });
    });
    group.finish();
}

fn criterion_benchmark(c: &mut Criterion) {
    let corpus = load_dictionary();
    let corpus_entries = entries(&corpus);
    let hit_keys = evenly_sampled_keys(&corpus, LOOKUP_SAMPLE_SIZE);
    let miss_keys = exact_misses(&hit_keys);
    let scan_text = tokenizer_text(&hit_keys, SCAN_TEXT_MIN_BYTES);
    let churn_keys = evenly_sampled_keys(&corpus, CHURN_WORKING_SET_SIZE);
    let churn_entries = entries(&churn_keys);

    benchmark_build(c, &corpus_entries);

    let mut cedar = Cedar::new();
    cedar.build(&corpus_entries).unwrap();
    benchmark_exact_hits(c, &cedar, &hit_keys);
    benchmark_exact_misses(c, &cedar, &miss_keys);
    benchmark_tokenizer_scan(c, &cedar, &scan_text);
    benchmark_churn(c, &churn_entries);
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
