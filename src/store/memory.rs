use std::collections::HashMap;
use std::sync::RwLock;

use crate::error::StoreError;

use super::{Store, StoreKey, StoreValue};

#[derive(Debug, Default)]
pub struct InMemoryStore {
    inner: RwLock<HashMap<StoreKey, StoreValue>>,
}

impl InMemoryStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Store for InMemoryStore {
    async fn get(&self, key: &StoreKey) -> Result<Option<StoreValue>, StoreError> {
        let guard = self
            .inner
            .read()
            .map_err(|_| StoreError::Internal("rwlock poisoned (read)".to_string()))?;
        Ok(guard.get(key).cloned())
    }

    async fn set(&self, key: StoreKey, value: StoreValue) -> Result<(), StoreError> {
        let mut guard = self
            .inner
            .write()
            .map_err(|_| StoreError::Internal("rwlock poisoned (write)".to_string()))?;
        guard.insert(key, value);
        Ok(())
    }

    async fn set_many(&self, entries: Vec<(StoreKey, StoreValue)>) -> Result<(), StoreError> {
        let mut guard = self
            .inner
            .write()
            .map_err(|_| StoreError::Internal("rwlock poisoned (write)".to_string()))?;

        for (key, value) in entries {
            guard.insert(key, value);
        }

        Ok(())
    }

    async fn get_many(&self, keys: &[StoreKey]) -> Result<Vec<Option<StoreValue>>, StoreError> {
        let guard = self
            .inner
            .read()
            .map_err(|_| StoreError::Internal("rwlock poisoned (read)".to_string()))?;
        Ok(keys.iter().map(|key| guard.get(key).cloned()).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::{InMemoryStore, Store, StoreKey, StoreValue};
    use crate::store::KeyKind;

    #[tokio::test]
    async fn set_many_writes_all_entries() {
        let store = InMemoryStore::new();
        let entries = vec![
            (
                StoreKey::metadata(1, KeyKind::LeafCount),
                StoreValue::U64(7),
            ),
            (
                StoreKey::new(1, KeyKind::NodeHash, 10),
                StoreValue::Hash([3u8; 32]),

    async fn commit_pending_batch(
        &self,
        mmr_id: MmrId,
    ) -> Result<Option<PendingBatch>, StoreError> {
        let pending = self
            .pending_batches
            .read()
            .map_err(|_| StoreError::Internal("rwlock poisoned (read)".to_string()))?
            .get(&mmr_id)
            .cloned();

        let Some(batch) = pending else {
            return Ok(None);
        };

        let leaf_key = StoreKey::metadata(mmr_id, super::KeyKind::LeafCount);
        let elements_key = StoreKey::metadata(mmr_id, super::KeyKind::ElementsCount);
        let mut inner_guard = self
            .inner
            .write()
            .map_err(|_| StoreError::Internal("rwlock poisoned (write)".to_string()))?;
        let current_leaves = match inner_guard.get(&leaf_key) {
            Some(StoreValue::U64(value)) => *value,
            Some(other) => {
                return Err(StoreError::TypeMismatch {
                    key: leaf_key,
                    expected: "u64",
                    actual: other.clone(),
                });
            }
            None => 0,
        };
        let current_elements = match inner_guard.get(&elements_key) {
            Some(StoreValue::U64(value)) => *value,
            Some(other) => {
                return Err(StoreError::TypeMismatch {
                    key: elements_key,
                    expected: "u64",
                    actual: other.clone(),
                });
            }
            None => 0,
        };
        if current_leaves != batch.base_leaves_count
            || current_elements != batch.base_elements_count
        {
            return Err(StoreError::PendingBatchBaseStateChanged { mmr_id });
        }

        for (key, value) in &batch.staged_writes {
            inner_guard.insert(key.clone(), value.clone());
        }

        self.pending_batches
            .write()
            .map_err(|_| StoreError::Internal("rwlock poisoned (write)".to_string()))?
            .remove(&mmr_id);

        Ok(Some(batch))
    }

    async fn remove_pending_batch(&self, mmr_id: MmrId) -> Result<bool, StoreError> {
        let mut guard = self
            .pending_batches
            .write()
            .map_err(|_| StoreError::Internal("rwlock poisoned (write)".to_string()))?;
        Ok(guard.remove(&mmr_id).is_some())
    }
            ),
        ];

        store.set_many(entries).await.unwrap();

        let leaf = store
            .get(&StoreKey::metadata(1, KeyKind::LeafCount))
            .await
            .unwrap()
            .unwrap();
        let node = store
            .get(&StoreKey::new(1, KeyKind::NodeHash, 10))
            .await
            .unwrap()
            .unwrap();

        assert_eq!(
            leaf.expect_u64(&StoreKey::metadata(1, KeyKind::LeafCount))
                .unwrap(),
            7
        );
        assert_eq!(
            node.expect_hash(&StoreKey::new(1, KeyKind::NodeHash, 10))
                .unwrap(),
            [3u8; 32]
        );
    }
}
            base_leaves_count: 0,
            base_elements_count: 0,
            store
                .create_pending_batch(
                    mmr_id,
                    PendingBatch {
                        base_leaves_count: 0,
                        base_elements_count: 0,
                        staged_writes: Vec::new(),
                        result: BatchAppendResult {
                            appended_count: 0,
                            first_element_index: 0,
                            last_element_index: 0,
                            leaves_count: 0,
                            elements_count: 0,
                            root_hash: [0u8; 32],
                            peaks_hashes: Vec::new(),
                        },
                    }
                )
                .await,
