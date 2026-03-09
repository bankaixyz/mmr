use crate::error::MmrError;
use crate::hasher::Hasher;
use crate::types::{Hash32, Proof, ZERO_HASH};

use super::helpers::{
    element_index_to_leaf_index, elements_count_to_leaf_count, find_peaks, get_peak_info,
};

pub fn bag_peaks_hashes(hasher: &dyn Hasher, peaks: &[Hash32]) -> Result<Hash32, MmrError> {
    match peaks.len() {
        0 => Ok(ZERO_HASH),
        1 => Ok(peaks[0]),
        _ => {
            let mut acc = hasher.hash_pair(&peaks[peaks.len() - 2], &peaks[peaks.len() - 1])?;

            for peak in peaks[..peaks.len() - 2].iter().rev() {
                acc = hasher.hash_pair(peak, &acc)?;
            }

            Ok(acc)
        }
    }
}

pub fn calculate_root_hash(
    hasher: &dyn Hasher,
    elements_count: u64,
    peaks: &[Hash32],
) -> Result<Hash32, MmrError> {
    elements_count_to_leaf_count(elements_count)?;

    if peaks.len() != find_peaks(elements_count).len() {
        return Err(MmrError::InvalidPeaksCountForElements);
    }

    let bag = bag_peaks_hashes(hasher, peaks)?;
    Ok(hasher.hash_count_and_bag(elements_count, &bag)?)
}

pub fn verify_proof_stateless(
    hasher: &dyn Hasher,
    proof: &Proof,
    element_value: Hash32,
) -> Result<bool, MmrError> {
    elements_count_to_leaf_count(proof.elements_count)?;

    if proof.peaks_hashes.len() != find_peaks(proof.elements_count).len() {
        return Err(MmrError::InvalidPeaksCount);
    }

    if proof.element_index == 0 || proof.element_index > proof.elements_count {
        return Err(MmrError::InvalidElementIndex);
    }

    if proof.element_hash != element_value {
        return Ok(false);
    }

    let (peak_index, peak_height) = get_peak_info(proof.elements_count, proof.element_index);
    if proof.siblings_hashes.len() != peak_height {
        return Ok(false);
    }

    let mut hash = element_value;
    let mut leaf_index = element_index_to_leaf_index(proof.element_index)?;

    for sibling_hash in &proof.siblings_hashes {
        let is_right = leaf_index % 2 == 1;
        leaf_index /= 2;
        hash = if is_right {
            hasher.hash_pair(sibling_hash, &hash)?
        } else {
            hasher.hash_pair(&hash, sibling_hash)?
        };
    }

    Ok(proof.peaks_hashes.get(peak_index).copied() == Some(hash))
}

pub fn verify_proof_stateless_with_root(
    hasher: &dyn Hasher,
    proof: &Proof,
    element_value: Hash32,
    expected_root: &Hash32,
) -> Result<bool, MmrError> {
    if !verify_proof_stateless(hasher, proof, element_value)? {
        return Ok(false);
    }

    Ok(calculate_root_hash(hasher, proof.elements_count, &proof.peaks_hashes)? == *expected_root)
}
