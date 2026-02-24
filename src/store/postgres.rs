use sqlx::postgres::{PgPoolOptions, PgRow};
use sqlx::{PgPool, Postgres, Row, Transaction};

use crate::error::StoreError;
use crate::types::{BatchAppendResult, Hash32, MmrId};

use super::{KeyKind, PendingBatch, Store, StoreKey, StoreValue};

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
        sqlx::query(&self.create_pending_batches_table_sql())
            .execute(&self.pool)
            .await?;
        sqlx::query(&self.create_pending_entries_table_sql())
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

    pub(crate) async fn has_pending_batch_in_tx(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        mmr_id: MmrId,
    ) -> Result<bool, StoreError> {
        let mmr_id_pg = to_pg_mmr_id(mmr_id)?;
        let query = self.has_pending_batch_query();
        let row = sqlx::query(&query)
            .bind(mmr_id_pg)
            .fetch_optional(&mut **tx)
            .await?;
        Ok(row.is_some())
    }

    async fn current_elements_count_in_tx(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        mmr_id: MmrId,
    ) -> Result<u64, StoreError> {
        let key = StoreKey::metadata(mmr_id, KeyKind::ElementsCount);
        let values = self.get_many_in_tx(tx, std::slice::from_ref(&key)).await?;
        match values.into_iter().next().flatten() {
            Some(StoreValue::U64(value)) => Ok(value),
            Some(other) => Err(StoreError::TypeMismatch {
                key,
                expected: "u64",
                actual: other,
            }),
            None => Ok(0),
        }
    }

    fn pending_batches_table_name(&self) -> String {
        format!("{}_pending_batches", self.table_name)
    }

    fn pending_entries_table_name(&self) -> String {
        format!("{}_pending_entries", self.table_name)
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

    fn create_pending_batches_table_sql(&self) -> String {
        format!(
            "CREATE TABLE IF NOT EXISTS {table} (
                mmr_id INT4 PRIMARY KEY,
                appended_count INT8 NOT NULL,
                first_element_index INT8 NOT NULL,
                last_element_index INT8 NOT NULL,
                leaves_count INT8 NOT NULL,
                elements_count INT8 NOT NULL,
                root_hash BYTEA NOT NULL,
                peaks_hashes BYTEA NOT NULL,
                CHECK (octet_length(root_hash) = 32)
            );",
            table = self.pending_batches_table_name()
        )
    }

    fn create_pending_entries_table_sql(&self) -> String {
        format!(
            "CREATE TABLE IF NOT EXISTS {table} (
                mmr_id INT4 NOT NULL REFERENCES {batches}(mmr_id) ON DELETE CASCADE,
                ord INT4 NOT NULL,
                kind INT2 NOT NULL,
                idx INT8 NOT NULL,
                value BYTEA NOT NULL,
                PRIMARY KEY (mmr_id, ord),
                CHECK (kind BETWEEN 0 AND 3),
                CHECK (
                    (kind IN (0, 1) AND octet_length(value) = 8)
                    OR
                    (kind IN (2, 3) AND octet_length(value) = 32)
                )
            );",
            table = self.pending_entries_table_name(),
            batches = self.pending_batches_table_name()
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

    fn has_pending_batch_query(&self) -> String {
        format!(
            "SELECT 1 FROM {} WHERE mmr_id = $1",
            self.pending_batches_table_name()
        )
    }

    fn insert_pending_batch_query(&self) -> String {
        format!(
            "INSERT INTO {table} (
                mmr_id,
                appended_count,
                first_element_index,
                last_element_index,
                leaves_count,
                elements_count,
                root_hash,
                peaks_hashes
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            ON CONFLICT (mmr_id)
            DO UPDATE SET
                appended_count = EXCLUDED.appended_count,
                first_element_index = EXCLUDED.first_element_index,
                last_element_index = EXCLUDED.last_element_index,
                leaves_count = EXCLUDED.leaves_count,
                elements_count = EXCLUDED.elements_count,
                root_hash = EXCLUDED.root_hash,
                peaks_hashes = EXCLUDED.peaks_hashes",
            table = self.pending_batches_table_name()
        )
    }

    fn select_pending_batch_for_update_query(&self) -> String {
        format!(
            "SELECT
                appended_count,
                first_element_index,
                last_element_index,
                leaves_count,
                elements_count,
                root_hash,
                peaks_hashes
             FROM {}
             WHERE mmr_id = $1
             FOR UPDATE",
            self.pending_batches_table_name()
        )
    }

    fn delete_pending_entries_query(&self) -> String {
        format!(
            "DELETE FROM {} WHERE mmr_id = $1",
            self.pending_entries_table_name()
        )
    }

    fn insert_pending_entries_query(&self) -> String {
        format!(
            "WITH input AS (
                SELECT *
                FROM unnest($1::int4[], $2::int4[], $3::int2[], $4::int8[], $5::bytea[])
                AS t(mmr_id, ord, kind, idx, value)
            )
            INSERT INTO {table} (mmr_id, ord, kind, idx, value)
            SELECT mmr_id, ord, kind, idx, value FROM input
            ON CONFLICT (mmr_id, ord)
            DO UPDATE SET
                kind = EXCLUDED.kind,
                idx = EXCLUDED.idx,
                value = EXCLUDED.value",
            table = self.pending_entries_table_name()
        )
    }

    fn get_pending_entries_query(&self) -> String {
        format!(
            "SELECT kind, idx, value
             FROM {}
             WHERE mmr_id = $1
             ORDER BY ord",
            self.pending_entries_table_name()
        )
    }

    fn delete_pending_batch_query(&self) -> String {
        format!(
            "DELETE FROM {} WHERE mmr_id = $1",
            self.pending_batches_table_name()
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

    async fn create_pending_batch(
        &self,
        mmr_id: MmrId,
        batch: PendingBatch,
    ) -> Result<(), StoreError> {
        let expected_elements_count = expected_elements_count(&batch.result)?;
        let mmr_id_pg = to_pg_mmr_id(mmr_id)?;
        let mut tx = self.pool.begin().await?;

        let actual_elements_count = self.current_elements_count_in_tx(&mut tx, mmr_id).await?;
        if expected_elements_count != actual_elements_count {
            tx.rollback().await?;
            return Err(StoreError::PendingBatchBaseMismatch {
                mmr_id,
                expected_elements_count,
                actual_elements_count,
            });
        }

        let result = &batch.result;
        let insert_batch_query = self.insert_pending_batch_query();
        sqlx::query(&insert_batch_query)
            .bind(mmr_id_pg)
            .bind(to_pg_idx(result.appended_count)?)
            .bind(to_pg_idx(result.first_element_index)?)
            .bind(to_pg_idx(result.last_element_index)?)
            .bind(to_pg_idx(result.leaves_count)?)
            .bind(to_pg_idx(result.elements_count)?)
            .bind(result.root_hash.to_vec())
            .bind(encode_peaks_hashes(&result.peaks_hashes))
            .execute(&mut *tx)
            .await?;

        let delete_entries_query = self.delete_pending_entries_query();
        sqlx::query(&delete_entries_query)
            .bind(mmr_id_pg)
            .execute(&mut *tx)
            .await?;

        if !batch.staged_writes.is_empty() {
            let (mmr_ids, ords, kinds, indices, values) =
                prepare_pending_entries(mmr_id, batch.staged_writes)?;
            let insert_entries_query = self.insert_pending_entries_query();
            sqlx::query(&insert_entries_query)
                .bind(&mmr_ids)
                .bind(&ords)
                .bind(&kinds)
                .bind(&indices)
                .bind(&values)
                .execute(&mut *tx)
                .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    async fn has_pending_batch(&self, mmr_id: MmrId) -> Result<bool, StoreError> {
        let mmr_id_pg = to_pg_mmr_id(mmr_id)?;
        let query = self.has_pending_batch_query();
        let row = sqlx::query(&query)
            .bind(mmr_id_pg)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.is_some())
    }

    async fn commit_pending_batch(
        &self,
        mmr_id: MmrId,
    ) -> Result<Option<BatchAppendResult>, StoreError> {
        let mmr_id_pg = to_pg_mmr_id(mmr_id)?;
        let mut tx = self.pool.begin().await?;

        let select_batch_query = self.select_pending_batch_for_update_query();
        let batch_row = sqlx::query(&select_batch_query)
            .bind(mmr_id_pg)
            .fetch_optional(&mut *tx)
            .await?;

        let Some(batch_row) = batch_row else {
            tx.rollback().await?;
            return Ok(None);
        };

        let result = decode_pending_batch_result(&batch_row)?;
        let expected_elements_count = expected_elements_count(&result)?;
        let actual_elements_count = self.current_elements_count_in_tx(&mut tx, mmr_id).await?;
        if expected_elements_count != actual_elements_count {
            tx.rollback().await?;
            return Err(StoreError::PendingBatchBaseMismatch {
                mmr_id,
                expected_elements_count,
                actual_elements_count,
            });
        }

        let entries_query = self.get_pending_entries_query();
        let entry_rows = sqlx::query(&entries_query)
            .bind(mmr_id_pg)
            .fetch_all(&mut *tx)
            .await?;

        let mut staged_writes = Vec::with_capacity(entry_rows.len());
        for row in entry_rows {
            let kind = kind_from_i16(row.try_get::<i16, _>("kind")?)?;
            let idx = to_u64(row.try_get::<i64, _>("idx")?, "idx")?;
            let key = StoreKey::new(mmr_id, kind, idx);
            let value_bytes: Vec<u8> = row.try_get("value")?;
            let value = decode_store_value(&key, &value_bytes)?;
            staged_writes.push((key, value));
        }

        self.set_many_in_tx(&mut tx, staged_writes).await?;

        let delete_batch_query = self.delete_pending_batch_query();
        sqlx::query(&delete_batch_query)
            .bind(mmr_id_pg)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;
        Ok(Some(result))
    }

    async fn delete_pending_batch_if_exists(&self, mmr_id: MmrId) -> Result<bool, StoreError> {
        let mmr_id_pg = to_pg_mmr_id(mmr_id)?;
        let query = self.delete_pending_batch_query();
        let result = sqlx::query(&query)
            .bind(mmr_id_pg)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
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

fn prepare_pending_entries(
    mmr_id: MmrId,
    entries: Vec<(StoreKey, StoreValue)>,
) -> Result<(Vec<i32>, Vec<i32>, Vec<i16>, Vec<i64>, Vec<Vec<u8>>), StoreError> {
    let mut mmr_ids = Vec::with_capacity(entries.len());
    let mut ords = Vec::with_capacity(entries.len());
    let mut kinds = Vec::with_capacity(entries.len());
    let mut indices = Vec::with_capacity(entries.len());
    let mut values = Vec::with_capacity(entries.len());

    let mmr_id_pg = to_pg_mmr_id(mmr_id)?;

    for (ord, (key, value)) in entries.into_iter().enumerate() {
        mmr_ids.push(mmr_id_pg);
        ords.push(i32::try_from(ord).map_err(|_| {
            StoreError::Internal(format!("pending entries ord out of i32 range: {ord}"))
        })?);
        kinds.push(kind_to_i16(key.kind));
        indices.push(to_pg_idx(key.index)?);
        values.push(encode_store_value(&key, &value)?);
    }

    Ok((mmr_ids, ords, kinds, indices, values))
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

fn kind_from_i16(kind: i16) -> Result<KeyKind, StoreError> {
    match kind {
        0 => Ok(KeyKind::LeafCount),
        1 => Ok(KeyKind::ElementsCount),
        2 => Ok(KeyKind::RootHash),
        3 => Ok(KeyKind::NodeHash),
        _ => Err(StoreError::Internal(format!(
            "invalid key kind returned by postgres: {kind}"
        ))),
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

fn to_u64(value: i64, field: &str) -> Result<u64, StoreError> {
    u64::try_from(value).map_err(|_| StoreError::Internal(format!("{field} is negative: {value}")))
}

fn expected_elements_count(result: &BatchAppendResult) -> Result<u64, StoreError> {
    result.first_element_index.checked_sub(1).ok_or_else(|| {
        StoreError::Internal(
            "pending batch has invalid first_element_index 0 while deriving expected base"
                .to_string(),
        )
    })
}

fn decode_pending_batch_result(row: &PgRow) -> Result<BatchAppendResult, StoreError> {
    let appended_count = to_u64(row.try_get::<i64, _>("appended_count")?, "appended_count")?;
    let first_element_index = to_u64(
        row.try_get::<i64, _>("first_element_index")?,
        "first_element_index",
    )?;
    let last_element_index = to_u64(
        row.try_get::<i64, _>("last_element_index")?,
        "last_element_index",
    )?;
    let leaves_count = to_u64(row.try_get::<i64, _>("leaves_count")?, "leaves_count")?;
    let elements_count = to_u64(row.try_get::<i64, _>("elements_count")?, "elements_count")?;

    let root_hash_bytes: Vec<u8> = row.try_get("root_hash")?;
    if root_hash_bytes.len() != 32 {
        return Err(StoreError::Internal(format!(
            "expected 32 bytes for pending root_hash, got {}",
            root_hash_bytes.len()
        )));
    }
    let mut root_hash = [0u8; 32];
    root_hash.copy_from_slice(&root_hash_bytes);

    let peaks_hashes_bytes: Vec<u8> = row.try_get("peaks_hashes")?;
    let peaks_hashes = decode_peaks_hashes(&peaks_hashes_bytes)?;

    Ok(BatchAppendResult {
        appended_count,
        first_element_index,
        last_element_index,
        leaves_count,
        elements_count,
        root_hash,
        peaks_hashes,
    })
}

fn encode_peaks_hashes(peaks_hashes: &[Hash32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(peaks_hashes.len() * 32);
    for peak in peaks_hashes {
        bytes.extend_from_slice(peak);
    }
    bytes
}

fn decode_peaks_hashes(bytes: &[u8]) -> Result<Vec<Hash32>, StoreError> {
    if bytes.len() % 32 != 0 {
        return Err(StoreError::Internal(format!(
            "expected peaks_hashes byte length multiple of 32, got {}",
            bytes.len()
        )));
    }

    let mut peaks_hashes = Vec::with_capacity(bytes.len() / 32);
    for chunk in bytes.chunks_exact(32) {
        let mut hash = [0u8; 32];
        hash.copy_from_slice(chunk);
        peaks_hashes.push(hash);
    }
    Ok(peaks_hashes)
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
