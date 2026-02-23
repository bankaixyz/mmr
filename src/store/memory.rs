use std::collections::HashMap;
use std::sync::RwLock;

use crate::error::StoreError;
use crate::types::MmrId;

use super::{PendingBatch, Store, StoreKey, StoreValue};

#[derive(Debug, Default)]
pub struct InMemoryStore {
    inner: RwLock<HashMap<StoreKey, StoreValue>>,
    pending_batches: RwLock<HashMap<MmrId, PendingBatch>>,
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

    async fn create_pending_batch(
        &self,
        mmr_id: MmrId,
        batch: PendingBatch,
    ) -> Result<(), StoreError> {
        let mut guard = self
            .pending_batches
            .write()
            .map_err(|_| StoreError::Internal("rwlock poisoned (write)".to_string()))?;
        if guard.contains_key(&mmr_id) {
            return Err(StoreError::PendingBatchAlreadyExists { mmr_id });
        }
        guard.insert(mmr_id, batch);
        Ok(())
    }

    async fn get_pending_batch(&self, mmr_id: MmrId) -> Result<Option<PendingBatch>, StoreError> {
        let guard = self
            .pending_batches
            .read()
            .map_err(|_| StoreError::Internal("rwlock poisoned (read)".to_string()))?;
        Ok(guard.get(&mmr_id).cloned())
    }

    async fn delete_pending_batch(&self, mmr_id: MmrId) -> Result<(), StoreError> {
        let mut guard = self
            .pending_batches
            .write()
            .map_err(|_| StoreError::Internal("rwlock poisoned (write)".to_string()))?;
        guard.remove(&mmr_id);
        Ok(())
    }

    async fn has_pending_batch(&self, mmr_id: MmrId) -> Result<bool, StoreError> {
        let guard = self
            .pending_batches
            .read()
            .map_err(|_| StoreError::Internal("rwlock poisoned (read)".to_string()))?;
        Ok(guard.contains_key(&mmr_id))
    }
}

#[cfg(test)]
mod tests {
    use super::{InMemoryStore, Store, StoreKey, StoreValue};
    use crate::store::KeyKind;
    use crate::store::PendingBatch;
    use crate::types::BatchAppendResult;

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

    #[tokio::test]
    async fn pending_batch_roundtrip_and_uniqueness_are_enforced() {
        let store = InMemoryStore::new();
        let mmr_id = 9;
        let batch = PendingBatch {
            staged_writes: vec![
                (
                    StoreKey::new(mmr_id, KeyKind::NodeHash, 1),
                    StoreValue::Hash([1u8; 32]),
                ),
                (
                    StoreKey::metadata(mmr_id, KeyKind::ElementsCount),
                    StoreValue::U64(1),
                ),
            ],
            result: BatchAppendResult {
                appended_count: 1,
                first_element_index: 1,
                last_element_index: 1,
                leaves_count: 1,
                elements_count: 1,
                root_hash: [2u8; 32],
                peaks_hashes: vec![[1u8; 32]],
            },
        };

        assert!(!store.has_pending_batch(mmr_id).await.unwrap());
        store
            .create_pending_batch(mmr_id, batch.clone())
            .await
            .unwrap();
        assert!(store.has_pending_batch(mmr_id).await.unwrap());
        assert_eq!(store.get_pending_batch(mmr_id).await.unwrap(), Some(batch));
        assert!(matches!(
            store.create_pending_batch(
                mmr_id,
                PendingBatch {
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
            Err(crate::error::StoreError::PendingBatchAlreadyExists { .. })
        ));

        store.delete_pending_batch(mmr_id).await.unwrap();
        assert!(!store.has_pending_batch(mmr_id).await.unwrap());
        assert!(store.get_pending_batch(mmr_id).await.unwrap().is_none());
    }
}
