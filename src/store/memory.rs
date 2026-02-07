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
    fn get(&self, key: &StoreKey) -> Result<Option<StoreValue>, StoreError> {
        let guard = self
            .inner
            .read()
            .map_err(|_| StoreError::Internal("rwlock poisoned (read)".to_string()))?;
        Ok(guard.get(key).cloned())
    }

    fn set(&self, key: StoreKey, value: StoreValue) -> Result<(), StoreError> {
        let mut guard = self
            .inner
            .write()
            .map_err(|_| StoreError::Internal("rwlock poisoned (write)".to_string()))?;
        guard.insert(key, value);
        Ok(())
    }

    fn set_many(&self, entries: Vec<(StoreKey, StoreValue)>) -> Result<(), StoreError> {
        let mut guard = self
            .inner
            .write()
            .map_err(|_| StoreError::Internal("rwlock poisoned (write)".to_string()))?;

        for (key, value) in entries {
            guard.insert(key, value);
        }

        Ok(())
    }

    fn get_many(&self, keys: &[StoreKey]) -> Result<Vec<Option<StoreValue>>, StoreError> {
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

    #[test]
    fn set_many_writes_all_entries() {
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

        store.set_many(entries).unwrap();

        let leaf = store
            .get(&StoreKey::metadata(1, KeyKind::LeafCount))
            .unwrap()
            .unwrap();
        let node = store
            .get(&StoreKey::new(1, KeyKind::NodeHash, 10))
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
