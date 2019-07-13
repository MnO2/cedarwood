#[macro_use]
extern crate criterion;
extern crate cedarwood;

use cedarwood::Cedar;
use criterion::Criterion;

fn build_cedar() -> Cedar {
    let dict = vec![
        "a",
        "ab",
        "abc",
        "アルゴリズム",
        "データ",
        "構造",
        "网",
        "网球",
        "网球拍",
        "中",
        "中华",
        "中华人民",
        "中华人民共和国",
    ];
    let key_values: Vec<(&str, i32)> = dict.into_iter().enumerate().map(|(k, s)| (s, k as i32)).collect();
    let mut cedar = Cedar::new();
    cedar.build(&key_values);
    cedar
}

fn bench_cedar_build() {
    let _cedar = build_cedar();
}

fn bench_exact_match_search() {
    let cedar = build_cedar();
    let _ret = cedar.exact_match_search("中华人民");
}

fn bench_common_prefix_search() {
    let cedar = build_cedar();
    let _ret = cedar.common_prefix_search("中华人民");
}

fn bench_common_prefix_predict() {
    let cedar = build_cedar();
    let _ret = cedar.common_prefix_predict("中");
}

fn criterion_benchmark(c: &mut Criterion) {
    c.bench_function("cedar build", |b| b.iter(bench_cedar_build));
    c.bench_function("cedar exact_match_search", |b| b.iter(bench_exact_match_search));
    c.bench_function("cedar common_prefix_search", |b| b.iter(bench_common_prefix_search));
    c.bench_function("cedar common_prefix_predict", |b| b.iter(bench_common_prefix_predict));
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
