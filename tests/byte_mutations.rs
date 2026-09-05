use cedarwood::{Cedar, MAX_VALUE};
use rand::{rngs::StdRng, Rng, SeedableRng};
use std::collections::BTreeMap;

fn check_contents(cedar: &Cedar, expected: &BTreeMap<Vec<u8>, i32>) {
    assert_eq!(cedar.len(), expected.len());
    let mut entries = cedar.entries();
    assert_eq!(entries.len(), expected.len());
    assert_eq!(entries.by_ref().collect::<BTreeMap<_, _>>(), *expected);
    assert_eq!(entries.len(), 0);
    assert_eq!(entries.next(), None);
    for (key, value) in expected {
        assert_eq!(cedar.exact_match_search_bytes(key), Some((*value, key.len())));
    }
    #[cfg(feature = "std")]
    {
        // The loader checks the allocator and trie graph as well as preserving all entries.
        let mut bytes = Vec::new();
        cedar.save_to_writer(&mut bytes).unwrap();
        let restored = Cedar::load_from_reader(bytes.as_slice()).unwrap();
        assert_eq!(restored.entries().collect::<BTreeMap<_, _>>(), *expected);
    }
}

#[test]
fn fully_occupied_root_block_remains_mutable() {
    let erased_labels: &[u8] = if cfg!(miri) { &[1] } else { &[1, 128, 255] };
    for bulk in [false, true] {
        for &erased_label in erased_labels {
            let mut expected: BTreeMap<_, _> = (1..=255).map(|label| (vec![label], i32::from(label) - 1)).collect();
            let entries: Vec<_> = expected.iter().map(|(key, value)| (key.as_slice(), *value)).collect();
            let mut cedar = if bulk {
                Cedar::from_sorted_bytes(&entries).unwrap()
            } else {
                let mut cedar = Cedar::new();
                cedar.build_bytes(&entries).unwrap();
                cedar
            };
            check_contents(&cedar, &expected);
            #[cfg(feature = "std")]
            {
                // A full root's saved e_head is stale; loading must still permit its first erase.
                let mut bytes = Vec::new();
                cedar.save_to_writer(&mut bytes).unwrap();
                cedar = Cedar::load_from_reader(bytes.as_slice()).unwrap();
            }
            assert!(cedar.erase_bytes(&[erased_label]));
            expected.remove(&vec![erased_label]);
            check_contents(&cedar, &expected);
            cedar.update_bytes(&[erased_label], MAX_VALUE).unwrap();
            expected.insert(vec![erased_label], MAX_VALUE);
            check_contents(&cedar, &expected);
            for label in 1..=255 {
                assert!(cedar.erase_bytes(&[label]));
            }
            expected.clear();
            check_contents(&cedar, &expected);
            assert_eq!(cedar.used_node_slots(), 1);
            cedar.update_bytes(&[255], 500).unwrap();
            assert_eq!(cedar.entries().collect::<Vec<_>>(), vec![(vec![255], 500)]);
        }
    }
}

#[test]
fn byte_mutations_match_reference_after_bulk_build_and_relocations() {
    let mut rng = StdRng::seed_from_u64(0x2026_0905_ceda_0001);
    let mut keys: Vec<_> = (1..=if cfg!(miri) { 16 } else { 255 })
        .map(|label| vec![label])
        .collect();
    for _ in 0..if cfg!(miri) { 32 } else { 768 } {
        let length = rng.gen_range(2..=16);
        let mut key: Vec<_> = (0..length).map(|_| rng.gen_range(1..=255)).collect();
        // Exercise dense siblings and deep shared prefixes in the same allocator.
        if rng.gen_bool(0.5) {
            key[0] = b'a';
        }
        if length > 4 && rng.gen_bool(0.5) {
            key[..4].copy_from_slice(b"aaaa");
        }
        keys.push(key);
    }
    keys.sort_unstable();
    keys.dedup();

    for ordered in [false, true] {
        for max_trial in [1, 3] {
            for bulk in [false, true] {
                let mut expected: BTreeMap<_, _> = keys.iter().step_by(2).cloned().map(|key| (key, 0)).collect();
                let entries: Vec<_> = expected.iter().map(|(key, value)| (key.as_slice(), *value)).collect();
                let builder = Cedar::builder().ordered(ordered).max_trial(max_trial).unwrap();
                let mut cedar = if bulk {
                    builder.from_sorted_bytes(&entries).unwrap()
                } else {
                    let mut cedar = builder.build();
                    cedar.build_bytes(&entries).unwrap();
                    cedar
                };
                for step in 0..if cfg!(miri) { 16 } else { 1536 } {
                    let key = &keys[rng.gen_range(0..keys.len())];
                    if rng.gen_bool(0.4) {
                        assert_eq!(cedar.erase_bytes(key), expected.remove(key).is_some());
                    } else {
                        let value = if step % 11 == 0 { MAX_VALUE } else { step };
                        cedar.update_bytes(key, value).unwrap();
                        expected.insert(key.clone(), value);
                    }
                    if step % 128 == 0 {
                        check_contents(&cedar, &expected);
                        for query in [b"".as_slice(), b"a".as_slice(), b"aaaa".as_slice(), key.as_slice()] {
                            let mut predicted = cedar.common_prefix_predict_bytes(query);
                            let mut expected_predictions: Vec<_> = expected
                                .iter()
                                .filter(|(key, _)| key.starts_with(query))
                                .map(|(key, value)| (*value, key.len() - query.len()))
                                .collect();
                            predicted.sort_unstable();
                            expected_predictions.sort_unstable();
                            assert_eq!(predicted, expected_predictions);
                            let mut prefixes = cedar.common_prefix_search_bytes(query);
                            let mut expected_prefixes: Vec<_> = expected
                                .iter()
                                .filter(|(key, _)| query.starts_with(key))
                                .map(|(key, value)| (*value, key.len() - 1))
                                .collect();
                            prefixes.sort_unstable();
                            expected_prefixes.sort_unstable();
                            assert_eq!(prefixes, expected_prefixes);
                        }
                    }
                }
                check_contents(&cedar, &expected);
            }
        }
    }
}

#[test]
fn entries_restore_shared_prefixes_when_backtracking_from_deep_branches() {
    let depth = if cfg!(miri) { 256 } else { 32 * 1024 };
    let mut expected = BTreeMap::new();
    for length in [1, 2, 64, depth / 2, depth] {
        let prefix = vec![b'a'; length];
        expected.insert(prefix.clone(), length as i32);
        for label in [1, 127, 255] {
            let mut key = prefix.clone();
            key.push(label);
            expected.insert(key, i32::from(label));
        }
    }
    for ordered in [false, true] {
        let mut cedar = Cedar::builder().ordered(ordered).build();
        for (key, value) in expected.iter().rev() {
            cedar.update_bytes(key, *value).unwrap();
        }
        check_contents(&cedar, &expected);
        // Internal terminals must not prevent traversal to deeper branches or later siblings.
        let erased = vec![b'a'; depth / 2];
        assert!(cedar.erase_bytes(&erased));
        let mut after_erase = expected.clone();
        after_erase.remove(&erased);
        check_contents(&cedar, &after_erase);
    }
}
