use alloc::string::String;
use core::fmt;

#[cfg(feature = "std")]
use crate::store::{StoreKey, StoreValue};
#[cfg(feature = "std")]
use crate::types::MmrId;

#[cfg(feature = "std")]
#[derive(Debug)]
pub enum StoreError {
    Internal(String),
    TypeMismatch {
        key: StoreKey,
        expected: &'static str,
        actual: StoreValue,
    },
    PendingBatchAlreadyExists {
        mmr_id: MmrId,
    },
    PendingBatchBaseMismatch {
        mmr_id: MmrId,
        expected_elements_count: u64,
        actual_elements_count: u64,
    },
    #[cfg(feature = "postgres-store")]
    Sqlx(sqlx::Error),
}

#[cfg(feature = "std")]
impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Internal(message) => write!(f, "store internal error: {message}"),
            Self::TypeMismatch {
                key,
                expected,
                actual,
            } => write!(
                f,
                "store type mismatch for key {key:?}: expected {expected}, got {actual:?}"
            ),
            Self::PendingBatchAlreadyExists { mmr_id } => {
                write!(f, "pending batch already exists for mmr_id {mmr_id}")
            }
            Self::PendingBatchBaseMismatch {
                mmr_id,
                expected_elements_count,
                actual_elements_count,
            } => write!(
                f,
                "pending batch base mismatch for mmr_id {mmr_id}: expected elements_count {expected_elements_count}, got {actual_elements_count}"
            ),
            #[cfg(feature = "postgres-store")]
            Self::Sqlx(error) => write!(f, "sqlx error: {error}"),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for StoreError {}

#[cfg(feature = "postgres-store")]
impl From<sqlx::Error> for StoreError {
    fn from(value: sqlx::Error) -> Self {
        Self::Sqlx(value)
    }
}

#[derive(Debug)]
pub enum HasherError {
    InvalidHex {
        value: String,
        source: hex::FromHexError,
    },
    InvalidDecimal {
        value: String,
    },
    InputTooLarge {
        value: String,
        max_bytes: usize,
    },
    InvalidFieldElement {
        value: String,
    },
}

impl fmt::Display for HasherError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidHex { value, source } => {
                write!(f, "invalid hex value `{value}`: {source}")
            }
            Self::InvalidDecimal { value } => write!(f, "invalid decimal value `{value}`"),
            Self::InputTooLarge { value, max_bytes } => {
                write!(f, "input `{value}` exceeds max byte length {max_bytes}")
            }
            Self::InvalidFieldElement { value } => {
                write!(
                    f,
                    "value `{value}` cannot be represented as a Starknet field element"
                )
            }
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for HasherError {}

#[derive(Debug)]
pub enum MmrError {
    #[cfg(feature = "std")]
    Store(StoreError),
    Hasher(HasherError),
    NonEmptyMmr,
    InvalidElementCount,
    InvalidElementIndex,
    InvalidPeaksCount,
    InvalidPeaksCountForElements,
    EmptyBatchAppend,
    PrecommitAlreadyPending,
    NoPendingPrecommit,
    AppendBlockedByPendingPrecommit,
    PrecommitBaseStateChanged,
    NoHashFoundForIndex(u64),
    Overflow,
}

impl fmt::Display for MmrError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            #[cfg(feature = "std")]
            Self::Store(error) => write!(f, "store error: {error}"),
            Self::Hasher(error) => write!(f, "hasher error: {error}"),
            Self::NonEmptyMmr => write!(f, "cannot initialize from peaks for non-empty MMR"),
            Self::InvalidElementCount => write!(f, "invalid element count"),
            Self::InvalidElementIndex => write!(f, "invalid element index"),
            Self::InvalidPeaksCount => write!(f, "invalid peaks count"),
            Self::InvalidPeaksCountForElements => {
                write!(f, "invalid peaks count for the given element count")
            }
            Self::EmptyBatchAppend => write!(f, "cannot batch append an empty list of values"),
            Self::PrecommitAlreadyPending => write!(f, "a precommit batch is already pending"),
            Self::NoPendingPrecommit => write!(f, "no pending precommit batch found"),
            Self::AppendBlockedByPendingPrecommit => write!(
                f,
                "cannot append while a precommit batch is pending; commit or revert first"
            ),
            Self::PrecommitBaseStateChanged => {
                write!(
                    f,
                    "precommit base state changed; retry from current committed state"
                )
            }
            Self::NoHashFoundForIndex(index) => write!(f, "no hash found for index {index}"),
            Self::Overflow => write!(f, "arithmetic overflow"),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for MmrError {}

impl From<HasherError> for MmrError {
    fn from(value: HasherError) -> Self {
        Self::Hasher(value)
    }
}

#[cfg(feature = "std")]
impl From<StoreError> for MmrError {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}
