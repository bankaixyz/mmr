#![cfg(feature = "postgres-store")]

use std::sync::Arc;

mod common;

use common::hash_from_hex;
use common::pg::{PostgresFixture, next_mmr_id};
use mmr::error::MmrError;
use mmr::hasher::{Hasher, KeccakHasher, PoseidonHasher};
use mmr::types::Hash32;
use mmr::{KeyKind, Mmr, Store, StoreKey, StoreValue, calculate_root_hash, verify_proof_stateless};

const LEAVES: [&str; 5] = ["1", "2", "3", "4", "5"];

fn lv(value: &str) -> mmr::Hash32 {
    if value.starts_with("0x") || value.starts_with("0X") {
        return hash_from_hex(value).unwrap();
    }

    let parsed = value.parse::<u128>().unwrap();
    let mut out = [0u8; 32];
    out[16..].copy_from_slice(&parsed.to_be_bytes());
    out
}

fn root_from_peaks(hasher: &dyn Hasher, peaks_hashes: &[Hash32], elements_count: u64) -> Hash32 {
    calculate_root_hash(hasher, elements_count, peaks_hashes).unwrap()
}

#[tokio::test]
async fn postgres_should_compute_parent_tree_for_keccak_hasher() {
    let fixture = PostgresFixture::start().await;
    let hasher = Arc::new(KeccakHasher::new());
    let mut mmr = Mmr::new(fixture.store.clone(), hasher.clone(), Some(next_mmr_id())).unwrap();

    let mut appends = Vec::new();
    for leaf in LEAVES {
        appends.push(mmr.append(lv(leaf)).await.unwrap());
    }

    let last_leaf_element_index = appends.last().unwrap().element_index;
    let appended_leaf = lv("6");

    let node3 = hasher.hash_pair(&lv("1"), &lv("2")).unwrap();
    let node6 = hasher.hash_pair(&lv("3"), &lv("4")).unwrap();
    let node7 = hasher.hash_pair(&node3, &node6).unwrap();
    let node10 = hasher.hash_pair(&lv("5"), &appended_leaf).unwrap();
    let bag = hasher.hash_pair(&node7, &node10).unwrap();
    let root = hasher.hash_count_and_bag(10, &bag).unwrap();

    let append = mmr.append(appended_leaf).await.unwrap();

    assert_eq!(append.element_index, 9);
    assert_eq!(append.leaves_count, 6);
    assert_eq!(append.elements_count, 10);
    assert_eq!(append.root_hash, root);
    assert_eq!(mmr.get_peaks(None).await.unwrap(), vec![node7, node10]);
    assert_eq!(mmr.bag_the_peaks(None).await.unwrap(), bag);

    let proof = mmr.get_proof(last_leaf_element_index, None).await.unwrap();
    assert!(mmr.verify_proof(&proof, lv("5"), None).await.unwrap());
}

#[tokio::test]
async fn postgres_batch_append_matches_repeated_append_for_identical_values() {
    let fixture = PostgresFixture::start().await;
    let hasher = Arc::new(KeccakHasher::new());
    let leaves = ["1", "2", "3", "4", "5", "6", "7", "8"];

    let mut single = Mmr::new(fixture.store.clone(), hasher.clone(), Some(next_mmr_id())).unwrap();
    let mut single_appends = Vec::new();
    for leaf in leaves {
        single_appends.push(single.append(lv(leaf)).await.unwrap());
    }

    let mut batched = Mmr::new(fixture.store.clone(), hasher.clone(), Some(next_mmr_id())).unwrap();
    let batch_values = leaves.iter().map(|leaf| lv(leaf)).collect::<Vec<_>>();
    let batch_result = batched.batch_append(&batch_values).await.unwrap();

    assert_eq!(batch_result.appended_count, leaves.len() as u64);
    assert_eq!(
        batch_result.first_element_index,
        single_appends.first().unwrap().element_index
    );
    assert_eq!(
        batch_result.last_element_index,
        single_appends.last().unwrap().element_index
    );
    assert_eq!(
        batch_result.leaves_count,
        single.get_leaves_count().await.unwrap()
    );
    assert_eq!(
        batch_result.elements_count,
        single.get_elements_count().await.unwrap()
    );
    assert_eq!(
        batch_result.root_hash,
        single.get_root_hash().await.unwrap().unwrap()
    );
    assert_eq!(
        batch_result.peaks_hashes,
        batched
            .get_peaks(Some(batch_result.elements_count))
            .await
            .unwrap()
    );
    assert_eq!(
        batch_result.root_hash,
        root_from_peaks(
            hasher.as_ref(),
            &batch_result.peaks_hashes,
            batch_result.elements_count,
        )
    );

    for (leaf, append) in leaves.iter().zip(single_appends.iter()) {
        let proof_single = single.get_proof(append.element_index, None).await.unwrap();
        let proof_batched = batched.get_proof(append.element_index, None).await.unwrap();
        assert_eq!(proof_single, proof_batched);
        assert!(
            single
                .verify_proof(&proof_single, lv(leaf), None)
                .await
                .unwrap()
        );
        assert!(
            batched
                .verify_proof(&proof_batched, lv(leaf), None)
                .await
                .unwrap()
        );
    }
}

#[tokio::test]
async fn postgres_append_matches_batch_append_single_value() {
    let fixture = PostgresFixture::start().await;
    let hasher = Arc::new(KeccakHasher::new());
    let prefill = ["1", "2", "3", "4", "5"];

    let mut append_mmr =
        Mmr::new(fixture.store.clone(), hasher.clone(), Some(next_mmr_id())).unwrap();
    let mut batch_mmr =
        Mmr::new(fixture.store.clone(), hasher.clone(), Some(next_mmr_id())).unwrap();

    for leaf in prefill {
        append_mmr.append(lv(leaf)).await.unwrap();
        batch_mmr.append(lv(leaf)).await.unwrap();
    }

    let append_result = append_mmr.append(lv("6")).await.unwrap();
    let batch_result = batch_mmr.batch_append(&[lv("6")]).await.unwrap();

    assert_eq!(batch_result.appended_count, 1);
    assert_eq!(
        batch_result.first_element_index,
        append_result.element_index
    );
    assert_eq!(batch_result.last_element_index, append_result.element_index);
    assert_eq!(batch_result.leaves_count, append_result.leaves_count);
    assert_eq!(batch_result.elements_count, append_result.elements_count);
    assert_eq!(batch_result.root_hash, append_result.root_hash);
}

#[tokio::test]
async fn postgres_batch_append_result_peaks_and_root_are_consistent_for_poseidon() {
    let fixture = PostgresFixture::start().await;
    let hasher = Arc::new(PoseidonHasher::new());
    let mut mmr = Mmr::new(fixture.store, hasher.clone(), Some(next_mmr_id())).unwrap();

    let result = mmr
        .batch_append(&[lv("1"), lv("2"), lv("3"), lv("4"), lv("5")])
        .await
        .unwrap();

    assert!(!result.peaks_hashes.is_empty());
    assert_eq!(
        result.peaks_hashes,
        mmr.get_peaks(Some(result.elements_count)).await.unwrap()
    );
    assert_eq!(
        result.root_hash,
        root_from_peaks(hasher.as_ref(), &result.peaks_hashes, result.elements_count)
    );
}

#[tokio::test]
async fn postgres_batch_append_rejects_empty_values() {
    let fixture = PostgresFixture::start().await;
    let mut mmr = Mmr::new(
        fixture.store,
        Arc::new(KeccakHasher::new()),
        Some(next_mmr_id()),
    )
    .unwrap();

    assert!(matches!(
        mmr.batch_append(&[]).await,
        Err(MmrError::EmptyBatchAppend)
    ));
}

#[tokio::test]
async fn postgres_should_create_from_peaks_and_match_followup_appends() {
    let fixture = PostgresFixture::start().await;
    let hasher = Arc::new(KeccakHasher::new());

    let mut original =
        Mmr::new(fixture.store.clone(), hasher.clone(), Some(next_mmr_id())).unwrap();
    for leaf in LEAVES {
        original.append(lv(leaf)).await.unwrap();
    }

    let original_elements_count = original.get_elements_count().await.unwrap();
    let original_peaks = original.get_peaks(None).await.unwrap();

    let mut from_peaks = Mmr::create_from_peaks(
        fixture.store.clone(),
        hasher.clone(),
        Some(next_mmr_id()),
        original_peaks.clone(),
        original_elements_count,
    )
    .await
    .unwrap();

    assert_eq!(from_peaks.get_peaks(None).await.unwrap(), original_peaks);
    assert_eq!(
        from_peaks.get_root_hash().await.unwrap(),
        original.get_root_hash().await.unwrap()
    );

    for element in ["6", "7", "8"] {
        assert_eq!(
            original.append(lv(element)).await.unwrap(),
            from_peaks.append(lv(element)).await.unwrap()
        );
    }
}

#[tokio::test]
async fn postgres_should_handle_create_from_peaks_edge_cases() {
    let fixture = PostgresFixture::start().await;
    let hasher = Arc::new(KeccakHasher::new());

    let mmr_id = next_mmr_id();
    let mut non_empty = Mmr::new(fixture.store.clone(), hasher.clone(), Some(mmr_id)).unwrap();
    non_empty.append(lv("1")).await.unwrap();

    let non_empty_res = Mmr::create_from_peaks(
        fixture.store.clone(),
        hasher.clone(),
        Some(mmr_id),
        vec![lv("1")],
        1,
    )
    .await;
    assert!(matches!(non_empty_res, Err(MmrError::NonEmptyMmr)));

    let invalid_peaks = Mmr::create_from_peaks(
        fixture.store.clone(),
        hasher.clone(),
        Some(next_mmr_id()),
        vec![lv("1"), lv("2")],
        1,
    )
    .await;
    assert!(matches!(
        invalid_peaks,
        Err(MmrError::InvalidPeaksCountForElements)
    ));
}

#[tokio::test]
async fn postgres_create_from_peaks_rejects_invalid_elements_count() {
    let fixture = PostgresFixture::start().await;
    let result = Mmr::create_from_peaks(
        fixture.store,
        Arc::new(KeccakHasher::new()),
        Some(next_mmr_id()),
        vec![],
        2,
    )
    .await;
    assert!(matches!(result, Err(MmrError::InvalidElementCount)));
}

#[tokio::test]
async fn postgres_get_peaks_and_bag_fail_when_expected_peak_is_missing() {
    let fixture = PostgresFixture::start().await;
    let store = fixture.store.clone();
    let mmr_id = next_mmr_id();

    store
        .set(
            StoreKey::new(mmr_id, KeyKind::NodeHash, 7),
            StoreValue::Hash(lv("7")),
        )
        .await
        .unwrap();
    store
        .set(
            StoreKey::new(mmr_id, KeyKind::NodeHash, 11),
            StoreValue::Hash(lv("11")),
        )
        .await
        .unwrap();

    let mmr = Mmr::new(store, Arc::new(KeccakHasher::new()), Some(mmr_id)).unwrap();
    assert!(matches!(
        mmr.get_peaks(Some(11)).await,
        Err(MmrError::NoHashFoundForIndex(10))
    ));
    assert!(matches!(
        mmr.bag_the_peaks(Some(11)).await,
        Err(MmrError::NoHashFoundForIndex(10))
    ));
}

#[tokio::test]
async fn postgres_should_keep_multiple_mmrs_isolated_in_one_store() {
    let fixture = PostgresFixture::start().await;
    let hasher = Arc::new(KeccakHasher::new());

    let mut mmr_a = Mmr::new(fixture.store.clone(), hasher.clone(), Some(next_mmr_id())).unwrap();
    let mut mmr_b = Mmr::new(fixture.store, hasher.clone(), Some(next_mmr_id())).unwrap();

    let a1 = mmr_a.append(lv("1")).await.unwrap();
    let a2 = mmr_a.append(lv("2")).await.unwrap();
    let b1 = mmr_b.append(lv("9")).await.unwrap();

    assert_eq!(a1.element_index, 1);
    assert_eq!(a2.elements_count, 3);
    assert_eq!(b1.elements_count, 1);

    let proof_a = mmr_a.get_proof(a1.element_index, None).await.unwrap();
    let proof_b = mmr_b.get_proof(b1.element_index, None).await.unwrap();
    assert!(mmr_a.verify_proof(&proof_a, lv("1"), None).await.unwrap());
    assert!(mmr_b.verify_proof(&proof_b, lv("9"), None).await.unwrap());
}

#[tokio::test]
async fn postgres_should_reject_invalid_index_and_fail_on_malformed_siblings() {
    let fixture = PostgresFixture::start().await;
    let mut mmr = Mmr::new(
        fixture.store,
        Arc::new(KeccakHasher::new()),
        Some(next_mmr_id()),
    )
    .unwrap();
    mmr.append(lv("1")).await.unwrap();
    mmr.append(lv("2")).await.unwrap();
    mmr.append(lv("3")).await.unwrap();

    assert!(matches!(
        mmr.get_proof(0, None).await,
        Err(MmrError::InvalidElementIndex)
    ));
    let mut proof = mmr.get_proof(1, None).await.unwrap();
    proof.siblings_hashes.push([0u8; 32]);
    assert!(!mmr.verify_proof(&proof, lv("1"), None).await.unwrap());
}

#[tokio::test]
async fn postgres_verify_proof_stateful_rejects_tampered_proof_fields() {
    let fixture = PostgresFixture::start().await;
    let mut mmr = Mmr::new(
        fixture.store,
        Arc::new(KeccakHasher::new()),
        Some(next_mmr_id()),
    )
    .unwrap();
    mmr.append(lv("1")).await.unwrap();
    mmr.append(lv("2")).await.unwrap();
    mmr.append(lv("3")).await.unwrap();

    let proof = mmr.get_proof(1, None).await.unwrap();
    assert!(mmr.verify_proof(&proof, lv("1"), None).await.unwrap());

    let mut tampered_peaks = proof.clone();
    tampered_peaks.peaks_hashes[0] = [0u8; 32];
    assert!(
        !mmr.verify_proof(&tampered_peaks, lv("1"), None)
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn postgres_stateless_verify_is_available_and_independent() {
    let fixture = PostgresFixture::start().await;
    let hasher = Arc::new(KeccakHasher::new());
    let mut mmr = Mmr::new(fixture.store, hasher.clone(), Some(next_mmr_id())).unwrap();
    mmr.append(lv("1")).await.unwrap();
    mmr.append(lv("2")).await.unwrap();
    mmr.append(lv("3")).await.unwrap();

    let proof = mmr.get_proof(1, None).await.unwrap();
    assert!(verify_proof_stateless(hasher.as_ref(), &proof, lv("1")).unwrap());
}
