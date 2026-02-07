pub mod error;
pub mod hasher;
pub mod mmr;
pub mod store;
pub mod types;

pub use error::{HasherError, MmrError, StoreError};
pub use hasher::{Hasher, KeccakHasher, PoseidonHasher};
pub use mmr::{
    Mmr, element_index_to_leaf_index, elements_count_to_leaf_count, find_peaks, find_siblings,
    get_peak_info, leaf_count_to_append_no_merges, leaf_count_to_mmr_size,
    leaf_count_to_peaks_count, map_leaf_index_to_element_index, mmr_size_to_leaf_count,
};
pub use store::{InMemoryStore, KeyKind, Store, StoreKey, StoreValue};
#[cfg(feature = "postgres-store")]
pub use store::{PostgresStore, PostgresStoreOptions};
pub use types::{AppendResult, BatchAppendResult, Hash32, MmrId, Proof};
