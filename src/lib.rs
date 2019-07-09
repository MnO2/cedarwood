#[derive(Default, Clone)]
struct NInfo {
    sibling: u8,
    child: u8,
}

#[derive(Default, Clone)]
struct Node {
    value: i32,
    check: i32,
}

impl Node {
    fn base(&self) -> i32 {
        -(self.value + 1)
    }
}

#[derive(Default, Clone)]
struct Block {
    prev: i32,
    next: i32,
    num: i32,
    reject: i32,
    trial: i32,
    e_head: i32,
}

impl Block {
    pub fn new() -> Self {
        Block {
            prev: 0,
            next: 0,
            num: 256,
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

pub struct Cedar {
    array: Vec<Node>,
    n_infos: Vec<NInfo>,
    blocks: Vec<Block>,
    reject: Vec<i32>,
    blocks_head_full: i32,
    blocks_head_closed: i32,
    blocks_head_open: i32,
    capacity: usize,
    size: usize,
    ordered: bool,
    max_trial: i32,
}

impl Cedar {
    pub fn new() -> Self {
        let mut array: Vec<Node> = Vec::with_capacity(256);
        let n_infos: Vec<NInfo> = vec![Default::default(); 256];
        let mut blocks: Vec<Block> = vec![Block::new(); 1];
        let reject: Vec<i32> = (0..=256).map(|i| i + 1).collect();

        array.push(Node { value: -2, check: 0 });

        for i in 1..256 {
            array.push(Node {
                value: -(i - 1),
                check: -(i + 1),
            })
        }

        array[1].value = -255;
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

    fn get(&mut self, key: Vec<u8>, mut from: i32, pos: i32) -> i32 {
        let n = key.len();
        let start = pos as usize;
        let mut to: i32;
        for i in start..n {
            let value = self.array[from as usize].value;

            if value >= 0 && value < std::i32::MAX {
                let to = self.follow(from, 0);
                self.array[to as usize].value = value;
            }

            from = self.follow(from, key[i]);
        }

        to = from;
        if self.array[from as usize].value < 0 {
            to = self.follow(from, 0);
        }

        self.array[to as usize].value
    }

    fn follow(&mut self, from: i32, label: u8) -> i32 {
        let base = self.array[from as usize].base();
        let mut to = base ^ (label as i32);

        if base < 0 || self.array[to as usize].check < 0 {
            let mut has_child = false;
            if base >= 0 {
                let branch: i32 = base ^ (self.n_infos[from as usize].child as i32);
                has_child = (self.array[branch as usize].check == from)
            }

            to = self.pop_e_node(base, label, from);

            let branch: i32 = base ^ (label as i32);
            self.push_sibling(from, branch, label, has_child);
        } else if self.array[to as usize].check != from {
            to = self.resolve(from, base, label);
        } else if self.array[to as usize].check == from {
            // skip
        } else {
            unreachable!();
        }

        to
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

    fn add_block(&mut self) -> i32 {
        if self.size == self.capacity {
            self.capacity *= 2;

            self.array.resize(self.capacity, Default::default());
            self.n_infos.resize(self.capacity, Default::default());
            self.blocks.resize(self.capacity >> 8, Default::default());
        }

        self.blocks[self.size >> 8] = Block::new();
        self.blocks[self.size >> 8].e_head = self.size as i32;

        self.array[self.size] = Node {
            value: -((self.size as i32) + 255),
            check: -((self.size as i32) + 1),
        };

        for i in (self.size + 1)..(self.size + 255) {
            self.array[i] = Node {
                value: -(i as i32 - 1),
                check: -(i as i32 + 1),
            };
        }

        self.array[self.size + 255] = Node {
            value: -((self.size as i32) + 254),
            check: -(self.size as i32),
        };

        let is_empty = (self.blocks_head_open == 0);
        let idx = (self.size >> 8) as i32;
        self.push_block(idx, BlockType::Open, is_empty);
        self.size += 256;

        ((self.size >> 8) - 1) as i32
    }

    fn transfer_block(&mut self, idx: i32, from: BlockType, to: BlockType, to_block_empty: bool) {
        let is_last = (idx == self.blocks[idx as usize].next);
        let is_empty = to_block_empty && (self.blocks[idx as usize].num != 0);

        self.pop_block(idx, from, is_empty);
        self.push_block(idx, to, is_last);
    }

    fn pop_e_node(&mut self, base: i32, label: u8, from: i32) -> i32 {
        let mut e = base ^ (label as i32);
        if base < 0 {
            e = self.find_place();
        }

        let idx = e >> 8;
        let n = self.array[e as usize].clone();
        self.blocks[idx as usize].num -= 1;

        if self.blocks[idx as usize].num == 0 {
            if idx != 0 {
                self.transfer_block(idx, BlockType::Closed, BlockType::Full, self.blocks_head_full == 0);
            }
        } else {
            self.array[(-n.value) as usize].check = n.check;
            self.array[(-n.check) as usize].value = n.value;

            if e == self.blocks[idx as usize].e_head {
                self.blocks[idx as usize].e_head = -n.check;
            }

            if idx != 0 && self.blocks[idx as usize].num == 1 && self.blocks[idx as usize].trial != self.max_trial {
                self.transfer_block(idx, BlockType::Open, BlockType::Closed, self.blocks_head_closed == 0);
            }
        }

        self.array[e as usize].value = std::i32::MAX;
        self.array[e as usize].check = from;

        if base < 0 {
            self.array[from as usize].value = -(e ^ (label as i32)) - 1;
        }

        e
    }

    fn push_e_node(&mut self, e: i32) {
        let idx = e >> 8;
        self.blocks[idx as usize].num += 1;

        if self.blocks[idx as usize].num == 1 {
            self.blocks[idx as usize].e_head = e;
            self.array[e as usize] = Node { value: -e, check: -e };

            if idx != 0 {
                self.transfer_block(idx, BlockType::Full, BlockType::Closed, self.blocks_head_closed == 0);
            }
        } else {
            let prev = self.blocks[idx as usize].e_head;

            let next = -self.array[prev as usize].check;

            self.array[e as usize] = Node {
                value: -prev,
                check: -next,
            };

            self.array[prev as usize].check = -e;
            self.array[next as usize].value = -e;

            if self.blocks[idx as usize].num == 2 || self.blocks[idx as usize].trial == self.max_trial {
                if idx != 0 {
                    self.transfer_block(idx, BlockType::Closed, BlockType::Open, self.blocks_head_open == 0);
                }
            }

            self.blocks[idx as usize].trial = 0;
        }

        if self.blocks[idx as usize].reject < self.reject[self.blocks[idx as usize].num as usize] {
            self.blocks[idx as usize].reject = self.reject[self.blocks[idx as usize].num as usize];
        }

        self.n_infos[e as usize] = Default::default();
    }

    fn resolve(&self, from_n: i32, base_n: i32, label_n: u8) -> i32 {
        unimplemented!();
    }

    fn push_sibling(&self, from: i32, base: i32, label: u8, has_child: bool) {
        unimplemented!();
    }

    fn find_place(&self) -> i32 {
        unimplemented!();
    }
}
