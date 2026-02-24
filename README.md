# mmr

`mmr` is a minimal, async Merkle Mountain Range (MMR) library with typed hashes, pluggable storage, and Keccak/Poseidon hashing.

## Functionality

- Build an MMR from scratch or from existing peaks.
- Append one value or many values (`batch_append`).
- Stage batched appends as precommits (`batch_precommit_append`) and either finalize (`commit_precommit`) or discard (`revert_precommit`).
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

## Precommit Flow

Precommit uses a pending journal per `mmr_id`:

- `batch_precommit_append` computes staged writes with the same internal append logic as `batch_append`, then stores them in the pending journal.
- Normal `append` and `batch_append` are blocked while a pending precommit exists.
- Reads (`get_root_hash`, `get_peaks`, proofs) continue to reflect committed state until finalize.
- `commit_precommit` applies staged writes to committed storage and returns the same `BatchAppendResult` produced during precommit.
- `revert_precommit` drops the pending journal without changing committed state.
- Precommit journaling is available with `PostgresStore` (`postgres-store` feature).

```rust
use std::sync::Arc;
use mmr::{KeccakHasher, Mmr, PostgresStore, PostgresStoreOptions};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let database_url = std::env::var("DATABASE_URL")?;
    let store = Arc::new(
        PostgresStore::connect_with_options(
            &database_url,
            PostgresStoreOptions {
                initialize_schema: true,
                max_connections: 2,
            },
        )
        .await?,
    );
    let hasher = Arc::new(KeccakHasher::new());
    let mut mmr = Mmr::new(store, hasher, Some(1))?;

    let staged = mmr
        .batch_precommit_append(&[[1u8; 32], [2u8; 32], [3u8; 32]])
        .await?;

    // Still committed-only until finalize.
    assert_eq!(mmr.get_elements_count().await?, 0);

    let committed = mmr.commit_precommit().await?;
    assert_eq!(staged, committed);

    Ok(())
}
```

Run with:

```bash
cargo run --features postgres-store
```

## Acknowledgements

Thanks to Herodotus for their work on MMRs and open-source reference implementations:
[HerodotusDev/rust-accumulators](https://github.com/HerodotusDev/rust-accumulators).
