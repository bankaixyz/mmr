use crate::error::HasherError;

pub type Hash32 = [u8; 32];
pub type MmrId = u32;
pub type ElementIndex = u64;
pub type ElementsCount = u64;
pub type LeavesCount = u64;

pub const ZERO_HASH: Hash32 = [0u8; 32];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proof {
    pub element_index: ElementIndex,
    pub element_hash: Hash32,
    pub siblings_hashes: Vec<Hash32>,
    pub peaks_hashes: Vec<Hash32>,
    pub elements_count: ElementsCount,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppendResult {
    pub leaves_count: LeavesCount,
    pub elements_count: ElementsCount,
    pub element_index: ElementIndex,
    pub root_hash: Hash32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchAppendResult {
    pub appended_count: u64,
    pub first_element_index: ElementIndex,
    pub last_element_index: ElementIndex,
    pub leaves_count: LeavesCount,
    pub elements_count: ElementsCount,
    pub root_hash: Hash32,
}

pub fn hash_to_hex(hash: &Hash32) -> String {
    format!("0x{}", hex::encode(hash))
}

pub fn hash_from_hex(value: &str) -> Result<Hash32, HasherError> {
    let raw = value.strip_prefix("0x").unwrap_or(value);

    if raw.is_empty() {
        return Ok([0u8; 32]);
    }

    let normalized = if raw.len() % 2 == 1 {
        format!("0{raw}")
    } else {
        raw.to_string()
    };

    let bytes = hex::decode(&normalized).map_err(|source| HasherError::InvalidHex {
        value: value.to_string(),
        source,
    })?;

    if bytes.len() > 32 {
        return Err(HasherError::InputTooLarge {
            value: value.to_string(),
            max_bytes: 32,
        });
    }

    let mut out = [0u8; 32];
    let start = 32 - bytes.len();
    out[start..].copy_from_slice(&bytes);
    Ok(out)
}
