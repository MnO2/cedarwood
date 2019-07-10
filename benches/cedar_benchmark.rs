#[macro_use]
extern crate criterion;
extern crate cedar;

use cedar::Cedar;
use criterion::Criterion;

fn bench_cedar_build() {
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
}

fn criterion_benchmark(c: &mut Criterion) {
    c.bench_function("cedar build", |b| b.iter(|| bench_cedar_build()));
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
