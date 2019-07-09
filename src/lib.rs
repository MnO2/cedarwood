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

pub struct Cedar {
    array: Vec<Node>,
    n_infos: Vec<NInfo>,
    blocks: Vec<Block>,
    reject: Vec<i32>,
    blocks_head_full: i32,
    blocks_head_closed: i32,
    blocks_head_open: i32,
    capacity: i32,
    size: i32,
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

    fn follow(&self, from: i32, label: u8) -> i32 {
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

    fn resolve(&self, from_n: i32, base_n: i32, label_n: u8) -> i32 {
        unimplemented!();
    }

    fn push_sibling(&self, from: i32, base: i32, label: u8, has_child: bool) {
        unimplemented!();
    }

    fn pop_e_node(&self, base: i32, label: u8, from: i32) -> i32 {
        unimplemented!();
    }
}
