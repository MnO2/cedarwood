use std::collections::BTreeMap;

pub const KEYS: &[&str] = &[
    "a",
    "ab",
    "abc",
    "abcd",
    "b",
    "ba",
    "中",
    "中华",
    "中华人民共和国",
    "データ",
    "データ構造",
    "e\u{301}",
    "e\u{301}clair",
    "讥䶯䶰䶱䶲䶳䶴䶵𦡦",
    "this-is-a-deliberately-long-key-used-to-exercise-deep-trie-mutations-0123456789-abcdefghijklmnopqrstuvwxyz",
];

pub const QUERIES: &[&str] = &[
    "",
    "a",
    "ab",
    "abc",
    "abcd",
    "abcde",
    "b",
    "ba",
    "bad",
    "中",
    "中华",
    "中华人民共和国",
    "中华人民共和国万岁",
    "データ",
    "データ構造",
    "データ構造とアルゴリズム",
    "e\u{301}",
    "e\u{301}clair",
    "讥䶯䶰䶱䶲䶳䶴䶵𦡦",
    "this-is-a-deliberately-long-key-used-to-exercise-deep-trie-mutations-0123456789-abcdefghijklmnopqrstuvwxyz",
    "this-is-a-deliberately-long-key-used-to-exercise-deep-trie-mutations-0123456789-abcdefghijklmnopqrstuvwxyz-tail",
    "missing",
];

#[derive(Clone, Debug)]
pub enum Operation {
    Update { key: &'static str, value: i32 },
    Overwrite { key: &'static str, value: i32 },
    Erase { key: &'static str },
    Exact { query: &'static str },
    Prefix { query: &'static str },
    Predict { query: &'static str },
}

pub trait TrieAdapter {
    fn update(&mut self, key: &str, value: i32);
    fn erase(&mut self, key: &str);
    fn exact(&self, query: &str) -> Option<i32>;
    fn prefixes(&self, query: &str) -> Vec<(i32, usize)>;
    fn predictions(&self, query: &str) -> Vec<(i32, usize)>;
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool;
    fn entries(&self) -> BTreeMap<Vec<u8>, i32>;
}

pub struct StateMachine<T> {
    trie: T,
    reference: BTreeMap<&'static str, i32>,
}

impl<T: Default> Default for StateMachine<T> {
    fn default() -> Self {
        Self {
            trie: T::default(),
            reference: BTreeMap::new(),
        }
    }
}

impl<T: TrieAdapter> StateMachine<T> {
    pub fn apply(&mut self, operation: &Operation) -> Result<(), String> {
        match *operation {
            Operation::Update { key, value } | Operation::Overwrite { key, value } => {
                debug_assert!(!key.is_empty());
                debug_assert!(value >= 0);
                self.trie.update(key, value);
                self.reference.insert(key, value);
            }
            Operation::Erase { key } => {
                self.trie.erase(key);
                self.reference.remove(key);
            }
            Operation::Exact { query } => self.check_exact(query)?,
            Operation::Prefix { query } => self.check_prefix(query)?,
            Operation::Predict { query } => self.check_predict(query)?,
        }

        self.verify_all()
            .map_err(|error| format!("after {operation:?}: {error}"))
    }

    fn verify_all(&self) -> Result<(), String> {
        for &query in QUERIES {
            self.check_exact(query)?;
            self.check_prefix(query)?;
            self.check_predict(query)?;
        }
        if self.trie.len() != self.reference.len() {
            return Err(format!(
                "entry-count mismatch: actual={}, expected={}",
                self.trie.len(),
                self.reference.len()
            ));
        }
        if self.trie.is_empty() != self.reference.is_empty() {
            return Err(format!(
                "empty-state mismatch: actual={}, expected={}",
                self.trie.is_empty(),
                self.reference.is_empty()
            ));
        }
        let actual_entries = self.trie.entries();
        let expected_entries: BTreeMap<_, _> = self
            .reference
            .iter()
            .map(|(key, value)| (key.as_bytes().to_vec(), *value))
            .collect();
        if actual_entries != expected_entries {
            return Err(format!(
                "entry iteration mismatch: actual={actual_entries:?}, expected={expected_entries:?}"
            ));
        }
        Ok(())
    }

    fn check_exact(&self, query: &str) -> Result<(), String> {
        let actual = self.trie.exact(query);
        let expected = self.reference.get(query).copied();
        if actual == expected {
            Ok(())
        } else {
            Err(format!(
                "exact lookup mismatch for {query:?}: actual={actual:?}, expected={expected:?}"
            ))
        }
    }

    fn check_prefix(&self, query: &str) -> Result<(), String> {
        let mut actual = self.trie.prefixes(query);
        let mut expected: Vec<_> = self
            .reference
            .iter()
            .filter(|(key, _)| query.as_bytes().starts_with(key.as_bytes()))
            .map(|(key, value)| (*value, key.len() - 1))
            .collect();
        actual.sort_unstable();
        expected.sort_unstable();

        if actual == expected {
            Ok(())
        } else {
            Err(format!(
                "common-prefix mismatch for {query:?}: actual={actual:?}, expected={expected:?}"
            ))
        }
    }

    fn check_predict(&self, query: &str) -> Result<(), String> {
        let mut actual = self.trie.predictions(query);
        let mut expected: Vec<_> = self
            .reference
            .iter()
            .filter(|(key, _)| key.as_bytes().starts_with(query.as_bytes()))
            .map(|(key, value)| (*value, key.len() - query.len()))
            .collect();
        actual.sort_unstable();
        expected.sort_unstable();

        if actual == expected {
            Ok(())
        } else {
            Err(format!(
                "predictive lookup mismatch for {query:?}: actual={actual:?}, expected={expected:?}"
            ))
        }
    }
}

pub fn decode_operations(data: &[u8]) -> Vec<Operation> {
    const BYTES_PER_OPERATION: usize = 4;
    const MAX_OPERATIONS: usize = 256;

    data.chunks(BYTES_PER_OPERATION)
        .take(MAX_OPERATIONS)
        .filter_map(|chunk| {
            let opcode = *chunk.first()? % 6;
            let selector = usize::from(*chunk.get(1).unwrap_or(&0));
            let value_high = u16::from(*chunk.get(2).unwrap_or(&0));
            let value_low = u16::from(*chunk.get(3).unwrap_or(&0));
            let value = i32::from((value_high << 8) | value_low);

            Some(match opcode {
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
            })
        })
        .collect()
}

pub fn run_operations<T>(operations: &[Operation]) -> Result<(), String>
where
    T: Default + TrieAdapter,
{
    let mut state = StateMachine::<T>::default();
    for operation in operations {
        state.apply(operation)?;
    }
    Ok(())
}
