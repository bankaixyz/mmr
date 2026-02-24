mod key;
mod memory;
#[cfg(feature = "postgres-store")]
mod postgres;

use std::sync::Arc;

use crate::error::StoreError;

pub use key::{KeyKind, StoreKey, StoreValue};
pub use memory::InMemoryStore;
#[cfg(feature = "postgres-store")]
pub use postgres::{PostgresStore, PostgresStoreOptions};

    pub base_leaves_count: u64,
    pub base_elements_count: u64,
#[allow(async_fn_in_trait)]
pub trait Store: Send + Sync {
    async fn get(&self, key: &StoreKey) -> Result<Option<StoreValue>, StoreError>;
    async fn set(&self, key: StoreKey, value: StoreValue) -> Result<(), StoreError>;
    async fn set_many(&self, entries: Vec<(StoreKey, StoreValue)>) -> Result<(), StoreError> {
        for (key, value) in entries {
            self.set(key, value).await?;
        }

        Ok(())
    }
    async fn get_many(&self, keys: &[StoreKey]) -> Result<Vec<Option<StoreValue>>, StoreError>;
    async fn commit_pending_batch(
        &self,
        _mmr_id: MmrId,
    ) -> Result<Option<PendingBatch>, StoreError> {
        Err(StoreError::Internal(
            "pending batches are not supported by this store backend".to_string(),
        ))
    }
    async fn remove_pending_batch(&self, _mmr_id: MmrId) -> Result<bool, StoreError> {
        Err(StoreError::Internal(
            "pending batches are not supported by this store backend".to_string(),
        ))
    }
}

impl<T: Store + ?Sized> Store for Arc<T> {
    async fn get(&self, key: &StoreKey) -> Result<Option<StoreValue>, StoreError> {
        (**self).get(key).await
    }

    async fn set(&self, key: StoreKey, value: StoreValue) -> Result<(), StoreError> {
        (**self).set(key, value).await
    }

    async fn set_many(&self, entries: Vec<(StoreKey, StoreValue)>) -> Result<(), StoreError> {
        (**self).set_many(entries).await
    }

    async fn get_many(&self, keys: &[StoreKey]) -> Result<Vec<Option<StoreValue>>, StoreError> {
        (**self).get_many(keys).await
    }

    async fn commit_pending_batch(
        &self,
        mmr_id: MmrId,
    ) -> Result<Option<PendingBatch>, StoreError> {
        (**self).commit_pending_batch(mmr_id).await
    }

    async fn remove_pending_batch(&self, mmr_id: MmrId) -> Result<bool, StoreError> {
        (**self).remove_pending_batch(mmr_id).await
    }
}

impl StoreValue {
    pub fn expect_u64(self, key: &StoreKey) -> Result<u64, StoreError> {
        match self {
            StoreValue::U64(value) => Ok(value),
            other => Err(StoreError::TypeMismatch {
                key: key.clone(),
                expected: "u64",
                actual: other,
            }),
        }
    }

    pub fn expect_hash(self, key: &StoreKey) -> Result<crate::types::Hash32, StoreError> {
        match self {
            StoreValue::Hash(value) => Ok(value),
            other => Err(StoreError::TypeMismatch {
                key: key.clone(),
                expected: "hash32",
                actual: other,
            }),
        }
    }
}
