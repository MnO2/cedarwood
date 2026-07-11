#![no_main]

use cedarwood::Cedar;
use cedarwood_test_support::{decode_operations, run_operations, TrieAdapter};
use libfuzzer_sys::fuzz_target;
use std::collections::BTreeMap;

#[derive(Default)]
struct CedarAdapter(Cedar);

impl TrieAdapter for CedarAdapter {
    fn update(&mut self, key: &str, value: i32) {
        self.0.update(key, value).unwrap();
    }

    fn erase(&mut self, key: &str) {
        self.0.erase(key);
    }

    fn exact(&self, query: &str) -> Option<i32> {
        self.0.exact_match_search(query).map(|result| result.0)
    }

    fn prefixes(&self, query: &str) -> Vec<(i32, usize)> {
        self.0.common_prefix_search(query)
    }

    fn predictions(&self, query: &str) -> Vec<(i32, usize)> {
        self.0.common_prefix_predict(query)
    }

    fn len(&self) -> usize {
        self.0.len()
    }

    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    fn entries(&self) -> BTreeMap<Vec<u8>, i32> {
        self.0.entries().collect()
    }
}

fuzz_target!(|data: &[u8]| {
    let operations = decode_operations(data);
    if let Err(error) = run_operations::<CedarAdapter>(&operations) {
        panic!("{error}");
    }
});
