use std::collections::HashMap;
use std::sync::RwLock;

use crate::error::StoreError;
use crate::types::{BatchAppendResult, MmrId};

use super::{PendingBatch, Store, StoreKey, StoreValue};

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

    async fn create_pending_batch(
        &self,
        _mmr_id: MmrId,
        _batch: PendingBatch,
    ) -> Result<(), StoreError> {
        Err(StoreError::Internal(
            "pending batches are not supported by this store backend".to_string(),
        ))
    }

    async fn get_pending_batch(&self, _mmr_id: MmrId) -> Result<Option<PendingBatch>, StoreError> {
        Err(StoreError::Internal(
            "pending batches are not supported by this store backend".to_string(),
        ))
    }

    async fn commit_pending_batch(
        &self,
        _mmr_id: MmrId,
    ) -> Result<Option<BatchAppendResult>, StoreError> {
        Err(StoreError::Internal(
            "pending batches are not supported by this store backend".to_string(),
        ))
    }

    async fn delete_pending_batch(&self, _mmr_id: MmrId) -> Result<(), StoreError> {
        Err(StoreError::Internal(
            "pending batches are not supported by this store backend".to_string(),
        ))
    }

    async fn delete_pending_batch_if_exists(&self, _mmr_id: MmrId) -> Result<bool, StoreError> {
        Err(StoreError::Internal(
            "pending batches are not supported by this store backend".to_string(),
        ))
    }

    async fn has_pending_batch(&self, _mmr_id: MmrId) -> Result<bool, StoreError> {
        Ok(false)
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
