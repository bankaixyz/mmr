use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

#[cfg(feature = "postgres-store")]
use sqlx::{Postgres, Transaction};

use crate::error::MmrError;
use crate::hasher::Hasher;
#[cfg(feature = "postgres-store")]
use crate::store::PostgresStore;
use crate::store::{KeyKind, Store, StoreKey, StoreValue};
use crate::types::{
    AppendResult, BatchAppendResult, ElementIndex, Hash32, MmrId, Proof, ZERO_HASH,
};

use super::helpers::{
    element_index_to_leaf_index, find_peaks, find_siblings, get_peak_info,
    leaf_count_to_append_no_merges, leaf_count_to_peaks_count, mmr_size_to_leaf_count,
};

static NEXT_MMR_ID: AtomicU32 = AtomicU32::new(1);

#[derive(Debug, Clone, Copy)]
struct CachedCounts {
    leaves_count: u64,
    elements_count: u64,
}

pub struct Mmr<S: Store> {
    pub mmr_id: MmrId,
    store: S,
    hasher: Arc<dyn Hasher>,
    cached_counts: Option<CachedCounts>,
}

impl<S: Store> fmt::Debug for Mmr<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Mmr").field("mmr_id", &self.mmr_id).finish()
    }
}

impl<S: Store> Mmr<S> {
    pub fn new(store: S, hasher: Arc<dyn Hasher>, mmr_id: Option<MmrId>) -> Result<Self, MmrError> {
        let resolved_id = mmr_id.unwrap_or_else(|| NEXT_MMR_ID.fetch_add(1, Ordering::Relaxed));

        Ok(Self {
            mmr_id: resolved_id,
            store,
            hasher,
            cached_counts: None,
        })
    }

    pub async fn create_from_peaks(
        store: S,
        hasher: Arc<dyn Hasher>,
        mmr_id: Option<MmrId>,
        peaks_hashes: Vec<Hash32>,
        elements_count: u64,
    ) -> Result<Self, MmrError> {
        let mut mmr = Self::new(store, hasher, mmr_id)?;

        let current_elements_count = mmr.get_elements_count().await?;
        if current_elements_count != 0 {
            return Err(MmrError::NonEmptyMmr);
        }

        let expected_peak_indices = find_peaks(elements_count);
        if expected_peak_indices.len() != peaks_hashes.len() {
            return Err(MmrError::InvalidPeaksCountForElements);
        }

        let leaves_count = mmr_size_to_leaf_count(elements_count);
        mmr.set_leaves_count(leaves_count).await?;
        mmr.set_elements_count(elements_count).await?;

        for (peak_index, peak_hash) in expected_peak_indices.iter().zip(peaks_hashes.iter()) {
            mmr.set_node_hash(*peak_index, *peak_hash).await?;
        }

        let bag = mmr.bag_the_peaks(Some(elements_count)).await?;
        let root_hash = mmr.calculate_root_hash(&bag, elements_count)?;
        mmr.set_root_hash(root_hash).await?;
        mmr.cached_counts = Some(CachedCounts {
            leaves_count,
            elements_count,
        });

        Ok(mmr)
    }

    pub async fn append(&mut self, value: Hash32) -> Result<AppendResult, MmrError> {
        let batch_result = self.batch_append(&[value]).await?;
        Ok(AppendResult {
            leaves_count: batch_result.leaves_count,
            elements_count: batch_result.elements_count,
            element_index: batch_result.first_element_index,
            root_hash: batch_result.root_hash,
        })
    }

    pub async fn batch_append(&mut self, values: &[Hash32]) -> Result<BatchAppendResult, MmrError> {
        if values.is_empty() {
            return Err(MmrError::EmptyBatchAppend);
        }

        let append_state = self.prepare_append_state().await?;
        let AppendComputation {
            staged_writes,
            result,
        } = self.build_append_writes(values, append_state)?;

        self.store.set_many(staged_writes).await?;
        self.cached_counts = Some(CachedCounts {
            leaves_count: result.leaves_count,
            elements_count: result.elements_count,
        });

        Ok(result)
    }

    pub async fn get_proof(
        &self,
        element_index: ElementIndex,
        elements_count: Option<u64>,
    ) -> Result<Proof, MmrError> {
        if element_index == 0 {
            return Err(MmrError::InvalidElementIndex);
        }

        let tree_size = match elements_count {
            Some(count) => count,
            None => self.get_elements_count().await?,
        };

        if element_index > tree_size {
            return Err(MmrError::InvalidElementIndex);
        }

        let peaks = find_peaks(tree_size);
        let siblings = find_siblings(element_index, tree_size)?;

        let peaks_hashes = self.retrieve_peaks_hashes(peaks).await?;

        let sibling_keys: Vec<StoreKey> = siblings.iter().map(|idx| self.node_key(*idx)).collect();
        let sibling_values = self.store.get_many(&sibling_keys).await?;
        let mut siblings_hashes = Vec::new();
        for (key, value) in sibling_keys.iter().zip(sibling_values.into_iter()) {
            if let Some(value) = value {
                siblings_hashes.push(value.expect_hash(key)?);
            }
        }

        let element_hash = self
            .get_node_hash(element_index)
            .await?
            .ok_or(MmrError::NoHashFoundForIndex(element_index))?;

        Ok(Proof {
            element_index,
            element_hash,
            siblings_hashes,
            peaks_hashes,
            elements_count: tree_size,
        })
    }

    pub async fn verify_proof(
        &self,
        proof: &Proof,
        element_value: Hash32,
        elements_count: Option<u64>,
    ) -> Result<bool, MmrError> {
        let tree_size = match elements_count {
            Some(count) => count,
            None => self.get_elements_count().await?,
        };
        let leaf_count = mmr_size_to_leaf_count(tree_size);
        let expected_peaks = leaf_count_to_peaks_count(leaf_count) as usize;

        if proof.peaks_hashes.len() != expected_peaks {
            return Err(MmrError::InvalidPeaksCount);
        }

        if proof.element_index == 0 || proof.element_index > tree_size {
            return Err(MmrError::InvalidElementIndex);
        }

        let (peak_index, peak_height) = get_peak_info(tree_size, proof.element_index);
        if proof.siblings_hashes.len() != peak_height {
            return Ok(false);
        }

        let mut hash = element_value;
        let mut leaf_index = element_index_to_leaf_index(proof.element_index)?;

        for sibling_hash in &proof.siblings_hashes {
            let is_right = leaf_index % 2 == 1;
            leaf_index /= 2;
            hash = if is_right {
                self.hasher.hash_pair(sibling_hash, &hash)?
            } else {
                self.hasher.hash_pair(&hash, sibling_hash)?
            };
        }

        let peak_hashes = self.retrieve_peaks_hashes(find_peaks(tree_size)).await?;

        Ok(peak_hashes.get(peak_index).copied() == Some(hash))
    }

    #[cfg(feature = "stateless-verify")]
    pub async fn verify_proof_stateless(
        &self,
        proof: &Proof,
        element_value: Hash32,
        elements_count: Option<u64>,
    ) -> Result<bool, MmrError> {
        let tree_size = match elements_count {
            Some(count) => count,
            None => self.get_elements_count().await?,
        };
        let leaf_count = mmr_size_to_leaf_count(tree_size);
        let expected_peaks = leaf_count_to_peaks_count(leaf_count) as usize;

        if proof.peaks_hashes.len() != expected_peaks {
            return Err(MmrError::InvalidPeaksCount);
        }

        if proof.element_index == 0 || proof.element_index > tree_size {
            return Err(MmrError::InvalidElementIndex);
        }

        let (peak_index, peak_height) = get_peak_info(tree_size, proof.element_index);
        if proof.siblings_hashes.len() != peak_height {
            return Ok(false);
        }

        let mut hash = element_value;
        let mut leaf_index = element_index_to_leaf_index(proof.element_index)?;

        for sibling_hash in &proof.siblings_hashes {
            let is_right = leaf_index % 2 == 1;
            leaf_index /= 2;
            hash = if is_right {
                self.hasher.hash_pair(sibling_hash, &hash)?
            } else {
                self.hasher.hash_pair(&hash, sibling_hash)?
            };
        }

        Ok(proof.peaks_hashes.get(peak_index).copied() == Some(hash))
    }

    pub async fn get_peaks(&self, elements_count: Option<u64>) -> Result<Vec<Hash32>, MmrError> {
        let tree_size = match elements_count {
            Some(count) => count,
            None => self.get_elements_count().await?,
        };
        self.retrieve_peaks_hashes(find_peaks(tree_size)).await
    }

    pub async fn bag_the_peaks(&self, elements_count: Option<u64>) -> Result<Hash32, MmrError> {
        let tree_size = match elements_count {
            Some(count) => count,
            None => self.get_elements_count().await?,
        };
        let peaks_idxs = find_peaks(tree_size);
        let peaks_hashes = self.retrieve_peaks_hashes(peaks_idxs.clone()).await?;
        self.bag_peaks_hashes(&peaks_idxs, &peaks_hashes)
    }

    fn bag_peaks_hashes(
        &self,
        peak_indices: &[u64],
        peak_hashes: &[Hash32],
    ) -> Result<Hash32, MmrError> {
        match peak_indices.len() {
            0 => Ok(ZERO_HASH),
            1 => peak_hashes
                .first()
                .copied()
                .ok_or(MmrError::NoHashFoundForIndex(peak_indices[0])),
            _ => {
                if peak_hashes.len() < 2 {
                    return Err(MmrError::NoHashFoundForIndex(peak_indices[0]));
                }

                let mut acc = self.hasher.hash_pair(
                    &peak_hashes[peak_hashes.len() - 2],
                    &peak_hashes[peak_hashes.len() - 1],
                )?;

                for peak in peak_hashes[..peak_hashes.len() - 2].iter().rev() {
                    acc = self.hasher.hash_pair(peak, &acc)?;
                }

                Ok(acc)
            }
        }
    }

    pub fn calculate_root_hash(
        &self,
        bag: &Hash32,
        elements_count: u64,
    ) -> Result<Hash32, MmrError> {
        Ok(self.hasher.hash_count_and_bag(elements_count, bag)?)
    }

    pub async fn get_root_hash(&self) -> Result<Option<Hash32>, MmrError> {
        match self.store.get(&self.root_hash_key()).await? {
            Some(value) => Ok(Some(value.expect_hash(&self.root_hash_key())?)),
            None => Ok(None),
        }
    }

    async fn retrieve_peaks_hashes(&self, peak_idxs: Vec<u64>) -> Result<Vec<Hash32>, MmrError> {
        let keys: Vec<StoreKey> = peak_idxs.iter().map(|idx| self.node_key(*idx)).collect();
        let values = self.store.get_many(&keys).await?;

        let mut hashes = Vec::with_capacity(values.len());
        for (key, value) in keys.iter().zip(values.into_iter()) {
            if let Some(value) = value {
                hashes.push(value.expect_hash(key)?);
            }
        }

        Ok(hashes)
    }

    async fn prepare_append_state(&mut self) -> Result<AppendState, MmrError> {
        let cached_counts = self.load_cached_counts().await?;
        if cached_counts.elements_count == 0 {
            return Ok(AppendState {
                leaves_count: cached_counts.leaves_count,
                elements_count: cached_counts.elements_count,
                peaks_hashes: Vec::new(),
            });
        }

        let peak_indices = find_peaks(cached_counts.elements_count);
        let append_state = self.load_append_state(&peak_indices).await?;

        if append_state.leaves_count != cached_counts.leaves_count
            || append_state.elements_count != cached_counts.elements_count
        {
            return Err(MmrError::Store(crate::error::StoreError::Internal(
                "mmr metadata changed unexpectedly; multiple writers for same mmr_id are not supported"
                    .to_string(),
            )));
        }

        Ok(append_state)
    }

    async fn load_cached_counts(&mut self) -> Result<CachedCounts, MmrError> {
        if let Some(cached_counts) = self.cached_counts {
            return Ok(cached_counts);
        }

        let leaf_count_key = self.leaf_count_key();
        let elements_count_key = self.elements_count_key();
        let keys = vec![leaf_count_key.clone(), elements_count_key.clone()];
        let values = self.store.get_many(&keys).await?;

        let leaves_count =
            Self::extract_counter(&leaf_count_key, values.first().cloned().flatten())?;
        let elements_count =
            Self::extract_counter(&elements_count_key, values.get(1).cloned().flatten())?;

        let cached_counts = CachedCounts {
            leaves_count,
            elements_count,
        };
        self.cached_counts = Some(cached_counts);
        Ok(cached_counts)
    }

    async fn load_append_state(&self, peak_indices: &[u64]) -> Result<AppendState, MmrError> {
        let leaf_count_key = self.leaf_count_key();
        let elements_count_key = self.elements_count_key();
        let mut keys = Vec::with_capacity(2 + peak_indices.len());
        keys.push(leaf_count_key.clone());
        keys.push(elements_count_key.clone());
        keys.extend(peak_indices.iter().map(|idx| self.node_key(*idx)));

        let values = self.store.get_many(&keys).await?;
        let leaves_count =
            Self::extract_counter(&leaf_count_key, values.first().cloned().flatten())?;
        let elements_count =
            Self::extract_counter(&elements_count_key, values.get(1).cloned().flatten())?;

        let mut peaks_hashes = Vec::with_capacity(peak_indices.len());
        for (key, value) in keys[2..].iter().zip(values.into_iter().skip(2)) {
            if let Some(value) = value {
                peaks_hashes.push(value.expect_hash(key)?);
            }
        }

        Ok(AppendState {
            leaves_count,
            elements_count,
            peaks_hashes,
        })
    }

    fn build_append_writes(
        &self,
        values: &[Hash32],
        append_state: AppendState,
    ) -> Result<AppendComputation, MmrError> {
        let mut leaves_count = append_state.leaves_count;
        let mut elements_count = append_state.elements_count;
        let mut peaks = append_state.peaks_hashes;
        let mut staged_writes = Vec::with_capacity(
            values
                .len()
                .checked_mul(2)
                .and_then(|v| v.checked_add(3))
                .ok_or(MmrError::Overflow)?,
        );

        let first_element_index = elements_count.checked_add(1).ok_or(MmrError::Overflow)?;
        let mut last_element_index = first_element_index;

        for value in values {
            let leaf_element_index = elements_count.checked_add(1).ok_or(MmrError::Overflow)?;
            last_element_index = leaf_element_index;
            elements_count = leaf_element_index;

            staged_writes.push((self.node_key(leaf_element_index), StoreValue::Hash(*value)));
            peaks.push(*value);

            let no_merges = leaf_count_to_append_no_merges(leaves_count);

            for _ in 0..no_merges {
                elements_count = elements_count.checked_add(1).ok_or(MmrError::Overflow)?;

                let right_hash = peaks
                    .pop()
                    .ok_or(MmrError::NoHashFoundForIndex(elements_count))?;
                let left_hash = peaks
                    .pop()
                    .ok_or(MmrError::NoHashFoundForIndex(elements_count))?;

                let parent_hash = self.hasher.hash_pair(&left_hash, &right_hash)?;
                staged_writes.push((self.node_key(elements_count), StoreValue::Hash(parent_hash)));
                peaks.push(parent_hash);
            }

            leaves_count = leaves_count.checked_add(1).ok_or(MmrError::Overflow)?;
        }

        let peak_indices = find_peaks(elements_count);
        let bag = self.bag_peaks_hashes(&peak_indices, &peaks)?;
        let root_hash = self.calculate_root_hash(&bag, elements_count)?;

        staged_writes.push((self.elements_count_key(), StoreValue::U64(elements_count)));
        staged_writes.push((self.root_hash_key(), StoreValue::Hash(root_hash)));
        staged_writes.push((self.leaf_count_key(), StoreValue::U64(leaves_count)));

        let appended_count = u64::try_from(values.len()).map_err(|_| MmrError::Overflow)?;

        Ok(AppendComputation {
            staged_writes,
            result: BatchAppendResult {
                appended_count,
                first_element_index,
                last_element_index,
                leaves_count,
                elements_count,
                root_hash,
                peaks_hashes: peaks,
            },
        })
    }

    fn extract_counter(key: &StoreKey, value: Option<StoreValue>) -> Result<u64, MmrError> {
        match value {
            Some(value) => Ok(value.expect_u64(key)?),
            None => Ok(0),
        }
    }

    pub async fn get_leaves_count(&self) -> Result<u64, MmrError> {
        match self.store.get(&self.leaf_count_key()).await? {
            Some(value) => Ok(value.expect_u64(&self.leaf_count_key())?),
            None => Ok(0),
        }
    }

    async fn set_leaves_count(&self, value: u64) -> Result<(), MmrError> {
        self.store
            .set(self.leaf_count_key(), StoreValue::U64(value))
            .await
            .map_err(MmrError::from)
    }

    pub async fn get_elements_count(&self) -> Result<u64, MmrError> {
        match self.store.get(&self.elements_count_key()).await? {
            Some(value) => Ok(value.expect_u64(&self.elements_count_key())?),
            None => Ok(0),
        }
    }

    async fn set_elements_count(&self, value: u64) -> Result<(), MmrError> {
        self.store
            .set(self.elements_count_key(), StoreValue::U64(value))
            .await
            .map_err(MmrError::from)
    }

    async fn set_root_hash(&self, hash: Hash32) -> Result<(), MmrError> {
        self.store
            .set(self.root_hash_key(), StoreValue::Hash(hash))
            .await
            .map_err(MmrError::from)
    }

    async fn get_node_hash(&self, index: u64) -> Result<Option<Hash32>, MmrError> {
        let key = self.node_key(index);
        match self.store.get(&key).await? {
            Some(value) => Ok(Some(value.expect_hash(&key)?)),
            None => Ok(None),
        }
    }

    async fn set_node_hash(&self, index: u64, hash: Hash32) -> Result<(), MmrError> {
        self.store
            .set(self.node_key(index), StoreValue::Hash(hash))
            .await
            .map_err(MmrError::from)
    }

    fn leaf_count_key(&self) -> StoreKey {
        StoreKey::metadata(self.mmr_id, KeyKind::LeafCount)
    }

    fn elements_count_key(&self) -> StoreKey {
        StoreKey::metadata(self.mmr_id, KeyKind::ElementsCount)
    }

    fn root_hash_key(&self) -> StoreKey {
        StoreKey::metadata(self.mmr_id, KeyKind::RootHash)
    }

    fn node_key(&self, index: u64) -> StoreKey {
        StoreKey::new(self.mmr_id, KeyKind::NodeHash, index)
    }
}

#[cfg(feature = "postgres-store")]
impl Mmr<Arc<PostgresStore>> {
    pub async fn append_in_tx(
        &mut self,
        tx: &mut Transaction<'_, Postgres>,
        value: Hash32,
    ) -> Result<AppendResult, MmrError> {
        let batch_result = self.batch_append_in_tx(tx, &[value]).await?;
        Ok(AppendResult {
            leaves_count: batch_result.leaves_count,
            elements_count: batch_result.elements_count,
            element_index: batch_result.first_element_index,
            root_hash: batch_result.root_hash,
        })
    }

    pub async fn batch_append_in_tx(
        &mut self,
        tx: &mut Transaction<'_, Postgres>,
        values: &[Hash32],
    ) -> Result<BatchAppendResult, MmrError> {
        if values.is_empty() {
            return Err(MmrError::EmptyBatchAppend);
        }

        self.cached_counts = None;
        let append_state = self.prepare_append_state_in_tx(tx).await?;
        let AppendComputation {
            staged_writes,
            result,
        } = self.build_append_writes(values, append_state)?;

        self.store.set_many_in_tx(tx, staged_writes).await?;
        self.cached_counts = None;

        Ok(result)
    }

    async fn prepare_append_state_in_tx(
        &self,
        tx: &mut Transaction<'_, Postgres>,
    ) -> Result<AppendState, MmrError> {
        let leaf_count_key = self.leaf_count_key();
        let elements_count_key = self.elements_count_key();
        let keys = vec![leaf_count_key.clone(), elements_count_key.clone()];
        let values = self.store.get_many_in_tx(tx, &keys).await?;

        let leaves_count =
            Self::extract_counter(&leaf_count_key, values.first().cloned().flatten())?;
        let elements_count =
            Self::extract_counter(&elements_count_key, values.get(1).cloned().flatten())?;

        if elements_count == 0 {
            return Ok(AppendState {
                leaves_count,
                elements_count,
                peaks_hashes: Vec::new(),
            });
        }

        let peak_indices = find_peaks(elements_count);
        self.load_append_state_in_tx(tx, &peak_indices).await
    }

    async fn load_append_state_in_tx(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        peak_indices: &[u64],
    ) -> Result<AppendState, MmrError> {
        let leaf_count_key = self.leaf_count_key();
        let elements_count_key = self.elements_count_key();
        let mut keys = Vec::with_capacity(2 + peak_indices.len());
        keys.push(leaf_count_key.clone());
        keys.push(elements_count_key.clone());
        keys.extend(peak_indices.iter().map(|idx| self.node_key(*idx)));

        let values = self.store.get_many_in_tx(tx, &keys).await?;
        let leaves_count =
            Self::extract_counter(&leaf_count_key, values.first().cloned().flatten())?;
        let elements_count =
            Self::extract_counter(&elements_count_key, values.get(1).cloned().flatten())?;

        let mut peaks_hashes = Vec::with_capacity(peak_indices.len());
        for (key, value) in keys[2..].iter().zip(values.into_iter().skip(2)) {
            if let Some(value) = value {
                peaks_hashes.push(value.expect_hash(key)?);
            }
        }

        Ok(AppendState {
            leaves_count,
            elements_count,
            peaks_hashes,
        })
    }
}

struct AppendComputation {
    staged_writes: Vec<(StoreKey, StoreValue)>,
    result: BatchAppendResult,
}

struct AppendState {
    leaves_count: u64,
    elements_count: u64,
    peaks_hashes: Vec<Hash32>,
}
