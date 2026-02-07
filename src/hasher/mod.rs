mod keccak;
mod poseidon;

use crate::error::HasherError;
use crate::types::Hash32;

pub use keccak::KeccakHasher;
pub use poseidon::PoseidonHasher;

pub trait Hasher: Send + Sync {
    fn hash_pair(&self, left: &Hash32, right: &Hash32) -> Result<Hash32, HasherError>;
    fn hash_count_and_bag(&self, elements_count: u64, bag: &Hash32) -> Result<Hash32, HasherError>;
}
