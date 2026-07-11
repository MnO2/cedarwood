# Internal Implementation

This document explains cedarwood's internal mechanics in detail: how free space is managed, how conflicts are resolved, and how the block heuristics work. This is intended for contributors or anyone curious about what happens beneath the public API.

## Initialization

`Cedar::new()` sets up the initial state:

1. **Array**: 256 `Node` entries are allocated (one block). Index 0 is the root node with `base_ = 0` (or `-1` in reduced-trie mode) and `check = -1`. Indices 1-255 are linked into a free list: each node's `base_` points backward and `check` points forward, forming a cyclic doubly-linked list.

2. **NInfo**: 256 default entries (all zeros -- no children, no siblings).

3. **Blocks**: One `Block` with `num = 256` (all free), `e_head = 1`, and `reject = 257`.

4. **Reject table**: `reject[i] = i + 1` for i in 0..=256. This initializes the global pruning heuristic.

5. **Block lists**: `blocks_head_open = 0` (block 0 is the only open block). `blocks_head_closed = 0` and `blocks_head_full = 0` (both empty -- the head value 0 is overloaded since block 0 is special).

## Free-List Structure Within a Block

Within each 256-element block, free nodes form a **cyclic doubly-linked list** using the `base_` (backward pointer) and `check` (forward pointer) fields of the `Node` struct. Both pointers are stored as negated values to distinguish free nodes from active ones:

```
Free node at index e:
    base_ = -(previous free node index)
    check = -(next free node index)

Active node at index e:
    base_ >= 0   (base value for child transitions)
    check >= 0   (parent node index)
```

The block's `e_head` points to the first free node. To enumerate free nodes, follow the `check` chain (negating to get the actual index) until you loop back to `e_head`.

### Allocating a free node (`pop_e_node`)

When a node is needed:

1. Determine the target index `e` (either from `find_place`/`find_places`, or directly from `base XOR label`).
2. Remove `e` from its block's free list by linking its predecessor to its successor.
3. Decrement the block's `num`. If `num` drops to 0, transfer the block from Closed to Full. If `num` drops to 1 (and `trial < max_trial`), transfer from Open to Closed.
4. Initialize the node: set `base_` to `-1` (or `0` for terminal) and `check` to the parent.
5. If this is the first child (`base < 0`), set the parent's `base_` to `e XOR label`.

### Freeing a node (`push_e_node`)

When a node is deleted:

1. Increment the block's `num`. If `num` goes from 0 to 1, transfer the block from Full to Closed. If `num` goes from 1 to 2 (or `trial == max_trial`), transfer from Closed to Open.
2. Insert the node back into the block's free list, immediately after `e_head`.
3. Update the `reject` heuristic if needed.
4. Clear the node's `NInfo`.

## Block Management

### The Three Block Lists

Blocks are organized into three cyclic doubly-linked lists based on their free slot count:

```
blocks_head_open ──→ [block A] ⇄ [block B] ⇄ [block C] ──→ (back to A)
                     num > 1     num > 1     num > 1

blocks_head_closed ─→ [block D] ⇄ [block E] ──→ (back to D)
                      num == 1    num == 1

blocks_head_full ───→ [block F] ──→ (back to F)
                      num == 0
```

A head value of `0` means the list is empty (except for block 0, which is special-cased). The `prev` and `next` fields in each `Block` struct maintain the doubly-linked list.

### Block Transfer

When a block's `num` changes, it may need to move between lists:

| Transition | Trigger | From | To |
|-----------|---------|------|----|
| Allocation | num: 2 → 1 | Open | Closed |
| Allocation | num: 1 → 0 | Closed | Full |
| Deletion | num: 0 → 1 | Full | Closed |
| Deletion | num: 1 → 2 | Closed | Open |
| Max trial reached | trial == max_trial | Open | Closed |

The `transfer_block` function handles this by calling `pop_block` (remove from source list) then `push_block` (insert at head of destination list).

### Adding New Blocks

When no existing block can fit the needed children, `add_block` allocates a new 256-element block:

1. If `size == capacity`, double the capacity and resize all vectors.
2. Initialize the new block's nodes as a cyclic free list (same structure as initialization).
3. Push the new block onto the Open list.
4. Increment `size` by 256.

The array grows by doubling (`capacity += capacity`), giving amortized O(1) for growth.

## Conflict Resolution (`resolve`)

A conflict occurs when `follow` tries to place a child at `base[from] XOR label`, but that slot is already owned by a different parent. The resolution strategy:

Direct sorted construction bypasses `follow` and `resolve`. Since each sorted prefix range reveals
all sibling labels at once, `allocate_bulk_siblings` chooses one base with `find_place` or
`find_places`, claims every label with `pop_e_node`, and writes the complete ordered `NInfo` chain.
Using `pop_e_node` is essential: it keeps the per-block free list, free count, and Open/Closed/Full
membership valid for later incremental mutation.

### Step 1: Identify the Parties

```
from_n = the node trying to insert a new child
base_n = base[from_n]
label_n = the label being inserted

to_pn = base_n XOR label_n    (the contested slot)
from_p = check[to_pn]          (the current owner of the slot)
base_p = base[from_p]
```

### Step 2: Choose Who to Relocate (`consult`)

`consult` races through the sibling chains of both nodes. Whichever chain ends first has fewer children -- that node gets relocated because it's cheaper. The function returns `true` if the new node (`from_n`) should be relocated, `false` if the existing node (`from_p`) should be.

### Step 3: Collect Children (`set_child`)

`set_child` walks the sibling chain starting from the first child, collecting all labels into a `SmallVec<[u8; 256]>`. If relocating the new node, the new label is also included in the list. The list is kept in sorted order (when `ordered` is true) to maintain the invariant needed by `common_prefix_predict`.

### Step 4: Find Free Space

- If only one child: `find_place` returns the first free slot from any Closed or Open block.
- If multiple children: `find_places` searches Open blocks for a contiguous-enough region. It iterates free slots within a block, checking if all `base XOR child[i]` positions are free. The `reject` heuristic prunes blocks that can't possibly fit.

### Step 5: Relocate

For each child in the list:

1. Allocate the new position via `pop_e_node`.
2. Copy `base_` from the old position to the new one.
3. If the child has its own children (non-leaf), update all grandchildren's `check` to point to the new position.
4. Free the old position via `push_e_node`.
5. If the node being relocated was `from_n`, update `from_n` to track the new position.

## The Reject Heuristic

The `reject` array is a global optimization that records, for each possible `num` value (0-256), the minimum number of children that failed to fit in any block with that many free slots. This lets `find_places` skip blocks early:

```rust
if self.blocks[idx].num >= nc && nc < self.blocks[idx].reject {
    // Worth searching this block
} else {
    // Skip: either not enough free slots, or previous experience
    // says nc children won't fit in a block with this many free slots
}
```

The heuristic is updated whenever a `find_places` search fails on a block:

```rust
self.blocks[idx].reject = nc;
if self.blocks[idx].reject < self.reject[self.blocks[idx].num] {
    self.reject[self.blocks[idx].num] = self.blocks[idx].reject;
}
```

This is a form of memoized pruning: once we learn that 5 children can't fit in a block with 10 free slots, we propagate that knowledge globally so that no future search even attempts it.

## Sibling Chain Management

### `push_sibling`

Inserts a label into a node's child list. When `ordered` is true (the default), the label is inserted in sorted order by walking the sibling chain until the correct position is found. This maintains the invariant that `common_prefix_predict_iter` yields results in lexicographic order.

### `pop_sibling`

Removes a label from the sibling chain. Walks the chain from `child` through `sibling` links until the target label is found, then splices it out.

## `max_trial` Parameter

The `max_trial` field (default: 1) controls how many times a block can be probed by `find_places` before it is demoted from Open to Closed. A lower value makes the search faster but may waste more space; a higher value searches more thoroughly. `Cedar::builder().max_trial(...)` exposes this setting and rejects nonpositive values.

With `max_trial = 1`, a block gets at most one chance per insertion cycle. After being probed once unsuccessfully, it moves to Closed and won't be searched again for multi-child allocations until a deletion reopens it.

## Reduced-Trie Mode

With the `reduced-trie` feature enabled, the key behavioral differences are:

1. **Values in leaves**: When a leaf node stores a value, it is placed directly in `base_` (as a non-negative integer) instead of creating a separate terminal child. The sentinel `CEDAR_VALUE_LIMIT = i32::MAX - 1` marks "allocated but no value yet."

   The public API reserves this sentinel and `i32::MAX`; accepted user values stop at
   `i32::MAX - 2`, matching the default layout's public contract.

2. **Leaf-to-internal promotion**: When inserting a key that extends an existing leaf, the existing value must be moved to a new terminal child before the leaf can become an internal node.

3. **Base encoding**: In reduced-trie mode, `base()` returns `-(base_ + 1)` instead of `base_` directly. This encoding allows distinguishing between a leaf with value 0 and an internal node with base 0.

4. **Deletion**: `erase__` checks whether the node is a leaf (base_ >= 0) or has a terminal child, and handles both cases.

These changes are scattered throughout the code as `#[cfg(feature = "reduced-trie")]` blocks, always paired with a `#[cfg(not(feature = "reduced-trie"))]` default.

## Traversal for Predictive Search

The `begin` and `next` functions implement depth-first traversal over the trie's leaves:

### `begin(from, p)`

From node `from`, follows `child` links all the way down to the leftmost leaf. Returns `(value, leaf_node, depth)`.

### `next(from, p, root)`

From a leaf node, finds the next leaf in depth-first order:

1. Check if the current terminal node has a sibling.
2. If not, walk up via `check` (parent pointer), checking for siblings at each level.
3. Once a sibling is found, call `begin` on it to descend to its leftmost leaf.
4. If we reach `root` without finding a sibling, the traversal is complete.

This gives an efficient in-order traversal without recursion or an explicit stack -- the trie's structure itself provides the traversal state through `check` (parent) and `sibling` links.

## Unsafe traversal boundary

The exact and predictive query hot paths use unchecked vector indexing only after construction or
deserialization has established the double-array invariants. Persistence first decodes into private
wire records, applies allocation limits, converts into an unexposed trie, and validates array
lengths, parent links, sibling chains, reachability, free lists, block lists, and value sentinels.
Checked indexing remains in `PrefixIter::next` because the measured unchecked candidate did not
produce a statistically significant scan improvement. Stateful reference-model tests, Miri, and
bounded fuzzing cover both layouts.
