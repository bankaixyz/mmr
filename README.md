# mmr

`mmr` is a minimal, async Merkle Mountain Range (MMR) library with typed hashes, pluggable storage, and Keccak/Poseidon hashing.

## Functionality

- Build an MMR from scratch or from existing peaks.
- Append one value or many values (`batch_append`).
- Query peaks, bag peaks, and compute root hashes.
- Generate and verify inclusion proofs.
- Verify proofs without storage state (`stateless-verify` feature).

## Storage Backends

- `InMemoryStore` for fast local/testing usage.
- `PostgresStore` for persistent storage (`postgres-store` feature).

## Hashers

- `KeccakHasher`
- `PoseidonHasher`

## Quick Example

```rust
use std::sync::Arc;
use mmr::{InMemoryStore, KeccakHasher, Mmr};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let store = Arc::new(InMemoryStore::default());
    let hasher = Arc::new(KeccakHasher::new());
    let mut mmr = Mmr::new(store, hasher, Some(1))?;

    let leaf = [1u8; 32];
    let append = mmr.append(leaf).await?;

    let proof = mmr.get_proof(append.element_index, None).await?;
    assert!(mmr.verify_proof(&proof, leaf, None).await?);

    Ok(())
}
```

## Optional Features

- `postgres-store`: enables PostgreSQL-backed storage.
- `stateless-verify`: enables `verify_proof_stateless`.

## Running Tests

```bash
cargo test
```

## Acknowledgements

Thanks to Herodotus for their work on MMRs and open-source reference implementations:
[HerodotusDev/rust-accumulators](https://github.com/HerodotusDev/rust-accumulators).
