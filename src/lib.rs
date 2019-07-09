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
}
