use sqlx::postgres::{PgPoolOptions, PgRow};
use sqlx::{PgPool, Postgres, Row, Transaction};

use crate::error::StoreError;

use super::{KeyKind, Store, StoreKey, StoreValue};

const DEFAULT_TABLE_NAME: &str = "mmr_nodes";
const DEFAULT_MAX_CONNECTIONS: u32 = 20;

#[derive(Debug, Clone, Copy)]
pub struct PostgresStoreOptions {
    pub initialize_schema: bool,
    pub max_connections: u32,
}

impl Default for PostgresStoreOptions {
    fn default() -> Self {
        Self {
            initialize_schema: true,
            max_connections: DEFAULT_MAX_CONNECTIONS,
        }
    }
}

pub struct PostgresStore {
    pool: PgPool,
    table_name: String,
}

impl std::fmt::Debug for PostgresStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PostgresStore")
            .field("table_name", &self.table_name)
            .finish()
    }
}

impl PostgresStore {
    pub async fn connect(connection_string: &str) -> Result<Self, StoreError> {
        Self::connect_with_options(connection_string, PostgresStoreOptions::default()).await
    }

    pub async fn connect_with_options(
        connection_string: &str,
        options: PostgresStoreOptions,
    ) -> Result<Self, StoreError> {
        let pool = PgPoolOptions::new()
            .max_connections(options.max_connections)
            .connect(connection_string)
            .await?;

        let store = Self {
            pool,
            table_name: DEFAULT_TABLE_NAME.to_string(),
        };

        if options.initialize_schema {
            store.init_schema().await?;
        }

        Ok(store)
    }

    pub async fn init_schema(&self) -> Result<(), StoreError> {
        sqlx::query(&self.create_table_sql())
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    pub async fn begin_write_tx(&self) -> Result<Transaction<'_, Postgres>, StoreError> {
        self.pool.begin().await.map_err(StoreError::from)
    }

    pub(crate) async fn set_many_in_tx(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        entries: Vec<(StoreKey, StoreValue)>,
    ) -> Result<(), StoreError> {
        if entries.is_empty() {
            return Ok(());
        }

        let (mmr_ids, kinds, indices, values) = prepare_entries(entries)?;
        let query = self.set_many_query();

        sqlx::query(&query)
            .bind(&mmr_ids)
            .bind(&kinds)
            .bind(&indices)
            .bind(&values)
            .execute(&mut **tx)
            .await?;

        Ok(())
    }

    pub(crate) async fn get_many_in_tx(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        keys: &[StoreKey],
    ) -> Result<Vec<Option<StoreValue>>, StoreError> {
        if keys.is_empty() {
            return Ok(Vec::new());
        }

        let (mmr_ids, kinds, indices) = prepare_keys(keys)?;
        let query = self.get_many_query();

        let rows = sqlx::query(&query)
            .bind(&mmr_ids)
            .bind(&kinds)
            .bind(&indices)
            .fetch_all(&mut **tx)
            .await?;

        decode_many_values(keys, rows)
    }

    fn create_table_sql(&self) -> String {
        format!(
            "CREATE TABLE IF NOT EXISTS {table} (
                mmr_id INT4 NOT NULL,
                kind INT2 NOT NULL,
                idx INT8 NOT NULL,
                value BYTEA NOT NULL,
                PRIMARY KEY (mmr_id, kind, idx),
                CHECK (kind BETWEEN 0 AND 3),
                CHECK (
                    (kind IN (0, 1) AND octet_length(value) = 8)
                    OR
                    (kind IN (2, 3) AND octet_length(value) = 32)
                )
            );",
            table = self.table_name
        )
    }

    fn get_query(&self) -> String {
        format!(
            "SELECT value FROM {} WHERE mmr_id = $1 AND kind = $2 AND idx = $3",
            self.table_name
        )
    }

    fn set_query(&self) -> String {
        format!(
            "INSERT INTO {} (mmr_id, kind, idx, value)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (mmr_id, kind, idx) DO UPDATE SET value = EXCLUDED.value",
            self.table_name
        )
    }

    fn set_many_query(&self) -> String {
        format!(
            "WITH input AS (
                SELECT *
                FROM unnest($1::int4[], $2::int2[], $3::int8[], $4::bytea[])
                AS t(mmr_id, kind, idx, value)
            )
            INSERT INTO {table} (mmr_id, kind, idx, value)
            SELECT mmr_id, kind, idx, value FROM input
            ON CONFLICT (mmr_id, kind, idx) DO UPDATE SET value = EXCLUDED.value",
            table = self.table_name
        )
    }

    fn get_many_query(&self) -> String {
        format!(
            "WITH requested AS (
                SELECT *
                FROM unnest($1::int4[], $2::int2[], $3::int8[])
                WITH ORDINALITY AS req(mmr_id, kind, idx, ord)
            )
            SELECT req.ord, store.value
            FROM requested req
            LEFT JOIN {table} store
                ON store.mmr_id = req.mmr_id
               AND store.kind = req.kind
               AND store.idx = req.idx
            ORDER BY req.ord",
            table = self.table_name
        )
    }
}

impl Store for PostgresStore {
    async fn get(&self, key: &StoreKey) -> Result<Option<StoreValue>, StoreError> {
        let mmr_id = to_pg_mmr_id(key.mmr_id)?;
        let kind = kind_to_i16(key.kind);
        let idx = to_pg_idx(key.index)?;
        let query = self.get_query();

        let row = sqlx::query(&query)
            .bind(mmr_id)
            .bind(kind)
            .bind(idx)
            .fetch_optional(&self.pool)
            .await?;

        match row {
            Some(row) => {
                let value: Vec<u8> = row.try_get("value")?;
                decode_store_value(key, &value).map(Some)
            }
            None => Ok(None),
        }
    }

    async fn set(&self, key: StoreKey, value: StoreValue) -> Result<(), StoreError> {
        let mmr_id = to_pg_mmr_id(key.mmr_id)?;
        let kind = kind_to_i16(key.kind);
        let idx = to_pg_idx(key.index)?;
        let query = self.set_query();
        let encoded = encode_store_value(&key, &value)?;

        sqlx::query(&query)
            .bind(mmr_id)
            .bind(kind)
            .bind(idx)
            .bind(encoded)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    async fn set_many(&self, entries: Vec<(StoreKey, StoreValue)>) -> Result<(), StoreError> {
        if entries.is_empty() {
            return Ok(());
        }

        let (mmr_ids, kinds, indices, values) = prepare_entries(entries)?;
        let query = self.set_many_query();

        sqlx::query(&query)
            .bind(&mmr_ids)
            .bind(&kinds)
            .bind(&indices)
            .bind(&values)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    async fn get_many(&self, keys: &[StoreKey]) -> Result<Vec<Option<StoreValue>>, StoreError> {
        if keys.is_empty() {
            return Ok(Vec::new());
        }

        let (mmr_ids, kinds, indices) = prepare_keys(keys)?;
        let query = self.get_many_query();

        let rows = sqlx::query(&query)
            .bind(&mmr_ids)
            .bind(&kinds)
            .bind(&indices)
            .fetch_all(&self.pool)
            .await?;

        decode_many_values(keys, rows)
    }
}

fn prepare_entries(
    entries: Vec<(StoreKey, StoreValue)>,
) -> Result<(Vec<i32>, Vec<i16>, Vec<i64>, Vec<Vec<u8>>), StoreError> {
    let mut mmr_ids = Vec::with_capacity(entries.len());
    let mut kinds = Vec::with_capacity(entries.len());
    let mut indices = Vec::with_capacity(entries.len());
    let mut values = Vec::with_capacity(entries.len());

    for (key, value) in entries {
        mmr_ids.push(to_pg_mmr_id(key.mmr_id)?);
        kinds.push(kind_to_i16(key.kind));
        indices.push(to_pg_idx(key.index)?);
        values.push(encode_store_value(&key, &value)?);
    }

    Ok((mmr_ids, kinds, indices, values))
}

fn prepare_keys(keys: &[StoreKey]) -> Result<(Vec<i32>, Vec<i16>, Vec<i64>), StoreError> {
    let mut mmr_ids = Vec::with_capacity(keys.len());
    let mut kinds = Vec::with_capacity(keys.len());
    let mut indices = Vec::with_capacity(keys.len());

    for key in keys {
        mmr_ids.push(to_pg_mmr_id(key.mmr_id)?);
        kinds.push(kind_to_i16(key.kind));
        indices.push(to_pg_idx(key.index)?);
    }

    Ok((mmr_ids, kinds, indices))
}

fn decode_many_values(
    keys: &[StoreKey],
    rows: Vec<PgRow>,
) -> Result<Vec<Option<StoreValue>>, StoreError> {
    let mut out = vec![None; keys.len()];
    for row in rows {
        let ord: i64 = row.try_get("ord")?;
        let position = usize::try_from(ord - 1).map_err(|_| {
            StoreError::Internal(format!("invalid ordinality returned by postgres: {ord}"))
        })?;
        let maybe_value: Option<Vec<u8>> = row.try_get("value")?;
        if let Some(value) = maybe_value {
            out[position] = Some(decode_store_value(&keys[position], &value)?);
        }
    }

    Ok(out)
}

fn kind_to_i16(kind: KeyKind) -> i16 {
    match kind {
        KeyKind::LeafCount => 0,
        KeyKind::ElementsCount => 1,
        KeyKind::RootHash => 2,
        KeyKind::NodeHash => 3,
    }
}

fn to_pg_mmr_id(mmr_id: u32) -> Result<i32, StoreError> {
    i32::try_from(mmr_id)
        .map_err(|_| StoreError::Internal(format!("mmr_id out of i32 range: {mmr_id}")))
}

fn to_pg_idx(index: u64) -> Result<i64, StoreError> {
    i64::try_from(index)
        .map_err(|_| StoreError::Internal(format!("index out of i64 range: {index}")))
}

fn encode_store_value(key: &StoreKey, value: &StoreValue) -> Result<Vec<u8>, StoreError> {
    match (key.kind, value) {
        (KeyKind::LeafCount | KeyKind::ElementsCount, StoreValue::U64(raw)) => {
            Ok(raw.to_be_bytes().to_vec())
        }
        (KeyKind::RootHash | KeyKind::NodeHash, StoreValue::Hash(hash)) => Ok(hash.to_vec()),
        _ => Err(StoreError::TypeMismatch {
            key: key.clone(),
            expected: expected_type_for_kind(key.kind),
            actual: value.clone(),
        }),
    }
}

fn decode_store_value(key: &StoreKey, bytes: &[u8]) -> Result<StoreValue, StoreError> {
    match key.kind {
        KeyKind::LeafCount | KeyKind::ElementsCount => {
            if bytes.len() != 8 {
                return Err(StoreError::Internal(format!(
                    "expected 8 bytes for {:?}, got {}",
                    key.kind,
                    bytes.len()
                )));
            }
            let mut out = [0u8; 8];
            out.copy_from_slice(bytes);
            Ok(StoreValue::U64(u64::from_be_bytes(out)))
        }
        KeyKind::RootHash | KeyKind::NodeHash => {
            if bytes.len() != 32 {
                return Err(StoreError::Internal(format!(
                    "expected 32 bytes for {:?}, got {}",
                    key.kind,
                    bytes.len()
                )));
            }
            let mut out = [0u8; 32];
            out.copy_from_slice(bytes);
            Ok(StoreValue::Hash(out))
        }
    }
}

fn expected_type_for_kind(kind: KeyKind) -> &'static str {
    match kind {
        KeyKind::LeafCount | KeyKind::ElementsCount => "u64",
        KeyKind::RootHash | KeyKind::NodeHash => "hash32",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn value_encoding_for_node_hash_is_compact() {
        let key = StoreKey::new(1, KeyKind::NodeHash, 42);
        let value = StoreValue::Hash([9u8; 32]);
        let encoded = encode_store_value(&key, &value).unwrap();
        assert_eq!(encoded.len(), 32);
    }

    #[test]
    fn value_encoding_for_counter_is_compact() {
        let key = StoreKey::metadata(1, KeyKind::LeafCount);
        let value = StoreValue::U64(7);
        let encoded = encode_store_value(&key, &value).unwrap();
        assert_eq!(encoded.len(), 8);
    }

    #[tokio::test]
    async fn set_many_roundtrip_works_when_database_url_is_available() {
        let database_url = match std::env::var("DATABASE_URL") {
            Ok(url) => url,
            Err(_) => return,
        };

        let store = PostgresStore::connect_with_options(
            &database_url,
            PostgresStoreOptions {
                initialize_schema: true,
                max_connections: 2,
            },
        )
        .await
        .unwrap();

        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;
        let mmr_id = ((nonce % ((i32::MAX as u64) - 10_000)) as u32) + 10_000;
        let node_index = nonce;

        let keys = vec![
            StoreKey::metadata(mmr_id, KeyKind::LeafCount),
            StoreKey::new(mmr_id, KeyKind::NodeHash, node_index),
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
                .expect_hash(&StoreKey::new(mmr_id, KeyKind::NodeHash, node_index))
                .unwrap(),
            [7u8; 32]
        );
    }

    #[tokio::test]
    async fn dropping_store_in_async_context_does_not_panic() {
        let database_url = match std::env::var("DATABASE_URL") {
            Ok(url) => url,
            Err(_) => return,
        };

        let store = PostgresStore::connect_with_options(
            &database_url,
            PostgresStoreOptions {
                initialize_schema: true,
                max_connections: 1,
            },
        )
        .await
        .unwrap();

        drop(store);
    }
}
