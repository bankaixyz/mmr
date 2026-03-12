#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

pub mod error;
pub mod hasher;
pub mod mmr;
#[cfg(feature = "std")]
pub mod store;
pub mod types;

pub use error::{HasherError, MmrError};
#[cfg(feature = "std")]
pub use error::StoreError;
pub use hasher::{Hasher, KeccakHasher};
#[cfg(feature = "poseidon")]
pub use hasher::PoseidonHasher;
pub use mmr::{
    bag_peaks_hashes, calculate_root_hash, element_index_to_leaf_index, elements_count_to_leaf_count,
    find_peaks, find_siblings, get_peak_info, leaf_count_to_append_no_merges,
    leaf_count_to_mmr_size, leaf_count_to_peaks_count, map_leaf_index_to_element_index,
    mmr_size_to_leaf_count, stateless, verify_proof_stateless, verify_proof_stateless_with_root,
};
#[cfg(feature = "std")]
pub use mmr::Mmr;
#[cfg(feature = "std")]
pub use store::{InMemoryStore, KeyKind, PendingBatch, Store, StoreKey, StoreValue};
#[cfg(feature = "postgres-store")]
pub use store::{PostgresStore, PostgresStoreOptions};
pub use types::{AppendResult, BatchAppendResult, Hash32, MmrId, Proof};
