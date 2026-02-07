# MMR Logic Spec (Current Crate Behavior, Refactor Baseline)

## 1. Purpose
This document specifies how MMR logic is implemented in this crate today, focusing only on the core algorithms needed for a minimal MMR library:

- create new MMR (with hasher + store)
- append leaf
- get proof for leaf
- verify proof for leaf
- get peaks
- bag the peaks
- calculate root hash

Out of scope for this spec: stacked MMR, draft MMR, non-MMR merkle trees.

## 2. Canonical Model

### 2.1 Indexing
- Element indices are `1`-based and refer to post-order MMR positions.
- Leaves are a subset of element indices.
- Mapping formulas:
  - `element_index = 2 * leaf_index + 1 - popcount(leaf_index)` where `leaf_index` is `0`-based.
  - `leaf_index = elements_count_to_leaf_count(element_index - 1)` (only valid for leaf indices).
- Example leaf element indices: `1, 2, 4, 5, 8, 9, 11, 12, ...`

### 2.2 Mountains and Peaks
- For MMR size `N` (`elements_count`), peaks are found by decomposing `N` into full mountain sizes `(2^h - 1)`.
- `find_peaks(N)` returns peak element indices from left to right.
- Valid canonical sizes include `1, 3, 4, 7, 8, 10, 11, 15, ...`

### 2.3 Stored State
State is stored under one `mmr_id` namespace:

- `leaf_count`
- `elements_count`
- `root_hash`
- `hashes:{element_index}` for node hashes

Current implementation stores hash/data values as `String`.

## 3. Hashing Contract (Minimal)
- MMR logic treats the input leaf as already in hash-domain form for that hasher.
- Append stores leaf value as-is at the leaf element index.
- Parent hash is computed as `H(left_hash, right_hash)`.
- Root hash is computed as `H(elements_count_as_decimal_string, bagged_peaks_hash)`.

## 4. Algorithms

### 4.1 Create New MMR
`new(store, hasher, mmr_id?)`

1. Choose `mmr_id` (`UUID` if missing).
2. Bind store-backed counters/tables for that id.
3. Do not write anything eagerly.

`create_from_peaks(store, hasher, mmr_id?, peaks_hashes, elements_count)`

1. Create MMR via `new`.
2. Require current `elements_count == 0`, else `NonEmptyMMR`.
3. Compute `expected_peak_indices = find_peaks(elements_count)`.
4. Require `expected_peak_indices.len() == peaks_hashes.len()`, else `InvalidPeaksCountForElements`.
5. Set:
   - `leaves_count = mmr_size_to_leaf_count(elements_count)`
   - `elements_count = elements_count`
6. Store each provided peak hash at its expected peak index.
7. Compute bag via `bag_the_peaks(Some(elements_count))`.
8. Compute root via `calculate_root_hash(bag, elements_count)` and persist `root_hash`.

Important behavior:
- Only peak hashes are present initially. Old internal nodes/leaves under these peaks are absent, so old proofs are not generally reproducible from this state.

### 4.2 Append Leaf
Input: `value: String`

1. Validate `value` size with hasher-specific `is_element_size_valid`.
2. Read current `elements_count = E`.
3. Read current peak hashes for `find_peaks(E)` (left to right).
4. Increment `elements_count` counter once to get leaf element index `L = E + 1`.
5. Store leaf at `hashes[L] = value`.
6. Append `value` to local peaks list.
7. Compute number of merges:
   - `m = trailing_ones(leaves_count_before_append)`
8. Repeat `m` times:
   - Increment local `last_element_idx` by `1` (no counter write yet).
   - Pop `right`, pop `left` from local peaks.
   - `parent = H(left, right)`
   - Store `hashes[last_element_idx] = parent`
   - Push `parent` back to local peaks.
9. Persist final `elements_count = last_element_idx`.
10. Recompute bag from store (`bag_the_peaks(None)`).
11. `root = H(elements_count_decimal, bag)`; store to `root_hash`.
12. Increment `leaves_count`.
13. Return:
   - `element_index = L`
   - new `leaves_count`
   - new `elements_count`
   - `root_hash`

Merge intuition:
- `trailing_ones(leaves_count_before)` is exactly how many carry-merges happen when adding one leaf.

### 4.3 Get Proof for Leaf
`get_proof(element_index, options?)`

1. Validate `element_index != 0`.
2. Determine proof tree size:
   - `tree_size = options.elements_count.unwrap_or(current_elements_count)`
3. Require `element_index <= tree_size`.
4. `peaks = find_peaks(tree_size)`.
5. `siblings = find_siblings(element_index, tree_size)`:
   - Walk upward by leaf parity and height offsets `(2 << h) - 1`.
   - Drop last overshoot entry.
6. Fetch peak hashes by peak indices.
   - If formatting options exist, pad peaks to fixed output size with `null_value`.
7. Fetch sibling hashes in sibling index order.
   - If formatting options exist, pad proof similarly.
8. Fetch element hash at `element_index`.
9. Return proof:
   - `element_index`
   - `element_hash`
   - `siblings_hashes`
   - `peaks_hashes`
   - `elements_count = tree_size`

### 4.4 Verify Proof for Leaf
`verify_proof(proof, element_value, options?) -> bool`

1. Determine `tree_size` exactly as in `get_proof`.
2. Compute expected peak count:
   - `leaf_count = mmr_size_to_leaf_count(tree_size)`
   - `expected_peaks = popcount(leaf_count)`
3. Require `proof.peaks_hashes.len() == expected_peaks`, else `InvalidPeaksCount`.
4. If formatting options provided:
   - Count entries equal to proof `null_value`, truncate that many items from end of `siblings_hashes`.
   - Same for `peaks_hashes`.
5. Validate `proof.element_index` in `[1, tree_size]`.
6. Compute `(peak_index, peak_height) = get_peak_info(tree_size, proof.element_index)`.
7. Require `siblings_hashes.len() == peak_height`; if not, return `false`.
8. Rebuild mountain root:
   - Start `hash = element_value`.
   - `leaf_index = element_index_to_leaf_index(proof.element_index)`.
   - For each sibling hash:
     - if current node is right child (`leaf_index % 2 == 1`): `hash = H(sibling, hash)`
     - else `hash = H(hash, sibling)`
     - `leaf_index /= 2`
9. Load canonical peaks from store for `tree_size`.
10. Return `canonical_peaks[peak_index] == hash`.

Current behavior details:
- Verification uses the supplied `element_value` as starting hash material.
- `proof.element_hash` is not used in verification.
- `proof.peaks_hashes` are only used for count/null-trimming checks; final peak comparison is against store-loaded peaks.
- Therefore, verification is store-coupled (not purely stateless proof verification).

### 4.5 Get Peaks
`get_peaks({ elements_count?, formatting_opts? })`

1. Resolve `tree_size`.
2. Compute peak indices `find_peaks(tree_size)`.
3. Fetch hashes for those indices in order.
4. Optionally pad with formatting nulls to fixed output size.
5. Return peaks left-to-right.

### 4.6 Bag the Peaks
`bag_the_peaks(elements_count?) -> bag_hash`

1. Resolve `tree_size`.
2. Fetch ordered peak hashes for that size.
3. Cases:
   - 0 peaks: return `"0x0"`
   - 1 peak: return that peak
   - 2+ peaks:
     - Start with rightmost pair: `acc = H(peak[n-2], peak[n-1])`
     - Fold remaining peaks from right to left:
       - `acc = H(peak[i], acc)` for `i = n-3 .. 0`
4. Return `acc`.

Equivalent shape for `[p0, p1, p2, p3]`:
- `bag = H(p0, H(p1, H(p2, p3)))`

### 4.7 Calculate Root Hash
`calculate_root_hash(bag, elements_count)`

- `root = H(elements_count_decimal_string, bag)`
- Return root.

## 5. Helper Formula Reference
- `leaf_count_to_append_no_merges(leaf_count) = trailing_ones(leaf_count)`
- `leaf_count_to_peaks_count(leaf_count) = popcount(leaf_count)`
- `leaf_count_to_mmr_size(leaf_count) = 2 * leaf_count - popcount(leaf_count)`
- `mmr_size_to_leaf_count(mmr_size)` via greedy mountain decomposition.
- `get_peak_info(elements_count, element_index)` returns:
  - peak ordinal (left-to-right)
  - mountain height of that element path.

## 6. Complexity and Current I/O Profile

### 6.1 CPU Complexity
- `append`: `O(log n)` hashes in worst case (number of merges).
- `get_proof`: `O(log n)` sibling computation/fetch.
- `verify_proof`: `O(log n)` hash recomputation.
- `get_peaks`: `O(number_of_peaks)` where peaks = `popcount(leaf_count)`.
- `bag_the_peaks`: `O(number_of_peaks)`.

### 6.2 Current Append Store Round-Trips
Per append (excluding hasher work), current flow performs:
- Reads: 7 logical read operations (`get`/`get_many` calls).
- Writes: `5 + m` logical writes where `m = trailing_ones(old_leaf_count)`.

Main sources:
- repeated reads of counts and peaks
- per-parent individual writes during merge
- recomputing bag by reading peaks from store after writing parents

This is the key reason append becomes DB-chatty.

## 7. Batch Append Ideas (Future Capability)

Target: append `k` leaves with one bulk write (or minimal batched writes), preserving identical root semantics.

### 7.1 Staged Frontier + Single Commit
Algorithm sketch:

1. Load once:
   - `old_leaves_count`, `old_elements_count`
   - current peaks hashes.
2. Keep in-memory mutable state:
   - `leaves_count`, `elements_count`, `peaks_stack`
   - `pending_entries: HashMap<SubKey, HashValue>`
3. For each new leaf:
   - assign next leaf element index
   - stage leaf write
   - apply `trailing_ones(leaves_count)` merges in memory
   - stage each parent write
   - increment local `leaves_count`
4. After all leaves:
   - bag in-memory peaks
   - compute root from final `elements_count`
   - stage metadata writes (`leaf_count`, `elements_count`, `root_hash`)
5. Execute one `set_many` (transactional in SQL stores).

Benefits:
- avoids re-reading peaks and counters per element
- collapses many single-row writes to one batched commit
- preserves exact append semantics

### 7.2 Optional API Shape
- `append_many(values: &[LeafValue]) -> BatchAppendResult`
- Return:
  - first/last appended element index
  - final counts
  - final root
  - optional per-leaf indices for callers that need them

### 7.3 Consistency Rule
- Treat batch as atomic.
- On failure, no partial metadata update.
- SQL stores: transaction.
- memory store: apply all in one lock section.

## 8. Datatype and Encoding Improvements for Minimal Library

Current code uses `String` for leaves, hashes, and counters. For a minimal high-throughput MMR, prefer:

- `Hash = [u8; 32]` (or newtype wrapper)
- `LeafValue` as explicit type:
  - either raw bytes (`Vec<u8>`) with a clear leaf-hash rule
  - or prehashed `Hash`
- `ElementIndex = u64` / `LeafCount = u64`
- Store counters as integers, not decimal strings
- Separate typed APIs:
  - `append_prehashed(hash: Hash)` or
  - `append_raw(data: &[u8])` with deterministic leaf hashing

This removes frequent string parse/format overhead and clarifies hash-domain semantics.

## 9. Behavioral Notes to Preserve or Intentionally Change in Refactor

Preserve (if compatibility needed):
- post-order indexing and peak bagging order
- root formula `H(elements_count, bag)`
- merge-count rule from trailing ones

Likely intentional changes for minimal library:
- make verification stateless by comparing against `proof.peaks_hashes` (not store)
- define whether verifier starts from raw leaf or prehashed leaf
- remove formatting/null-padding from core proof logic (presentation concern)

