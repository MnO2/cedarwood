use cedarwood::Cedar;
use cedarwood_test_support::{run_operations, Operation, TrieAdapter, KEYS, QUERIES};
use proptest::prelude::*;
use proptest::test_runner::{Config, RngSeed};
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

fn operation_strategy() -> impl Strategy<Value = Operation> {
    (0_u8..6, any::<u8>(), 0_u16..=u16::MAX).prop_map(|(opcode, selector, value)| {
        let selector = usize::from(selector);
        let value = i32::from(value);
        match opcode {
            0 => Operation::Update {
                key: KEYS[selector % KEYS.len()],
                value,
            },
            1 => Operation::Overwrite {
                key: KEYS[selector % KEYS.len()],
                value,
            },
            2 => Operation::Erase {
                key: if selector % (KEYS.len() + 1) == KEYS.len() {
                    ""
                } else {
                    KEYS[selector % KEYS.len()]
                },
            },
            3 => Operation::Exact {
                query: QUERIES[selector % QUERIES.len()],
            },
            4 => Operation::Prefix {
                query: QUERIES[selector % QUERIES.len()],
            },
            _ => Operation::Predict {
                query: QUERIES[selector % QUERIES.len()],
            },
        }
    })
}

#[test]
fn deterministic_mutation_regression_sequence() {
    let long_key = KEYS[KEYS.len() - 1];
    let operations = vec![
        Operation::Update { key: "a", value: 1 },
        Operation::Update { key: "ab", value: 2 },
        Operation::Update { key: "abc", value: 3 },
        Operation::Update {
            key: "中华", value: 4
        },
        Operation::Update {
            key: "データ",
            value: 5,
        },
        Operation::Update {
            key: "e\u{301}",
            value: 6,
        },
        Operation::Update {
            key: long_key,
            value: 7,
        },
        Operation::Overwrite { key: "ab", value: 8 },
        Operation::Erase { key: "" },
        Operation::Exact { query: "" },
        Operation::Prefix { query: "" },
        Operation::Predict { query: "" },
        Operation::Erase { key: "ab" },
        Operation::Erase { key: "ab" },
        Operation::Update { key: "ab", value: 9 },
        Operation::Erase { key: "a" },
        Operation::Update { key: "a", value: 10 },
        Operation::Erase { key: long_key },
        Operation::Update {
            key: long_key,
            value: 11,
        },
    ];

    run_operations::<CedarAdapter>(&operations).unwrap();
}

proptest! {
    #![proptest_config(Config {
        cases: if cfg!(miri) { 8 } else { 128 },
        failure_persistence: None,
        max_shrink_iters: 4_096,
        rng_seed: RngSeed::Fixed(0x5eed_ceda_2026_0710),
        ..Config::default()
    })]

    #[test]
    fn stateful_operations_match_reference_model(
        operations in prop::collection::vec(
            operation_strategy(),
            1..if cfg!(miri) { 32 } else { 128 },
        )
    ) {
        let result = run_operations::<CedarAdapter>(&operations);
        prop_assert!(result.is_ok(), "{result:?}");
    }
}
