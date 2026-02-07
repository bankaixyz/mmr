use mmr::hasher::{Hasher, KeccakHasher};
use mmr::types::{Hash32, hash_from_hex, hash_to_hex};

#[test]
fn should_compute_a_hash_pair() {
    let hasher = KeccakHasher::new();

    let a = hash_from_hex("0xa4b1d5793b631de611c922ea3ec938b359b3a49e687316d9a79c27be8ce84590")
        .unwrap();
    let b = hash_from_hex("0xa4b1d5793b631de611c922ea3ec938b359b3a49e687316d9a79c27be8ce84590")
        .unwrap();

    let result = hasher.hash_pair(&a, &b).unwrap();

    assert_eq!(
        hash_to_hex(&result),
        "0xa960dc82e45665d5b1340ee84f6c3f27abaac8235a1a3b7e954001c1bc682268"
    );
}

#[test]
fn should_compute_hash_count_and_bag() {
    let hasher = KeccakHasher::new();
    let bag = hash_from_hex("0xead5d1fa438c36f2c341756e97b2327214f21fee27aaeae4c91238c2c76374f5")
        .unwrap();

    let result = hasher.hash_count_and_bag(10, &bag).unwrap();

    assert_eq!(
        hash_to_hex(&result),
        "0x70c01463d822d2205868c5a46eefc55658828015b83e4553c8462d2c6711d0e0"
    );
}

#[test]
fn hash_pair_is_deterministic_for_typed_inputs() {
    let hasher = KeccakHasher::new();
    let a: Hash32 = [1u8; 32];
    let b: Hash32 = [2u8; 32];
    let first = hasher.hash_pair(&a, &b).unwrap();
    let second = hasher.hash_pair(&a, &b).unwrap();
    assert_eq!(first, second);
}
