#[cfg(feature = "postgres-store")]
mod common;

#[cfg(feature = "postgres-store")]
use common::pg::{PostgresFixture, next_mmr_id};
#[cfg(feature = "postgres-store")]
use mmr::{BatchAppendResult, KeyKind, PendingBatch, Store, StoreKey, StoreValue};

#[cfg(feature = "postgres-store")]
#[tokio::test]
async fn set_many_roundtrip_works() {
    let fixture = PostgresFixture::start().await;
    let store = fixture.store.clone();
    let mmr_id = next_mmr_id();

    let keys = vec![
        StoreKey::metadata(mmr_id, KeyKind::LeafCount),
        StoreKey::new(mmr_id, KeyKind::NodeHash, 42),
    ];

    store
        .set_many(vec![
            (keys[0].clone(), StoreValue::U64(12)),
            (keys[1].clone(), StoreValue::Hash([7u8; 32])),
        ])
        .await
        .unwrap();

    let values = store.get_many(&keys).await.unwrap();
    assert_eq!(
        values[0]
            .clone()
            .unwrap()
            .expect_u64(&StoreKey::metadata(mmr_id, KeyKind::LeafCount))
            .unwrap(),
        12
    );
    assert_eq!(
        values[1]
            .clone()
            .unwrap()
            .expect_hash(&StoreKey::new(mmr_id, KeyKind::NodeHash, 42))
            .unwrap(),
        [7u8; 32]
    );
}

#[cfg(feature = "postgres-store")]
#[tokio::test]
async fn pending_batch_roundtrip_preserves_order_and_enforces_uniqueness() {
    let fixture = PostgresFixture::start().await;
    let store = fixture.store.clone();
    let mmr_id = next_mmr_id();

    let mut staged_writes = Vec::new();
    for i in 1..=2048u64 {
        staged_writes.push((
            StoreKey::new(mmr_id, KeyKind::NodeHash, i),
            StoreValue::Hash([(i % 251) as u8; 32]),
        ));
    }
    staged_writes.push((
        StoreKey::metadata(mmr_id, KeyKind::ElementsCount),
        StoreValue::U64(2048),
    ));
    staged_writes.push((
        StoreKey::metadata(mmr_id, KeyKind::LeafCount),
        StoreValue::U64(1024),
    ));
    staged_writes.push((
        StoreKey::metadata(mmr_id, KeyKind::RootHash),
        StoreValue::Hash([33u8; 32]),
    ));

    let pending = PendingBatch {
        staged_writes: staged_writes.clone(),
        result: BatchAppendResult {
            appended_count: 1024,
            first_element_index: 1,
            last_element_index: 2048,
            leaves_count: 1024,
            elements_count: 2048,
            root_hash: [33u8; 32],
            peaks_hashes: vec![[9u8; 32], [10u8; 32], [11u8; 32]],
        },
    };

    assert!(!store.has_pending_batch(mmr_id).await.unwrap());
    store
        .create_pending_batch(mmr_id, pending.clone())
        .await
        .unwrap();
    assert!(store.has_pending_batch(mmr_id).await.unwrap());
    assert!(matches!(
        store.create_pending_batch(mmr_id, pending.clone()).await,
        Err(mmr::StoreError::PendingBatchAlreadyExists { .. })
    ));

    let loaded = store.get_pending_batch(mmr_id).await.unwrap().unwrap();
    assert_eq!(loaded.result, pending.result);
    assert_eq!(loaded.staged_writes, pending.staged_writes);

    store.delete_pending_batch(mmr_id).await.unwrap();
    assert!(!store.has_pending_batch(mmr_id).await.unwrap());
    assert!(store.get_pending_batch(mmr_id).await.unwrap().is_none());
}

#[cfg(feature = "postgres-store")]
#[tokio::test]
async fn dropping_store_in_async_context_does_not_panic() {
    let fixture = PostgresFixture::start().await;
    drop(fixture.store);
}
