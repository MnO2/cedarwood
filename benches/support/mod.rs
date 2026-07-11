use std::collections::HashSet;

pub const DICTIONARY_SOURCE: &str = include_str!("../macro-benchmark/dict.txt");
pub const LOOKUP_SAMPLE_SIZE: usize = 4_096;
#[allow(dead_code)] // The standalone comparison has no mutable churn workload.
pub const CHURN_WORKING_SET_SIZE: usize = 2_048;
pub const SCAN_TEXT_MIN_BYTES: usize = 64 * 1_024;
const MINIMUM_CORPUS_KEYS: usize = 300_000;
const TOKENIZER_STRIDE: usize = 7_919;

/// Loads the first whitespace-delimited field from every nonempty corpus line.
///
/// Duplicate keys are removed and the remaining keys are sorted by UTF-8 byte order. Both the
/// Criterion suite and the standalone comparison harness use this ordering and assign values by
/// the resulting key position.
pub fn load_dictionary() -> Vec<&'static str> {
    let mut seen = HashSet::with_capacity(DICTIONARY_SOURCE.lines().count());
    let mut keys = Vec::with_capacity(seen.capacity());

    for line in DICTIONARY_SOURCE.lines() {
        if let Some(key) = line.split_whitespace().next() {
            if seen.insert(key) {
                keys.push(key);
            }
        }
    }

    assert!(
        keys.len() >= MINIMUM_CORPUS_KEYS,
        "benchmark corpus must contain at least {MINIMUM_CORPUS_KEYS} unique nonempty keys; found {}",
        keys.len()
    );
    keys.sort_unstable();
    assert!(keys.windows(2).all(|pair| pair[0] < pair[1]));
    keys
}

pub fn entries(keys: &[&'static str]) -> Vec<(&'static str, i32)> {
    keys.iter()
        .enumerate()
        .map(|(value, key)| (*key, i32::try_from(value).expect("dictionary values fit in i32")))
        .collect()
}

pub fn evenly_sampled_keys(keys: &[&'static str], sample_size: usize) -> Vec<&'static str> {
    assert!(sample_size > 0);
    assert!(sample_size <= keys.len());

    (0..sample_size)
        .map(|index| keys[index * keys.len() / sample_size])
        .collect()
}

pub fn exact_misses(keys: &[&str]) -> Vec<String> {
    keys.iter()
        // Appending a noncharacter preserves a complete real key as the miss's shared prefix.
        .map(|key| format!("{key}\u{10ffff}"))
        .collect()
}

pub fn tokenizer_text(keys: &[&str], minimum_bytes: usize) -> String {
    assert!(!keys.is_empty());
    let mut text = String::with_capacity(minimum_bytes + 128);
    let mut index = 0;

    while text.len() < minimum_bytes {
        text.push_str(keys[index]);
        text.push('。');
        index = (index + TOKENIZER_STRIDE) % keys.len();
    }

    text
}
