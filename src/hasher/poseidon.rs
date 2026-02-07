use std::str::FromStr;

use starknet::core::types::FieldElement;
use starknet_crypto::{poseidon_hash, poseidon_hash_single};

use crate::error::HasherError;
use crate::types::{Hash32, ZERO_HASH};

use super::Hasher;

#[derive(Debug, Default, Clone, Copy)]
pub struct PoseidonHasher;

impl PoseidonHasher {
    pub fn new() -> Self {
        Self
    }

    pub fn genesis_hash(&self) -> Result<Hash32, HasherError> {
        let mut seed = [0u8; 32];
        let seed_bytes = b"brave new world";
        let start = seed.len() - seed_bytes.len();
        seed[start..].copy_from_slice(seed_bytes);
        let seed_fe = hash32_to_field_element(&seed)?;
        Ok(field_element_to_hash32(&poseidon_hash_single(seed_fe)))
    }
}

impl Hasher for PoseidonHasher {
    fn hash_pair(&self, left: &Hash32, right: &Hash32) -> Result<Hash32, HasherError> {
        let left_fe = hash32_to_field_element(left)?;
        let right_fe = hash32_to_field_element(right)?;
        let out = poseidon_hash(left_fe, right_fe);
        Ok(field_element_to_hash32(&out))
    }

    fn hash_count_and_bag(&self, elements_count: u64, bag: &Hash32) -> Result<Hash32, HasherError> {
        let count_fe = FieldElement::from(elements_count);
        let bag_fe = hash32_to_field_element(bag)?;
        let out = poseidon_hash(count_fe, bag_fe);
        Ok(field_element_to_hash32(&out))
    }
}

fn hash32_to_field_element(value: &Hash32) -> Result<FieldElement, HasherError> {
    if value == &ZERO_HASH {
        return Ok(FieldElement::ZERO);
    }

    let hex_value = format!("0x{}", hex::encode(value));
    FieldElement::from_str(&hex_value)
        .map_err(|_| HasherError::InvalidFieldElement { value: hex_value })
}

fn field_element_to_hash32(value: &FieldElement) -> Hash32 {
    let mut out = [0u8; 32];
    out.copy_from_slice(&value.to_bytes_be());
    out
}
