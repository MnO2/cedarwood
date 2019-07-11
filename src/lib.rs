//!  Efficiently-updatable double-array trie in Rust (ported from cedar).
//!
//! Add it to your `Cargo.toml`:
//!
//! ```toml
//! [dependencies]
//! cedarwood = "0.3"
//! ```
//!
//! then you are good to go. If you are using Rust 2015 you have to `extern crate cedarwood` to your crate root as well.
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
//! cedar.build(&key_values);
//!
//! let result: Vec<i32> = cedar.common_prefix_search("abcdefg").unwrap().iter().map(|x| x.0).collect();
//! assert_eq!(vec![0, 1, 2], result);
//!
//! let result: Vec<i32> = cedar
//!     .common_prefix_search("网球拍卖会")
//!     .unwrap()
//!     .iter()
//!     .map(|x| x.0)
//!     .collect();
//! assert_eq!(vec![6, 7, 8], result);
//!
//! let result: Vec<i32> = cedar
//!     .common_prefix_search("中华人民共和国")
//!     .unwrap()
//!     .iter()
//!     .map(|x| x.0)
//!     .collect();
//! assert_eq!(vec![9, 10, 11, 12], result);
//!
//! let result: Vec<i32> = cedar
//!     .common_prefix_search("データ構造とアルゴリズム")
//!     .unwrap()
//!     .iter()
//!     .map(|x| x.0)
//!     .collect();
//! assert_eq!(vec![4], result);
//! ```

/// NInfo stores the information about the trie
#[derive(Debug, Default, Clone)]
struct NInfo {
    sibling: u8, // the index of right sibling, it is 0 if it doesn't have a sibling.
    child: u8,   // the index of the first child
}

/// Node contains the array of `base` and `check` as specified in the paper: "An efficient implementation of trie structures"
/// https://dl.acm.org/citation.cfm?id=146691
#[derive(Debug, Default, Clone)]
struct Node {
    base_: i32, // if it is a negative value, then it stores the value of previous index that is free.
    check: i32, // if it is a negative value, then it stores the value of next index that is free.
}

impl Node {
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
    e_head: i32, // the index of the first empty elemenet in this block
}

impl Block {
    pub fn new() -> Self {
        Block {
            prev: 0,
            next: 0,
            num: 256, // each of block has 256 free slots at the beginning
            reject: 257,
            trial: 0,
            e_head: 0,
        }
    }
}

enum BlockType {
    Open,
    Closed,
    Full,
}

/// `Cedar` holds all of the information about double array trie.
pub struct Cedar {
    array: Vec<Node>,
    n_infos: Vec<NInfo>,
    blocks: Vec<Block>,
    reject: Vec<i16>,
    blocks_head_full: i32,   // the index of the first 'Full' block, 0 means no 'Full' block
    blocks_head_closed: i32, // the index of the first 'Closed' block, 0 means no ' Closed' block
    blocks_head_open: i32,   // the index of the first 'Open' block, 0 means no 'Open' block
    capacity: usize,
    size: usize,
    ordered: bool,
    max_trial: i32,
}

#[allow(dead_code)]
const CEDAR_VALUE_LIMIT: i32 = std::i32::MAX - 5;
const CEDAR_NO_VALUE: i32 = std::i32::MAX - 5;

pub struct PrefixIter<'a> {
    cedar: &'a Cedar,
    key: &'a [u8],
    from: usize,
    i: usize,
}

impl<'a> Iterator for PrefixIter<'a> {
    type Item = (i32, usize, usize);

    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, Some(self.key.len()))
    }

    fn next(&mut self) -> Option<Self::Item> {
        while self.i < self.key.len() {
            if let Some(value) = self.cedar.find(&self.key[self.i..self.i + 1], &mut self.from) {
                if value == CEDAR_NO_VALUE {
                    self.i += 1;
                    continue;
                } else {
                    let result = Some((value, self.i, self.from));
                    self.i += 1;
                    return result;
                }
            } else {
                break;
            }
        }

        return None;
    }
}

pub struct PrefixPredictIter<'a> {
    cedar: &'a Cedar,
    key: &'a [u8],
    from: usize,
    p: usize,
    root: usize,
    value: Option<i32>,
}

impl<'a> Iterator for PrefixPredictIter<'a> {
    type Item = (i32, usize, usize);

    fn next(&mut self) -> Option<Self::Item> {
        if self.from == 0 && self.p == 0 {
            if self.cedar.find(self.key, &mut self.from).is_some() {
                self.root = self.from;

                let (v_, from_, p_) = self.cedar.begin(self.from, self.p);
                self.from = from_;
                self.p = p_;
                self.value = v_;

                while self.value.is_some() {
                    let result = (self.value.unwrap().clone(), self.p.clone(), self.from.clone());

                    let (v_, from_, p_) = self.cedar.next(self.from, self.p, self.root);
                    self.from = from_;
                    self.p = p_;
                    self.value = v_;

                    return Some(result);
                }
            }
        } else {
            while self.value.is_some() {
                let result = (self.value.unwrap().clone(), self.p.clone(), self.from.clone());

                let (v_, from_, p_) = self.cedar.next(self.from, self.p, self.root);
                self.from = from_;
                self.p = p_;
                self.value = v_;

                return Some(result);
            }
        }

        return None;
    }
}

impl Cedar {
    pub fn new() -> Self {
        let mut array: Vec<Node> = Vec::with_capacity(256);
        let n_infos: Vec<NInfo> = (0..256).map(|_| Default::default()).collect();
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
            array: array,
            n_infos: n_infos,
            blocks: blocks,
            reject: reject,
            blocks_head_full: 0,
            blocks_head_closed: 0,
            blocks_head_open: 0,
            capacity: 256,
            size: 256,
            ordered: true,
            max_trial: 1,
        }
    }

    #[allow(dead_code)]
    pub fn build(&mut self, key_values: &[(&str, i32)]) {
        for (key, value) in key_values {
            self.update(key, *value);
        }
    }

    /// Update the key for the value.
    pub fn update(&mut self, key: &str, value: i32) {
        let from = 0;
        let pos = 0;
        self.update_(key.as_bytes(), value, from, pos);
    }

    fn update_(&mut self, key: &[u8], value: i32, mut from: usize, mut pos: usize) -> i32 {
        if from == 0 && key.len() == 0 {
            panic!("failed to insert zero-length key");
        }

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
        #[cfg(not(feature = "reduced-trie"))]
        let to = self.follow(from, 0);

        self.array[to as usize].base_ += value;
        self.array[to as usize].base_
    }

    fn follow(&mut self, from: usize, label: u8) -> i32 {
        let base = self.array[from].base();

        #[allow(unused_assignments)]
        let mut to = 0;

        if base < 0 || self.array[(base ^ (label as i32)) as usize].check < 0 {
            to = self.pop_e_node(base, label, from as i32);
            let branch: i32 = to ^ (label as i32);
            self.push_sibling(from, branch, label, base >= 0);
        } else {
            to = base ^ (label as i32);
            if self.array[to as usize].check != (from as i32) {
                to = self.resolve(from, base, label);
            }
        }

        to
    }

    // Find key from double array trie
    fn find(&self, key: &[u8], from: &mut usize) -> Option<i32> {
        #[allow(unused_assignments)]
        let mut to: usize = 0;
        let mut pos = 0;

        while pos < key.len() {
            #[cfg(feature = "reduced-trie")]
            {
                if self.array[*from].base_ >= 0 {
                    break;
                }
            }

            to = (self.array[*from].base() ^ (key[pos] as i32)) as usize;
            if self.array[to as usize].check != (*from as i32) {
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

        let n = &self.array[(self.array[*from].base() ^ 0) as usize];
        if n.check != (*from as i32) {
            return Some(CEDAR_NO_VALUE);
        } else {
            return Some(n.base_);
        }
    }

    /// Delete the key from the trie
    pub fn erase(&mut self, key: &str) {
        self.erase_(key.as_bytes())
    }

    fn erase_(&mut self, key: &[u8]) {
        let mut from = 0;

        if self.find(&key, &mut from).is_some() {
            self.erase__(from);
        }
    }

    fn erase__(&mut self, mut from: usize) {
        #[cfg(feature = "reduced-trie")]
        let mut e: i32 = if self.array[from].base_ >= 0 {
            from as i32
        } else {
            self.array[from].base() ^ 0
        };

        #[cfg(feature = "reduced-trie")]
        {
            from = self.array[e as usize].check as usize;
        }

        #[cfg(not(feature = "reduced-trie"))]
        let mut e = self.array[from].base() ^ 0;

        #[allow(unused_assignments)]
        let mut has_sibling = false;
        loop {
            let n = self.array[from].clone();
            has_sibling = self.n_infos[(n.base() ^ (self.n_infos[from].child as i32)) as usize].sibling != 0;

            if has_sibling {
                self.pop_sibling(from as i32, n.base(), (n.base() ^ e) as u8);
            }

            self.push_e_node(e);
            e = from as i32;
            from = self.array[from].check as usize;

            if has_sibling {
                break;
            }
        }
    }

    /// To check if `key` is in the dictionary.
    pub fn exact_match_search(&self, key: &str) -> Option<(i32, usize, usize)> {
        let key = key.as_bytes();
        let mut from = 0;

        if let Some(value) = self.find(&key, &mut from) {
            return Some((value, key.len(), from));
        } else {
            return None;
        }
    }

    /// To return an iterator to iterate through the common prefix in the dictionary with the `key` passed in.
    pub fn common_prefix_iter<'a>(&'a self, key: &'a str) -> PrefixIter<'a> {
        let key = key.as_bytes();

        PrefixIter {
            cedar: self,
            key: key,
            from: 0,
            i: 0,
        }
    }

    /// To return the collection of the common prefix in the dictionary with the `key` passed in.
    pub fn common_prefix_search(&self, key: &str) -> Option<Vec<(i32, usize, usize)>> {
        self.common_prefix_iter(key).map(Some).collect()
    }

    pub fn common_prefix_predict_iter<'a>(&'a self, key: &'a str) -> PrefixPredictIter<'a> {
        let key = key.as_bytes();

        PrefixPredictIter {
            cedar: self,
            key: key,
            from: 0,
            p: 0,
            root: 0,
            value: None,
        }
    }

    /// To return the list of words in the dictionary that has `key` as their prefix.
    pub fn common_prefix_predict(&self, key: &str) -> Option<Vec<(i32, usize, usize)>> {
        self.common_prefix_predict_iter(key).map(Some).collect()
    }

    fn begin(&self, mut from: usize, mut p: usize) -> (Option<i32>, usize, usize) {
        let base = self.array[from].base();
        let mut c = self.n_infos[from].child;

        if from == 0 {
            c = self.n_infos[(base ^ (c as i32)) as usize].sibling;
            if c == 0 {
                return (None, from, p);
            }
        }

        while c != 0 {
            from = (self.array[from].base() ^ (c as i32)) as usize;
            c = self.n_infos[from].child;
            p += 1;
        }

        #[cfg(feature = "reduced-trie")]
        {
            if self.array[from].base_ >= 0 {
                return (Some(self.array[from].base_), from, p);
            }
        }

        let v = self.array[(self.array[from].base() ^ (c as i32)) as usize].base_;
        return (Some(v), from, p);
    }

    fn next(&self, mut from: usize, mut p: usize, root: usize) -> (Option<i32>, usize, usize) {
        #[allow(unused_assignments)]
        let mut c: u8 = 0;

        #[cfg(feature = "reduced-trie")]
        {
            if self.array[from].base_ < 0 {
                c = self.n_infos[(self.array[from].base() ^ 0) as usize].sibling;
            }
        }
        #[cfg(not(feature = "reduced-trie"))]
        {
            c = self.n_infos[(self.array[from].base() ^ 0) as usize].sibling;
        }

        while c == 0 && from != root {
            c = self.n_infos[from as usize].sibling;
            from = self.array[from as usize].check as usize;

            p -= 1;
        }

        if c != 0 {
            from = (self.array[from].base() ^ (c as i32)) as usize;
            let (v_, from_, p_) = self.begin(from, p + 1);
            return (v_, from_, p_);
        } else {
            return (None, from, p);
        }
    }

    fn pop_block(&mut self, idx: i32, from: BlockType, last: bool) {
        let head: &mut i32 = match from {
            BlockType::Open => &mut self.blocks_head_open,
            BlockType::Closed => &mut self.blocks_head_closed,
            BlockType::Full => &mut self.blocks_head_full,
        };

        if last {
            *head = 0;
        } else {
            let b = self.blocks[idx as usize].clone();
            self.blocks[b.prev as usize].next = b.next;
            self.blocks[b.next as usize].prev = b.prev;

            if idx == *head {
                *head = b.next;
            }
        }
    }

    fn push_block(&mut self, idx: i32, to: BlockType, empty: bool) {
        let head: &mut i32 = match to {
            BlockType::Open => &mut self.blocks_head_open,
            BlockType::Closed => &mut self.blocks_head_closed,
            BlockType::Full => &mut self.blocks_head_full,
        };

        if empty {
            *head = idx;
            self.blocks[idx as usize].prev = idx;
            self.blocks[idx as usize].next = idx;
        } else {
            self.blocks[idx as usize].prev = self.blocks[*head as usize].prev;
            self.blocks[idx as usize].next = *head;

            *head = idx;

            let t = self.blocks[*head as usize].prev;
            self.blocks[*head as usize].prev = idx;
            self.blocks[t as usize].next = idx;
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

        // make it a doubley linked list
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
        self.push_block(idx, BlockType::Open, is_empty);

        self.size += 256;

        ((self.size >> 8) - 1) as i32
    }

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
            base ^ (label as i32)
        };

        let idx = e >> 8;

        let n = self.array[e as usize].clone();
        self.blocks[idx as usize].num -= 1;

        if self.blocks[idx as usize].num == 0 {
            if idx != 0 {
                self.transfer_block(idx, BlockType::Closed, BlockType::Full, self.blocks_head_full == 0);
            }
        } else {
            self.array[(-n.base_) as usize].check = n.check;
            self.array[(-n.check) as usize].base_ = n.base_;

            if e == self.blocks[idx as usize].e_head {
                self.blocks[idx as usize].e_head = -n.check;
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
                self.array[from as usize].base_ = -(e ^ (label as i32)) - 1;
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
                self.array[from as usize].base_ = e ^ (label as i32);
            }
        }

        e
    }

    /// Mark an edge `e` as free in a trie node.
    fn push_e_node(&mut self, e: i32) {
        let idx = e >> 8;
        self.blocks[idx as usize].num += 1;

        if self.blocks[idx as usize].num == 1 {
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

    fn push_sibling(&mut self, from: usize, base: i32, label: u8, has_child: bool) {
        let keep_order: bool = if self.ordered {
            label > self.n_infos[from].child
        } else {
            self.n_infos[from].child == 0
        };

        let sibling: u8;
        {
            let mut c: &mut u8 = &mut self.n_infos[from as usize].child;
            if has_child && keep_order {
                loop {
                    let code = *c as i32;
                    c = &mut self.n_infos[(base ^ code) as usize].sibling;

                    if !(self.ordered && (*c != 0) && (*c < label)) {
                        break;
                    }
                }
            }
            sibling = *c;

            *c = label;
        }

        self.n_infos[(base ^ (label as i32)) as usize].sibling = sibling;
    }

    #[allow(dead_code)]
    fn pop_sibling(&mut self, from: i32, base: i32, label: u8) {
        let mut c: (*mut u8) = &mut self.n_infos[from as usize].child;
        unsafe {
            while *c != label {
                let code = *c as i32;
                c = &mut self.n_infos[(base ^ code) as usize].sibling;
            }

            let code = label as i32;
            *c = self.n_infos[(base ^ code) as usize].sibling;
        }
    }

    fn consult(&self, base_n: i32, base_p: i32, mut c_n: u8, mut c_p: u8) -> bool {
        loop {
            c_n = self.n_infos[(base_n ^ (c_n as i32)) as usize].sibling;
            c_p = self.n_infos[(base_p ^ (c_p as i32)) as usize].sibling;

            if !(c_n != 0 && c_p != 0) {
                break;
            }
        }

        c_p != 0
    }

    fn set_child(&self, base: i32, mut c: u8, label: u8, not_terminal: bool) -> Vec<u8> {
        let mut child: Vec<u8> = Vec::new();

        if c == 0 {
            child.push(c);
            c = self.n_infos[(base ^ (c as i32)) as usize].sibling;
        }

        if self.ordered {
            while c != 0 && c <= label {
                child.push(c);
                c = self.n_infos[(base ^ (c as i32)) as usize].sibling;
            }
        }

        if not_terminal {
            child.push(label);
        }

        while c != 0 {
            child.push(c);
            c = self.n_infos[(base ^ (c as i32)) as usize].sibling;
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

        self.add_block() << 8
    }

    // For the case where multiple free slots are needed.
    fn find_places(&mut self, child: &Vec<u8>) -> i32 {
        let mut idx = self.blocks_head_open;
        if idx != 0 {
            let bz = self.blocks[self.blocks_head_open as usize].prev;
            let nc = child.len() as i16;

            loop {
                // only proceed if the free slots are more than the number of children.
                if self.blocks[idx as usize].num >= nc && nc < self.blocks[idx as usize].reject {
                    let mut e = self.blocks[idx as usize].e_head;
                    loop {
                        let base = e ^ (child[0] as i32);

                        let mut i = 0;
                        while self.array[(base ^ (child[i] as i32)) as usize].check < 0 {
                            if i == child.len() - 1 {
                                self.blocks[idx as usize].e_head = e;
                                return e;
                            }
                            i += 1;
                        }

                        e = -self.array[e as usize].check;
                        if e == self.blocks[idx as usize].e_head {
                            break;
                        }
                    }

                    self.blocks[idx as usize].reject = nc;
                    if self.blocks[idx as usize].reject < self.reject[self.blocks[idx as usize].num as usize] {
                        self.reject[self.blocks[idx as usize].num as usize] = self.blocks[idx as usize].reject;
                    }

                    let idx_ = self.blocks[idx as usize].next;

                    self.blocks[idx as usize].trial += 1;

                    // move this block to the 'Closed' block list since it has reached the max_trial
                    if self.blocks[idx as usize].trial == self.max_trial {
                        self.transfer_block(idx, BlockType::Open, BlockType::Closed, self.blocks_head_closed == 0);
                    }

                    if idx == bz {
                        break;
                    }

                    // going to the next in this linked list group
                    idx = idx_;
                }
            }
        }

        self.add_block() << 8
    }

    // resolve the conflict by moving one of the the nodes to a free block.
    fn resolve(&mut self, mut from_n: usize, base_n: i32, label_n: u8) -> i32 {
        let to_pn = base_n ^ (label_n as i32);
        let from_p = self.array[to_pn as usize].check;
        let base_p = self.array[from_p as usize].base();

        // whether to replace siblings of newly added
        let flag = self.consult(
            base_n,
            base_p,
            self.n_infos[from_n as usize].child,
            self.n_infos[from_p as usize].child,
        );

        let children: Vec<u8> = if flag {
            self.set_child(base_n, self.n_infos[from_n as usize].child, label_n, true)
        } else {
            self.set_child(base_p, self.n_infos[from_p as usize].child, 255, false)
        };

        let mut base = if children.len() == 1 {
            self.find_place()
        } else {
            self.find_places(&children)
        };

        base ^= children[0] as i32;

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

        for i in 0..(children.len()) {
            let to = self.pop_e_node(base, children[i], from);
            let to_ = base_ ^ (children[i] as i32);

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
            {
                if self.array[to as usize].base_ < 0 && children[i] != 0 {
                    let mut c = self.n_infos[to_ as usize].child;

                    self.n_infos[to as usize].child = c;

                    loop {
                        let idx = (self.array[to as usize].base() ^ (c as i32)) as usize;
                        self.array[idx].check = to;
                        c = self.n_infos[idx].sibling;

                        if c == 0 {
                            break;
                        }
                    }
                }
            }

            #[cfg(not(feature = "reduced-trie"))]
            {
                if self.array[to as usize].base_ > 0 && children[i] != 0 {
                    let mut c = self.n_infos[to_ as usize].child;

                    self.n_infos[to as usize].child = c;

                    loop {
                        let idx = (self.array[to as usize].base() ^ (c as i32)) as usize;
                        self.array[idx].check = to;
                        c = self.n_infos[idx].sibling;

                        if c == 0 {
                            break;
                        }
                    }
                }
            }

            if !flag && to_ == (from_n as i32) {
                from_n = to as usize;
            }

            if !flag && to_ == to_pn {
                self.push_sibling(from_n, to_pn ^ (label_n as i32), label_n, true);
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

        if flag {
            base ^ (label_n as i32)
        } else {
            to_pn
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_insert_and_delete() {
        let dict = vec!["a"];
        let key_values: Vec<(&str, i32)> = dict.into_iter().enumerate().map(|(k, s)| (s, k as i32)).collect();
        let mut cedar = Cedar::new();
        cedar.build(&key_values);

        let result = cedar.exact_match_search("ab").map(|x| x.0);
        assert_eq!(None, result);

        cedar.update("ab", 1);
        let result = cedar.exact_match_search("ab").map(|x| x.0);
        assert_eq!(Some(1), result);

        cedar.erase("ab");
        let result = cedar.exact_match_search("ab").map(|x| x.0);
        assert_eq!(None, result);

        cedar.update("abc", 2);
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
        cedar.build(&key_values);

        let result: Vec<i32> = cedar
            .common_prefix_search("abcdefg")
            .unwrap()
            .iter()
            .map(|x| x.0)
            .collect();
        assert_eq!(vec![0, 1, 2], result);

        let result: Vec<i32> = cedar
            .common_prefix_search("网球拍卖会")
            .unwrap()
            .iter()
            .map(|x| x.0)
            .collect();
        assert_eq!(vec![6, 7, 8], result);

        let result: Vec<i32> = cedar
            .common_prefix_search("中华人民共和国")
            .unwrap()
            .iter()
            .map(|x| x.0)
            .collect();
        assert_eq!(vec![9, 10, 11, 12], result);

        let result: Vec<i32> = cedar
            .common_prefix_search("データ構造とアルゴリズム")
            .unwrap()
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
        cedar.build(&key_values);

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
        cedar.build(&key_values);

        let result: Vec<i32> = cedar.common_prefix_predict("a").unwrap().iter().map(|x| x.0).collect();
        assert_eq!(vec![0, 1, 2], result);
    }

    #[test]
    fn test_exact_match_search() {
        let dict = vec!["a", "ab", "abc"];
        let key_values: Vec<(&str, i32)> = dict.into_iter().enumerate().map(|(k, s)| (s, k as i32)).collect();
        let mut cedar = Cedar::new();
        cedar.build(&key_values);

        let result = cedar.exact_match_search("abc").map(|x| x.0);
        assert_eq!(Some(2), result);
    }

    #[test]
    fn test_unicode_han_sip() {
        let dict = vec!["讥䶯䶰", "讥䶯䶰䶱䶲", "讥䶯䶰䶱䶲䶳䶴䶵𦡦"];

        let key_values: Vec<(&str, i32)> = dict.into_iter().enumerate().map(|(k, s)| (s, k as i32)).collect();
        let mut cedar = Cedar::new();
        cedar.build(&key_values);

        let result: Vec<i32> = cedar
            .common_prefix_iter("讥䶯䶰䶱䶲䶳䶴䶵𦡦")
            .map(|x| x.0)
            .collect();
        assert_eq!(vec![0, 1, 2], result);
    }

    #[test]
    fn test_unicode_grapheme_cluster() {
        let dict = vec!["a", "abc", "abcde\u{0301}"];

        let key_values: Vec<(&str, i32)> = dict.into_iter().enumerate().map(|(k, s)| (s, k as i32)).collect();
        let mut cedar = Cedar::new();
        cedar.build(&key_values);

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
        cedar.build(&key_values);

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
    fn test_update() {
        let dict = vec!["a", "ab", "abc"];
        let key_values: Vec<(&str, i32)> = dict.into_iter().enumerate().map(|(k, s)| (s, k as i32)).collect();
        let mut cedar = Cedar::new();
        cedar.build(&key_values);

        cedar.update("abcd", 3);

        assert!(cedar.exact_match_search("a").is_some());
        assert!(cedar.exact_match_search("ab").is_some());
        assert!(cedar.exact_match_search("abc").is_some());
        assert!(cedar.exact_match_search("abcd").is_some());
        assert!(cedar.exact_match_search("abcde").is_none());

        let dict = vec!["a", "ab", "abc"];
        let key_values: Vec<(&str, i32)> = dict.into_iter().enumerate().map(|(k, s)| (s, k as i32)).collect();
        let mut cedar = Cedar::new();
        cedar.build(&key_values);
        cedar.update("bachelor", 1);
        cedar.update("jar", 2);
        cedar.update("badge", 3);
        cedar.update("baby", 4);

        assert!(cedar.exact_match_search("bachelor").is_some());
        assert!(cedar.exact_match_search("jar").is_some());
        assert!(cedar.exact_match_search("badge").is_some());
        assert!(cedar.exact_match_search("baby").is_some());
        assert!(cedar.exact_match_search("abcde").is_none());

        let dict = vec!["a", "ab", "abc"];
        let key_values: Vec<(&str, i32)> = dict.into_iter().enumerate().map(|(k, s)| (s, k as i32)).collect();
        let mut cedar = Cedar::new();
        cedar.build(&key_values);
        cedar.update("中", 1);
        cedar.update("中华", 2);
        cedar.update("中华人民", 3);
        cedar.update("中华人民共和国", 4);

        assert!(cedar.exact_match_search("中").is_some());
        assert!(cedar.exact_match_search("中华").is_some());
        assert!(cedar.exact_match_search("中华人民").is_some());
        assert!(cedar.exact_match_search("中华人民共和国").is_some());
    }
}
