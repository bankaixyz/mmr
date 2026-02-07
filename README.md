# mmr-core

`mmr-core` is a minimal, synchronous Merkle Mountain Range (MMR) library focused on clear APIs, typed data, and efficient storage access.

## What it provides

- Core MMR operations:
  - `new`
  - `create_from_peaks`
  - `append`
  - `batch_append`
  - `get_proof`
  - `verify_proof`
  - `get_peaks`
  - `bag_the_peaks`
  - `calculate_root_hash`
- Typed hashes and counters (`[u8; 32]`, `u64`, `u32`)
- Pluggable `Store` and `Hasher` traits
- Included stores:
  - `InMemoryStore`
  - `PostgresStore` (with `postgres-store` feature)
- Included hashers:
  - `KeccakHasher`
  - `PoseidonHasher`

## Public exports

From `mmr`:

- MMR:
  - `Mmr`
- Hashers:
  - `Hasher`
  - `KeccakHasher`
  - `PoseidonHasher`
- Stores:
  - `Store`
  - `InMemoryStore`
  - `PostgresStore` and `PostgresStoreOptions` (feature-gated)
  - `StoreKey`, `StoreValue`, `KeyKind`
- Types:
  - `Hash32`
  - `MmrId`
  - `Proof`
  - `AppendResult`
  - `BatchAppendResult`
- Helper functions:
  - `find_peaks`
  - `find_siblings`
  - `get_peak_info`
  - `map_leaf_index_to_element_index`
  - `elements_count_to_leaf_count`
  - `mmr_size_to_leaf_count`
  - `leaf_count_to_mmr_size`
  - `leaf_count_to_peaks_count`
  - `leaf_count_to_append_no_merges`
  - `element_index_to_leaf_index`

## Quick example

```rust
use std::sync::Arc;
use mmr::{InMemoryStore, KeccakHasher, Mmr};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let store = Arc::new(InMemoryStore::default());
    let hasher = Arc::new(KeccakHasher::new());
    let mut mmr = Mmr::new(store, hasher, Some(1))?;

    let a = [1u8; 32];
    let b = [2u8; 32];

    let append_a = mmr.append(a)?;
    let _append_b = mmr.append(b)?;

    let proof = mmr.get_proof(append_a.element_index, None)?;
    let ok = mmr.verify_proof(&proof, a, None)?;
    assert!(ok);

    Ok(())
}
```

## Features

- `postgres-store`: enables PostgreSQL-backed storage.
- `stateless-verify`: enables stateless proof verification API.
