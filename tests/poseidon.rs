mod common;

use common::{hash_from_hex, hash_to_hex};
use mmr::error::HasherError;
use mmr::hasher::{Hasher, PoseidonHasher};
use mmr::types::Hash32;

fn assert_matches_hex(actual: Hash32, expected_hex: &str) {
    let expected = hash_from_hex(expected_hex).unwrap();
    assert_eq!(
        actual,
        expected,
        "hash mismatch: actual={} expected={}",
        hash_to_hex(&actual),
        expected_hex
    );
}

#[test]
fn should_compute_a_hash_pair() {
    let hasher = PoseidonHasher::new();

    let a =
        hash_from_hex("0x6109f1949f6a7555eccf4e15ce1f10fbd78091dfe715cc2e0c5a244d9d17761").unwrap();
    let b = hash_from_hex("0x0194791558611599fe4ae0fcfa48f095659c90db18e54de86f2d2f547f7369bf")
        .unwrap();

    let result = hasher.hash_pair(&a, &b).unwrap();
    assert_matches_hex(
        result,
        "0x7b8180db85fa1e0b5041f38f57926743905702c498576991f04998b5d9476b4",
    );
}

#[test]
fn should_compute_hash_count_and_bag() {
    let hasher = PoseidonHasher::new();
    let bag = hash_from_hex("0x0194791558611599fe4ae0fcfa48f095659c90db18e54de86f2d2f547f7369bf")
        .unwrap();
    let result = hasher.hash_count_and_bag(10, &bag).unwrap();

    assert_matches_hex(
        result,
        "0x020694b6bc7e4bdc420bade9a4126e5ac6698958b011d6c486ebd269d84d426a",
    );
}

#[test]
fn check_genesis_hash() {
    let hasher = PoseidonHasher::new();
    let genesis = hasher.genesis_hash().unwrap();

    assert_matches_hex(
        genesis,
        "0x2241b3b7f1c4b9cf63e670785891de91f7237b1388f6635c1898ae397ad32dd",
    );
}

#[test]
fn should_error_for_non_field_hash_input() {
    let hasher = PoseidonHasher::new();
    let invalid = [0xffu8; 32];
    let valid = [0u8; 32];

    let err = hasher.hash_pair(&invalid, &valid).unwrap_err();
    assert!(matches!(err, HasherError::InvalidFieldElement { .. }));
}
