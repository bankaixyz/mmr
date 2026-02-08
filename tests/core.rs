use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

mod common;

use common::{hash_from_hex, hash_to_hex};
use mmr::error::MmrError;
use mmr::hasher::{Hasher, KeccakHasher};
use mmr::types::ZERO_HASH;
use mmr::{InMemoryStore, KeyKind, Mmr, Store, StoreError, StoreKey, StoreValue};
#[cfg(feature = "postgres-store")]
use mmr::{PostgresStore, PostgresStoreOptions};

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

#[cfg(feature = "postgres-store")]
fn unique_test_mmr_id() -> u32 {
    use std::time::{SystemTime, UNIX_EPOCH};

    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64;
    ((nonce % ((i32::MAX as u64) - 10_000)) as u32) + 10_000
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

    let mut batched = Mmr::new(Arc::new(InMemoryStore::default()), hasher, Some(102)).unwrap();
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
    let mut batch_mmr = Mmr::new(Arc::new(InMemoryStore::default()), hasher, Some(104)).unwrap();

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
async fn batch_append_rejects_empty_values() {
    let store = Arc::new(InMemoryStore::default());
    let hasher = Arc::new(KeccakHasher::new());
    let mut mmr = Mmr::new(store, hasher, Some(105)).unwrap();

    assert!(matches!(
        mmr.batch_append(&[]).await,
        Err(MmrError::EmptyBatchAppend)
    ));
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
    let expected_zero_root = zero_mmr.calculate_root_hash(&zero_bag, 0).unwrap();
    assert_eq!(zero_root, expected_zero_root);

    let zero_append = zero_mmr.append(lv("1000")).await.unwrap();
    assert_eq!(zero_append.elements_count, 1);
    assert_eq!(zero_append.leaves_count, 1);

    let single = lv("0x1001");
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
    let expected_one_root = one_mmr.calculate_root_hash(&single, 1).unwrap();
    assert_eq!(one_root, expected_one_root);

    let one_append = one_mmr.append(lv("2000")).await.unwrap();
    assert_eq!(one_append.elements_count, 3);
    assert_eq!(one_append.leaves_count, 2);
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

#[cfg(feature = "stateless-verify")]
#[tokio::test]
async fn stateless_verify_is_available_and_independent() {
    let store = Arc::new(InMemoryStore::default());
    let hasher = Arc::new(KeccakHasher::new());

    let mut mmr = Mmr::new(store, hasher, Some(51)).unwrap();
    mmr.append(lv("1")).await.unwrap();
    mmr.append(lv("2")).await.unwrap();
    mmr.append(lv("3")).await.unwrap();

    let proof = mmr.get_proof(1, None).await.unwrap();
    assert!(
        mmr.verify_proof_stateless(&proof, lv("1"), None)
            .await
            .unwrap()
    );

    let mut tampered = proof.clone();
    tampered.peaks_hashes[0] = [0u8; 32];

    assert!(
        !mmr.verify_proof_stateless(&tampered, lv("1"), None)
            .await
            .unwrap()
    );

    assert!(mmr.verify_proof(&tampered, lv("1"), None).await.unwrap());
}

#[derive(Debug, Default)]
struct SpyStoreMetrics {
    get_calls: usize,
    set_calls: usize,
    get_many_calls: usize,
    set_many_calls: usize,
}

#[derive(Default)]
struct SpyStore {
    inner: Mutex<HashMap<StoreKey, StoreValue>>,
    get_calls: AtomicUsize,
    set_calls: AtomicUsize,
    get_many_calls: AtomicUsize,
    set_many_calls: AtomicUsize,
    fail_set_many: AtomicBool,
}

impl SpyStore {
    fn metrics(&self) -> SpyStoreMetrics {
        SpyStoreMetrics {
            get_calls: self.get_calls.load(Ordering::Relaxed),
            set_calls: self.set_calls.load(Ordering::Relaxed),
            get_many_calls: self.get_many_calls.load(Ordering::Relaxed),
            set_many_calls: self.set_many_calls.load(Ordering::Relaxed),
        }
    }

    fn set_fail_set_many(&self, fail: bool) {
        self.fail_set_many.store(fail, Ordering::Relaxed);
    }

    fn entry_count(&self) -> usize {
        self.inner.lock().unwrap().len()
    }
}

impl Store for SpyStore {
    async fn get(&self, key: &StoreKey) -> Result<Option<StoreValue>, StoreError> {
        self.get_calls.fetch_add(1, Ordering::Relaxed);
        Ok(self.inner.lock().unwrap().get(key).cloned())
    }

    async fn set(&self, key: StoreKey, value: StoreValue) -> Result<(), StoreError> {
        self.set_calls.fetch_add(1, Ordering::Relaxed);
        self.inner.lock().unwrap().insert(key, value);
        Ok(())
    }

    async fn set_many(&self, entries: Vec<(StoreKey, StoreValue)>) -> Result<(), StoreError> {
        self.set_many_calls.fetch_add(1, Ordering::Relaxed);
        if self.fail_set_many.load(Ordering::Relaxed) {
            return Err(StoreError::Internal("forced set_many failure".to_string()));
        }

        let mut guard = self.inner.lock().unwrap();
        for (key, value) in entries {
            guard.insert(key, value);
        }

        Ok(())
    }

    async fn get_many(&self, keys: &[StoreKey]) -> Result<Vec<Option<StoreValue>>, StoreError> {
        self.get_many_calls.fetch_add(1, Ordering::Relaxed);
        let guard = self.inner.lock().unwrap();
        Ok(keys.iter().map(|key| guard.get(key).cloned()).collect())
    }
}

#[tokio::test]
async fn append_uses_one_get_many_and_one_set_many_in_steady_state() {
    let store = Arc::new(SpyStore::default());
    let hasher = Arc::new(KeccakHasher::new());
    let mut mmr = Mmr::new(store.clone(), hasher, Some(61)).unwrap();

    mmr.append(lv("1")).await.unwrap();

    let before = store.metrics();
    mmr.append(lv("2")).await.unwrap();
    let after = store.metrics();

    assert_eq!(after.get_many_calls - before.get_many_calls, 1);
    assert_eq!(after.set_many_calls - before.set_many_calls, 1);
    assert_eq!(after.get_calls - before.get_calls, 0);
    assert_eq!(after.set_calls - before.set_calls, 0);
}

#[tokio::test]
async fn batch_append_uses_one_get_many_and_one_set_many_in_steady_state() {
    let store = Arc::new(SpyStore::default());
    let hasher = Arc::new(KeccakHasher::new());
    let mut mmr = Mmr::new(store.clone(), hasher, Some(63)).unwrap();

    mmr.batch_append(&[lv("1"), lv("2"), lv("3")])
        .await
        .unwrap();

    let before = store.metrics();
    mmr.batch_append(&[lv("4"), lv("5"), lv("6"), lv("7")])
        .await
        .unwrap();
    let after = store.metrics();

    assert_eq!(after.get_many_calls - before.get_many_calls, 1);
    assert_eq!(after.set_many_calls - before.set_many_calls, 1);
    assert_eq!(after.get_calls - before.get_calls, 0);
    assert_eq!(after.set_calls - before.set_calls, 0);
}

#[tokio::test]
async fn append_returns_error_and_avoids_partial_writes_when_set_many_fails() {
    let store = Arc::new(SpyStore::default());
    store.set_fail_set_many(true);
    let hasher = Arc::new(KeccakHasher::new());
    let mut mmr = Mmr::new(store.clone(), hasher, Some(62)).unwrap();

    let result = mmr.append(lv("1")).await;
    assert!(result.is_err());
    assert_eq!(store.entry_count(), 0);

    assert_eq!(mmr.get_elements_count().await.unwrap(), 0);
    assert_eq!(mmr.get_leaves_count().await.unwrap(), 0);

    let key = StoreKey::new(62, KeyKind::NodeHash, 1);
    assert!(store.get(&key).await.unwrap().is_none());
}

#[tokio::test]
async fn batch_append_returns_error_and_avoids_partial_writes_when_set_many_fails() {
    let store = Arc::new(SpyStore::default());
    store.set_fail_set_many(true);
    let hasher = Arc::new(KeccakHasher::new());
    let mut mmr = Mmr::new(store.clone(), hasher, Some(64)).unwrap();

    let result = mmr.batch_append(&[lv("1"), lv("2"), lv("3")]).await;
    assert!(result.is_err());
    assert_eq!(store.entry_count(), 0);

    assert_eq!(mmr.get_elements_count().await.unwrap(), 0);
    assert_eq!(mmr.get_leaves_count().await.unwrap(), 0);

    let key = StoreKey::new(64, KeyKind::NodeHash, 1);
    assert!(store.get(&key).await.unwrap().is_none());
}

#[cfg(feature = "postgres-store")]
#[tokio::test]
async fn postgres_batch_append_in_tx_rollback_leaves_store_unchanged() {
    let database_url = match std::env::var("DATABASE_URL") {
        Ok(url) => url,
        Err(_) => return,
    };

    let store = Arc::new(
        PostgresStore::connect_with_options(
            &database_url,
            PostgresStoreOptions {
                initialize_schema: true,
                max_connections: 2,
            },
        )
        .await
        .unwrap(),
    );
    let mut mmr = Mmr::new(
        store.clone(),
        Arc::new(KeccakHasher::new()),
        Some(unique_test_mmr_id()),
    )
    .unwrap();

    let mut tx = store.begin_write_tx().await.unwrap();
    let result = mmr
        .batch_append_in_tx(&mut tx, &[lv("1"), lv("2"), lv("3")])
        .await
        .unwrap();
    assert_eq!(result.appended_count, 3);
    tx.rollback().await.unwrap();

    assert_eq!(mmr.get_elements_count().await.unwrap(), 0);
    assert_eq!(mmr.get_leaves_count().await.unwrap(), 0);
    assert!(mmr.get_root_hash().await.unwrap().is_none());
}

#[cfg(feature = "postgres-store")]
#[tokio::test]
async fn postgres_append_in_tx_commit_persists_write() {
    let database_url = match std::env::var("DATABASE_URL") {
        Ok(url) => url,
        Err(_) => return,
    };

    let store = Arc::new(
        PostgresStore::connect_with_options(
            &database_url,
            PostgresStoreOptions {
                initialize_schema: true,
                max_connections: 2,
            },
        )
        .await
        .unwrap(),
    );
    let mut mmr = Mmr::new(
        store.clone(),
        Arc::new(KeccakHasher::new()),
        Some(unique_test_mmr_id()),
    )
    .unwrap();

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
    let database_url = match std::env::var("DATABASE_URL") {
        Ok(url) => url,
        Err(_) => return,
    };

    let store = Arc::new(
        PostgresStore::connect_with_options(
            &database_url,
            PostgresStoreOptions {
                initialize_schema: true,
                max_connections: 2,
            },
        )
        .await
        .unwrap(),
    );
    let mut mmr = Mmr::new(
        store.clone(),
        Arc::new(KeccakHasher::new()),
        Some(unique_test_mmr_id()),
    )
    .unwrap();

    let mut tx = store.begin_write_tx().await.unwrap();
    let first = mmr.append_in_tx(&mut tx, lv("21")).await.unwrap();
    let second = mmr.append_in_tx(&mut tx, lv("22")).await.unwrap();
    tx.commit().await.unwrap();

    assert_eq!(first.elements_count, 1);
    assert_eq!(second.elements_count, 3);
    assert_eq!(mmr.get_elements_count().await.unwrap(), 3);
    assert_eq!(mmr.get_leaves_count().await.unwrap(), 2);
}
