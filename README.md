# cedarwood

Efficiently-updatable double-array trie in Rust (ported from cedar). This library is still in alpha, feedbacks are welcomed. 

[![Build Status](https://travis-ci.com/MnO2/cedarwood.svg?branch=master)](https://travis-ci.org/MnO2/cedarwood)
[![codecov](https://codecov.io/gh/MnO2/cedarwood/branch/master/graph/badge.svg)](https://codecov.io/gh/MnO2/cedarwood)
[![Crates.io](https://img.shields.io/crates/v/cedarwood.svg)](https://crates.io/crates/cedarwood)
[![docs.rs](https://docs.rs/cedarwood/badge.svg)](https://docs.rs/cedarwood/)

## Installation

Add it to your `Cargo.toml`:

```toml
[dependencies]
cedarwood = "0.2"
```

then you are good to go. If you are using Rust 2015 you have to `extern crate cedarwood` to your crate root as well.

## Example

```rust
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

let result: Vec<i32> = cedar.common_prefix_search("abcdefg").iter().map(|x| x.0).collect();
assert_eq!(vec![0, 1, 2], result);

let result: Vec<i32> = cedar
    .common_prefix_search("网球拍卖会")
    .iter()
    .map(|x| x.0)
    .collect();
assert_eq!(vec![6, 7, 8], result);

let result: Vec<i32> = cedar
    .common_prefix_search("中华人民共和国")
    .iter()
    .map(|x| x.0)
    .collect();
assert_eq!(vec![9, 10, 11, 12], result);

let result: Vec<i32> = cedar
    .common_prefix_search("データ構造とアルゴリズム")
    .iter()
    .map(|x| x.0)
    .collect();
assert_eq!(vec![4], result);
```

## To run benchmark tests

```bash
cargo bench 
```

## License

This work is released under the BSD-2 license, following the original license of C++ cedar. A copy of the license is provided in the LICENSE file.

## Reference

* [cedar - C++ implementation of efficiently-updatable double-array trie](http://www.tkl.iis.u-tokyo.ac.jp/~ynaga/cedar/)


