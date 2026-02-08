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
    pub peaks_hashes: Vec<Hash32>,
}
