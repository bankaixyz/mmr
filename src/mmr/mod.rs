#[cfg(feature = "std")]
mod core;
mod helpers;
pub mod stateless;

#[cfg(feature = "std")]
pub use core::Mmr;
pub use helpers::{
    element_index_to_leaf_index, elements_count_to_leaf_count, find_peaks, find_siblings,
    get_peak_info, leaf_count_to_append_no_merges, leaf_count_to_mmr_size,
    leaf_count_to_peaks_count, map_leaf_index_to_element_index, mmr_size_to_leaf_count,
};
pub use stateless::{
    bag_peaks_hashes, calculate_root_hash, verify_proof_stateless, verify_proof_stateless_with_root,
};
