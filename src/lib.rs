#![cfg_attr(not(feature = "std"), no_std)]

//!  Efficiently-updatable double-array trie in Rust (ported from cedar).
//!
//! Add it to your `Cargo.toml`:
//!
//! ```toml
//! [dependencies]
//! cedarwood = "0.6"
//! ```
//!
//! then you are good to go.
//!
//! ## Example
//!
//! ```rust
//! use cedarwood::Cedar;
//!
//! let dict = vec![
//!     "a",
//!     "ab",
//!     "abc",
//!     "アルゴリズム",
//!     "データ",
//!     "構造",
//!     "网",
//!     "网球",
//!     "网球拍",
//!     "中",
//!     "中华",
//!     "中华人民",
//!     "中华人民共和国",
//! ];
//! let key_values: Vec<(&str, i32)> = dict.into_iter().enumerate().map(|(k, s)| (s, k as i32)).collect();
//! let mut cedar = Cedar::new();
//! cedar.build(&key_values)?;
//!
//! let result: Vec<i32> = cedar.common_prefix_search("abcdefg").iter().map(|x| x.0).collect();
//! assert_eq!(vec![0, 1, 2], result);
//!
//! let result: Vec<i32> = cedar
//!     .common_prefix_search("网球拍卖会")
//!     .iter()
//!     .map(|x| x.0)
//!     .collect();
//! assert_eq!(vec![6, 7, 8], result);
//!
//! let result: Vec<i32> = cedar
//!     .common_prefix_search("中华人民共和国")
//!     .iter()
//!     .map(|x| x.0)
//!     .collect();
//! assert_eq!(vec![9, 10, 11, 12], result);
//!
//! let result: Vec<i32> = cedar
//!     .common_prefix_search("データ構造とアルゴリズム")
//!     .iter()
//!     .map(|x| x.0)
//!     .collect();
//! assert_eq!(vec![4], result);
//! # Ok::<(), cedarwood::CedarError>(())
//! ```

extern crate alloc;
#[cfg(test)]
extern crate std;

use alloc::string::{FromUtf8Error, String};
use alloc::vec;
use alloc::vec::Vec;
use core::{fmt, mem};
use smallvec::SmallVec;

/// The smallest value accepted by [`Cedar::update`].
pub const MIN_VALUE: i32 = 0;

/// The largest value accepted by [`Cedar::update`].
///
/// The two larger `i32` values and all negative values are kept out of the public contract so the
/// default and `reduced-trie` layouts accept exactly the same inputs.
pub const MAX_VALUE: i32 = i32::MAX - 2;

/// Default ceiling for peak memory allocated while loading an untrusted serialized trie (512 MiB).
///
/// Use [`Cedar::load_from_reader_with_limit`] when a different application-specific ceiling is
/// appropriate. The limit applies to vector storage, not merely the number of bytes in the file.
#[cfg(feature = "std")]
pub const DEFAULT_LOAD_MEMORY_LIMIT: usize = 512 * 1024 * 1024;

/// Errors returned when configuring or mutating a trie.
#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CedarError {
    /// Empty keys cannot be stored because the root is not a value node.
    EmptyKey,
    /// Byte `0` is reserved as cedar's terminal label.
    NulByte { position: usize },
    /// Values must be in [`MIN_VALUE`] through [`MAX_VALUE`], inclusive.
    InvalidValue { value: i32 },
    /// `max_trial` must be greater than zero.
    InvalidMaxTrial { value: i32 },
    /// A sorted constructor received the same key twice.
    DuplicateKey { index: usize },
    /// A sorted constructor received a key smaller than its predecessor.
    KeysNotSorted { index: usize },
}

impl fmt::Display for CedarError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyKey => write!(f, "cedar keys must not be empty"),
            Self::NulByte { position } => write!(
                f,
                "cedar keys must not contain the terminal byte 0x00 (at byte {position})"
            ),
            Self::InvalidValue { value } => write!(
                f,
                "cedar value {value} is outside the supported range {MIN_VALUE}..={MAX_VALUE}"
            ),
            Self::InvalidMaxTrial { value } => {
                write!(f, "max_trial must be greater than zero, got {value}")
            }
            Self::DuplicateKey { index } => {
                write!(f, "duplicate key at sorted input index {index}")
            }
            Self::KeysNotSorted { index } => {
                write!(f, "key at input index {index} is smaller than its predecessor")
            }
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for CedarError {}

/// Errors returned while saving or loading cedarwood's versioned binary format.
#[cfg(feature = "std")]
#[non_exhaustive]
#[derive(Debug)]
pub enum CedarPersistenceError {
    /// The underlying stream or filesystem operation failed.
    Io(std::io::Error),
    /// The input ended before the complete declared representation was read.
    Truncated,
    /// The eight-byte cedarwood format marker did not match.
    InvalidMagic,
    /// The file uses a format version this reader does not understand.
    UnsupportedVersion { major: u16, minor: u16 },
    /// The file is not explicitly marked as little-endian.
    UnsupportedByteOrder { byte_order: u8 },
    /// The file was produced for a different compile-time trie layout.
    LayoutMismatch { expected: u8, found: u8 },
    /// Reserved header flags were set or a header field was otherwise invalid.
    InvalidHeader { field: &'static str, value: u64 },
    /// Loading and structural validation would reserve more memory than the caller allows.
    AllocationLimitExceeded { requested: usize, limit: usize },
    /// The allocator could not reserve the validated amount of memory.
    AllocationFailed { requested: usize },
    /// A structural invariant failed validation.
    CorruptData {
        section: &'static str,
        index: usize,
        reason: &'static str,
    },
    /// Bytes remained after the complete declared representation.
    TrailingData,
}

#[cfg(feature = "std")]
impl fmt::Display for CedarPersistenceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "persistence I/O error: {error}"),
            Self::Truncated => write!(f, "serialized cedar trie is truncated"),
            Self::InvalidMagic => write!(f, "input is not a cedarwood serialized trie"),
            Self::UnsupportedVersion { major, minor } => {
                write!(f, "unsupported cedarwood format version {major}.{minor}")
            }
            Self::UnsupportedByteOrder { byte_order } => {
                write!(f, "unsupported cedarwood byte-order marker {byte_order}")
            }
            Self::LayoutMismatch { expected, found } => write!(
                f,
                "serialized trie layout {found} does not match this build's layout {expected}"
            ),
            Self::InvalidHeader { field, value } => {
                write!(f, "invalid serialized header field {field}: {value}")
            }
            Self::AllocationLimitExceeded { requested, limit } => write!(
                f,
                "serialized trie requires up to {requested} bytes while loading, limit is {limit}"
            ),
            Self::AllocationFailed { requested } => {
                write!(
                    f,
                    "could not reserve {requested} elements while loading serialized trie"
                )
            }
            Self::CorruptData { section, index, reason } => write!(f, "corrupt {section} at index {index}: {reason}"),
            Self::TrailingData => write!(f, "serialized trie has trailing data"),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for CedarPersistenceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

#[cfg(feature = "std")]
impl From<std::io::Error> for CedarPersistenceError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

/// Configuration used to create a [`Cedar`] trie.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CedarBuilder {
    ordered: bool,
    max_trial: i32,
}

impl CedarBuilder {
    /// Creates a builder with ordered sibling chains and `max_trial = 1`.
    pub const fn new() -> Self {
        Self {
            ordered: true,
            max_trial: 1,
        }
    }

    /// Controls whether sibling labels remain sorted. The default is `true`.
    #[must_use]
    pub const fn ordered(mut self, ordered: bool) -> Self {
        self.ordered = ordered;
        self
    }

    /// Sets the number of failed probes allowed before an open block is closed.
    pub fn max_trial(mut self, max_trial: i32) -> Result<Self, CedarError> {
        if max_trial <= 0 {
            return Err(CedarError::InvalidMaxTrial { value: max_trial });
        }
        self.max_trial = max_trial;
        Ok(self)
    }

    /// Builds an empty trie with this configuration.
    #[must_use]
    pub fn build(self) -> Cedar {
        Cedar::with_config(self.ordered, self.max_trial)
    }

    /// Directly constructs a trie from strictly byte-sorted, unique UTF-8 keys using this
    /// builder's configuration.
    pub fn from_sorted(self, key_values: &[(&str, i32)]) -> Result<Cedar, CedarError> {
        Cedar::validate_sorted_entries(key_values)?;
        let mut cedar = Cedar::with_config(self.ordered, self.max_trial);
        cedar.build_sorted_validated(key_values);
        Ok(cedar)
    }

    /// Directly constructs a trie from strictly sorted, unique byte keys using this builder's
    /// configuration.
    pub fn from_sorted_bytes(self, key_values: &[(&[u8], i32)]) -> Result<Cedar, CedarError> {
        Cedar::validate_sorted_entries(key_values)?;
        let mut cedar = Cedar::with_config(self.ordered, self.max_trial);
        cedar.build_sorted_validated(key_values);
        Ok(cedar)
    }
}

impl Default for CedarBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// A snapshot of the trie's owned-memory and occupancy metrics.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MemoryStats {
    /// Number of stored key/value entries. This is identical to [`Cedar::len`].
    pub entries: usize,
    /// Occupied double-array slots, including the root and terminal slots where applicable.
    pub used_node_slots: usize,
    /// Number of `Node` elements for which the node vector has allocated storage.
    pub node_capacity: usize,
    /// Bytes reserved by all four owned vectors, computed from each vector's capacity and element
    /// size. It excludes the inline `Cedar` value and allocator bookkeeping.
    pub allocated_bytes: usize,
    /// `used_node_slots / node_capacity`, or `0.0` if capacity is zero.
    pub load_factor: f64,
}

/// NInfo stores the information about the trie
#[derive(Debug, Default, Clone, Copy)]
struct NInfo {
    sibling: u8, // the index of right sibling, it is 0 if it doesn't have a sibling.
    child: u8,   // the index of the first child
}

/// Node contains the array of `base` and `check` as specified in the paper: "An efficient implementation of trie structures"
/// https://dl.acm.org/citation.cfm?id=146691
#[derive(Debug, Default, Clone, Copy)]
struct Node {
    base_: i32, // if it is a negative value, then it stores the value of previous index that is free.
    check: i32, // if it is a negative value, then it stores the value of next index that is free.
}

impl Node {
    #[inline]
    fn base(&self) -> i32 {
        #[cfg(feature = "reduced-trie")]
        return -(self.base_ + 1);
        #[cfg(not(feature = "reduced-trie"))]
        return self.base_;
    }
}

/// Block stores the linked-list pointers and the stats info for blocks.
#[derive(Debug, Clone)]
struct Block {
    prev: i32,   // previous block's index, 3 bytes width
    next: i32,   // next block's index, 3 bytes width
    num: i16,    // the number of slots that is free, the range is 0-256
    reject: i16, // a heuristic number to make the search for free space faster, it is the minimum number of iteration in each trie node it has to try before we can conclude that we can reject this block. If the number of kids for the block we are looking for is less than this number then this block is worthy of searching.
    trial: i32,  // the number of times this block has been probed by `find_places` for the free block.
    e_head: i32, // the index of the first empty element in this block
}

impl Block {
    pub fn new() -> Self {
        Block {
            prev: 0,
            next: 0,
            num: 256,    // each of block has 256 free slots at the beginning
            reject: 257, // initially every block need to be fully iterated through so that we can reject it to be unusable.
            trial: 0,
            e_head: 0,
        }
    }
}

impl Default for Block {
    fn default() -> Self {
        Self::new()
    }
}

/// Blocks are marked as either of three categories, so that we can quickly decide if we can
/// allocate it for use or not.
enum BlockType {
    Open,   // The block has spaces more than 1.
    Closed, // The block is only left with one free slot
    Full,   // The block's slots are fully used.
}

/// `Cedar` holds all of the information about double array trie.
#[derive(Clone)]
pub struct Cedar {
    array: Vec<Node>, // storing the `base` and `check` info from the original paper.
    n_infos: Vec<NInfo>,
    blocks: Vec<Block>,
    reject: Vec<i16>,
    blocks_head_full: i32,   // the index of the first 'Full' block, 0 means no 'Full' block
    blocks_head_closed: i32, // the index of the first 'Closed' block, 0 means no ' Closed' block
    blocks_head_open: i32,   // the index of the first 'Open' block, 0 means no 'Open' block
    capacity: usize,
    size: usize,
    entries: usize,
    ordered: bool,
    max_trial: i32, // the parameter for cedar, it could be tuned for more, but the default is 1.
}

// Persistence decoding must pass through this private state before a queryable Cedar can escape.
#[cfg(feature = "std")]
struct UnvalidatedCedar(Cedar);

#[cfg(feature = "std")]
impl UnvalidatedCedar {
    fn validate(self) -> Result<Cedar, CedarPersistenceError> {
        self.0.validate_loaded_state()?;
        Ok(self.0)
    }
}

impl fmt::Debug for Cedar {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "Cedar(entries={}, node_slots={}, ordered={})",
            self.entries, self.size, self.ordered
        )
    }
}

#[cfg(feature = "reduced-trie")]
const CEDAR_VALUE_LIMIT: i32 = i32::MAX - 1;
const CEDAR_NO_VALUE: i32 = -1;

// The versioned wire DTOs and primitive codec are kept separate from the live trie structs. Phase
// 7 can gate this module and the public persistence methods behind `std` without touching query or
// mutation code.
#[cfg(feature = "std")]
mod persistence {
    pub(super) mod v1 {
        use super::super::CedarPersistenceError;
        use std::io::{self, Read, Write};

        pub const MAGIC: [u8; 8] = *b"CEDARW\0\0";
        pub const MAJOR: u16 = 1;
        pub const MINOR: u16 = 0;
        pub const LITTLE_ENDIAN: u8 = 1;
        pub const FLAG_ORDERED: u16 = 1;
        #[cfg(test)]
        pub const HEADER_LEN: usize = 88;

        #[cfg(feature = "reduced-trie")]
        pub const LAYOUT: u8 = 1;
        #[cfg(not(feature = "reduced-trie"))]
        pub const LAYOUT: u8 = 0;

        #[derive(Clone, Copy)]
        pub struct Header {
            pub flags: u16,
            pub max_trial: i32,
            pub blocks_head_full: i32,
            pub blocks_head_closed: i32,
            pub blocks_head_open: i32,
            pub array_len: usize,
            pub n_infos_len: usize,
            pub blocks_len: usize,
            pub reject_len: usize,
            pub capacity: usize,
            pub size: usize,
            pub entries: usize,
        }

        #[derive(Clone, Copy)]
        pub struct NodeRecord {
            pub base: i32,
            pub check: i32,
        }

        #[derive(Clone, Copy)]
        pub struct NInfoRecord {
            pub sibling: u8,
            pub child: u8,
        }

        #[derive(Clone, Copy)]
        pub struct BlockRecord {
            pub prev: i32,
            pub next: i32,
            pub num: i16,
            pub reject: i16,
            pub trial: i32,
            pub e_head: i32,
        }

        pub struct Decoded {
            pub header: Header,
            pub nodes: Vec<NodeRecord>,
            pub n_infos: Vec<NInfoRecord>,
            pub blocks: Vec<BlockRecord>,
            pub reject: Vec<i16>,
        }

        fn persistence_io(error: io::Error) -> CedarPersistenceError {
            if error.kind() == io::ErrorKind::UnexpectedEof {
                CedarPersistenceError::Truncated
            } else {
                CedarPersistenceError::Io(error)
            }
        }

        fn read_exact(reader: &mut impl Read, bytes: &mut [u8]) -> Result<(), CedarPersistenceError> {
            reader.read_exact(bytes).map_err(persistence_io)
        }

        pub fn read_u8(reader: &mut impl Read) -> Result<u8, CedarPersistenceError> {
            let mut bytes = [0; 1];
            read_exact(reader, &mut bytes)?;
            Ok(bytes[0])
        }

        pub fn read_u16(reader: &mut impl Read) -> Result<u16, CedarPersistenceError> {
            let mut bytes = [0; 2];
            read_exact(reader, &mut bytes)?;
            Ok(u16::from_le_bytes(bytes))
        }

        pub fn read_i16(reader: &mut impl Read) -> Result<i16, CedarPersistenceError> {
            let mut bytes = [0; 2];
            read_exact(reader, &mut bytes)?;
            Ok(i16::from_le_bytes(bytes))
        }

        pub fn read_i32(reader: &mut impl Read) -> Result<i32, CedarPersistenceError> {
            let mut bytes = [0; 4];
            read_exact(reader, &mut bytes)?;
            Ok(i32::from_le_bytes(bytes))
        }

        pub fn read_u64(reader: &mut impl Read) -> Result<u64, CedarPersistenceError> {
            let mut bytes = [0; 8];
            read_exact(reader, &mut bytes)?;
            Ok(u64::from_le_bytes(bytes))
        }

        pub fn write_i16(writer: &mut impl Write, value: i16) -> Result<(), CedarPersistenceError> {
            writer.write_all(&value.to_le_bytes()).map_err(Into::into)
        }

        pub fn write_i32(writer: &mut impl Write, value: i32) -> Result<(), CedarPersistenceError> {
            writer.write_all(&value.to_le_bytes()).map_err(Into::into)
        }

        pub fn write_usize(writer: &mut impl Write, value: usize) -> Result<(), CedarPersistenceError> {
            let value = u64::try_from(value).map_err(|_| CedarPersistenceError::InvalidHeader {
                field: "usize",
                value: u64::MAX,
            })?;
            writer.write_all(&value.to_le_bytes()).map_err(Into::into)
        }

        pub fn read_usize(reader: &mut impl Read, field: &'static str) -> Result<usize, CedarPersistenceError> {
            let value = read_u64(reader)?;
            usize::try_from(value).map_err(|_| CedarPersistenceError::InvalidHeader { field, value })
        }

        pub fn reserve_exact<T>(vector: &mut Vec<T>, len: usize) -> Result<(), CedarPersistenceError> {
            vector
                .try_reserve_exact(len)
                .map_err(|_| CedarPersistenceError::AllocationFailed { requested: len })
        }

        pub fn write_header(writer: &mut impl Write, header: Header) -> Result<(), CedarPersistenceError> {
            writer.write_all(&MAGIC)?;
            writer.write_all(&MAJOR.to_le_bytes())?;
            writer.write_all(&MINOR.to_le_bytes())?;
            writer.write_all(&[LITTLE_ENDIAN, LAYOUT])?;
            writer.write_all(&header.flags.to_le_bytes())?;
            write_i32(writer, header.max_trial)?;
            write_i32(writer, header.blocks_head_full)?;
            write_i32(writer, header.blocks_head_closed)?;
            write_i32(writer, header.blocks_head_open)?;
            write_usize(writer, header.array_len)?;
            write_usize(writer, header.n_infos_len)?;
            write_usize(writer, header.blocks_len)?;
            write_usize(writer, header.reject_len)?;
            write_usize(writer, header.capacity)?;
            write_usize(writer, header.size)?;
            write_usize(writer, header.entries)
        }

        pub fn read_header(reader: &mut impl Read) -> Result<Header, CedarPersistenceError> {
            let mut magic = [0; 8];
            read_exact(reader, &mut magic)?;
            if magic != MAGIC {
                return Err(CedarPersistenceError::InvalidMagic);
            }
            let major = read_u16(reader)?;
            let minor = read_u16(reader)?;
            if major != MAJOR || minor > MINOR {
                return Err(CedarPersistenceError::UnsupportedVersion { major, minor });
            }
            let byte_order = read_u8(reader)?;
            if byte_order != LITTLE_ENDIAN {
                return Err(CedarPersistenceError::UnsupportedByteOrder { byte_order });
            }
            let layout = read_u8(reader)?;
            if layout != LAYOUT {
                return Err(CedarPersistenceError::LayoutMismatch {
                    expected: LAYOUT,
                    found: layout,
                });
            }
            let flags = read_u16(reader)?;
            if flags & !FLAG_ORDERED != 0 {
                return Err(CedarPersistenceError::InvalidHeader {
                    field: "flags",
                    value: u64::from(flags),
                });
            }
            Ok(Header {
                flags,
                max_trial: read_i32(reader)?,
                blocks_head_full: read_i32(reader)?,
                blocks_head_closed: read_i32(reader)?,
                blocks_head_open: read_i32(reader)?,
                array_len: read_usize(reader, "array_len")?,
                n_infos_len: read_usize(reader, "n_infos_len")?,
                blocks_len: read_usize(reader, "blocks_len")?,
                reject_len: read_usize(reader, "reject_len")?,
                capacity: read_usize(reader, "capacity")?,
                size: read_usize(reader, "size")?,
                entries: read_usize(reader, "entries")?,
            })
        }

        pub fn decode(reader: &mut impl Read, memory_limit: usize) -> Result<Decoded, CedarPersistenceError> {
            let header = read_header(reader)?;
            if header.array_len != header.n_infos_len || header.array_len != header.capacity {
                return Err(CedarPersistenceError::InvalidHeader {
                    field: "node_vector_lengths",
                    value: header.array_len as u64,
                });
            }
            if header.capacity < 256
                || !header.capacity.is_power_of_two()
                || header.capacity % 256 != 0
                || header.capacity > i32::MAX as usize
            {
                return Err(CedarPersistenceError::InvalidHeader {
                    field: "capacity",
                    value: header.capacity as u64,
                });
            }
            if header.size < 256 || header.size > header.capacity || header.size % 256 != 0 {
                return Err(CedarPersistenceError::InvalidHeader {
                    field: "size",
                    value: header.size as u64,
                });
            }
            if header.blocks_len != header.capacity / 256 || header.reject_len != 257 || header.entries > header.size {
                return Err(CedarPersistenceError::InvalidHeader {
                    field: "state_lengths",
                    value: header.blocks_len as u64,
                });
            }

            let one_copy_bytes = header
                .array_len
                .checked_mul(std::mem::size_of::<NodeRecord>())
                .and_then(|total| {
                    header
                        .n_infos_len
                        .checked_mul(std::mem::size_of::<NInfoRecord>())
                        .and_then(|bytes| total.checked_add(bytes))
                })
                .and_then(|total| {
                    header
                        .blocks_len
                        .checked_mul(std::mem::size_of::<BlockRecord>())
                        .and_then(|bytes| total.checked_add(bytes))
                })
                .and_then(|total| {
                    header
                        .reject_len
                        .checked_mul(std::mem::size_of::<i16>())
                        .and_then(|bytes| total.checked_add(bytes))
                })
                .ok_or(CedarPersistenceError::InvalidHeader {
                    field: "state_lengths",
                    value: u64::MAX,
                })?;
            let validation_bytes = header
                .size
                .checked_mul(std::mem::size_of::<bool>() * 2 + std::mem::size_of::<(usize, bool)>())
                .and_then(|total| total.checked_add(header.blocks_len * std::mem::size_of::<u8>()))
                .ok_or(CedarPersistenceError::InvalidHeader {
                    field: "validation_memory",
                    value: u64::MAX,
                })?;
            let peak_bytes = one_copy_bytes
                .checked_mul(2)
                .and_then(|total| total.checked_add(validation_bytes))
                .ok_or(CedarPersistenceError::InvalidHeader {
                    field: "peak_allocation",
                    value: u64::MAX,
                })?;
            if peak_bytes > memory_limit {
                return Err(CedarPersistenceError::AllocationLimitExceeded {
                    requested: peak_bytes,
                    limit: memory_limit,
                });
            }

            let mut nodes = Vec::new();
            reserve_exact(&mut nodes, header.array_len)?;
            for _ in 0..header.array_len {
                nodes.push(NodeRecord {
                    base: read_i32(reader)?,
                    check: read_i32(reader)?,
                });
            }
            let mut n_infos = Vec::new();
            reserve_exact(&mut n_infos, header.n_infos_len)?;
            for _ in 0..header.n_infos_len {
                n_infos.push(NInfoRecord {
                    sibling: read_u8(reader)?,
                    child: read_u8(reader)?,
                });
            }
            let mut blocks = Vec::new();
            reserve_exact(&mut blocks, header.blocks_len)?;
            for _ in 0..header.blocks_len {
                blocks.push(BlockRecord {
                    prev: read_i32(reader)?,
                    next: read_i32(reader)?,
                    num: read_i16(reader)?,
                    reject: read_i16(reader)?,
                    trial: read_i32(reader)?,
                    e_head: read_i32(reader)?,
                });
            }
            let mut reject = Vec::new();
            reserve_exact(&mut reject, header.reject_len)?;
            for _ in 0..header.reject_len {
                reject.push(read_i16(reader)?);
            }

            let mut trailing = [0; 1];
            match reader.read(&mut trailing) {
                Ok(0) => {}
                Ok(_) => return Err(CedarPersistenceError::TrailingData),
                Err(error) => return Err(CedarPersistenceError::Io(error)),
            }
            Ok(Decoded {
                header,
                nodes,
                n_infos,
                blocks,
                reject,
            })
        }
    }
}

/// Iterator for `common_prefix_search`
#[derive(Clone)]
pub struct PrefixIter<'a> {
    cedar: &'a Cedar,
    key: &'a [u8],
    from: usize,
    i: usize,
}

impl<'a> Iterator for PrefixIter<'a> {
    type Item = (i32, usize);

    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, Some(self.key.len()))
    }

    fn next(&mut self) -> Option<Self::Item> {
        while self.i < self.key.len() {
            // Zero is the structural terminal label, not a traversable key byte. Stop here rather
            // than scanning the complete query up front: tokenizer callers create one suffix per
            // character boundary, so eager validation would turn a linear scan into O(n^2).
            if self.key[self.i] == 0 {
                break;
            }

            // Inline the single-byte traversal instead of calling find() with a 1-byte slice
            let from = self.from;

            #[cfg(feature = "reduced-trie")]
            {
                if self.cedar.array[from].base_ >= 0 {
                    break;
                }
            }

            let base = self.cedar.array[from].base();
            let to = (base ^ i32::from(self.key[self.i])) as usize;
            if self.cedar.array[to].check != (from as i32) {
                break;
            }

            self.from = to;
            self.i += 1;

            // Check for value at this position
            #[cfg(feature = "reduced-trie")]
            {
                if self.cedar.array[to].base_ >= 0 {
                    return Some((self.cedar.array[to].base_, self.i - 1));
                }
            }

            let terminal_base = self.cedar.array[to].base();
            let terminal = &self.cedar.array[terminal_base as usize];
            if terminal.check == (to as i32) && terminal.base_ != CEDAR_NO_VALUE {
                return Some((terminal.base_, self.i - 1));
            }
        }

        None
    }
}

/// Iterator for `common_prefix_predict`
#[derive(Clone)]
pub struct PrefixPredictIter<'a> {
    cedar: &'a Cedar,
    key: &'a [u8],
    started: bool,
    from: usize,
    p: usize,
    root: usize,
    value: Option<i32>,
    valid: bool,
}

impl<'a> PrefixPredictIter<'a> {
    fn next_until_none(&mut self) -> Option<(i32, usize)> {
        if let Some(value) = self.value {
            let result = (value, self.p);

            let (v_, from_, p_) = self.cedar.next(self.from, self.p, self.root);
            self.from = from_;
            self.p = p_;
            self.value = v_;

            Some(result)
        } else {
            None
        }
    }
}

impl<'a> Iterator for PrefixPredictIter<'a> {
    type Item = (i32, usize);

    fn next(&mut self) -> Option<Self::Item> {
        if !self.valid {
            return None;
        }
        if !self.started {
            self.started = true;
            // To locate the prefix's position first, if it doesn't exist then that means we
            // don't have do anything. `from` would serve as the cursor.
            if self.cedar.find(self.key, &mut self.from).is_some() {
                self.root = self.from;

                let (v_, from_, p_) = self.cedar.begin(self.from, self.p);
                self.from = from_;
                self.p = p_;
                self.value = v_;

                self.next_until_none()
            } else {
                None
            }
        } else {
            self.next_until_none()
        }
    }
}

/// Iterator over every stored byte key and value.
pub struct Entries<'a> {
    cedar: &'a Cedar,
    stack: Vec<(usize, Vec<u8>)>,
    remaining: usize,
}

impl Iterator for Entries<'_> {
    type Item = (Vec<u8>, i32);

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }

    fn next(&mut self) -> Option<Self::Item> {
        while let Some((index, key)) = self.stack.pop() {
            let base = self.cedar.array[index].base();
            let children = self.cedar.child_labels(index);
            for label in children.into_iter().rev() {
                let child = (base ^ i32::from(label)) as usize;
                let mut child_key = key.clone();
                child_key.push(label);
                self.stack.push((child, child_key));
            }

            if let Some(value) = self.cedar.value_at_node(index) {
                self.remaining -= 1;
                return Some((key, value));
            }
        }
        debug_assert_eq!(self.remaining, 0);
        None
    }
}

impl ExactSizeIterator for Entries<'_> {}

/// UTF-8 adapter for [`Entries`]. Invalid byte keys are returned as [`FromUtf8Error`] values.
pub struct Utf8Entries<'a> {
    inner: Entries<'a>,
}

impl Iterator for Utf8Entries<'_> {
    type Item = Result<(String, i32), FromUtf8Error>;

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }

    fn next(&mut self) -> Option<Self::Item> {
        self.inner
            .next()
            .map(|(key, value)| String::from_utf8(key).map(|key| (key, value)))
    }
}

impl Cedar {
    /// Creates an empty trie using [`CedarBuilder`]'s defaults.
    pub fn new() -> Self {
        CedarBuilder::new().build()
    }

    /// Directly constructs a trie from strictly byte-sorted, unique UTF-8 keys.
    ///
    /// Unlike [`Cedar::build`], this constructor allocates every sibling set together instead of
    /// incrementally resolving conflicts for each key. The complete input is validated before any
    /// trie storage is allocated. Empty input constructs an empty trie.
    pub fn from_sorted(key_values: &[(&str, i32)]) -> Result<Self, CedarError> {
        CedarBuilder::new().from_sorted(key_values)
    }

    /// Directly constructs a trie from strictly sorted, unique byte keys.
    ///
    /// Keys are ordered by their raw bytes. Every key must be nonempty and contain no `0x00`, and
    /// every value must be in [`MIN_VALUE`] through [`MAX_VALUE`]. Invalid input returns a typed
    /// error without constructing a partial trie.
    pub fn from_sorted_bytes(key_values: &[(&[u8], i32)]) -> Result<Self, CedarError> {
        CedarBuilder::new().from_sorted_bytes(key_values)
    }

    /// Returns a configurable builder for an empty trie.
    pub const fn builder() -> CedarBuilder {
        CedarBuilder::new()
    }

    fn with_config(ordered: bool, max_trial: i32) -> Self {
        let mut array: Vec<Node> = Vec::with_capacity(256);
        let n_infos: Vec<NInfo> = vec![NInfo::default(); 256];
        let mut blocks: Vec<Block> = vec![Block::new(); 1];
        let reject: Vec<i16> = (0..=256).map(|i| i + 1).collect();

        #[cfg(feature = "reduced-trie")]
        array.push(Node { base_: -1, check: -1 });
        #[cfg(not(feature = "reduced-trie"))]
        array.push(Node { base_: 0, check: -1 });

        for i in 1..256 {
            // make `base_` point to the previous element, and make `check` point to the next element
            array.push(Node {
                base_: -(i - 1),
                check: -(i + 1),
            })
        }

        // make them link as a cyclic doubly-linked list
        array[1].base_ = -255;
        array[255].check = -1;

        blocks[0].e_head = 1;

        Cedar {
            array,
            n_infos,
            blocks,
            reject,
            blocks_head_full: 0,
            blocks_head_closed: 0,
            blocks_head_open: 0,
            capacity: 256,
            size: 256,
            entries: 0,
            ordered,
            max_trial,
        }
    }

    /// Returns the number of stored key/value entries.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.entries
    }

    /// Returns `true` when no key/value entries are stored.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.entries == 0
    }

    /// Returns the number of occupied double-array slots, including the root.
    #[must_use]
    pub fn used_node_slots(&self) -> usize {
        let free_slots = self
            .blocks
            .iter()
            .take(self.size >> 8)
            .map(|block| block.num as usize)
            .sum::<usize>();
        1 + self.size - free_slots
    }

    /// Returns the allocated element capacity of the double-array node vector.
    #[must_use]
    pub fn node_capacity(&self) -> usize {
        self.array.capacity()
    }

    /// Returns bytes reserved by all owned vectors, excluding `Cedar` itself and allocator
    /// bookkeeping.
    #[must_use]
    pub fn allocated_bytes(&self) -> usize {
        self.array.capacity() * mem::size_of::<Node>()
            + self.n_infos.capacity() * mem::size_of::<NInfo>()
            + self.blocks.capacity() * mem::size_of::<Block>()
            + self.reject.capacity() * mem::size_of::<i16>()
    }

    /// Returns occupied node slots divided by allocated node capacity.
    #[must_use]
    pub fn load_factor(&self) -> f64 {
        if self.node_capacity() == 0 {
            0.0
        } else {
            self.used_node_slots() as f64 / self.node_capacity() as f64
        }
    }

    /// Returns all public memory and occupancy metrics in one snapshot.
    #[must_use]
    pub fn memory_stats(&self) -> MemoryStats {
        MemoryStats {
            entries: self.len(),
            used_node_slots: self.used_node_slots(),
            node_capacity: self.node_capacity(),
            allocated_bytes: self.allocated_bytes(),
            load_factor: self.load_factor(),
        }
    }

    /// Writes this trie using cedarwood's stable, versioned little-endian binary format.
    ///
    /// The representation preserves insertion configuration and allocator state, so a loaded trie
    /// remains mutable. Rust struct memory is never written directly.
    #[cfg(feature = "std")]
    pub fn save_to_writer(&self, mut writer: impl std::io::Write) -> Result<(), CedarPersistenceError> {
        use persistence::v1;

        let flags = if self.ordered { v1::FLAG_ORDERED } else { 0 };
        v1::write_header(
            &mut writer,
            v1::Header {
                flags,
                max_trial: self.max_trial,
                blocks_head_full: self.blocks_head_full,
                blocks_head_closed: self.blocks_head_closed,
                blocks_head_open: self.blocks_head_open,
                array_len: self.array.len(),
                n_infos_len: self.n_infos.len(),
                blocks_len: self.blocks.len(),
                reject_len: self.reject.len(),
                capacity: self.capacity,
                size: self.size,
                entries: self.entries,
            },
        )?;

        for node in &self.array {
            let record = v1::NodeRecord {
                base: node.base_,
                check: node.check,
            };
            v1::write_i32(&mut writer, record.base)?;
            v1::write_i32(&mut writer, record.check)?;
        }
        for info in &self.n_infos {
            let record = v1::NInfoRecord {
                sibling: info.sibling,
                child: info.child,
            };
            writer.write_all(&[record.sibling, record.child])?;
        }
        for block in &self.blocks {
            let record = v1::BlockRecord {
                prev: block.prev,
                next: block.next,
                num: block.num,
                reject: block.reject,
                trial: block.trial,
                e_head: block.e_head,
            };
            v1::write_i32(&mut writer, record.prev)?;
            v1::write_i32(&mut writer, record.next)?;
            v1::write_i16(&mut writer, record.num)?;
            v1::write_i16(&mut writer, record.reject)?;
            v1::write_i32(&mut writer, record.trial)?;
            v1::write_i32(&mut writer, record.e_head)?;
        }
        for value in &self.reject {
            v1::write_i16(&mut writer, *value)?;
        }
        Ok(())
    }

    /// Saves this trie to a file by truncating or creating it.
    ///
    /// This helper is not atomic: an I/O failure can leave a partial file. For atomic replacement,
    /// write with [`Cedar::save_to_writer`] to an application-managed temporary file, flush and
    /// sync it as required, then rename it according to the platform's durability needs.
    #[cfg(feature = "std")]
    pub fn save_to_path(&self, path: impl AsRef<std::path::Path>) -> Result<(), CedarPersistenceError> {
        self.save_to_writer(std::fs::File::create(path)?)
    }

    /// Loads and validates a trie using [`DEFAULT_LOAD_MEMORY_LIMIT`].
    ///
    /// Validation completes before the `Cedar` is returned, so malformed input can never reach
    /// the unchecked query paths through this API.
    #[cfg(feature = "std")]
    pub fn load_from_reader(reader: impl std::io::Read) -> Result<Self, CedarPersistenceError> {
        Self::load_from_reader_with_limit(reader, DEFAULT_LOAD_MEMORY_LIMIT)
    }

    /// Loads and validates a trie while limiting total reserved vector storage.
    #[cfg(feature = "std")]
    pub fn load_from_reader_with_limit(
        mut reader: impl std::io::Read,
        memory_limit: usize,
    ) -> Result<Self, CedarPersistenceError> {
        let decoded = persistence::v1::decode(&mut reader, memory_limit)?;

        Self::from_persistence_v1(decoded)
    }

    /// Loads and validates a trie from a file using [`DEFAULT_LOAD_MEMORY_LIMIT`].
    #[cfg(feature = "std")]
    pub fn load_from_path(path: impl AsRef<std::path::Path>) -> Result<Self, CedarPersistenceError> {
        Self::load_from_reader(std::fs::File::open(path)?)
    }

    #[cfg(feature = "std")]
    fn from_persistence_v1(decoded: persistence::v1::Decoded) -> Result<Self, CedarPersistenceError> {
        use persistence::v1;

        let header = decoded.header;
        let mut array = Vec::new();
        v1::reserve_exact(&mut array, decoded.nodes.len())?;
        array.extend(decoded.nodes.into_iter().map(|record| Node {
            base_: record.base,
            check: record.check,
        }));
        let mut n_infos = Vec::new();
        v1::reserve_exact(&mut n_infos, decoded.n_infos.len())?;
        n_infos.extend(decoded.n_infos.into_iter().map(|record| NInfo {
            sibling: record.sibling,
            child: record.child,
        }));
        let mut blocks = Vec::new();
        v1::reserve_exact(&mut blocks, decoded.blocks.len())?;
        blocks.extend(decoded.blocks.into_iter().map(|record| Block {
            prev: record.prev,
            next: record.next,
            num: record.num,
            reject: record.reject,
            trial: record.trial,
            e_head: record.e_head,
        }));
        let mut reject = Vec::new();
        v1::reserve_exact(&mut reject, decoded.reject.len())?;
        reject.extend(decoded.reject);

        let unvalidated = UnvalidatedCedar(Cedar {
            array,
            n_infos,
            blocks,
            reject,
            blocks_head_full: header.blocks_head_full,
            blocks_head_closed: header.blocks_head_closed,
            blocks_head_open: header.blocks_head_open,
            capacity: header.capacity,
            size: header.size,
            entries: header.entries,
            ordered: header.flags & v1::FLAG_ORDERED != 0,
            max_trial: header.max_trial,
        });
        unvalidated.validate()
    }

    #[cfg(feature = "std")]
    fn corrupt(section: &'static str, index: usize, reason: &'static str) -> CedarPersistenceError {
        CedarPersistenceError::CorruptData { section, index, reason }
    }

    #[cfg(feature = "std")]
    fn validate_loaded_state(&self) -> Result<(), CedarPersistenceError> {
        if self.max_trial <= 0 {
            return Err(CedarPersistenceError::InvalidHeader {
                field: "max_trial",
                value: self.max_trial as u32 as u64,
            });
        }
        if self.capacity != self.array.len()
            || self.n_infos.len() != self.capacity
            || self.capacity < 256
            || !self.capacity.is_power_of_two()
            || self.capacity % 256 != 0
            || self.capacity > i32::MAX as usize
        {
            return Err(Self::corrupt("vectors", 0, "invalid node vector relationship"));
        }
        if self.size < 256 || self.size > self.capacity || self.size % 256 != 0 {
            return Err(Self::corrupt("vectors", 0, "invalid active node length"));
        }
        if self.blocks.len() != self.capacity / 256 {
            return Err(Self::corrupt("vectors", 0, "block vector length does not match nodes"));
        }
        if self.reject.len() != 257 {
            return Err(Self::corrupt("reject", 0, "reject table must contain 257 values"));
        }
        if self.entries > self.size {
            return Err(Self::corrupt("entries", 0, "entry count exceeds active nodes"));
        }
        #[cfg(feature = "reduced-trie")]
        let valid_root_base = self.array[0].base_ == -1;
        #[cfg(not(feature = "reduced-trie"))]
        let valid_root_base = self.array[0].base_ == 0;
        if self.array[0].check != -1 || !valid_root_base {
            return Err(Self::corrupt("nodes", 0, "invalid root sentinel"));
        }

        for index in self.size..self.capacity {
            if self.array[index].base_ != 0
                || self.array[index].check != 0
                || self.n_infos[index].child != 0
                || self.n_infos[index].sibling != 0
            {
                return Err(Self::corrupt("nodes", index, "inactive node is not zero initialized"));
            }
        }
        let active_blocks = self.size / 256;
        for index in active_blocks..self.blocks.len() {
            let block = &self.blocks[index];
            if block.prev != 0
                || block.next != 0
                || block.num != 256
                || block.reject != 257
                || block.trial != 0
                || block.e_head != 0
            {
                return Err(Self::corrupt("blocks", index, "inactive block is not initialized"));
            }
        }

        for (index, value) in self.reject.iter().copied().enumerate() {
            if value < 1 || i32::from(value) > index as i32 + 1 {
                return Err(Self::corrupt("reject", index, "value is outside its heuristic range"));
            }
        }

        let mut free_seen = Vec::new();
        persistence::v1::reserve_exact(&mut free_seen, self.size)?;
        free_seen.resize(self.size, false);
        for block_index in 0..active_blocks {
            let block = &self.blocks[block_index];
            let minimum = if block_index == 0 { 1 } else { 0 };
            if i32::from(block.num) < minimum || block.num > 256 {
                return Err(Self::corrupt("blocks", block_index, "invalid free-slot count"));
            }
            if block.reject < 1 || block.reject > 257 {
                return Err(Self::corrupt("blocks", block_index, "invalid rejection threshold"));
            }
            if block.trial < 0 || block.trial > self.max_trial {
                return Err(Self::corrupt("blocks", block_index, "invalid trial count"));
            }

            let free_count = block.num as usize - usize::from(block_index == 0);
            if free_count == 0 {
                continue;
            }
            let block_start = block_index * 256;
            let block_end = block_start + 256;
            let head = usize::try_from(block.e_head)
                .ok()
                .filter(|head| (*head >= block_start + usize::from(block_index == 0)) && *head < block_end)
                .ok_or_else(|| Self::corrupt("blocks", block_index, "free-list head is outside its block"))?;
            let mut current = head;
            for position in 0..free_count {
                if free_seen[current] {
                    return Err(Self::corrupt("free-list", current, "node appears more than once"));
                }
                let node = self.array[current];
                if node.base_ >= 0 || node.check >= 0 {
                    return Err(Self::corrupt("free-list", current, "free node has nonnegative link"));
                }
                let previous = usize::try_from(-i64::from(node.base_))
                    .ok()
                    .filter(|value| *value >= block_start && *value < block_end)
                    .ok_or_else(|| Self::corrupt("free-list", current, "previous link leaves block"))?;
                let next = usize::try_from(-i64::from(node.check))
                    .ok()
                    .filter(|value| *value >= block_start && *value < block_end)
                    .ok_or_else(|| Self::corrupt("free-list", current, "next link leaves block"))?;
                if self.array[previous].check != -(current as i32) || self.array[next].base_ != -(current as i32) {
                    return Err(Self::corrupt("free-list", current, "links are not reciprocal"));
                }
                free_seen[current] = true;
                current = next;
                if current == head && position + 1 != free_count {
                    return Err(Self::corrupt("free-list", head, "cycle is shorter than block count"));
                }
            }
            if current != head {
                return Err(Self::corrupt("free-list", head, "cycle is longer than block count"));
            }
        }

        for (index, is_free) in free_seen.iter().copied().enumerate().skip(1) {
            if (self.array[index].check < 0) != is_free {
                return Err(Self::corrupt("free-list", index, "free-node membership mismatch"));
            }
            if is_free && (self.n_infos[index].child != 0 || self.n_infos[index].sibling != 0) {
                return Err(Self::corrupt("n_infos", index, "free node retains trie links"));
            }
        }

        self.validate_block_lists(active_blocks)?;
        self.validate_trie_graph(&free_seen)
    }

    #[cfg(feature = "std")]
    fn validate_block_lists(&self, active_blocks: usize) -> Result<(), CedarPersistenceError> {
        if self.blocks[0].trial != 0 {
            return Err(Self::corrupt("blocks", 0, "root block has trial metadata"));
        }
        let mut membership = Vec::new();
        persistence::v1::reserve_exact(&mut membership, active_blocks)?;
        membership.resize(active_blocks, 0_u8);
        let mut full_sentinel_seen = false;
        for (kind, head) in [
            (1_u8, self.blocks_head_open),
            (2_u8, self.blocks_head_closed),
            (3_u8, self.blocks_head_full),
        ] {
            if head == 0 {
                continue;
            }
            let head = usize::try_from(head)
                .ok()
                .filter(|head| *head > 0 && *head < active_blocks)
                .ok_or_else(|| Self::corrupt("block-lists", 0, "head is outside active blocks"))?;
            let mut current = head;
            loop {
                if current == 0 {
                    if kind != 3 || full_sentinel_seen {
                        return Err(Self::corrupt("block-lists", 0, "invalid block-zero sentinel"));
                    }
                    full_sentinel_seen = true;
                } else {
                    if membership[current] != 0 {
                        return Err(Self::corrupt("block-lists", current, "block occurs in multiple lists"));
                    }
                    membership[current] = kind;
                }
                let block = &self.blocks[current];
                let previous = usize::try_from(block.prev)
                    .ok()
                    .filter(|value| *value < active_blocks && (kind == 3 || *value > 0))
                    .ok_or_else(|| Self::corrupt("block-lists", current, "previous block is invalid"))?;
                let next = usize::try_from(block.next)
                    .ok()
                    .filter(|value| *value < active_blocks && (kind == 3 || *value > 0))
                    .ok_or_else(|| Self::corrupt("block-lists", current, "next block is invalid"))?;
                if self.blocks[previous].next != current as i32 || self.blocks[next].prev != current as i32 {
                    return Err(Self::corrupt("block-lists", current, "links are not reciprocal"));
                }
                current = next;
                if current == head {
                    break;
                }
            }
        }

        if self.blocks_head_full == 0 {
            if self.blocks[0].prev != 0 || self.blocks[0].next != 0 || full_sentinel_seen {
                return Err(Self::corrupt("block-lists", 0, "unused full-list sentinel has links"));
            }
        } else if !full_sentinel_seen {
            return Err(Self::corrupt("block-lists", 0, "full list omits block-zero sentinel"));
        }

        for (index, actual) in membership.iter().copied().enumerate().skip(1) {
            let block = &self.blocks[index];
            let expected = if block.num == 0 {
                3
            } else if block.num == 1 || block.trial == self.max_trial {
                2
            } else {
                1
            };
            if actual != expected {
                return Err(Self::corrupt("block-lists", index, "block is in the wrong category"));
            }
        }
        Ok(())
    }

    #[cfg(feature = "std")]
    fn validate_trie_graph(&self, free_seen: &[bool]) -> Result<(), CedarPersistenceError> {
        let mut reached = Vec::new();
        persistence::v1::reserve_exact(&mut reached, self.size)?;
        reached.resize(self.size, false);
        let mut stack = Vec::new();
        persistence::v1::reserve_exact(&mut stack, self.size)?;
        stack.push((0_usize, false));
        let mut entry_count = 0_usize;

        while let Some((index, terminal)) = stack.pop() {
            if reached[index] {
                return Err(Self::corrupt("trie", index, "node is reachable more than once"));
            }
            if free_seen[index] {
                return Err(Self::corrupt("trie", index, "free node is reachable"));
            }
            reached[index] = true;
            let node = self.array[index];

            if terminal {
                if !(MIN_VALUE..=MAX_VALUE).contains(&node.base_) || self.n_infos[index].child != 0 {
                    return Err(Self::corrupt("trie", index, "invalid terminal value node"));
                }
                entry_count += 1;
                continue;
            }

            #[cfg(feature = "reduced-trie")]
            let structural = if node.base_ < 0 {
                true
            } else if (MIN_VALUE..=MAX_VALUE).contains(&node.base_) {
                if self.n_infos[index].child != 0 {
                    return Err(Self::corrupt("trie", index, "inline value node has children"));
                }
                entry_count += 1;
                false
            } else if node.base_ == CEDAR_VALUE_LIMIT {
                if self.n_infos[index].child != 0 {
                    return Err(Self::corrupt("trie", index, "empty leaf has children"));
                }
                false
            } else {
                return Err(Self::corrupt("trie", index, "invalid reduced-layout sentinel"));
            };

            #[cfg(not(feature = "reduced-trie"))]
            let structural = if node.base_ == -1 {
                if self.n_infos[index].child != 0 {
                    return Err(Self::corrupt("trie", index, "leaf has children"));
                }
                false
            } else if node.base_ >= 0 {
                true
            } else {
                return Err(Self::corrupt("trie", index, "invalid default-layout base"));
            };

            if !structural {
                continue;
            }
            let base = usize::try_from(node.base())
                .ok()
                .filter(|base| *base < self.size)
                .ok_or_else(|| Self::corrupt("trie", index, "base is outside active nodes"))?;

            let mut label = if index == 0 {
                if self.n_infos[0].child != 0 {
                    return Err(Self::corrupt("trie", 0, "root child sentinel is not zero"));
                }
                self.n_infos[base].sibling
            } else {
                self.n_infos[index].child
            };
            let mut saw_labels = [false; 256];
            let mut child_count = 0_usize;
            if index != 0 && label == 0 && self.array[base].check == index as i32 {
                saw_labels[0] = true;
                stack.push((base, true));
                child_count += 1;
                label = self.n_infos[base].sibling;
            }

            let mut previous = 0_u8;
            while label != 0 {
                if saw_labels[label as usize] {
                    return Err(Self::corrupt("siblings", index, "sibling chain contains a cycle"));
                }
                if self.ordered && previous != 0 && label <= previous {
                    return Err(Self::corrupt("siblings", index, "ordered sibling chain is not sorted"));
                }
                saw_labels[label as usize] = true;
                let child = base ^ usize::from(label);
                if child >= self.size || free_seen[child] || self.array[child].check != index as i32 {
                    return Err(Self::corrupt("siblings", index, "child ownership link is invalid"));
                }
                stack.push((child, false));
                child_count += 1;
                previous = label;
                label = self.n_infos[child].sibling;
            }
            if child_count == 0 && !(index == 0 && self.entries == 0) {
                return Err(Self::corrupt("trie", index, "structural node has no children"));
            }
        }

        for index in 0..self.size {
            if !free_seen[index] && !reached[index] {
                return Err(Self::corrupt("trie", index, "occupied node is unreachable"));
            }
        }
        if entry_count != self.entries {
            return Err(Self::corrupt("entries", 0, "entry count does not match trie values"));
        }
        Ok(())
    }

    /// SAFETY: `i` must be a valid index into `self.array`. Query callers derive indices from an
    /// occupied node's base and one byte label; allocation reserves complete 256-slot blocks, so
    /// XOR with a byte remains inside the allocated array. Parent `check` links and query cursors
    /// always refer to occupied slots. Debug builds retain the bounds assertion below.
    #[inline(always)]
    unsafe fn node_unchecked(&self, i: usize) -> &Node {
        debug_assert!(
            i < self.array.len(),
            "node_unchecked: index {} out of bounds (len {})",
            i,
            self.array.len()
        );
        self.array.get_unchecked(i)
    }

    /// SAFETY: `i` must be a valid index into `self.n_infos`. `n_infos` is initialized and grown in
    /// lockstep with `array`; query callers use either a validated node index or an occupied node's
    /// base XOR a recorded child label. Debug builds retain the bounds assertion below.
    #[inline(always)]
    unsafe fn ninfo_unchecked(&self, i: usize) -> &NInfo {
        debug_assert!(
            i < self.n_infos.len(),
            "ninfo_unchecked: index {} out of bounds (len {})",
            i,
            self.n_infos.len()
        );
        self.n_infos.get_unchecked(i)
    }

    fn validate_entry(key: &[u8], value: i32) -> Result<(), CedarError> {
        if key.is_empty() {
            return Err(CedarError::EmptyKey);
        }
        if let Some(position) = key.iter().position(|byte| *byte == 0) {
            return Err(CedarError::NulByte { position });
        }
        if !(MIN_VALUE..=MAX_VALUE).contains(&value) {
            return Err(CedarError::InvalidValue { value });
        }
        Ok(())
    }

    fn validate_sorted_entries<K: AsRef<[u8]>>(key_values: &[(K, i32)]) -> Result<(), CedarError> {
        for (index, (key, value)) in key_values.iter().enumerate() {
            let key = key.as_ref();
            Self::validate_entry(key, *value)?;
            if index != 0 {
                match key_values[index - 1].0.as_ref().cmp(key) {
                    core::cmp::Ordering::Less => {}
                    core::cmp::Ordering::Equal => return Err(CedarError::DuplicateKey { index }),
                    core::cmp::Ordering::Greater => return Err(CedarError::KeysNotSorted { index }),
                }
            }
        }
        Ok(())
    }

    // Build an already validated sorted range by allocating each node's complete sibling set in
    // one operation. This intentionally does not use update_/follow/resolve: the sorted ranges
    // expose every child label before allocation, so conflicts never need to be introduced.
    fn build_sorted_validated<K: AsRef<[u8]>>(&mut self, key_values: &[(K, i32)]) {
        if key_values.is_empty() {
            return;
        }

        // (parent node, first key, one-past-last key, shared prefix length)
        let mut pending = vec![(0_usize, 0_usize, key_values.len(), 0_usize)];

        while let Some((from, start, end, depth)) = pending.pop() {
            let terminal = (key_values[start].0.as_ref().len() == depth).then_some(key_values[start].1);

            #[cfg(feature = "reduced-trie")]
            if end == start + 1 {
                if let Some(value) = terminal {
                    self.array[from].base_ = value;
                    continue;
                }
            }

            let mut labels = SmallVec::<[u8; 256]>::new();
            let mut cursor = start;
            if terminal.is_some() {
                labels.push(0);
                cursor += 1;
            }

            while cursor < end {
                let label = key_values[cursor].0.as_ref()[depth];
                labels.push(label);
                cursor += 1;
                while cursor < end
                    && key_values[cursor].0.as_ref().len() > depth
                    && key_values[cursor].0.as_ref()[depth] == label
                {
                    cursor += 1;
                }
            }

            let base = self.allocate_bulk_siblings(from, &labels);
            if let Some(value) = terminal {
                self.array[base as usize].base_ = value;
            }

            cursor = start + usize::from(terminal.is_some());
            while cursor < end {
                let child_start = cursor;
                let label = key_values[cursor].0.as_ref()[depth];
                cursor += 1;
                while cursor < end
                    && key_values[cursor].0.as_ref().len() > depth
                    && key_values[cursor].0.as_ref()[depth] == label
                {
                    cursor += 1;
                }
                pending.push(((base ^ i32::from(label)) as usize, child_start, cursor, depth + 1));
            }
        }

        self.entries = key_values.len();
    }

    fn allocate_bulk_siblings(&mut self, from: usize, labels: &[u8]) -> i32 {
        debug_assert!(!labels.is_empty());
        debug_assert!(labels.windows(2).all(|pair| pair[0] < pair[1]));

        // The root owns block zero at base zero. All other sibling sets use the ordinary block
        // allocator, which updates free-list membership and Block::num exactly as incremental
        // insertion does.
        let base = if from == 0 {
            0
        } else {
            let first = if labels.len() == 1 {
                self.find_place()
            } else {
                self.find_places(labels)
            };
            first ^ i32::from(labels[0])
        };

        #[cfg(feature = "reduced-trie")]
        {
            self.array[from].base_ = -base - 1;
        }
        #[cfg(not(feature = "reduced-trie"))]
        {
            self.array[from].base_ = base;
        }

        if from == 0 {
            // Root label zero is reserved as the traversal anchor. Incremental insertion keeps the
            // first real root label in the root slot's sibling field for erase/predict traversal.
            self.n_infos[from].child = 0;
            self.n_infos[base as usize].sibling = labels[0];
        } else {
            self.n_infos[from].child = labels[0];
        }
        for (index, label) in labels.iter().copied().enumerate() {
            let to = self.pop_e_node(base, label, from as i32);
            self.n_infos[to as usize].sibling = labels.get(index + 1).copied().unwrap_or(0);
        }
        base
    }

    /// Inserts all UTF-8 key/value pairs after validating the complete input.
    ///
    /// If validation fails, the trie is left unchanged. Duplicate keys overwrite earlier values.
    pub fn build(&mut self, key_values: &[(&str, i32)]) -> Result<(), CedarError> {
        for (key, value) in key_values {
            Self::validate_entry(key.as_bytes(), *value)?;
        }
        for (key, value) in key_values {
            self.update_validated(key.as_bytes(), *value);
        }
        Ok(())
    }

    /// Inserts all byte key/value pairs after validating the complete input.
    ///
    /// Keys must be non-empty and must not contain byte `0`, which is cedar's terminal label.
    pub fn build_bytes(&mut self, key_values: &[(&[u8], i32)]) -> Result<(), CedarError> {
        for (key, value) in key_values {
            Self::validate_entry(key, *value)?;
        }
        for (key, value) in key_values {
            self.update_validated(key, *value);
        }
        Ok(())
    }

    /// Inserts or overwrites a UTF-8 key.
    pub fn update(&mut self, key: &str, value: i32) -> Result<(), CedarError> {
        self.update_bytes(key.as_bytes(), value)
    }

    /// Inserts or overwrites a byte key.
    ///
    /// Values must be in [`MIN_VALUE`] through [`MAX_VALUE`]. Keys must be non-empty and cannot
    /// contain byte `0`, which is reserved as the terminal label.
    pub fn update_bytes(&mut self, key: &[u8], value: i32) -> Result<(), CedarError> {
        Self::validate_entry(key, value)?;
        self.update_validated(key, value);
        Ok(())
    }

    fn update_validated(&mut self, key: &[u8], value: i32) {
        let is_new = self.exact_match_search_bytes(key).is_none();
        self.update_(key, value, 0, 0);
        if is_new {
            self.entries += 1;
        }
    }

    // Internal update interface for a validated byte key and cursor.
    fn update_(&mut self, key: &[u8], value: i32, mut from: usize, mut pos: usize) -> i32 {
        while pos < key.len() {
            #[cfg(feature = "reduced-trie")]
            {
                let val_ = self.array[from].base_;
                if val_ >= 0 && val_ != CEDAR_VALUE_LIMIT {
                    let to = self.follow(from, 0);
                    self.array[to as usize].base_ = val_;
                }
            }

            from = self.follow(from, key[pos]) as usize;
            pos += 1;
        }

        #[cfg(feature = "reduced-trie")]
        let to = if self.array[from].base_ >= 0 {
            from as i32
        } else {
            self.follow(from, 0)
        };

        #[cfg(feature = "reduced-trie")]
        {
            if self.array[to as usize].base_ == CEDAR_VALUE_LIMIT {
                self.array[to as usize].base_ = 0;
            }
        }

        #[cfg(not(feature = "reduced-trie"))]
        let to = self.follow(from, 0);

        self.array[to as usize].base_ = value;
        self.array[to as usize].base_
    }

    // To move in the trie by following the `label`, and insert the node if the node is not there,
    // it is used by the `update` to populate the trie.
    #[inline]
    fn follow(&mut self, from: usize, label: u8) -> i32 {
        let base = self.array[from].base();

        let mut to;

        // the node is not there
        if base < 0 || self.array[(base ^ i32::from(label)) as usize].check < 0 {
            // allocate a e node
            to = self.pop_e_node(base, label, from as i32);
            let branch: i32 = to ^ i32::from(label);

            // maintain the info in ninfo
            self.push_sibling(from, branch, label, base >= 0);
        } else {
            // the node is already there and the ownership is not `from`, therefore a conflict.
            to = base ^ i32::from(label);
            if self.array[to as usize].check != (from as i32) {
                // call `resolve` to relocate.
                to = self.resolve(from, base, label);
            }
        }

        to
    }

    // Find key from double array trie, with `from` as the cursor to traverse the nodes.
    //
    // SAFETY: The inner loop uses unchecked indexing for performance (this loop runs once per byte
    // of the key and is the primary query bottleneck). Indices are valid by trie structural
    // invariants: `from` starts at the root or the previously ownership-checked child; an occupied
    // branch's base points into a complete 256-slot block, so XOR with a byte remains allocated.
    // In reduced layout, a nonnegative base is a terminal value and traversal stops before using it
    // as an index. Debug builds retain bounds assertions in the helpers.
    #[inline]
    fn find(&self, key: &[u8], from: &mut usize) -> Option<i32> {
        let mut pos = 0;

        while pos < key.len() {
            #[cfg(feature = "reduced-trie")]
            {
                if unsafe { self.node_unchecked(*from) }.base_ >= 0 {
                    break;
                }
            }

            let to = (unsafe { self.node_unchecked(*from) }.base() ^ i32::from(key[pos])) as usize;
            if unsafe { self.node_unchecked(to) }.check != (*from as i32) {
                return None;
            }

            *from = to;
            pos += 1;
        }

        #[cfg(feature = "reduced-trie")]
        {
            if self.array[*from].base_ >= 0 {
                if pos == key.len() {
                    return Some(self.array[*from].base_);
                } else {
                    return None;
                }
            }
        }

        // return the value of the node if `check` is correctly marked for the ownership, otherwise
        // it means no value is stored.
        let n = &self.array[(self.array[*from].base()) as usize];
        if n.check != (*from as i32) {
            Some(CEDAR_NO_VALUE)
        } else {
            Some(n.base_)
        }
    }

    /// Deletes a UTF-8 key, returning whether an entry was removed.
    pub fn erase(&mut self, key: &str) -> bool {
        self.erase_bytes(key.as_bytes())
    }

    /// Deletes a byte key, returning whether an entry was removed.
    ///
    /// Empty keys and keys containing byte `0` are not representable and return `false`.
    pub fn erase_bytes(&mut self, key: &[u8]) -> bool {
        if key.is_empty() || key.contains(&0) {
            return false;
        }
        let mut from = 0;

        // move the cursor to the right place and use erase__ to delete it.
        if let Some(v) = self.find(key, &mut from) {
            if v != CEDAR_NO_VALUE {
                self.erase__(from);
                self.entries -= 1;
                return true;
            }
        }
        false
    }

    fn erase__(&mut self, mut from: usize) {
        #[cfg(feature = "reduced-trie")]
        let mut e: i32 = if self.array[from].base_ >= 0 {
            from as i32
        } else {
            self.array[from].base()
        };

        #[cfg(feature = "reduced-trie")]
        {
            from = self.array[e as usize].check as usize;
        }

        #[cfg(not(feature = "reduced-trie"))]
        let mut e = self.array[from].base();

        loop {
            let base = self.array[from].base();
            let has_sibling = self.n_infos[(base ^ i32::from(self.n_infos[from].child)) as usize].sibling != 0;

            // if the node has siblings, then remove `e` from the sibling.
            if has_sibling {
                self.pop_sibling(from as i32, base, (base ^ e) as u8);
            }

            // maintain the data structures.
            self.push_e_node(e);
            e = from as i32;

            // traverse to the parent.
            from = self.array[from].check as usize;

            // if it has sibling then this layer has more than one nodes, then we are done.
            if has_sibling {
                break;
            }
        }
    }

    /// Looks up an exact UTF-8 key.
    pub fn exact_match_search(&self, key: &str) -> Option<(i32, usize)> {
        self.exact_match_search_bytes(key.as_bytes())
    }

    /// Looks up an exact byte key.
    ///
    /// Empty keys and keys containing byte `0` are not representable and return `None`.
    pub fn exact_match_search_bytes(&self, key: &[u8]) -> Option<(i32, usize)> {
        if key.is_empty() || key.contains(&0) {
            return None;
        }
        let mut from = 0;

        if let Some(value) = self.find(key, &mut from) {
            if value == CEDAR_NO_VALUE {
                return None;
            }

            Some((value, key.len()))
        } else {
            None
        }
    }

    /// Iterates over stored keys that are prefixes of a UTF-8 query.
    pub fn common_prefix_iter<'a>(&'a self, key: &'a str) -> PrefixIter<'a> {
        self.common_prefix_iter_bytes(key.as_bytes())
    }

    /// Iterates over stored keys that are prefixes of a byte query.
    pub fn common_prefix_iter_bytes<'a>(&'a self, key: &'a [u8]) -> PrefixIter<'a> {
        PrefixIter {
            cedar: self,
            key,
            from: 0,
            i: 0,
        }
    }

    /// Collects stored keys that are prefixes of a UTF-8 query.
    pub fn common_prefix_search(&self, key: &str) -> Vec<(i32, usize)> {
        self.common_prefix_search_bytes(key.as_bytes())
    }

    /// Collects stored keys that are prefixes of a byte query.
    pub fn common_prefix_search_bytes(&self, key: &[u8]) -> Vec<(i32, usize)> {
        self.common_prefix_iter_bytes(key).collect()
    }

    /// Iterates over stored UTF-8 keys that start with `key`.
    pub fn common_prefix_predict_iter<'a>(&'a self, key: &'a str) -> PrefixPredictIter<'a> {
        self.common_prefix_predict_iter_bytes(key.as_bytes())
    }

    /// Iterates over stored byte keys that start with `key`.
    pub fn common_prefix_predict_iter_bytes<'a>(&'a self, key: &'a [u8]) -> PrefixPredictIter<'a> {
        PrefixPredictIter {
            cedar: self,
            key,
            started: false,
            from: 0,
            p: 0,
            root: 0,
            value: None,
            valid: !key.contains(&0),
        }
    }

    /// Collects stored UTF-8 keys that start with `key`.
    pub fn common_prefix_predict(&self, key: &str) -> Vec<(i32, usize)> {
        self.common_prefix_predict_bytes(key.as_bytes())
    }

    /// Collects stored byte keys that start with `key`.
    pub fn common_prefix_predict_bytes(&self, key: &[u8]) -> Vec<(i32, usize)> {
        self.common_prefix_predict_iter_bytes(key).collect()
    }

    /// Iterates over all entries as owned byte keys and values.
    ///
    /// Order follows the current double-array slot layout and is not stable across mutations or
    /// cedarwood versions.
    pub fn entries(&self) -> Entries<'_> {
        Entries {
            cedar: self,
            stack: vec![(0, Vec::new())],
            remaining: self.len(),
        }
    }

    /// Iterates over all entries as UTF-8 strings and values.
    ///
    /// Entries inserted through byte APIs may not be UTF-8. Those items are returned as
    /// [`FromUtf8Error`] rather than being skipped or converted lossily.
    pub fn entries_str(&self) -> Utf8Entries<'_> {
        Utf8Entries { inner: self.entries() }
    }

    fn value_at_node(&self, index: usize) -> Option<i32> {
        let node = self.array[index];
        #[cfg(feature = "reduced-trie")]
        {
            if (MIN_VALUE..=MAX_VALUE).contains(&node.base_) {
                return Some(node.base_);
            }
        }

        let base = node.base();
        if base < 0 {
            return None;
        }
        let terminal = self.array[base as usize];
        (terminal.check == index as i32 && (MIN_VALUE..=MAX_VALUE).contains(&terminal.base_)).then_some(terminal.base_)
    }

    fn child_labels(&self, index: usize) -> SmallVec<[u8; 256]> {
        #[cfg(feature = "reduced-trie")]
        if self.array[index].base_ >= 0 {
            return SmallVec::new();
        }

        let base = self.array[index].base();
        if base < 0 {
            return SmallVec::new();
        }

        let mut labels = SmallVec::new();
        let mut label = self.n_infos[index].child;
        if label == 0 {
            label = self.n_infos[base as usize].sibling;
        }
        while label != 0 {
            labels.push(label);
            label = self.n_infos[(base ^ i32::from(label)) as usize].sibling;
        }
        labels
    }

    // To get the cursor of the first leaf node starting by `from`
    //
    // SAFETY: `from` is the root or a cursor returned by `find`/`next`. Recorded child and sibling
    // labels describe occupied children of that cursor; their parent base points into a complete
    // 256-slot block, so `base ^ child` remains allocated. `array` and `n_infos` have equal length.
    // This function is called per-result in common_prefix_predict and is performance-critical.
    #[inline]
    fn begin(&self, mut from: usize, mut p: usize) -> (Option<i32>, usize, usize) {
        let mut c = unsafe { self.ninfo_unchecked(from) }.child;

        if from == 0 {
            let base = unsafe { self.node_unchecked(0) }.base();
            c = unsafe { self.ninfo_unchecked((base ^ i32::from(c)) as usize) }.sibling;

            if c == 0 {
                return (None, from, p);
            }
        }

        while c != 0 {
            from = (unsafe { self.node_unchecked(from) }.base() ^ i32::from(c)) as usize;
            c = unsafe { self.ninfo_unchecked(from) }.child;
            p += 1;
        }

        let node = unsafe { self.node_unchecked(from) };

        #[cfg(feature = "reduced-trie")]
        if node.base_ >= 0 {
            return (Some(node.base_), from, p);
        }

        let v = unsafe { self.node_unchecked(node.base() as usize) }.base_;
        (Some(v), from, p)
    }

    // To move the cursor from one leaf to the next for the common_prefix_predict.
    //
    // SAFETY: `from` and `root` come from `find`/`begin`. Occupied nodes have `check` links to valid
    // parents, recorded sibling labels identify occupied children, and each parent base points into
    // a complete 256-slot block. Thus every parent and `base ^ sibling` index remains allocated;
    // `array` and `n_infos` are grown in lockstep.
    // This function is called per-result in common_prefix_predict and is performance-critical.
    #[inline]
    fn next(&self, mut from: usize, mut p: usize, root: usize) -> (Option<i32>, usize, usize) {
        #[cfg(feature = "reduced-trie")]
        let mut c: u8 = {
            let node = unsafe { self.node_unchecked(from) };
            if node.base_ < 0 {
                unsafe { self.ninfo_unchecked(node.base() as usize) }.sibling
            } else {
                0
            }
        };

        #[cfg(not(feature = "reduced-trie"))]
        let mut c: u8 = {
            let base = unsafe { self.node_unchecked(from) }.base();
            unsafe { self.ninfo_unchecked(base as usize) }.sibling
        };

        while c == 0 && from != root {
            c = unsafe { self.ninfo_unchecked(from) }.sibling;
            from = unsafe { self.node_unchecked(from) }.check as usize;

            p -= 1;
        }

        if c != 0 {
            from = (unsafe { self.node_unchecked(from) }.base() ^ i32::from(c)) as usize;
            self.begin(from, p + 1)
        } else {
            (None, from, p)
        }
    }

    // pop a block at idx from the linked-list of type `from`, specially handled if it is the last
    // one in the linked-list.
    fn pop_block(&mut self, idx: i32, from: BlockType, last: bool) {
        let head: &mut i32 = match from {
            BlockType::Open => &mut self.blocks_head_open,
            BlockType::Closed => &mut self.blocks_head_closed,
            BlockType::Full => &mut self.blocks_head_full,
        };

        if last {
            *head = 0;
        } else {
            let b_prev = self.blocks[idx as usize].prev;
            let b_next = self.blocks[idx as usize].next;
            self.blocks[b_prev as usize].next = b_next;
            self.blocks[b_next as usize].prev = b_prev;

            if idx == *head {
                *head = b_next;
            }
        }
    }

    // return the block at idx to the linked-list of `to`, specially handled if the linked-list is
    // empty
    fn push_block(&mut self, idx: i32, to: BlockType, empty: bool) {
        let head: &mut i32 = match to {
            BlockType::Open => &mut self.blocks_head_open,
            BlockType::Closed => &mut self.blocks_head_closed,
            BlockType::Full => &mut self.blocks_head_full,
        };

        if empty {
            self.blocks[idx as usize].next = idx;
            self.blocks[idx as usize].prev = idx;
            *head = idx;
        } else {
            self.blocks[idx as usize].prev = self.blocks[*head as usize].prev;
            self.blocks[idx as usize].next = *head;

            let t = self.blocks[*head as usize].prev;
            self.blocks[t as usize].next = idx;
            self.blocks[*head as usize].prev = idx;
            *head = idx;
        }
    }

    /// Reallocate more spaces so that we have more free blocks.
    fn add_block(&mut self) -> i32 {
        if self.size == self.capacity {
            self.capacity += self.capacity;

            self.array.resize(self.capacity, Default::default());
            self.n_infos.resize(self.capacity, Default::default());
            self.blocks.resize(self.capacity >> 8, Block::new());
        }

        self.blocks[self.size >> 8].e_head = self.size as i32;

        // make it a doubly linked list
        self.array[self.size] = Node {
            base_: -((self.size as i32) + 255),
            check: -((self.size as i32) + 1),
        };

        for i in (self.size + 1)..(self.size + 255) {
            self.array[i] = Node {
                base_: -(i as i32 - 1),
                check: -(i as i32 + 1),
            };
        }

        self.array[self.size + 255] = Node {
            base_: -((self.size as i32) + 254),
            check: -(self.size as i32),
        };

        let is_empty = self.blocks_head_open == 0;
        let idx = (self.size >> 8) as i32;
        debug_assert!(self.blocks[idx as usize].num > 1);
        self.push_block(idx, BlockType::Open, is_empty);

        self.size += 256;

        ((self.size >> 8) - 1) as i32
    }

    // transfer the block at idx from the linked-list of `from` to the linked-list of `to`,
    // specially handle the case where the destination linked-list is empty.
    fn transfer_block(&mut self, idx: i32, from: BlockType, to: BlockType, to_block_empty: bool) {
        let is_last = idx == self.blocks[idx as usize].next; //it's the last one if the next points to itself
        let is_empty = to_block_empty && (self.blocks[idx as usize].num != 0);

        self.pop_block(idx, from, is_last);
        self.push_block(idx, to, is_empty);
    }

    /// Mark an edge `e` as used in a trie node.
    fn pop_e_node(&mut self, base: i32, label: u8, from: i32) -> i32 {
        let e: i32 = if base < 0 {
            self.find_place()
        } else {
            base ^ i32::from(label)
        };

        let idx = e >> 8;
        let n_base = self.array[e as usize].base_;
        let n_check = self.array[e as usize].check;

        self.blocks[idx as usize].num -= 1;
        // move the block at idx to the correct linked-list depending the free slots it still have.
        // Block zero's count includes the reserved root slot, which is never on its free list.
        if self.blocks[idx as usize].num == i16::from(idx == 0) {
            if idx != 0 {
                self.transfer_block(idx, BlockType::Closed, BlockType::Full, self.blocks_head_full == 0);
            }
        } else {
            self.array[(-n_base) as usize].check = n_check;
            self.array[(-n_check) as usize].base_ = n_base;

            if e == self.blocks[idx as usize].e_head {
                self.blocks[idx as usize].e_head = -n_check;
            }

            if idx != 0 && self.blocks[idx as usize].num == 1 && self.blocks[idx as usize].trial != self.max_trial {
                self.transfer_block(idx, BlockType::Open, BlockType::Closed, self.blocks_head_closed == 0);
            }
        }

        #[cfg(feature = "reduced-trie")]
        {
            self.array[e as usize].base_ = CEDAR_VALUE_LIMIT;
            self.array[e as usize].check = from;
            if base < 0 {
                self.array[from as usize].base_ = -(e ^ i32::from(label)) - 1;
            }
        }

        #[cfg(not(feature = "reduced-trie"))]
        {
            if label != 0 {
                self.array[e as usize].base_ = -1;
            } else {
                self.array[e as usize].base_ = 0;
            }
            self.array[e as usize].check = from;
            if base < 0 {
                self.array[from as usize].base_ = e ^ i32::from(label);
            }
        }

        e
    }

    /// Mark an edge `e` as free in a trie node.
    fn push_e_node(&mut self, e: i32) {
        let idx = e >> 8;
        self.blocks[idx as usize].num += 1;

        // A physically full root block has num == 1 because the reserved root is counted. Its
        // stale e_head refers to an occupied node, so create a fresh singleton free list here.
        if self.blocks[idx as usize].num == 1 + i16::from(idx == 0) {
            self.blocks[idx as usize].e_head = e;
            self.array[e as usize] = Node { base_: -e, check: -e };

            if idx != 0 {
                // Move the block from 'Full' to 'Closed' since it has one free slot now.
                self.transfer_block(idx, BlockType::Full, BlockType::Closed, self.blocks_head_closed == 0);
            }
        } else {
            let prev = self.blocks[idx as usize].e_head;

            let next = -self.array[prev as usize].check;

            // Insert to the edge immediately after the e_head
            self.array[e as usize] = Node {
                base_: -prev,
                check: -next,
            };

            self.array[prev as usize].check = -e;
            self.array[next as usize].base_ = -e;

            // Move the block from 'Closed' to 'Open' since it has more than one free slot now.
            if self.blocks[idx as usize].num == 2 || self.blocks[idx as usize].trial == self.max_trial {
                debug_assert!(self.blocks[idx as usize].num > 1);
                if idx != 0 {
                    self.transfer_block(idx, BlockType::Closed, BlockType::Open, self.blocks_head_open == 0);
                }
            }

            // Reset the trial stats
            self.blocks[idx as usize].trial = 0;
        }

        if self.blocks[idx as usize].reject < self.reject[self.blocks[idx as usize].num as usize] {
            self.blocks[idx as usize].reject = self.reject[self.blocks[idx as usize].num as usize];
        }

        self.n_infos[e as usize] = Default::default();
    }

    // push the `label` into the sibling chain
    fn push_sibling(&mut self, from: usize, base: i32, label: u8, has_child: bool) {
        let keep_order: bool = if self.ordered {
            label > self.n_infos[from].child
        } else {
            self.n_infos[from].child == 0
        };

        let sibling: u8;
        {
            let mut c: &mut u8 = &mut self.n_infos[from].child;
            if has_child && keep_order {
                loop {
                    let code = i32::from(*c);
                    c = &mut self.n_infos[(base ^ code) as usize].sibling;

                    if !(self.ordered && (*c != 0) && (*c < label)) {
                        break;
                    }
                }
            }
            sibling = *c;

            *c = label;
        }

        self.n_infos[(base ^ i32::from(label)) as usize].sibling = sibling;
    }

    // remove the `label` from the sibling chain.
    fn pop_sibling(&mut self, from: i32, base: i32, label: u8) {
        let mut idx = from as usize;
        let mut is_child = true;

        loop {
            let next_label = if is_child {
                self.n_infos[idx].child
            } else {
                self.n_infos[idx].sibling
            };

            if next_label == label {
                let sibling_of_target = self.n_infos[(base ^ i32::from(label)) as usize].sibling;
                if is_child {
                    self.n_infos[idx].child = sibling_of_target;
                } else {
                    self.n_infos[idx].sibling = sibling_of_target;
                }
                return;
            }

            idx = (base ^ i32::from(next_label)) as usize;
            is_child = false;
        }
    }

    // Loop through the siblings to see which one reached the end first, which means it is the one
    // with smaller in children size, and we should try to relocate the smaller one.
    fn consult(&self, base_n: i32, base_p: i32, mut c_n: u8, mut c_p: u8) -> bool {
        loop {
            c_n = self.n_infos[(base_n ^ i32::from(c_n)) as usize].sibling;
            c_p = self.n_infos[(base_p ^ i32::from(c_p)) as usize].sibling;

            if !(c_n != 0 && c_p != 0) {
                break;
            }
        }

        c_p != 0
    }

    // Collect the list of the children, and push the label as well if it is not terminal node.
    fn set_child(&self, base: i32, mut c: u8, label: u8, not_terminal: bool) -> SmallVec<[u8; 256]> {
        let mut child: SmallVec<[u8; 256]> = SmallVec::new();

        if c == 0 {
            child.push(c);
            c = self.n_infos[(base ^ i32::from(c)) as usize].sibling;
        }

        if self.ordered {
            while c != 0 && c <= label {
                child.push(c);
                c = self.n_infos[(base ^ i32::from(c)) as usize].sibling;
            }
        }

        if not_terminal {
            child.push(label);
        }

        while c != 0 {
            child.push(c);
            c = self.n_infos[(base ^ i32::from(c)) as usize].sibling;
        }

        child
    }

    // For the case where only one free slot is needed
    fn find_place(&mut self) -> i32 {
        if self.blocks_head_closed != 0 {
            return self.blocks[self.blocks_head_closed as usize].e_head;
        }

        if self.blocks_head_open != 0 {
            return self.blocks[self.blocks_head_open as usize].e_head;
        }

        // the block is not enough, resize it and allocate it.
        self.add_block() << 8
    }

    // For the case where multiple free slots are needed.
    fn find_places(&mut self, child: &[u8]) -> i32 {
        let mut idx = self.blocks_head_open;

        // we still have available 'Open' blocks.
        if idx != 0 {
            debug_assert!(self.blocks[idx as usize].num > 1);
            let bz = self.blocks[self.blocks_head_open as usize].prev;
            let nc = child.len() as i16;

            loop {
                // only proceed if the free slots are more than the number of children. Also, we
                // save the minimal number of attempts to fail in the `reject`, it only worths to
                // try out this block if the number of children is less than that number.
                if self.blocks[idx as usize].num >= nc && nc < self.blocks[idx as usize].reject {
                    let mut e = self.blocks[idx as usize].e_head;
                    loop {
                        let base = e ^ i32::from(child[0]);

                        let mut i = 1;
                        // iterate through the children to see if they are available: (check < 0)
                        while self.array[(base ^ i32::from(child[i])) as usize].check < 0 {
                            if i == child.len() - 1 {
                                // we have found the available block.
                                self.blocks[idx as usize].e_head = e;
                                return e;
                            }
                            i += 1;
                        }

                        // we save the next free block's information in `check`
                        e = -self.array[e as usize].check;
                        if e == self.blocks[idx as usize].e_head {
                            break;
                        }
                    }
                }

                // we broke out of the loop, that means we failed. We save the information in
                // `reject` for future pruning.
                self.blocks[idx as usize].reject = nc;
                if self.blocks[idx as usize].reject < self.reject[self.blocks[idx as usize].num as usize] {
                    // put this stats into the global array of information as well.
                    self.reject[self.blocks[idx as usize].num as usize] = self.blocks[idx as usize].reject;
                }

                let idx_ = self.blocks[idx as usize].next;

                self.blocks[idx as usize].trial += 1;

                // move this block to the 'Closed' block list since it has reached the max_trial
                if self.blocks[idx as usize].trial == self.max_trial {
                    self.transfer_block(idx, BlockType::Open, BlockType::Closed, self.blocks_head_closed == 0);
                }

                // we have finished one round of this cyclic doubly-linked-list.
                if idx == bz {
                    break;
                }

                // going to the next in this linked list group
                idx = idx_;
            }
        }

        self.add_block() << 8
    }

    // resolve the conflict by moving one of the the nodes to a free block.
    fn resolve(&mut self, mut from_n: usize, base_n: i32, label_n: u8) -> i32 {
        let to_pn = base_n ^ i32::from(label_n);

        // the `base` and `from` for the conflicting one.
        let from_p = self.array[to_pn as usize].check;
        let base_p = self.array[from_p as usize].base();

        // whether to replace siblings of newly added
        let flag = self.consult(
            base_n,
            base_p,
            self.n_infos[from_n].child,
            self.n_infos[from_p as usize].child,
        );

        // collect the list of children for the block that we are going to relocate.
        let children = if flag {
            self.set_child(base_n, self.n_infos[from_n].child, label_n, true)
        } else {
            self.set_child(base_p, self.n_infos[from_p as usize].child, 255, false)
        };

        // decide which algorithm to allocate free block depending on the number of children we
        // have.
        let mut base = if children.len() == 1 {
            self.find_place()
        } else {
            self.find_places(&children)
        };

        base ^= i32::from(children[0]);

        let (from, base_) = if flag {
            (from_n as i32, base_n)
        } else {
            (from_p, base_p)
        };

        if flag && children[0] == label_n {
            self.n_infos[from as usize].child = label_n;
        }

        #[cfg(feature = "reduced-trie")]
        {
            self.array[from as usize].base_ = -base - 1;
        }

        #[cfg(not(feature = "reduced-trie"))]
        {
            self.array[from as usize].base_ = base;
        }

        // the actual work for relocating the children
        for i in 0..(children.len()) {
            let to = self.pop_e_node(base, children[i], from);
            let to_ = base_ ^ i32::from(children[i]);

            if i == children.len() - 1 {
                self.n_infos[to as usize].sibling = 0;
            } else {
                self.n_infos[to as usize].sibling = children[i + 1];
            }

            if flag && to_ == to_pn {
                continue;
            }

            self.array[to as usize].base_ = self.array[to_ as usize].base_;

            #[cfg(feature = "reduced-trie")]
            let condition = self.array[to as usize].base_ < 0 && children[i] != 0;
            #[cfg(not(feature = "reduced-trie"))]
            let condition = self.array[to as usize].base_ > 0 && children[i] != 0;

            if condition {
                let mut c = self.n_infos[to_ as usize].child;

                self.n_infos[to as usize].child = c;

                loop {
                    let idx = (self.array[to as usize].base() ^ i32::from(c)) as usize;
                    self.array[idx].check = to;
                    c = self.n_infos[idx].sibling;

                    if c == 0 {
                        break;
                    }
                }
            }

            if !flag && to_ == (from_n as i32) {
                from_n = to as usize;
            }

            // clean up the space that was moved away from.
            if !flag && to_ == to_pn {
                self.push_sibling(from_n, to_pn ^ i32::from(label_n), label_n, true);
                self.n_infos[to_ as usize].child = 0;

                #[cfg(feature = "reduced-trie")]
                {
                    self.array[to_ as usize].base_ = CEDAR_VALUE_LIMIT;
                }

                #[cfg(not(feature = "reduced-trie"))]
                {
                    if label_n != 0 {
                        self.array[to_ as usize].base_ = -1;
                    } else {
                        self.array[to_ as usize].base_ = 0;
                    }
                }

                self.array[to_ as usize].check = from_n as i32;
            } else {
                self.push_e_node(to_);
            }
        }

        // return the position that is free now.
        if flag {
            base ^ i32::from(label_n)
        } else {
            to_pn
        }
    }
}

impl Default for Cedar {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::distributions::Alphanumeric;
    use rand::{thread_rng, Rng};
    use std::iter;

    #[test]
    fn test_insert_and_delete() {
        let dict = vec!["a"];
        let key_values: Vec<(&str, i32)> = dict.into_iter().enumerate().map(|(k, s)| (s, k as i32)).collect();
        let mut cedar = Cedar::new();
        cedar.build(&key_values).unwrap();

        let result = cedar.exact_match_search("ab").map(|x| x.0);
        assert_eq!(None, result);

        cedar.update("ab", 1).unwrap();
        let result = cedar.exact_match_search("ab").map(|x| x.0);
        assert_eq!(Some(1), result);

        cedar.erase("ab");
        let result = cedar.exact_match_search("ab").map(|x| x.0);
        assert_eq!(None, result);

        cedar.update("abc", 2).unwrap();
        let result = cedar.exact_match_search("abc").map(|x| x.0);
        assert_eq!(Some(2), result);

        cedar.erase("abc");
        let result = cedar.exact_match_search("abc").map(|x| x.0);
        assert_eq!(None, result);
    }

    #[test]
    fn test_common_prefix_search() {
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
        cedar.build(&key_values).unwrap();

        let result: Vec<i32> = cedar.common_prefix_search("abcdefg").iter().map(|x| x.0).collect();
        assert_eq!(vec![0, 1, 2], result);

        let result: Vec<i32> = cedar.common_prefix_search("网球拍卖会").iter().map(|x| x.0).collect();
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
    }

    #[test]
    fn test_common_prefix_iter() {
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
        cedar.build(&key_values).unwrap();

        let result: Vec<i32> = cedar.common_prefix_iter("abcdefg").map(|x| x.0).collect();
        assert_eq!(vec![0, 1, 2], result);

        let result: Vec<i32> = cedar.common_prefix_iter("网球拍卖会").map(|x| x.0).collect();
        assert_eq!(vec![6, 7, 8], result);

        let result: Vec<i32> = cedar.common_prefix_iter("中华人民共和国").map(|x| x.0).collect();
        assert_eq!(vec![9, 10, 11, 12], result);

        let result: Vec<i32> = cedar
            .common_prefix_iter("データ構造とアルゴリズム")
            .map(|x| x.0)
            .collect();
        assert_eq!(vec![4], result);
    }

    #[test]
    fn test_common_prefix_predict() {
        let dict = vec!["a", "ab", "abc"];
        let key_values: Vec<(&str, i32)> = dict.into_iter().enumerate().map(|(k, s)| (s, k as i32)).collect();
        let mut cedar = Cedar::new();
        cedar.build(&key_values).unwrap();

        let result: Vec<i32> = cedar.common_prefix_predict("a").iter().map(|x| x.0).collect();
        assert_eq!(vec![0, 1, 2], result);
    }

    #[test]
    fn test_exact_match_search() {
        let dict = vec!["a", "ab", "abc"];
        let key_values: Vec<(&str, i32)> = dict.into_iter().enumerate().map(|(k, s)| (s, k as i32)).collect();
        let mut cedar = Cedar::new();
        cedar.build(&key_values).unwrap();

        let result = cedar.exact_match_search("abc").map(|x| x.0);
        assert_eq!(Some(2), result);
    }

    #[test]
    fn test_unicode_han_sip() {
        let dict = vec!["讥䶯䶰", "讥䶯䶰䶱䶲", "讥䶯䶰䶱䶲䶳䶴䶵𦡦"];

        let key_values: Vec<(&str, i32)> = dict.into_iter().enumerate().map(|(k, s)| (s, k as i32)).collect();
        let mut cedar = Cedar::new();
        cedar.build(&key_values).unwrap();

        let result: Vec<i32> = cedar.common_prefix_iter("讥䶯䶰䶱䶲䶳䶴䶵𦡦").map(|x| x.0).collect();
        assert_eq!(vec![0, 1, 2], result);
    }

    #[test]
    fn test_unicode_grapheme_cluster() {
        let dict = vec!["a", "abc", "abcde\u{0301}"];

        let key_values: Vec<(&str, i32)> = dict.into_iter().enumerate().map(|(k, s)| (s, k as i32)).collect();
        let mut cedar = Cedar::new();
        cedar.build(&key_values).unwrap();

        let result: Vec<i32> = cedar
            .common_prefix_iter("abcde\u{0301}\u{1100}\u{1161}\u{AC00}")
            .map(|x| x.0)
            .collect();
        assert_eq!(vec![0, 1, 2], result);
    }

    #[test]
    fn test_erase() {
        let dict = vec!["a", "ab", "abc"];
        let key_values: Vec<(&str, i32)> = dict.into_iter().enumerate().map(|(k, s)| (s, k as i32)).collect();
        let mut cedar = Cedar::new();
        cedar.build(&key_values).unwrap();

        cedar.erase("abc");
        assert!(cedar.exact_match_search("abc").is_none());
        assert!(cedar.exact_match_search("ab").is_some());
        assert!(cedar.exact_match_search("a").is_some());

        cedar.erase("ab");
        assert!(cedar.exact_match_search("ab").is_none());
        assert!(cedar.exact_match_search("a").is_some());

        cedar.erase("a");
        assert!(cedar.exact_match_search("a").is_none());
    }

    #[test]
    fn test_erase_on_internal_key() {
        let mut cedar = Cedar::new();

        cedar.update("aa", 0).unwrap();
        assert!(cedar.exact_match_search("aa").is_some());
        cedar.update("ab", 1).unwrap();
        assert!(cedar.exact_match_search("ab").is_some());

        cedar.erase("a");
        assert!(cedar.exact_match_search("a").is_none());
        cedar.erase("aa");
        assert!(cedar.exact_match_search("aa").is_none());
        cedar.erase("ab");
        assert!(cedar.exact_match_search("ab").is_none());
    }

    #[test]
    fn test_update() {
        let dict = vec!["a", "ab", "abc"];
        let key_values: Vec<(&str, i32)> = dict.into_iter().enumerate().map(|(k, s)| (s, k as i32)).collect();
        let mut cedar = Cedar::new();
        cedar.build(&key_values).unwrap();

        cedar.update("abcd", 3).unwrap();

        assert!(cedar.exact_match_search("a").is_some());
        assert!(cedar.exact_match_search("ab").is_some());
        assert!(cedar.exact_match_search("abc").is_some());
        assert!(cedar.exact_match_search("abcd").is_some());
        assert!(cedar.exact_match_search("abcde").is_none());

        let dict = vec!["a", "ab", "abc"];
        let key_values: Vec<(&str, i32)> = dict.into_iter().enumerate().map(|(k, s)| (s, k as i32)).collect();
        let mut cedar = Cedar::new();
        cedar.build(&key_values).unwrap();
        cedar.update("bachelor", 1).unwrap();
        cedar.update("jar", 2).unwrap();
        cedar.update("badge", 3).unwrap();
        cedar.update("baby", 4).unwrap();

        assert!(cedar.exact_match_search("bachelor").is_some());
        assert!(cedar.exact_match_search("jar").is_some());
        assert!(cedar.exact_match_search("badge").is_some());
        assert!(cedar.exact_match_search("baby").is_some());
        assert!(cedar.exact_match_search("abcde").is_none());

        let dict = vec!["a", "ab", "abc"];
        let key_values: Vec<(&str, i32)> = dict.into_iter().enumerate().map(|(k, s)| (s, k as i32)).collect();
        let mut cedar = Cedar::new();
        cedar.build(&key_values).unwrap();
        cedar.update("中", 1).unwrap();
        cedar.update("中华", 2).unwrap();
        cedar.update("中华人民", 3).unwrap();
        cedar.update("中华人民共和国", 4).unwrap();

        assert!(cedar.exact_match_search("中").is_some());
        assert!(cedar.exact_match_search("中华").is_some());
        assert!(cedar.exact_match_search("中华人民").is_some());
        assert!(cedar.exact_match_search("中华人民共和国").is_some());
    }

    #[test]
    fn test_quickcheck_like() {
        let mut rng = thread_rng();
        let case_count = if cfg!(miri) { 32 } else { 1000 };
        let mut dict: Vec<String> = Vec::with_capacity(case_count);
        for _ in 0..case_count {
            let chars: Vec<u8> = iter::repeat(()).map(|()| rng.sample(Alphanumeric)).take(30).collect();
            let s = String::from_utf8(chars).unwrap();
            dict.push(s);
        }

        let key_values: Vec<(&str, i32)> = dict.iter().enumerate().map(|(k, s)| (s.as_ref(), k as i32)).collect();
        let mut cedar = Cedar::new();
        cedar.build(&key_values).unwrap();

        for (k, s) in dict.iter().enumerate() {
            assert_eq!(cedar.exact_match_search(s).map(|x| x.0), Some(k as i32));
        }
    }

    #[test]
    fn test_quickcheck_like_with_deep_trie() {
        let mut rng = thread_rng();
        let case_count = if cfg!(miri) { 32 } else { 1000 };
        let mut dict: Vec<String> = Vec::with_capacity(case_count);
        let mut s = String::new();
        for _ in 0..case_count {
            let c: char = rng.sample(Alphanumeric) as char;
            s.push(c);
            dict.push(s.clone());
        }

        let key_values: Vec<(&str, i32)> = dict.iter().enumerate().map(|(k, s)| (s.as_ref(), k as i32)).collect();
        let mut cedar = Cedar::new();
        cedar.build(&key_values).unwrap();

        for (k, s) in dict.iter().enumerate() {
            assert_eq!(cedar.exact_match_search(s).map(|x| x.0), Some(k as i32));
        }
    }

    #[test]
    fn test_mass_erase() {
        let mut rng = thread_rng();
        let case_count = if cfg!(miri) { 32 } else { 1000 };
        let mut dict: Vec<String> = Vec::with_capacity(case_count);
        for _ in 0..case_count {
            let chars: Vec<u8> = iter::repeat(()).map(|()| rng.sample(Alphanumeric)).take(30).collect();
            let s = String::from_utf8(chars).unwrap();

            dict.push(s);
        }

        let key_values: Vec<(&str, i32)> = dict.iter().enumerate().map(|(k, s)| (s.as_ref(), k as i32)).collect();
        let mut cedar = Cedar::new();
        cedar.build(&key_values).unwrap();

        for s in dict.iter() {
            cedar.erase(s);
            assert!(cedar.exact_match_search(s).is_none());
        }
    }

    #[test]
    fn test_common_prefix_search_returns_empty_vec_on_miss() {
        let mut cedar = Cedar::new();
        cedar.update("abc", 0).unwrap();
        assert!(cedar.common_prefix_search("xyz").is_empty());
    }

    #[test]
    fn test_common_prefix_predict_returns_empty_vec_on_miss() {
        let mut cedar = Cedar::new();
        cedar.update("abc", 0).unwrap();
        assert!(cedar.common_prefix_predict("xyz").is_empty());
    }

    #[test]
    fn checked_mutations_reject_invalid_inputs_without_changes() {
        let mut cedar = Cedar::new();

        assert_eq!(cedar.update("", 0), Err(CedarError::EmptyKey));
        assert_eq!(cedar.update_bytes(b"a\0b", 0), Err(CedarError::NulByte { position: 1 }));
        for value in [-2, -1, i32::MAX - 1, i32::MAX] {
            assert_eq!(cedar.update("invalid", value), Err(CedarError::InvalidValue { value }));
        }
        assert!(cedar.is_empty());

        let entries = [("valid", 1), ("", 2)];
        assert_eq!(cedar.build(&entries), Err(CedarError::EmptyKey));
        assert!(cedar.exact_match_search("valid").is_none());
        assert!(cedar.is_empty());
    }

    #[test]
    fn sorted_constructors_validate_order_duplicates_keys_and_values() {
        assert!(Cedar::from_sorted(&[]).unwrap().is_empty());
        assert_eq!(
            Cedar::from_sorted(&[("a", 1), ("a", 2)]).unwrap_err(),
            CedarError::DuplicateKey { index: 1 }
        );
        assert_eq!(
            Cedar::from_sorted(&[("b", 1), ("a", 2)]).unwrap_err(),
            CedarError::KeysNotSorted { index: 1 }
        );
        assert_eq!(Cedar::from_sorted(&[("", 1)]).unwrap_err(), CedarError::EmptyKey);
        assert_eq!(
            Cedar::from_sorted_bytes(&[(b"a\0b", 1)]).unwrap_err(),
            CedarError::NulByte { position: 1 }
        );
        assert_eq!(
            Cedar::from_sorted(&[("a", -1)]).unwrap_err(),
            CedarError::InvalidValue { value: -1 }
        );
    }

    #[test]
    fn sorted_bulk_construction_matches_incremental_and_remains_mutable() {
        let entries = [
            (b"a".as_slice(), 1),
            (b"ab".as_slice(), 2),
            (b"abc".as_slice(), 3),
            (b"b".as_slice(), 4),
            (b"ba".as_slice(), 5),
            (b"\x80".as_slice(), 6),
            ("网".as_bytes(), 7),
            ("网球".as_bytes(), 8),
        ];
        assert!(entries.windows(2).all(|pair| pair[0].0 < pair[1].0));

        let mut incremental = Cedar::new();
        incremental.build_bytes(&entries).unwrap();
        let mut bulk = Cedar::from_sorted_bytes(&entries).unwrap();

        let assert_equivalent = |left: &Cedar, right: &Cedar| {
            assert_eq!(left.len(), right.len());
            for query in [
                b"".as_slice(),
                b"a".as_slice(),
                b"abc".as_slice(),
                b"abcd".as_slice(),
                b"ba".as_slice(),
                b"missing".as_slice(),
                b"\x80".as_slice(),
                "网球拍".as_bytes(),
            ] {
                assert_eq!(
                    left.exact_match_search_bytes(query),
                    right.exact_match_search_bytes(query)
                );
                assert_eq!(
                    left.common_prefix_search_bytes(query),
                    right.common_prefix_search_bytes(query)
                );
                assert_eq!(
                    left.common_prefix_predict_bytes(query),
                    right.common_prefix_predict_bytes(query)
                );
            }

            let mut left_entries: Vec<_> = left.entries().collect();
            let mut right_entries: Vec<_> = right.entries().collect();
            left_entries.sort_unstable();
            right_entries.sort_unstable();
            assert_eq!(left_entries, right_entries);
        };

        assert_equivalent(&incremental, &bulk);

        for cedar in [&mut incremental, &mut bulk] {
            cedar.update_bytes(b"abd", 30).unwrap();
            cedar.update_bytes(b"ab", 20).unwrap();
            assert!(cedar.erase_bytes(b"a"));
            assert!(cedar.erase_bytes("网球".as_bytes()));
            cedar.update_bytes(b"a", 10).unwrap();
            assert!(!cedar.erase_bytes(b"not-present"));
        }
        assert_equivalent(&incremental, &bulk);

        let utf8 = Cedar::from_sorted(&[("a", 1), ("ab", 2), ("网", 3)]).unwrap();
        assert_eq!(utf8.exact_match_search("网"), Some((3, "网".len())));

        let mut configured = Cedar::builder()
            .ordered(false)
            .max_trial(3)
            .unwrap()
            .from_sorted(&[("a", 1), ("ab", 2)])
            .unwrap();
        assert!(!configured.ordered);
        assert_eq!(configured.max_trial, 3);
        configured.update("b", 3).unwrap();
        assert_eq!(configured.exact_match_search("b"), Some((3, 1)));
    }

    #[test]
    fn sorted_bulk_construction_handles_a_full_sibling_block() {
        let mut keys = vec![vec![b'a']];
        keys.extend((1_u8..=u8::MAX).map(|label| vec![b'a', label]));
        let entries: Vec<_> = keys
            .iter()
            .enumerate()
            .map(|(value, key)| (key.as_slice(), value as i32))
            .collect();
        let mut cedar = Cedar::from_sorted_bytes(&entries).unwrap();

        assert_eq!(cedar.len(), 256);
        for (key, value) in &entries {
            assert_eq!(cedar.exact_match_search_bytes(key).map(|item| item.0), Some(*value));
        }

        for (key, _) in entries.iter().rev() {
            assert!(cedar.erase_bytes(key));
        }
        assert!(cedar.is_empty());
        cedar.update_bytes(b"after", 7).unwrap();
        assert_eq!(cedar.exact_match_search_bytes(b"after"), Some((7, 5)));
    }

    #[test]
    fn value_boundaries_are_layout_independent() {
        let mut cedar = Cedar::new();
        cedar.update("minimum", MIN_VALUE).unwrap();
        cedar.update("maximum", MAX_VALUE).unwrap();

        assert_eq!(cedar.exact_match_search("minimum").map(|item| item.0), Some(MIN_VALUE));
        assert_eq!(cedar.exact_match_search("maximum").map(|item| item.0), Some(MAX_VALUE));
    }

    #[test]
    fn byte_and_string_apis_agree_and_nul_queries_stop_safely() {
        let mut cedar = Cedar::new();
        cedar
            .build_bytes(&[(b"a".as_slice(), 1), (b"ab".as_slice(), 2), (b"abc".as_slice(), 3)])
            .unwrap();

        assert_eq!(cedar.exact_match_search("abc"), cedar.exact_match_search_bytes(b"abc"));
        assert_eq!(
            cedar.common_prefix_search("abcdef"),
            cedar.common_prefix_search_bytes(b"abcdef")
        );
        assert_eq!(
            cedar.common_prefix_predict("a"),
            cedar.common_prefix_predict_bytes(b"a")
        );
        assert_eq!(
            cedar.common_prefix_iter("abcdef").collect::<Vec<_>>(),
            cedar.common_prefix_iter_bytes(b"abcdef").collect::<Vec<_>>()
        );
        assert_eq!(
            cedar.common_prefix_predict_iter("a").collect::<Vec<_>>(),
            cedar.common_prefix_predict_iter_bytes(b"a").collect::<Vec<_>>()
        );

        assert!(cedar.exact_match_search_bytes(b"a\0").is_none());
        assert_eq!(cedar.common_prefix_search_bytes(b"a\0"), vec![(1, 0)]);
        assert!(cedar.common_prefix_predict_bytes(b"a\0").is_empty());
        assert!(!cedar.erase_bytes(b"a\0"));
    }

    #[test]
    fn len_and_entry_iteration_track_mixed_mutations() {
        use std::collections::BTreeMap;

        let mut cedar = Cedar::new();
        assert_eq!(cedar.len(), 0);
        assert!(cedar.is_empty());

        cedar.update_bytes(b"alpha", 1).unwrap();
        cedar.update_bytes(b"beta", 2).unwrap();
        cedar.update_bytes(&[0xff, b'x'], 3).unwrap();
        cedar.update_bytes(b"alpha", 4).unwrap();
        assert!(!cedar.erase_bytes(b"missing"));
        assert_eq!(cedar.len(), 3);

        assert!(cedar.erase_bytes(b"beta"));
        cedar.update_bytes(b"gamma", 5).unwrap();
        assert_eq!(cedar.len(), 3);

        let actual: BTreeMap<Vec<u8>, i32> = cedar.entries().collect();
        let expected = BTreeMap::from([(b"alpha".to_vec(), 4), (b"gamma".to_vec(), 5), (vec![0xff, b'x'], 3)]);
        assert_eq!(actual, expected);

        let utf8_entries: Vec<_> = cedar.entries_str().collect();
        assert_eq!(utf8_entries.iter().filter(|entry| entry.is_err()).count(), 1);
        assert_eq!(utf8_entries.len(), cedar.len());
    }

    #[test]
    fn builder_and_memory_metrics_have_documented_defaults() {
        assert_eq!(Cedar::builder(), CedarBuilder::default());
        assert_eq!(
            Cedar::builder().max_trial(0),
            Err(CedarError::InvalidMaxTrial { value: 0 })
        );

        let mut cedar = Cedar::builder().ordered(false).max_trial(3).unwrap().build();
        cedar.update("abc", 1).unwrap();
        let stats = cedar.memory_stats();
        assert_eq!(stats.entries, 1);
        assert_eq!(stats.entries, cedar.len());
        assert_eq!(stats.used_node_slots, cedar.used_node_slots());
        assert_eq!(stats.node_capacity, cedar.node_capacity());
        assert_eq!(stats.allocated_bytes, cedar.allocated_bytes());
        assert_eq!(stats.load_factor, cedar.load_factor());
        assert!(stats.used_node_slots <= stats.node_capacity);
        assert!(stats.allocated_bytes > 0);
        assert!((0.0..=1.0).contains(&stats.load_factor));
    }

    #[test]
    fn structural_entry_iteration_handles_unordered_siblings() {
        use std::collections::BTreeMap;

        let mut cedar = Cedar::builder().ordered(false).build();
        for (key, value) in [("z", 1), ("a", 2), ("ab", 3), ("中", 4), ("aa", 5)] {
            cedar.update(key, value).unwrap();
        }
        assert!(cedar.erase("ab"));
        cedar.update("ab", 6).unwrap();

        let mut entries = cedar.entries();
        assert_eq!(entries.len(), cedar.len());
        let actual: BTreeMap<_, _> = entries.by_ref().collect();
        assert_eq!(entries.len(), 0);
        assert_eq!(
            actual,
            BTreeMap::from([
                (b"a".to_vec(), 2),
                (b"aa".to_vec(), 5),
                (b"ab".to_vec(), 6),
                (b"z".to_vec(), 1),
                ("中".as_bytes().to_vec(), 4),
            ])
        );
    }

    #[cfg(feature = "std")]
    fn serialized_fixture() -> (Cedar, Vec<u8>) {
        let mut cedar = Cedar::builder().ordered(false).max_trial(3).unwrap().build();
        for (value, key) in [
            b"a".as_slice(),
            b"ab".as_slice(),
            b"alphabet".as_slice(),
            b"beta".as_slice(),
            b"gamma".as_slice(),
            "网球".as_bytes(),
            &[0xff, b'x'],
        ]
        .into_iter()
        .enumerate()
        {
            cedar.update_bytes(key, value as i32).unwrap();
        }
        assert!(cedar.erase_bytes(b"beta"));
        cedar.update_bytes(b"delta", 40).unwrap();
        cedar.update_bytes(b"ab", 20).unwrap();

        let mut bytes = Vec::new();
        cedar.save_to_writer(&mut bytes).unwrap();
        (cedar, bytes)
    }

    #[test]
    #[cfg(feature = "std")]
    fn serialization_round_trip_is_deterministic_and_remains_mutable() {
        let (cedar, bytes) = serialized_fixture();
        let mut second = Vec::new();
        cedar.save_to_writer(&mut second).unwrap();
        assert_eq!(bytes, second);
        assert_eq!(&bytes[..8], &persistence::v1::MAGIC);

        let mut loaded = Cedar::load_from_reader(bytes.as_slice()).unwrap();
        assert_eq!(loaded.ordered, cedar.ordered);
        assert_eq!(loaded.max_trial, cedar.max_trial);
        assert_eq!(loaded.len(), cedar.len());
        assert_eq!(loaded.used_node_slots(), cedar.used_node_slots());
        assert_eq!(loaded.node_capacity(), cedar.node_capacity());
        assert_eq!(loaded.load_factor(), cedar.load_factor());

        let mut expected: Vec<_> = cedar.entries().collect();
        let mut actual: Vec<_> = loaded.entries().collect();
        expected.sort_unstable();
        actual.sort_unstable();
        assert_eq!(actual, expected);
        for query in [b"a".as_slice(), b"alphabet", b"missing", "网球拍".as_bytes()] {
            assert_eq!(
                loaded.exact_match_search_bytes(query),
                cedar.exact_match_search_bytes(query)
            );
            assert_eq!(
                loaded.common_prefix_search_bytes(query),
                cedar.common_prefix_search_bytes(query)
            );
            assert_eq!(
                loaded.common_prefix_predict_bytes(query),
                cedar.common_prefix_predict_bytes(query)
            );
        }

        loaded.update_bytes(b"after-load", 99).unwrap();
        assert_eq!(loaded.exact_match_search_bytes(b"after-load"), Some((99, 10)));
        assert!(loaded.erase_bytes(b"alphabet"));
        assert!(loaded.exact_match_search_bytes(b"alphabet").is_none());

        if !cfg!(miri) {
            let path = std::env::temp_dir().join(format!(
                "cedarwood-serialization-{}-{}.bin",
                std::process::id(),
                persistence::v1::LAYOUT
            ));
            cedar.save_to_path(&path).unwrap();
            let from_file = Cedar::load_from_path(&path).unwrap();
            std::fs::remove_file(path).unwrap();
            assert_eq!(from_file.used_node_slots(), cedar.used_node_slots());
            assert_eq!(from_file.node_capacity(), cedar.node_capacity());
        }
    }

    #[test]
    #[cfg(feature = "std")]
    fn serialization_round_trips_empty_full_and_inactive_blocks() {
        let assert_round_trip = |cedar: &Cedar| {
            let mut bytes = Vec::new();
            cedar.save_to_writer(&mut bytes).unwrap();
            let loaded = Cedar::load_from_reader(bytes.as_slice()).unwrap();
            let mut expected: Vec<_> = cedar.entries().collect();
            let mut actual: Vec<_> = loaded.entries().collect();
            expected.sort_unstable();
            actual.sort_unstable();
            assert_eq!(actual, expected);
            assert_eq!(loaded.len(), cedar.len());
            assert_eq!(loaded.used_node_slots(), cedar.used_node_slots());
            assert_eq!(loaded.node_capacity(), cedar.node_capacity());
        };

        assert_round_trip(&Cedar::new());

        let mut keys = Vec::new();
        for prefix in *b"ab" {
            keys.push(vec![prefix]);
            keys.extend((1_u8..=u8::MAX).map(|label| vec![prefix, label]));
        }
        let entries: Vec<_> = keys
            .iter()
            .enumerate()
            .map(|(value, key)| (key.as_slice(), value as i32))
            .collect();
        let mut cedar = Cedar::from_sorted_bytes(&entries).unwrap();
        assert!(cedar.size < cedar.capacity, "fixture must retain an inactive block");
        assert!(cedar.blocks.iter().take(cedar.size / 256).any(|block| block.num == 0));
        assert_round_trip(&cedar);

        for (key, _) in entries.iter().step_by(3) {
            assert!(cedar.erase_bytes(key));
        }
        assert_round_trip(&cedar);
    }

    #[test]
    #[cfg(feature = "std")]
    fn serialization_rejects_headers_limits_truncation_and_trailing_data() {
        let (_, bytes) = serialized_fixture();

        let mut invalid_magic = bytes.clone();
        invalid_magic[0] ^= 0xff;
        assert!(matches!(
            Cedar::load_from_reader(invalid_magic.as_slice()),
            Err(CedarPersistenceError::InvalidMagic)
        ));

        let mut unknown_version = bytes.clone();
        unknown_version[8..10].copy_from_slice(&2_u16.to_le_bytes());
        assert!(matches!(
            Cedar::load_from_reader(unknown_version.as_slice()),
            Err(CedarPersistenceError::UnsupportedVersion { major: 2, minor: 0 })
        ));

        let mut wrong_byte_order = bytes.clone();
        wrong_byte_order[12] = 2;
        assert!(matches!(
            Cedar::load_from_reader(wrong_byte_order.as_slice()),
            Err(CedarPersistenceError::UnsupportedByteOrder { byte_order: 2 })
        ));

        let mut wrong_layout = bytes.clone();
        wrong_layout[13] ^= 1;
        assert!(matches!(
            Cedar::load_from_reader(wrong_layout.as_slice()),
            Err(CedarPersistenceError::LayoutMismatch { .. })
        ));

        let mut unknown_flags = bytes.clone();
        unknown_flags[14..16].copy_from_slice(&2_u16.to_le_bytes());
        assert!(matches!(
            Cedar::load_from_reader(unknown_flags.as_slice()),
            Err(CedarPersistenceError::InvalidHeader { field: "flags", .. })
        ));

        let mut invalid_config = bytes.clone();
        invalid_config[16..20].copy_from_slice(&0_i32.to_le_bytes());
        assert!(matches!(
            Cedar::load_from_reader(invalid_config.as_slice()),
            Err(CedarPersistenceError::InvalidHeader { field: "max_trial", .. })
        ));

        let mut overflowing_length = bytes.clone();
        overflowing_length[32..40].copy_from_slice(&u64::MAX.to_le_bytes());
        assert!(matches!(
            Cedar::load_from_reader(overflowing_length.as_slice()),
            Err(CedarPersistenceError::InvalidHeader { .. })
        ));

        assert!(matches!(
            Cedar::load_from_reader_with_limit(bytes.as_slice(), 1),
            Err(CedarPersistenceError::AllocationLimitExceeded { .. })
        ));
        assert!(matches!(
            Cedar::load_from_reader(&bytes[..bytes.len() - 1]),
            Err(CedarPersistenceError::Truncated)
        ));

        let mut trailing = bytes;
        trailing.push(0);
        assert!(matches!(
            Cedar::load_from_reader(trailing.as_slice()),
            Err(CedarPersistenceError::TrailingData)
        ));
    }

    #[test]
    #[cfg(feature = "std")]
    fn serialization_rejects_corrupt_allocator_and_trie_state() {
        let (_, bytes) = serialized_fixture();

        let mut bad_root = bytes.clone();
        bad_root[persistence::v1::HEADER_LEN + 4..persistence::v1::HEADER_LEN + 8]
            .copy_from_slice(&0_i32.to_le_bytes());
        assert!(matches!(
            Cedar::load_from_reader(bad_root.as_slice()),
            Err(CedarPersistenceError::CorruptData {
                section: "nodes",
                index: 0,
                ..
            })
        ));

        let mut bad_free_link = bytes.clone();
        let node_one_check = persistence::v1::HEADER_LEN + mem::size_of::<Node>() + 4;
        bad_free_link[node_one_check..node_one_check + 4].copy_from_slice(&0_i32.to_le_bytes());
        assert!(matches!(
            Cedar::load_from_reader(bad_free_link.as_slice()),
            Err(CedarPersistenceError::CorruptData { .. })
        ));

        let array_len = u64::from_le_bytes(bytes[32..40].try_into().unwrap()) as usize;
        let n_infos_len = u64::from_le_bytes(bytes[40..48].try_into().unwrap()) as usize;
        let block_zero_num = persistence::v1::HEADER_LEN + array_len * 8 + n_infos_len * 2 + 8;
        let mut bad_block_count = bytes.clone();
        bad_block_count[block_zero_num..block_zero_num + 2].copy_from_slice(&0_i16.to_le_bytes());
        assert!(matches!(
            Cedar::load_from_reader(bad_block_count.as_slice()),
            Err(CedarPersistenceError::CorruptData {
                section: "blocks",
                index: 0,
                ..
            })
        ));

        let mut bad_entries = bytes.clone();
        let entries = u64::from_le_bytes(bytes[80..88].try_into().unwrap());
        bad_entries[80..88].copy_from_slice(&(entries + 1).to_le_bytes());
        assert!(matches!(
            Cedar::load_from_reader(bad_entries.as_slice()),
            Err(CedarPersistenceError::CorruptData { section: "entries", .. })
        ));

        let mut bad_block_head = bytes.clone();
        bad_block_head[20..24].copy_from_slice(&i32::MAX.to_le_bytes());
        assert!(matches!(
            Cedar::load_from_reader(bad_block_head.as_slice()),
            Err(CedarPersistenceError::CorruptData {
                section: "block-lists",
                ..
            })
        ));

        let n_infos_start = persistence::v1::HEADER_LEN + array_len * 8;
        let first_root_label = bytes[n_infos_start];
        assert_ne!(first_root_label, 0);
        let mut sibling_cycle = bytes;
        let child_sibling = n_infos_start + usize::from(first_root_label) * 2;
        sibling_cycle[child_sibling] = first_root_label;
        assert!(matches!(
            Cedar::load_from_reader(sibling_cycle.as_slice()),
            Err(CedarPersistenceError::CorruptData {
                section: "siblings",
                ..
            })
        ));

        let mut single = Cedar::new();
        single.update("a", 7).unwrap();
        let mut empty_structural = Vec::new();
        single.save_to_writer(&mut empty_structural).unwrap();
        let child_base = persistence::v1::HEADER_LEN + usize::from(b'a') * 8;
        #[cfg(feature = "reduced-trie")]
        let structural_without_children = -1_i32;
        #[cfg(not(feature = "reduced-trie"))]
        let structural_without_children = 0_i32;
        empty_structural[child_base..child_base + 4].copy_from_slice(&structural_without_children.to_le_bytes());
        assert!(matches!(
            Cedar::load_from_reader(empty_structural.as_slice()),
            Err(CedarPersistenceError::CorruptData {
                section: "trie",
                index,
                reason: "structural node has no children",
            }) if index == usize::from(b'a')
        ));
    }

    #[test]
    fn test_duplication() {
        let dict = vec!["些许端", "些須", "些须", "亜", "亝", "亞", "亞", "亞丁", "亞丁港"];
        let key_values: Vec<(&str, i32)> = dict.into_iter().enumerate().map(|(k, s)| (s, k as i32)).collect();
        let mut cedar = Cedar::new();
        cedar.build(&key_values).unwrap();

        assert_eq!(cedar.exact_match_search("亞").map(|t| t.0), Some(6));
        assert_eq!(cedar.exact_match_search("亞丁港").map(|t| t.0), Some(8));
        assert_eq!(cedar.exact_match_search("亝").map(|t| t.0), Some(4));
        assert_eq!(cedar.exact_match_search("些須").map(|t| t.0), Some(1));
    }
}
