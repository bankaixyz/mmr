use std::sync::Arc;

mod common;

#[cfg(feature = "postgres-store")]
use common::pg::{PostgresFixture, next_mmr_id};
use common::{hash_from_hex, hash_to_hex};
#[cfg(feature = "postgres-store")]
use mmr::PostgresStore;
use mmr::error::MmrError;
use mmr::hasher::{Hasher, KeccakHasher, PoseidonHasher};
use mmr::types::{Hash32, ZERO_HASH};
use mmr::{
    InMemoryStore, KeyKind, Mmr, Store, StoreKey, StoreValue, bag_peaks_hashes,
    calculate_root_hash, verify_proof_stateless, verify_proof_stateless_with_root,
};

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

#[cfg(feature = "postgres-store")]
fn new_postgres_mmr(store: Arc<PostgresStore>, hasher: Arc<dyn Hasher>) -> Mmr<Arc<PostgresStore>> {
    Mmr::new(store, hasher, Some(next_mmr_id())).unwrap()
}

#[tokio::test]
async fn should_compute_parent_tree_for_keccak_hasher() {
    let store = Arc::new(InMemoryStore::default());
    let hasher = Arc::new(KeccakHasher::new());

    let mut mmr = Mmr::new(store, hasher.clone(), Some(1)).unwrap();

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
async fn batch_append_matches_repeated_append_for_identical_values() {
    let hasher = Arc::new(KeccakHasher::new());
    let leaves = ["1", "2", "3", "4", "5", "6", "7", "8"];

    let mut single = Mmr::new(
        Arc::new(InMemoryStore::default()),
        hasher.clone(),
        Some(101),
    )
    .unwrap();
    let mut single_appends = Vec::new();
    for leaf in leaves {
        single_appends.push(single.append(lv(leaf)).await.unwrap());
    }

    let mut batched = Mmr::new(
        Arc::new(InMemoryStore::default()),
        hasher.clone(),
        Some(102),
    )
    .unwrap();
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

    assert_eq!(
        batched.get_peaks(None).await.unwrap(),
        single.get_peaks(None).await.unwrap()
    );
    assert_eq!(
        batched.bag_the_peaks(None).await.unwrap(),
        single.bag_the_peaks(None).await.unwrap()
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
async fn append_matches_batch_append_single_value() {
    let hasher = Arc::new(KeccakHasher::new());
    let prefill = ["1", "2", "3", "4", "5"];

    let mut append_mmr = Mmr::new(
        Arc::new(InMemoryStore::default()),
        hasher.clone(),
        Some(103),
    )
    .unwrap();
    let mut batch_mmr = Mmr::new(
        Arc::new(InMemoryStore::default()),
        hasher.clone(),
        Some(104),
    )
    .unwrap();

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
    assert_eq!(
        batch_result.peaks_hashes,
        batch_mmr
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
}

#[tokio::test]
async fn batch_append_result_peaks_and_root_are_consistent_for_poseidon() {
    let hasher = Arc::new(PoseidonHasher::new());
    let mut mmr = Mmr::new(
        Arc::new(InMemoryStore::default()),
        hasher.clone(),
        Some(106),
    )
    .unwrap();

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
async fn batch_append_rejects_empty_values() {
    let store = Arc::new(InMemoryStore::default());
    let hasher = Arc::new(KeccakHasher::new());
    let mut mmr = Mmr::new(store, hasher, Some(105)).unwrap();

    assert!(matches!(
        mmr.batch_append(&[]).await,
        Err(MmrError::EmptyBatchAppend)
    ));
}

#[cfg(feature = "postgres-store")]
#[tokio::test]
async fn precommit_rejects_second_pending_batch() {
    let fixture = PostgresFixture::start().await;
    let store = fixture.store.clone();
    let hasher = Arc::new(KeccakHasher::new());
    let mut mmr = new_postgres_mmr(store, hasher);

    mmr.batch_precommit_append(&[lv("1"), lv("2")])
        .await
        .unwrap();
    assert!(matches!(
        mmr.batch_precommit_append(&[lv("3")]).await,
        Err(MmrError::PrecommitAlreadyPending)
    ));
}

#[cfg(feature = "postgres-store")]
#[tokio::test]
async fn append_and_batch_append_are_blocked_while_precommit_is_pending() {
    let fixture = PostgresFixture::start().await;
    let store = fixture.store.clone();
    let hasher = Arc::new(KeccakHasher::new());
    let mut mmr = new_postgres_mmr(store, hasher);

    mmr.append(lv("1")).await.unwrap();
    mmr.batch_precommit_append(&[lv("2"), lv("3")])
        .await
        .unwrap();

    assert!(matches!(
        mmr.append(lv("4")).await,
        Err(MmrError::AppendBlockedByPendingPrecommit)
    ));
    assert!(matches!(
        mmr.batch_append(&[lv("5"), lv("6")]).await,
        Err(MmrError::AppendBlockedByPendingPrecommit)
    ));
}

#[cfg(feature = "postgres-store")]
#[tokio::test]
async fn precommit_does_not_change_committed_state_until_commit() {
    let fixture = PostgresFixture::start().await;
    let store = fixture.store.clone();
    let hasher = Arc::new(KeccakHasher::new());
    let mut mmr = new_postgres_mmr(store, hasher);

    for leaf in ["1", "2", "3", "4", "5"] {
        mmr.append(lv(leaf)).await.unwrap();
    }

    let committed_elements = mmr.get_elements_count().await.unwrap();
    let committed_leaves = mmr.get_leaves_count().await.unwrap();
    let committed_root = mmr.get_root_hash().await.unwrap().unwrap();
    let committed_peaks = mmr.get_peaks(None).await.unwrap();

    let precommit_result = mmr
        .batch_precommit_append(&[lv("6"), lv("7"), lv("8")])
        .await
        .unwrap();
    assert_eq!(precommit_result.appended_count, 3);

    assert_eq!(mmr.get_elements_count().await.unwrap(), committed_elements);
    assert_eq!(mmr.get_leaves_count().await.unwrap(), committed_leaves);
    assert_eq!(mmr.get_root_hash().await.unwrap().unwrap(), committed_root);
    assert_eq!(mmr.get_peaks(None).await.unwrap(), committed_peaks);
}

#[cfg(feature = "postgres-store")]
#[tokio::test]
async fn commit_precommit_returns_batch_result_and_promotes_state() {
    let fixture = PostgresFixture::start().await;
    let store = fixture.store.clone();
    let hasher = Arc::new(KeccakHasher::new());
    let mut mmr = new_postgres_mmr(store, hasher);

    for leaf in ["1", "2", "3", "4", "5"] {
        mmr.append(lv(leaf)).await.unwrap();
    }

    let precommit_result = mmr
        .batch_precommit_append(&[lv("6"), lv("7"), lv("8")])
        .await
        .unwrap();
    let commit_result = mmr.commit_precommit().await.unwrap();

    assert_eq!(commit_result, precommit_result);
    assert_eq!(
        mmr.get_elements_count().await.unwrap(),
        commit_result.elements_count
    );
    assert_eq!(
        mmr.get_leaves_count().await.unwrap(),
        commit_result.leaves_count
    );
    assert_eq!(
        mmr.get_root_hash().await.unwrap().unwrap(),
        commit_result.root_hash
    );
    assert_eq!(
        mmr.get_peaks(None).await.unwrap(),
        commit_result.peaks_hashes
    );
    assert!(matches!(
        mmr.commit_precommit().await,
        Err(MmrError::NoPendingPrecommit)
    ));
}

#[cfg(feature = "postgres-store")]
#[tokio::test]
async fn commit_precommit_base_mismatch_returns_conflict_and_keeps_pending() {
    let fixture = PostgresFixture::start().await;
    let store = fixture.store.clone();
    let hasher = Arc::new(KeccakHasher::new());
    let mmr_id = next_mmr_id();
    let mut mmr = Mmr::new(store.clone(), hasher, Some(mmr_id)).unwrap();

    mmr.append(lv("1")).await.unwrap();
    let precommit_result = mmr.batch_precommit_append(&[lv("2")]).await.unwrap();
    let staged_key = StoreKey::new(
        mmr_id,
        KeyKind::NodeHash,
        precommit_result.first_element_index,
    );

    assert!(store.get(&staged_key).await.unwrap().is_none());
    store
        .set(
            StoreKey::metadata(mmr_id, KeyKind::ElementsCount),
            StoreValue::U64(precommit_result.first_element_index),
        )
        .await
        .unwrap();

    assert!(matches!(
        mmr.commit_precommit().await,
        Err(MmrError::PrecommitBaseStateChanged)
    ));
    assert!(store.has_pending_batch(mmr_id).await.unwrap());
    assert!(store.get(&staged_key).await.unwrap().is_none());
}

#[cfg(feature = "postgres-store")]
#[tokio::test]
async fn revert_precommit_discards_staged_state_and_preserves_committed_state() {
    let fixture = PostgresFixture::start().await;
    let store = fixture.store.clone();
    let hasher = Arc::new(KeccakHasher::new());
    let mut mmr = new_postgres_mmr(store, hasher);

    for leaf in ["1", "2", "3", "4", "5"] {
        mmr.append(lv(leaf)).await.unwrap();
    }

    let committed_elements = mmr.get_elements_count().await.unwrap();
    let committed_leaves = mmr.get_leaves_count().await.unwrap();
    let committed_root = mmr.get_root_hash().await.unwrap().unwrap();
    let committed_peaks = mmr.get_peaks(None).await.unwrap();

    mmr.batch_precommit_append(&[lv("6"), lv("7"), lv("8")])
        .await
        .unwrap();
    mmr.revert_precommit().await.unwrap();

    assert_eq!(mmr.get_elements_count().await.unwrap(), committed_elements);
    assert_eq!(mmr.get_leaves_count().await.unwrap(), committed_leaves);
    assert_eq!(mmr.get_root_hash().await.unwrap().unwrap(), committed_root);
    assert_eq!(mmr.get_peaks(None).await.unwrap(), committed_peaks);
    assert!(matches!(
        mmr.revert_precommit().await,
        Err(MmrError::NoPendingPrecommit)
    ));
}

#[cfg(feature = "postgres-store")]
#[tokio::test]
async fn precommit_matches_batch_append_output_exactly_from_same_base_state() {
    let fixture = PostgresFixture::start().await;
    let store = fixture.store.clone();
    let hasher = Arc::new(KeccakHasher::new());
    let mut normal = new_postgres_mmr(store.clone(), hasher.clone());
    let mut precommit = new_postgres_mmr(store, hasher);

    for leaf in ["1", "2", "3", "4", "5", "6", "7"] {
        normal.append(lv(leaf)).await.unwrap();
        precommit.append(lv(leaf)).await.unwrap();
    }

    let values = [lv("8"), lv("9"), lv("10"), lv("11")];
    let normal_result = normal.batch_append(&values).await.unwrap();
    let precommit_result = precommit.batch_precommit_append(&values).await.unwrap();

    assert_eq!(precommit_result, normal_result);
}

#[cfg(feature = "postgres-store")]
#[tokio::test]
async fn precommit_peaks_include_unchanged_peaks_from_base_state() {
    let fixture = PostgresFixture::start().await;
    let store = fixture.store.clone();
    let hasher = Arc::new(KeccakHasher::new());
    let mut normal = new_postgres_mmr(store.clone(), hasher.clone());
    let mut precommit = new_postgres_mmr(store, hasher);

    for leaf in ["1", "2", "3", "4", "5"] {
        normal.append(lv(leaf)).await.unwrap();
        precommit.append(lv(leaf)).await.unwrap();
    }

    let base_peaks = precommit.get_peaks(None).await.unwrap();
    let normal_result = normal.batch_append(&[lv("6")]).await.unwrap();
    let precommit_result = precommit.batch_precommit_append(&[lv("6")]).await.unwrap();

    assert_eq!(precommit_result.peaks_hashes, normal_result.peaks_hashes);
    assert!(
        base_peaks
            .iter()
            .any(|peak| precommit_result.peaks_hashes.contains(peak))
    );
}

#[tokio::test]
async fn should_create_from_peaks_and_match_followup_appends() {
    let hasher = Arc::new(KeccakHasher::new());

    let store1 = Arc::new(InMemoryStore::default());
    let mut original = Mmr::new(store1.clone(), hasher.clone(), Some(11)).unwrap();

    let mut original_appends = Vec::new();
    for leaf in LEAVES {
        original_appends.push(original.append(lv(leaf)).await.unwrap());
    }

    let original_elements_count = original.get_elements_count().await.unwrap();
    let original_leaves_count = original.get_leaves_count().await.unwrap();
    let original_peaks = original.get_peaks(None).await.unwrap();
    let original_bag = original.bag_the_peaks(None).await.unwrap();
    let original_root = original.get_root_hash().await.unwrap().unwrap();

    let store2 = Arc::new(InMemoryStore::default());
    let mut from_peaks = Mmr::create_from_peaks(
        store2,
        hasher.clone(),
        Some(12),
        original_peaks.clone(),
        original_elements_count,
    )
    .await
    .unwrap();

    assert_eq!(
        from_peaks.get_elements_count().await.unwrap(),
        original_elements_count
    );
    assert_eq!(
        from_peaks.get_leaves_count().await.unwrap(),
        original_leaves_count
    );
    assert_eq!(from_peaks.get_peaks(None).await.unwrap(), original_peaks);
    assert_eq!(from_peaks.bag_the_peaks(None).await.unwrap(), original_bag);
    assert_eq!(
        from_peaks.get_root_hash().await.unwrap().unwrap(),
        original_root
    );

    let new_elements = ["6", "7", "8"];
    let mut new_appends_orig = Vec::new();
    let mut new_appends_peaks = Vec::new();

    for element in new_elements {
        new_appends_orig.push(original.append(lv(element)).await.unwrap());
        new_appends_peaks.push(from_peaks.append(lv(element)).await.unwrap());
    }

    assert_eq!(new_appends_orig, new_appends_peaks);

    let final_elements_count = original.get_elements_count().await.unwrap();
    let final_leaves_count = original.get_leaves_count().await.unwrap();
    let final_peaks = original.get_peaks(None).await.unwrap();
    let final_bag = original.bag_the_peaks(None).await.unwrap();
    let final_root = original.get_root_hash().await.unwrap().unwrap();

    assert_eq!(
        from_peaks.get_elements_count().await.unwrap(),
        final_elements_count
    );
    assert_eq!(
        from_peaks.get_leaves_count().await.unwrap(),
        final_leaves_count
    );
    assert_eq!(from_peaks.get_peaks(None).await.unwrap(), final_peaks);
    assert_eq!(from_peaks.bag_the_peaks(None).await.unwrap(), final_bag);
    assert_eq!(
        from_peaks.get_root_hash().await.unwrap().unwrap(),
        final_root
    );

    for (idx, element) in ["6", "7", "8"].iter().enumerate() {
        let element_index = new_appends_orig[idx].element_index;

        let proof_orig = original.get_proof(element_index, None).await.unwrap();
        let proof_peaks = from_peaks.get_proof(element_index, None).await.unwrap();

        assert_eq!(proof_orig, proof_peaks);
        assert!(
            original
                .verify_proof(&proof_orig, lv(element), None)
                .await
                .unwrap()
        );
        assert!(
            from_peaks
                .verify_proof(&proof_peaks, lv(element), None)
                .await
                .unwrap()
        );
    }

    let old_element_index = original_appends[0].element_index;
    let old_proof = original.get_proof(old_element_index, None).await.unwrap();
    assert!(
        original
            .verify_proof(&old_proof, lv("1"), None)
            .await
            .unwrap()
    );

    if let Ok(old_from_peaks_proof) = from_peaks.get_proof(old_element_index, None).await {
        assert!(
            !from_peaks
                .verify_proof(&old_from_peaks_proof, lv("1"), None)
                .await
                .unwrap_or(false)
        );
    }
}

#[tokio::test]
async fn should_handle_create_from_peaks_edge_cases() {
    let hasher = Arc::new(KeccakHasher::new());

    let store = Arc::new(InMemoryStore::default());
    let mut non_empty = Mmr::new(store.clone(), hasher.clone(), Some(21)).unwrap();
    non_empty.append(lv("1")).await.unwrap();

    let non_empty_res =
        Mmr::create_from_peaks(store, hasher.clone(), Some(21), vec![lv("1")], 1).await;
    assert!(matches!(non_empty_res, Err(MmrError::NonEmptyMmr)));

    let invalid_peaks = Mmr::create_from_peaks(
        Arc::new(InMemoryStore::default()),
        hasher.clone(),
        Some(22),
        vec![lv("1"), lv("2")],
        1,
    )
    .await;
    assert!(matches!(
        invalid_peaks,
        Err(MmrError::InvalidPeaksCountForElements)
    ));

    let mut zero_mmr = Mmr::create_from_peaks(
        Arc::new(InMemoryStore::default()),
        hasher.clone(),
        Some(23),
        vec![],
        0,
    )
    .await
    .unwrap();

    assert_eq!(zero_mmr.get_elements_count().await.unwrap(), 0);
    assert_eq!(zero_mmr.get_leaves_count().await.unwrap(), 0);
    assert!(zero_mmr.get_peaks(None).await.unwrap().is_empty());

    let zero_bag = zero_mmr.bag_the_peaks(None).await.unwrap();
    assert_eq!(zero_bag, ZERO_HASH);

    let zero_root = zero_mmr.get_root_hash().await.unwrap().unwrap();
    let expected_zero_root = calculate_root_hash(hasher.as_ref(), 0, &[]).unwrap();
    assert_eq!(zero_root, expected_zero_root);

    let zero_append = zero_mmr.append(lv("1000")).await.unwrap();
    assert_eq!(zero_append.elements_count, 1);
    assert_eq!(zero_append.leaves_count, 1);

    let single = lv("0x1001");
    let one_hasher = hasher.clone();
    let mut one_mmr = Mmr::create_from_peaks(
        Arc::new(InMemoryStore::default()),
        hasher,
        Some(24),
        vec![single],
        1,
    )
    .await
    .unwrap();

    assert_eq!(one_mmr.get_elements_count().await.unwrap(), 1);
    assert_eq!(one_mmr.get_leaves_count().await.unwrap(), 1);
    assert_eq!(one_mmr.get_peaks(None).await.unwrap(), vec![single]);
    assert_eq!(one_mmr.bag_the_peaks(None).await.unwrap(), single);

    let one_root = one_mmr.get_root_hash().await.unwrap().unwrap();
    let expected_one_root = calculate_root_hash(one_hasher.as_ref(), 1, &[single]).unwrap();
    assert_eq!(one_root, expected_one_root);

    let one_append = one_mmr.append(lv("2000")).await.unwrap();
    assert_eq!(one_append.elements_count, 3);
    assert_eq!(one_append.leaves_count, 2);
}

#[tokio::test]
async fn create_from_peaks_rejects_invalid_elements_count() {
    let hasher = Arc::new(KeccakHasher::new());
    let result = Mmr::create_from_peaks(
        Arc::new(InMemoryStore::default()),
        hasher,
        Some(25),
        vec![],
        2,
    )
    .await;

    assert!(matches!(result, Err(MmrError::InvalidElementCount)));
}

#[tokio::test]
async fn get_peaks_and_bag_fail_when_expected_peak_is_missing() {
    let store = Arc::new(InMemoryStore::default());
    let hasher = Arc::new(KeccakHasher::new());
    let mmr_id = 26;

    // elements_count=11 has expected peak indices [7, 10, 11]; index 10 is intentionally missing.
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

    let mmr = Mmr::new(store, hasher, Some(mmr_id)).unwrap();

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
async fn should_keep_multiple_mmrs_isolated_in_one_store() {
    let shared_store = Arc::new(InMemoryStore::default());
    let hasher = Arc::new(KeccakHasher::new());

    let mut mmr_a = Mmr::new(shared_store.clone(), hasher.clone(), Some(31)).unwrap();
    let mut mmr_b = Mmr::new(shared_store, hasher.clone(), Some(32)).unwrap();

    let a1 = mmr_a.append(lv("1")).await.unwrap();
    let a2 = mmr_a.append(lv("2")).await.unwrap();
    let b1 = mmr_b.append(lv("9")).await.unwrap();

    assert_eq!(a1.element_index, 1);
    assert_eq!(a2.elements_count, 3);
    assert_eq!(b1.elements_count, 1);

    assert_eq!(mmr_a.get_leaves_count().await.unwrap(), 2);
    assert_eq!(mmr_b.get_leaves_count().await.unwrap(), 1);
    assert_ne!(
        hash_to_hex(&mmr_a.get_root_hash().await.unwrap().unwrap()),
        hash_to_hex(&mmr_b.get_root_hash().await.unwrap().unwrap())
    );

    let proof_a = mmr_a.get_proof(a1.element_index, None).await.unwrap();
    let proof_b = mmr_b.get_proof(b1.element_index, None).await.unwrap();

    assert!(mmr_a.verify_proof(&proof_a, lv("1"), None).await.unwrap());
    assert!(mmr_b.verify_proof(&proof_b, lv("9"), None).await.unwrap());
}

#[tokio::test]
async fn should_reject_invalid_index_and_fail_on_malformed_siblings() {
    let store = Arc::new(InMemoryStore::default());
    let hasher = Arc::new(KeccakHasher::new());

    let mut mmr = Mmr::new(store, hasher, Some(41)).unwrap();
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
async fn verify_proof_stateful_rejects_tampered_proof_fields() {
    let store = Arc::new(InMemoryStore::default());
    let hasher = Arc::new(KeccakHasher::new());
    let mut mmr = Mmr::new(store, hasher, Some(52)).unwrap();
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

    let mut tampered_elements_count = proof.clone();
    tampered_elements_count.elements_count -= 1;
    assert!(
        !mmr.verify_proof(&tampered_elements_count, lv("1"), None)
            .await
            .unwrap()
    );

    let mut tampered_element_hash = proof;
    tampered_element_hash.element_hash = [0u8; 32];
    assert!(
        !mmr.verify_proof(&tampered_element_hash, lv("1"), None)
            .await
            .unwrap()
    );
}

#[test]
fn stateless_helpers_cover_peak_bagging_edges() {
    let hasher = KeccakHasher::new();
    let empty: Vec<Hash32> = vec![];
    let single = lv("1");
    let multi = vec![lv("1"), lv("2"), lv("3")];

    assert_eq!(bag_peaks_hashes(&hasher, &empty).unwrap(), ZERO_HASH);
    assert_eq!(bag_peaks_hashes(&hasher, &[single]).unwrap(), single);

    let expected_multi = hasher
        .hash_pair(&multi[0], &hasher.hash_pair(&multi[1], &multi[2]).unwrap())
        .unwrap();
    assert_eq!(bag_peaks_hashes(&hasher, &multi).unwrap(), expected_multi);
    assert!(matches!(
        calculate_root_hash(&hasher, 1, &multi),
        Err(MmrError::InvalidPeaksCountForElements)
    ));
}

#[tokio::test]
async fn stateless_verify_is_available_and_independent() {
    let store = Arc::new(InMemoryStore::default());
    let hasher = Arc::new(KeccakHasher::new());

    let mut mmr = Mmr::new(store, hasher.clone(), Some(51)).unwrap();
    mmr.append(lv("1")).await.unwrap();
    mmr.append(lv("2")).await.unwrap();
    mmr.append(lv("3")).await.unwrap();

    let proof = mmr.get_proof(1, None).await.unwrap();
    let root = mmr.get_root_hash().await.unwrap().unwrap();
    assert!(verify_proof_stateless(hasher.as_ref(), &proof, lv("1")).unwrap());
    assert!(verify_proof_stateless_with_root(hasher.as_ref(), &proof, lv("1"), &root).unwrap());

    let mut tampered = proof.clone();
    tampered.peaks_hashes[0] = [0u8; 32];

    assert!(!verify_proof_stateless(hasher.as_ref(), &tampered, lv("1")).unwrap());
    assert!(
        !verify_proof_stateless_with_root(hasher.as_ref(), &proof, lv("1"), &[9u8; 32]).unwrap()
    );

    assert!(!mmr.verify_proof(&tampered, lv("1"), None).await.unwrap());
}

#[tokio::test]
async fn stateless_verify_rejects_invalid_inputs_and_detects_tampering() {
    let store = Arc::new(InMemoryStore::default());
    let hasher = Arc::new(KeccakHasher::new());

    let mut mmr = Mmr::new(store, hasher.clone(), Some(52)).unwrap();
    mmr.append(lv("1")).await.unwrap();
    mmr.append(lv("2")).await.unwrap();
    mmr.append(lv("3")).await.unwrap();

    let proof = mmr.get_proof(1, None).await.unwrap();

    assert!(!verify_proof_stateless(hasher.as_ref(), &proof, lv("9")).unwrap());

    let mut tampered_siblings = proof.clone();
    tampered_siblings.siblings_hashes.clear();
    assert!(!verify_proof_stateless(hasher.as_ref(), &tampered_siblings, lv("1")).unwrap());

    let mut tampered_element_hash = proof.clone();
    tampered_element_hash.element_hash = lv("99");
    assert!(!verify_proof_stateless(hasher.as_ref(), &tampered_element_hash, lv("1")).unwrap());

    let mut invalid_peaks = proof.clone();
    invalid_peaks.peaks_hashes.clear();
    assert!(matches!(
        verify_proof_stateless(hasher.as_ref(), &invalid_peaks, lv("1")),
        Err(MmrError::InvalidPeaksCount)
    ));

    let mut invalid_index = proof.clone();
    invalid_index.element_index = 0;
    assert!(matches!(
        verify_proof_stateless(hasher.as_ref(), &invalid_index, lv("1")),
        Err(MmrError::InvalidElementIndex)
    ));
}

#[cfg(feature = "postgres-store")]
#[tokio::test]
async fn postgres_batch_append_in_tx_rollback_leaves_store_unchanged() {
    let fixture = PostgresFixture::start().await;
    let store = fixture.store.clone();
    let hasher = Arc::new(KeccakHasher::new());
    let mut mmr = new_postgres_mmr(store.clone(), hasher.clone());

    let mut tx = store.begin_write_tx().await.unwrap();
    let result = mmr
        .batch_append_in_tx(&mut tx, &[lv("1"), lv("2"), lv("3")])
        .await
        .unwrap();
    assert_eq!(result.appended_count, 3);
    assert!(!result.peaks_hashes.is_empty());
    assert_eq!(
        result.root_hash,
        root_from_peaks(hasher.as_ref(), &result.peaks_hashes, result.elements_count)
    );
    tx.rollback().await.unwrap();

    assert_eq!(mmr.get_elements_count().await.unwrap(), 0);
    assert_eq!(mmr.get_leaves_count().await.unwrap(), 0);
    assert!(mmr.get_root_hash().await.unwrap().is_none());
}

#[cfg(feature = "postgres-store")]
#[tokio::test]
async fn postgres_batch_append_in_tx_returns_peaks_matching_committed_state() {
    let fixture = PostgresFixture::start().await;
    let store = fixture.store.clone();
    let hasher = Arc::new(KeccakHasher::new());
    let mut mmr = new_postgres_mmr(store.clone(), hasher.clone());

    let mut tx = store.begin_write_tx().await.unwrap();
    let result = mmr
        .batch_append_in_tx(&mut tx, &[lv("1"), lv("2"), lv("3")])
        .await
        .unwrap();
    tx.commit().await.unwrap();

    assert!(!result.peaks_hashes.is_empty());
    assert_eq!(
        result.root_hash,
        root_from_peaks(hasher.as_ref(), &result.peaks_hashes, result.elements_count)
    );
    assert_eq!(
        result.peaks_hashes,
        mmr.get_peaks(Some(result.elements_count)).await.unwrap()
    );
    assert_eq!(
        result.root_hash,
        mmr.get_root_hash().await.unwrap().unwrap()
    );
}

#[cfg(feature = "postgres-store")]
#[tokio::test]
async fn postgres_batch_append_in_tx_is_blocked_when_precommit_is_pending() {
    let fixture = PostgresFixture::start().await;
    let store = fixture.store.clone();
    let mut mmr = new_postgres_mmr(store.clone(), Arc::new(KeccakHasher::new()));

    mmr.batch_precommit_append(&[lv("1")]).await.unwrap();

    let mut tx = store.begin_write_tx().await.unwrap();
    assert!(matches!(
        mmr.batch_append_in_tx(&mut tx, &[lv("2")]).await,
        Err(MmrError::AppendBlockedByPendingPrecommit)
    ));
    tx.rollback().await.unwrap();
}

#[cfg(feature = "postgres-store")]
#[tokio::test]
async fn postgres_batch_precommit_in_tx_matches_non_tx_batch_precommit_output() {
    let fixture = PostgresFixture::start().await;
    let store = fixture.store.clone();
    let hasher = Arc::new(KeccakHasher::new());
    let mut non_tx = new_postgres_mmr(store.clone(), hasher.clone());
    let mut in_tx = new_postgres_mmr(store.clone(), hasher);

    for leaf in ["1", "2", "3", "4", "5"] {
        non_tx.append(lv(leaf)).await.unwrap();
        in_tx.append(lv(leaf)).await.unwrap();
    }

    let values = [lv("6"), lv("7"), lv("8")];
    let expected = non_tx.batch_precommit_append(&values).await.unwrap();

    let mut tx = store.begin_write_tx().await.unwrap();
    let actual = in_tx
        .batch_precommit_append_in_tx(&mut tx, &values)
        .await
        .unwrap();
    tx.commit().await.unwrap();

    assert_eq!(actual, expected);
}

#[cfg(feature = "postgres-store")]
#[tokio::test]
async fn postgres_batch_append_in_tx_is_blocked_by_pending_created_in_same_tx() {
    let fixture = PostgresFixture::start().await;
    let store = fixture.store.clone();
    let mut mmr = new_postgres_mmr(store.clone(), Arc::new(KeccakHasher::new()));

    let mut tx = store.begin_write_tx().await.unwrap();
    mmr.batch_precommit_append_in_tx(&mut tx, &[lv("1")])
        .await
        .unwrap();
    assert!(matches!(
        mmr.batch_append_in_tx(&mut tx, &[lv("2")]).await,
        Err(MmrError::AppendBlockedByPendingPrecommit)
    ));
    tx.rollback().await.unwrap();
}

#[cfg(feature = "postgres-store")]
#[tokio::test]
async fn postgres_commit_precommit_in_tx_promotes_state_after_outer_commit() {
    let fixture = PostgresFixture::start().await;
    let store = fixture.store.clone();
    let hasher = Arc::new(KeccakHasher::new());
    let mut mmr = new_postgres_mmr(store.clone(), hasher.clone());

    for leaf in ["1", "2", "3", "4", "5"] {
        mmr.append(lv(leaf)).await.unwrap();
    }

    let mut tx = store.begin_write_tx().await.unwrap();
    let staged = mmr
        .batch_precommit_append_in_tx(&mut tx, &[lv("6"), lv("7"), lv("8")])
        .await
        .unwrap();
    let committed = mmr.commit_precommit_in_tx(&mut tx).await.unwrap();
    assert_eq!(committed, staged);
    tx.commit().await.unwrap();

    assert_eq!(
        mmr.get_elements_count().await.unwrap(),
        committed.elements_count
    );
    assert_eq!(
        mmr.get_leaves_count().await.unwrap(),
        committed.leaves_count
    );
    assert_eq!(
        mmr.get_root_hash().await.unwrap().unwrap(),
        committed.root_hash
    );
    assert_eq!(mmr.get_peaks(None).await.unwrap(), committed.peaks_hashes);
    assert!(!store.has_pending_batch(mmr.mmr_id).await.unwrap());
}

#[cfg(feature = "postgres-store")]
#[tokio::test]
async fn postgres_revert_precommit_in_tx_discards_pending_after_outer_commit() {
    let fixture = PostgresFixture::start().await;
    let store = fixture.store.clone();
    let mut mmr = new_postgres_mmr(store.clone(), Arc::new(KeccakHasher::new()));

    for leaf in ["1", "2", "3", "4", "5"] {
        mmr.append(lv(leaf)).await.unwrap();
    }
    let committed_elements = mmr.get_elements_count().await.unwrap();
    let committed_leaves = mmr.get_leaves_count().await.unwrap();
    let committed_root = mmr.get_root_hash().await.unwrap().unwrap();
    let committed_peaks = mmr.get_peaks(None).await.unwrap();

    let mut tx = store.begin_write_tx().await.unwrap();
    mmr.batch_precommit_append_in_tx(&mut tx, &[lv("6"), lv("7"), lv("8")])
        .await
        .unwrap();
    mmr.revert_precommit_in_tx(&mut tx).await.unwrap();
    tx.commit().await.unwrap();

    assert_eq!(mmr.get_elements_count().await.unwrap(), committed_elements);
    assert_eq!(mmr.get_leaves_count().await.unwrap(), committed_leaves);
    assert_eq!(mmr.get_root_hash().await.unwrap().unwrap(), committed_root);
    assert_eq!(mmr.get_peaks(None).await.unwrap(), committed_peaks);
    assert!(!store.has_pending_batch(mmr.mmr_id).await.unwrap());
}

#[cfg(feature = "postgres-store")]
#[tokio::test]
async fn postgres_precommit_in_tx_is_discarded_on_outer_rollback() {
    let fixture = PostgresFixture::start().await;
    let store = fixture.store.clone();
    let mut mmr = new_postgres_mmr(store.clone(), Arc::new(KeccakHasher::new()));

    mmr.append(lv("1")).await.unwrap();
    let committed_elements = mmr.get_elements_count().await.unwrap();
    let committed_leaves = mmr.get_leaves_count().await.unwrap();
    let committed_root = mmr.get_root_hash().await.unwrap().unwrap();

    let mut tx = store.begin_write_tx().await.unwrap();
    mmr.precommit_append_in_tx(&mut tx, lv("2")).await.unwrap();
    tx.rollback().await.unwrap();

    assert_eq!(mmr.get_elements_count().await.unwrap(), committed_elements);
    assert_eq!(mmr.get_leaves_count().await.unwrap(), committed_leaves);
    assert_eq!(mmr.get_root_hash().await.unwrap().unwrap(), committed_root);
    assert!(!store.has_pending_batch(mmr.mmr_id).await.unwrap());
}

#[cfg(feature = "postgres-store")]
#[tokio::test]
async fn postgres_commit_precommit_in_tx_returns_no_pending() {
    let fixture = PostgresFixture::start().await;
    let store = fixture.store.clone();
    let mut mmr = new_postgres_mmr(store.clone(), Arc::new(KeccakHasher::new()));

    let mut tx = store.begin_write_tx().await.unwrap();
    assert!(matches!(
        mmr.commit_precommit_in_tx(&mut tx).await,
        Err(MmrError::NoPendingPrecommit)
    ));
    tx.rollback().await.unwrap();
}

#[cfg(feature = "postgres-store")]
#[tokio::test]
async fn postgres_revert_precommit_in_tx_returns_no_pending() {
    let fixture = PostgresFixture::start().await;
    let store = fixture.store.clone();
    let mut mmr = new_postgres_mmr(store.clone(), Arc::new(KeccakHasher::new()));

    let mut tx = store.begin_write_tx().await.unwrap();
    assert!(matches!(
        mmr.revert_precommit_in_tx(&mut tx).await,
        Err(MmrError::NoPendingPrecommit)
    ));
    tx.rollback().await.unwrap();
}

#[cfg(feature = "postgres-store")]
#[tokio::test]
async fn postgres_append_in_tx_commit_persists_write() {
    let fixture = PostgresFixture::start().await;
    let store = fixture.store.clone();
    let mut mmr = new_postgres_mmr(store.clone(), Arc::new(KeccakHasher::new()));

    let mut tx = store.begin_write_tx().await.unwrap();
    let append = mmr.append_in_tx(&mut tx, lv("10")).await.unwrap();
    tx.commit().await.unwrap();

    assert_eq!(append.element_index, 1);
    assert_eq!(mmr.get_elements_count().await.unwrap(), 1);
    assert_eq!(mmr.get_leaves_count().await.unwrap(), 1);
    assert!(mmr.get_root_hash().await.unwrap().is_some());
}

#[cfg(feature = "postgres-store")]
#[tokio::test]
async fn postgres_multiple_appends_in_same_tx_are_composable() {
    let fixture = PostgresFixture::start().await;
    let store = fixture.store.clone();
    let hasher = Arc::new(KeccakHasher::new());
    let mut mmr = new_postgres_mmr(store.clone(), hasher.clone());

    let mut tx = store.begin_write_tx().await.unwrap();
    let first = mmr.append_in_tx(&mut tx, lv("21")).await.unwrap();
    let second = mmr.append_in_tx(&mut tx, lv("22")).await.unwrap();
    tx.commit().await.unwrap();

    assert_eq!(first.elements_count, 1);
    assert_eq!(second.elements_count, 3);
    assert_eq!(mmr.get_elements_count().await.unwrap(), 3);
    assert_eq!(mmr.get_leaves_count().await.unwrap(), 2);

    let mut tx = store.begin_write_tx().await.unwrap();
    let first_batch = mmr.batch_append_in_tx(&mut tx, &[lv("31")]).await.unwrap();
    let second_batch = mmr.batch_append_in_tx(&mut tx, &[lv("32")]).await.unwrap();
    tx.commit().await.unwrap();

    assert!(!first_batch.peaks_hashes.is_empty());
    assert_eq!(
        first_batch.root_hash,
        root_from_peaks(
            hasher.as_ref(),
            &first_batch.peaks_hashes,
            first_batch.elements_count,
        )
    );
    assert!(!second_batch.peaks_hashes.is_empty());
    assert_eq!(
        second_batch.root_hash,
        root_from_peaks(
            hasher.as_ref(),
            &second_batch.peaks_hashes,
            second_batch.elements_count,
        )
    );
    assert_eq!(
        second_batch.peaks_hashes,
        mmr.get_peaks(Some(second_batch.elements_count))
            .await
            .unwrap()
    );
}
