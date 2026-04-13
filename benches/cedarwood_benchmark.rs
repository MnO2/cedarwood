use cedarwood::Cedar;
use criterion::{black_box, criterion_group, criterion_main, Criterion};

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

fn criterion_benchmark(c: &mut Criterion) {
    c.bench_function("cedar build", |b| b.iter(|| black_box(build_cedar())));

    c.bench_function("cedar exact_match_search", |b| {
        let cedar = build_cedar();
        b.iter(|| black_box(cedar.exact_match_search(black_box("中华人民"))))
    });

    c.bench_function("cedar common_prefix_search", |b| {
        let cedar = build_cedar();
        b.iter(|| black_box(cedar.common_prefix_search(black_box("中华人民"))))
    });

    c.bench_function("cedar common_prefix_search (iter)", |b| {
        let cedar = build_cedar();
        b.iter(|| {
            let mut count = 0i32;
            for r in cedar.common_prefix_iter(black_box("中华人民")) {
                count += r.0;
            }
            black_box(count)
        })
    });

    c.bench_function("cedar common_prefix_predict", |b| {
        let cedar = build_cedar();
        b.iter(|| black_box(cedar.common_prefix_predict(black_box("中"))))
    });

    c.bench_function("cedar common_prefix_predict (iter)", |b| {
        let cedar = build_cedar();
        b.iter(|| {
            let mut count = 0i32;
            for r in cedar.common_prefix_predict_iter(black_box("中")) {
                count += r.0;
            }
            black_box(count)
        })
    });
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
