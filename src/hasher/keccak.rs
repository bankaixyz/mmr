use tiny_keccak::{Hasher as TinyHasher, Keccak};

use crate::error::HasherError;
use crate::types::Hash32;

use super::Hasher;

#[derive(Debug, Default, Clone, Copy)]
pub struct KeccakHasher;

impl KeccakHasher {
    pub fn new() -> Self {
        Self
    }
}

impl Hasher for KeccakHasher {
    fn hash_pair(&self, left: &Hash32, right: &Hash32) -> Result<Hash32, HasherError> {
        let mut bytes = [0u8; 64];
        bytes[..32].copy_from_slice(left);
        bytes[32..].copy_from_slice(right);

        let mut keccak = Keccak::v256();
        keccak.update(&bytes);
        Ok(finalize_keccak(keccak))
    }

    fn hash_count_and_bag(&self, elements_count: u64, bag: &Hash32) -> Result<Hash32, HasherError> {
        let mut count_hash = [0u8; 32];
        count_hash[24..].copy_from_slice(&elements_count.to_be_bytes());
        self.hash_pair(&count_hash, bag)
    }
}

fn finalize_keccak(keccak: Keccak) -> Hash32 {
    let mut output = [0u8; 32];
    keccak.finalize(&mut output);
    output
}
