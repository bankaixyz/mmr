use crate::types::{Hash32, MmrId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum KeyKind {
    LeafCount = 0,
    ElementsCount = 1,
    RootHash = 2,
    NodeHash = 3,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StoreKey {
    pub mmr_id: MmrId,
    pub kind: KeyKind,
    pub index: u64,
}

impl StoreKey {
    pub const fn new(mmr_id: MmrId, kind: KeyKind, index: u64) -> Self {
        Self {
            mmr_id,
            kind,
            index,
        }
    }

    pub const fn metadata(mmr_id: MmrId, kind: KeyKind) -> Self {
        Self::new(mmr_id, kind, 0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreValue {
    U64(u64),
    Hash(Hash32),
}
