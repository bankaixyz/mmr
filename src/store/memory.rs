use std::collections::HashMap;
use std::sync::RwLock;

use crate::error::StoreError;
use crate::types::MmrId;

use super::{KeyKind, PendingBatch, Store, StoreKey, StoreValue};

#[derive(Debug, Default)]
struct MemoryState {
    entries: HashMap<StoreKey, StoreValue>,
    pending_batches: HashMap<MmrId, PendingBatch>,
}

#[derive(Debug, Default)]
pub struct InMemoryStore {
    inner: RwLock<MemoryState>,
}

impl InMemoryStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn current_elements_count(state: &MemoryState, mmr_id: MmrId) -> Result<u64, StoreError> {
        let key = StoreKey::metadata(mmr_id, KeyKind::ElementsCount);
        match state.entries.get(&key) {
            Some(StoreValue::U64(value)) => Ok(*value),
            Some(other) => Err(StoreError::TypeMismatch {
                key,
                expected: "u64",
                actual: other.clone(),
            }),
            None => Ok(0),
        }
    }

    fn expected_elements_count(batch: &PendingBatch) -> Result<u64, StoreError> {
        batch
            .result
            .first_element_index
            .checked_sub(1)
            .ok_or_else(|| {
                StoreError::Internal(
                    "pending batch has invalid first_element_index 0 while deriving expected base"
                        .to_string(),
                )
            })
    }
}

impl Store for InMemoryStore {
    async fn get(&self, key: &StoreKey) -> Result<Option<StoreValue>, StoreError> {
        let guard = self
            .inner
            .read()
            .map_err(|_| StoreError::Internal("rwlock poisoned (read)".to_string()))?;
        Ok(guard.entries.get(key).cloned())
    }

    async fn set(&self, key: StoreKey, value: StoreValue) -> Result<(), StoreError> {
        let mut guard = self
            .inner
            .write()
            .map_err(|_| StoreError::Internal("rwlock poisoned (write)".to_string()))?;
        guard.entries.insert(key, value);
        Ok(())
    }

    async fn set_many(&self, entries: Vec<(StoreKey, StoreValue)>) -> Result<(), StoreError> {
        let mut guard = self
            .inner
            .write()
            .map_err(|_| StoreError::Internal("rwlock poisoned (write)".to_string()))?;

        for (key, value) in entries {
            guard.entries.insert(key, value);
        }

        Ok(())
    }

    async fn get_many(&self, keys: &[StoreKey]) -> Result<Vec<Option<StoreValue>>, StoreError> {
        let guard = self
            .inner
            .read()
            .map_err(|_| StoreError::Internal("rwlock poisoned (read)".to_string()))?;
        Ok(keys
            .iter()
            .map(|key| guard.entries.get(key).cloned())
            .collect())
    }

    async fn create_pending_batch(
        &self,
        mmr_id: MmrId,
        batch: PendingBatch,
    ) -> Result<(), StoreError> {
        let mut guard = self
            .inner
            .write()
            .map_err(|_| StoreError::Internal("rwlock poisoned (write)".to_string()))?;

        let expected_elements_count = Self::expected_elements_count(&batch)?;
        let actual_elements_count = Self::current_elements_count(&guard, mmr_id)?;
        if expected_elements_count != actual_elements_count {
            return Err(StoreError::PendingBatchBaseMismatch {
                mmr_id,
                expected_elements_count,
                actual_elements_count,
            });
        }

        guard.pending_batches.insert(mmr_id, batch);
        Ok(())
    }

    async fn has_pending_batch(&self, mmr_id: MmrId) -> Result<bool, StoreError> {
        let guard = self
            .inner
            .read()
            .map_err(|_| StoreError::Internal("rwlock poisoned (read)".to_string()))?;
        Ok(guard.pending_batches.contains_key(&mmr_id))
    }

    async fn commit_pending_batch(
        &self,
        mmr_id: MmrId,
    ) -> Result<Option<crate::types::BatchAppendResult>, StoreError> {
        let mut guard = self
            .inner
            .write()
            .map_err(|_| StoreError::Internal("rwlock poisoned (write)".to_string()))?;

        let Some(batch) = guard.pending_batches.get(&mmr_id).cloned() else {
            return Ok(None);
        };

        let expected_elements_count = Self::expected_elements_count(&batch)?;
        let actual_elements_count = Self::current_elements_count(&guard, mmr_id)?;
        if expected_elements_count != actual_elements_count {
            return Err(StoreError::PendingBatchBaseMismatch {
                mmr_id,
                expected_elements_count,
                actual_elements_count,
            });
        }

        for (key, value) in &batch.staged_writes {
            guard.entries.insert(key.clone(), value.clone());
        }
        guard.pending_batches.remove(&mmr_id);

        Ok(Some(batch.result))
    }

    async fn delete_pending_batch_if_exists(&self, mmr_id: MmrId) -> Result<bool, StoreError> {
        let mut guard = self
            .inner
            .write()
            .map_err(|_| StoreError::Internal("rwlock poisoned (write)".to_string()))?;
        Ok(guard.pending_batches.remove(&mmr_id).is_some())
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
