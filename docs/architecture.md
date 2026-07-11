# Architecture & Design

This document explains how cedarwood implements an efficiently-updatable double-array trie, and why each design choice was made.

## Background: Tries and Double-Array Tries

A **trie** (prefix tree) is a tree data structure where each edge is labeled with a character. A path from the root to a node spells out a key. Tries give O(k) lookup time for a key of length k, regardless of how many keys are stored.

The naive pointer-based trie uses one node object per character, with pointers to children. This wastes memory and causes poor cache behavior because nodes are scattered across the heap.

A **double-array trie** compresses the entire trie into two flat integer arrays -- `base` and `check` -- achieving the same O(k) lookup but with much better memory density and cache locality.

### The core idea

Each trie node is identified by an index `s` into the arrays. To follow an edge labeled `c` from node `s`:

```
t = base[s] XOR c
```

The transition is valid if and only if `check[t] == s` (i.e., node `t` is owned by node `s`). If it is valid, `t` is the target node.

This means each transition is a single XOR plus one comparison -- extremely fast.

### Storing values

To store an associated value for a key, cedarwood adds a virtual "terminal" edge with label `0` at the end of each key. The value is stored in `base[t]` where `t = base[s] XOR 0 = base[s]`. The terminal node is recognized by its `check` field pointing back to the parent.

Because label `0` has structural meaning, the public byte-key API rejects stored keys containing
`0x00`. The shared public value range is `0..=i32::MAX - 2`; this keeps layout-specific sentinels
outside user data.

## cedarwood's Data Structures

### Node

```rust
struct Node {
    base_: i32,
    check: i32,
}
```

When a node is **in use**, `base_` stores the base value for child transitions, and `check` stores the index of the parent (owner). When a node is **free**, `base_` and `check` are repurposed as backward and forward pointers in a free-list (both stored as negative values to distinguish from active nodes).

### NInfo

```rust
struct NInfo {
    sibling: u8,
    child: u8,
}
```

Each node has an `NInfo` entry that records its first child label and its next sibling label. This forms an implicit linked list of children through the sibling chain. `NInfo` is not part of the classic double-array paper -- it is an addition from cedar that enables efficient updates (insertion and deletion) by making it possible to enumerate a node's children without scanning all 256 possible labels.

### Block

```rust
struct Block {
    prev: i32,
    next: i32,
    num: i16,
    reject: i16,
    trial: i32,
    e_head: i32,
}
```

The array is divided into **blocks** of 256 elements each. Each block tracks:

- **prev / next**: pointers forming a cyclic doubly-linked list with other blocks of the same category.
- **num**: count of free slots in this block (0-256).
- **reject**: minimum number of children that will definitely fail to fit in this block. This is a heuristic pruning parameter.
- **trial**: how many times `find_places` has probed this block without success.
- **e_head**: index of the first free element in this block.

Blocks are categorized into three linked lists:

| Category | Condition | Purpose |
|----------|-----------|---------|
| **Open** | num > 1 | Preferred for multi-child allocation |
| **Closed** | num == 1 | Only useful for single-child allocation |
| **Full** | num == 0 | No free slots, skipped entirely |

This three-way classification lets cedarwood skip entire 256-element regions that can't possibly fit, dramatically reducing search time during insertion.

## Operations

### Lookup (`find`)

```
from = 0 (root)
for each byte c in key:
    to = base[from] XOR c
    if check[to] != from:
        return NOT FOUND
    from = to
// check terminal node
to = base[from] XOR 0
if check[to] == from:
    return base[to]  (the stored value)
else:
    return NO VALUE
```

This is the classic double-array lookup: one XOR and one comparison per byte, giving O(k) time with excellent cache behavior since `base` and `check` are contiguous in memory.

### Insertion (`update` / `follow`)

Inserting a key follows the same traversal as lookup, but at each step, if the target node doesn't exist, `follow` allocates it:

1. If `base[from]` is negative (no children yet), call `find_place` to locate a free slot, set `base[from]` to point there, and mark the slot as used.
2. If `base[from] XOR label` points to a free slot, claim it directly.
3. If the target slot is occupied by a **different** parent (a conflict), call `resolve` to relocate one of the conflicting sets of children.

The `push_sibling` / `pop_sibling` functions maintain the sorted sibling chain in `NInfo`, which enables the ordered traversal needed for predictive search.

### Direct sorted construction

`Cedar::from_sorted` and `Cedar::from_sorted_bytes` validate strictly sorted unique input, then
partition it into ranges that share each byte prefix. For every parent, the builder knows the full
set of child labels before allocating anything. It finds one compatible base, claims the complete
sibling set through the normal block allocator, writes the sibling chain, and schedules each child
range iteratively. It therefore avoids per-key conflict resolution while preserving the free lists,
block categories, parent checks, and `NInfo` links required by later `update` and `erase` calls.

In reduced-trie mode, a range containing only one completed key stores the value in its leaf. A key
that is also a prefix of another key receives the structural terminal child alongside its byte
children, matching incremental promotion semantics.

### Persistence safety boundary

Serialization records the node, node-info, block, reject, configuration, and list-head state with
explicit little-endian fields. A private `persistence::v1` module owns versioned wire DTOs; decoder
records are never aliases for live `Node`, `NInfo`, or `Block` structs. Length-bounded DTOs are
converted into a private unvalidated trie, then vector relationships, free and block lists, parent
ownership, sibling chains, sentinels, reachability, values, and entry count are validated before
the trie is returned. This establishes the invariants relied on by unchecked query indexing. See
[Binary Serialization Format](serialization.md) for the stable format and compatibility policy.

### Portability boundary

The crate is `no_std` when its default features are disabled. Core trie storage and traversal use
`core` plus `alloc`; only the persistence codec, stream and path helpers, persistence error type,
and `std::error::Error` integrations are gated by `std`. The `reduced-trie` layout flag is
orthogonal, producing four CI-checked build combinations. Rust 1.62.0 is the declared and tested
minimum toolchain.

### Deletion (`erase`)

Deletion reverses insertion: it walks up from the terminal node, removing each node that has no remaining siblings, until it reaches a node that still has other children. Freed nodes are returned to the block's free list via `push_e_node`.

### Conflict Resolution (`resolve`)

When two trie nodes compete for the same slot, `resolve` must relocate one of them. It uses `consult` to compare the number of children: the node with **fewer children** gets relocated, minimizing the amount of work. The relocation process:

1. Collects the children of the node to be moved (`set_child`).
2. Finds a free region that can fit all children (`find_place` or `find_places`).
3. Copies each child to its new location, updating `check` for all grandchildren.
4. Frees the old locations.

### Common Prefix Search

`common_prefix_search` (and its iterator variant `common_prefix_iter`) traverses the trie byte by byte, checking at each step whether the current node has a terminal (value) node. If it does, that prefix match is yielded. The traversal continues until the key is exhausted or no matching edge exists.

This is the core operation for dictionary-based text segmentation: given a string like "网球拍卖会", it finds "网" (net), "网球" (tennis), "网球拍" (tennis racket).

### Predictive Search (Common Prefix Predict)

`common_prefix_predict` (and `common_prefix_predict_iter`) first navigates to the node representing the given prefix, then performs a depth-first traversal of the entire subtree below it, yielding every stored value. It uses `begin` to find the leftmost leaf and `next` to advance to the next leaf in order.

## Reduced-Trie Mode

When compiled with `--features reduced-trie`, cedarwood stores values directly in the leaf node's `base_` field instead of using a separate terminal node with label 0. This reduces the number of nodes needed, but requires additional bookkeeping (guarded by `#[cfg(feature = "reduced-trie")]` throughout the code).

The trade-off:
- **Pro**: fewer nodes, less memory for dictionaries where most keys are leaves.
- **Con**: slightly more complex insertion logic when a leaf becomes an internal node.

The 2026-07-10 Phase 5 benchmark retained the default layout: reduced-trie occupied fewer slots but
was slower for exact hit and miss queries, while its build, prefix-scan, and churn results were
mixed. See `benches/results/2026-07-10-phase5-apple-m4-pro.md` for the measurements.

## Memory Layout

```
┌──────────────────────────────────────────────────────────┐
│ array: Vec<Node>     [Node; capacity]                    │
│ ┌──────┬──────┬──────┬──────┬──────┬──────┬─────────┐    │
│ │ 0    │ 1    │ 2    │ ...  │ 255  │ 256  │ ...     │    │
│ │base_ │base_ │base_ │      │base_ │base_ │         │    │
│ │check │check │check │      │check │check │         │    │
│ └──────┴──────┴──────┴──────┴──────┴──────┴─────────┘    │
│ ◄──── block 0 (256) ────►◄──── block 1 (256) ────►      │
│                                                          │
│ n_infos: Vec<NInfo>  [NInfo; capacity]  (parallel array) │
│ blocks:  Vec<Block>  [Block; capacity/256]               │
│ reject:  Vec<i16>    [i16; 257]  (global pruning table)  │
└──────────────────────────────────────────────────────────┘
```

The `array` and `n_infos` vectors are parallel -- index `i` in both refers to the same logical node. The `blocks` vector has one entry per 256-element chunk. The `reject` table is a global pruning heuristic indexed by the number of free slots in a block.

## Complexity

| Operation | Time | Space |
|-----------|------|-------|
| Lookup | O(k) | -- |
| Insert | O(k) amortized | O(1) amortized |
| Delete | O(k) | O(1) |
| Common prefix search | O(k) | O(matches) |
| Predictive search | O(k + results) | O(results) |

Where k is the key length in bytes. Insert is amortized O(k) because conflict resolution may need to relocate children, but the relocation cost is bounded by the smaller sibling set size and amortizes over multiple insertions.
