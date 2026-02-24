use crate::store::{StoreKey, StoreValue};
use crate::types::MmrId;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("store internal error: {0}")]
    Internal(String),
    #[error("store type mismatch for key {key:?}: expected {expected}, got {actual:?}")]
    TypeMismatch {
        key: StoreKey,
        expected: &'static str,
        actual: StoreValue,
    },
    #[error("pending batch already exists for mmr_id {mmr_id}")]
    PendingBatchAlreadyExists { mmr_id: MmrId },
    #[error(
        "pending batch base mismatch for mmr_id {mmr_id}: expected elements_count {expected_elements_count}, got {actual_elements_count}"
    )]
    PendingBatchBaseMismatch {
        mmr_id: MmrId,
        expected_elements_count: u64,
        actual_elements_count: u64,
    },
    #[cfg(feature = "postgres-store")]
    #[error("sqlx error: {0}")]
    Sqlx(#[from] sqlx::Error),
}

#[derive(Debug, Error)]
pub enum HasherError {
    #[error("invalid hex value `{value}`: {source}")]
    InvalidHex {
        value: String,
        source: hex::FromHexError,
    },
    #[error("invalid decimal value `{value}`")]
    InvalidDecimal { value: String },
    #[error("input `{value}` exceeds max byte length {max_bytes}")]
    InputTooLarge { value: String, max_bytes: usize },
    #[error("value `{value}` cannot be represented as a Starknet field element")]
    InvalidFieldElement { value: String },
}

#[derive(Debug, Error)]
pub enum MmrError {
    #[error("store error: {0}")]
    Store(#[from] StoreError),
    #[error("hasher error: {0}")]
    Hasher(#[from] HasherError),
    #[error("cannot initialize from peaks for non-empty MMR")]
    NonEmptyMmr,
    #[error("invalid element count")]
    InvalidElementCount,
    #[error("invalid element index")]
    InvalidElementIndex,
    #[error("invalid peaks count")]
    InvalidPeaksCount,
    #[error("invalid peaks count for the given element count")]
    InvalidPeaksCountForElements,
    #[error("cannot batch append an empty list of values")]
    EmptyBatchAppend,
    #[error("a precommit batch is already pending")]
    PrecommitAlreadyPending,
    #[error("no pending precommit batch found")]
    NoPendingPrecommit,
    #[error("cannot append while a precommit batch is pending; commit or revert first")]
    AppendBlockedByPendingPrecommit,
    #[error("precommit base state changed; retry from current committed state")]
    PrecommitBaseStateChanged,
    #[error("no hash found for index {0}")]
    NoHashFoundForIndex(u64),
    #[error("arithmetic overflow")]
    Overflow,
}
