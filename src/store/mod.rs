mod key;
mod memory;
#[cfg(feature = "postgres-store")]
mod postgres;

use crate::error::StoreError;

pub use key::{KeyKind, StoreKey, StoreValue};
pub use memory::InMemoryStore;
#[cfg(feature = "postgres-store")]
pub use postgres::{PostgresStore, PostgresStoreOptions};

pub trait Store: Send + Sync {
    fn get(&self, key: &StoreKey) -> Result<Option<StoreValue>, StoreError>;
    fn set(&self, key: StoreKey, value: StoreValue) -> Result<(), StoreError>;
    fn set_many(&self, entries: Vec<(StoreKey, StoreValue)>) -> Result<(), StoreError> {
        for (key, value) in entries {
            self.set(key, value)?;
        }

        Ok(())
    }
    fn get_many(&self, keys: &[StoreKey]) -> Result<Vec<Option<StoreValue>>, StoreError>;
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
