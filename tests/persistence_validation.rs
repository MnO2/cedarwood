#![cfg(feature = "std")]

use cedarwood::{Cedar, CedarPersistenceError};

const HEADER_LEN: usize = 88;

fn read_i32(bytes: &[u8], offset: usize) -> i32 {
    i32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn write_i32(bytes: &mut [u8], offset: usize, value: i32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

#[test]
fn loading_rejects_reachable_empty_leaf_sentinels() {
    let mut bytes = Vec::new();
    Cedar::new().save_to_writer(&mut bytes).unwrap();
    let node_count = u64::from_le_bytes(bytes[32..40].try_into().unwrap()) as usize;
    let n_infos_start = HEADER_LEN + node_count * 8;
    let blocks_start = n_infos_start + node_count * 2;
    let leaf = usize::from(b'a');
    let leaf_start = HEADER_LEN + leaf * 8;

    // Allocate a root child by removing it from the existing circular free list. All allocator
    // metadata and the zero entry count remain consistent; only the leaf sentinel is invalid.
    let previous = -read_i32(&bytes, leaf_start) as usize;
    let next = -read_i32(&bytes, leaf_start + 4) as usize;
    write_i32(&mut bytes, HEADER_LEN + previous * 8 + 4, -(next as i32));
    write_i32(&mut bytes, HEADER_LEN + next * 8, -(previous as i32));
    bytes[blocks_start + 8..blocks_start + 10].copy_from_slice(&255_i16.to_le_bytes());
    bytes[n_infos_start] = b'a';
    #[cfg(feature = "reduced-trie")]
    let empty_leaf = i32::MAX - 1;
    #[cfg(not(feature = "reduced-trie"))]
    let empty_leaf = -1;
    write_i32(&mut bytes, leaf_start, empty_leaf);
    write_i32(&mut bytes, leaf_start + 4, 0);

    let result = Cedar::load_from_reader(bytes.as_slice());
    assert!(
        matches!(
            result,
            Err(CedarPersistenceError::CorruptData { section: "trie", index, .. }) if index == leaf
        ),
        "unexpected load result: {result:?}"
    );
}

#[test]
fn loading_retries_interrupted_end_of_stream_check() {
    use std::io::{self, Read};

    struct InterruptedAtEof<'a> {
        bytes: &'a [u8],
        interrupted: bool,
    }

    impl Read for InterruptedAtEof<'_> {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            if self.bytes.is_empty() && !self.interrupted {
                self.interrupted = true;
                return Err(io::Error::from(io::ErrorKind::Interrupted));
            }
            self.bytes.read(buffer)
        }
    }

    let mut cedar = Cedar::new();
    cedar.update("a", 7).unwrap();
    let mut bytes = Vec::new();
    cedar.save_to_writer(&mut bytes).unwrap();
    let reader = InterruptedAtEof {
        bytes: bytes.as_slice(),
        interrupted: false,
    };
    let loaded = Cedar::load_from_reader(reader).unwrap();
    assert_eq!(loaded.exact_match_search("a"), Some((7, 1)));
}
