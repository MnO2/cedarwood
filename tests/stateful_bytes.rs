use cedarwood::{Cedar, MAX_VALUE};
use proptest::prelude::*;
use proptest::test_runner::{Config, RngSeed};
use std::collections::BTreeMap;

fn verify_queries(cedar: &Cedar, reference: &BTreeMap<Vec<u8>, i32>, query: &[u8]) {
    assert_eq!(
        cedar.exact_match_search_bytes(query),
        reference.get(query).map(|value| (*value, query.len()))
    );
    let expected_prefixes: Vec<_> = reference
        .iter()
        .filter(|(key, _)| query.starts_with(key))
        .map(|(key, value)| (*value, key.len() - 1))
        .collect();
    assert_eq!(cedar.common_prefix_search_bytes(query), expected_prefixes);

    let mut expected_predictions: Vec<_> = reference
        .iter()
        .filter(|(key, _)| key.starts_with(query))
        .map(|(key, value)| (*value, key.len() - query.len()))
        .collect();
    let mut actual_predictions = cedar.common_prefix_predict_bytes(query);
    expected_predictions.sort_unstable();
    actual_predictions.sort_unstable();
    assert_eq!(actual_predictions, expected_predictions);
}

proptest! {
    #![proptest_config(Config {
        cases: if cfg!(miri) { 2 } else { 128 },
        failure_persistence: None,
        rng_seed: RngSeed::Fixed(0xceda_0601),
        ..Config::default()
    })]

    #[test]
    fn arbitrary_byte_mutations_match_reference(
        keys in prop::collection::vec(prop::collection::vec(1_u8..=255, 1..24), 1..32),
        operations in prop::collection::vec(
            (0_u8..4, any::<u8>(), prop_oneof![Just(0), Just(MAX_VALUE), 0..=MAX_VALUE]),
            1..if cfg!(miri) { 12 } else { 128 },
        ),
        ordered in any::<bool>(),
        max_trial in 1_i32..5,
        sorted in any::<bool>(),
    ) {
        // Seed both constructors, including keys that end inside another key's path.
        let mut reference = BTreeMap::new();
        for key in &keys {
            reference.insert(key.clone(), 0);
            reference.insert(key[..1].to_vec(), MAX_VALUE);
        }
        let initial: Vec<_> = reference.iter().map(|(key, value)| (key.as_slice(), *value)).collect();
        let builder = Cedar::builder().ordered(ordered).max_trial(max_trial).unwrap();
        let mut cedar = if sorted {
            builder.from_sorted_bytes(&initial).unwrap()
        } else {
            let mut cedar = builder.build();
            cedar.build_bytes(&initial).unwrap();
            cedar
        };

        for (step, (opcode, selector, value)) in operations.into_iter().enumerate() {
            let full_key = &keys[usize::from(selector) % keys.len()];
            let key = if opcode % 2 == 0 { full_key.as_slice() } else { &full_key[..1] };
            if opcode < 2 {
                cedar.update_bytes(key, value).unwrap();
                reference.insert(key.to_vec(), value);
            } else {
                assert_eq!(cedar.erase_bytes(key), reference.remove(key).is_some());
            }

            // Loading must restore a state that can accept the subsequent operations.
            #[cfg(feature = "std")]
            if step % 16 == 0 {
                let mut bytes = Vec::new();
                cedar.save_to_writer(&mut bytes).unwrap();
                cedar = Cedar::load_from_reader(bytes.as_slice()).unwrap();
            }
            #[cfg(not(feature = "std"))]
            let _ = step;

            assert_eq!(cedar.len(), reference.len());
            assert_eq!(cedar.is_empty(), reference.is_empty());
            assert_eq!(cedar.entries().collect::<BTreeMap<_, _>>(), reference);
            verify_queries(&cedar, &reference, full_key);
            verify_queries(&cedar, &reference, &full_key[..1]);
            verify_queries(&cedar, &reference, &[]);
        }
    }
}
