#![allow(dead_code)]

use mmr::error::HasherError;
use mmr::types::Hash32;

pub fn hash_to_hex(hash: &Hash32) -> String {
    format!("0x{}", hex::encode(hash))
}

pub fn hash_from_hex(value: &str) -> Result<Hash32, HasherError> {
    let raw = value.strip_prefix("0x").unwrap_or(value);

    if raw.is_empty() {
        return Ok([0u8; 32]);
    }

    let normalized = if raw.len() % 2 == 1 {
        format!("0{raw}")
    } else {
        raw.to_string()
    };

    let bytes = hex::decode(&normalized).map_err(|source| HasherError::InvalidHex {
        value: value.to_string(),
        source,
    })?;

    if bytes.len() > 32 {
        return Err(HasherError::InputTooLarge {
            value: value.to_string(),
            max_bytes: 32,
        });
    }

    let mut out = [0u8; 32];
    let start = 32 - bytes.len();
    out[start..].copy_from_slice(&bytes);
    Ok(out)
}

#[cfg(feature = "postgres-store")]
pub mod pg {
    use std::sync::Arc;
    use std::sync::OnceLock;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::Duration;
    use std::time::{SystemTime, UNIX_EPOCH};

    use mmr::{PostgresStore, PostgresStoreOptions};
    use testcontainers::core::{IntoContainerPort, WaitFor};
    use testcontainers::runners::AsyncRunner;
    use testcontainers::{ContainerAsync, GenericImage, ImageExt};

    fn mmr_seed() -> u32 {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos() as u64;
        ((now % ((i32::MAX as u64) - 100_000)) as u32) + 100_000
    }

    pub fn next_mmr_id() -> u32 {
        static NEXT: OnceLock<AtomicU32> = OnceLock::new();
        NEXT.get_or_init(|| AtomicU32::new(mmr_seed()))
            .fetch_add(1, Ordering::Relaxed)
    }

    pub struct PostgresFixture {
        pub store: Arc<PostgresStore>,
        _container: ContainerAsync<GenericImage>,
    }

    impl PostgresFixture {
        pub async fn start() -> Self {
            let mut last_error = None;
            for _ in 0..3 {
                let start = tokio::time::timeout(Duration::from_secs(45), async {
                    GenericImage::new("postgres", "16-alpine")
                        .with_exposed_port(5432.tcp())
                        .with_wait_for(WaitFor::message_on_stdout(
                            "database system is ready to accept connections",
                        ))
                        .with_env_var("POSTGRES_PASSWORD", "postgres")
                        .with_env_var("POSTGRES_USER", "postgres")
                        .with_env_var("POSTGRES_DB", "postgres")
                        .start()
                        .await
                })
                .await;
                let container = match start {
                    Ok(Ok(container)) => container,
                    Ok(Err(error)) => {
                        last_error =
                            Some(format!("failed to start postgres test container: {error}"));
                        continue;
                    }
                    Err(_) => {
                        last_error = Some("timed out starting postgres container".to_string());
                        continue;
                    }
                };

                let host = match container.get_host().await {
                    Ok(host) => host,
                    Err(error) => {
                        last_error = Some(format!(
                            "failed to resolve postgres container host: {error}"
                        ));
                        continue;
                    }
                };
                let port = match container.get_host_port_ipv4(5432.tcp()).await {
                    Ok(port) => port,
                    Err(error) => {
                        last_error =
                            Some(format!("failed to resolve postgres mapped port: {error}"));
                        continue;
                    }
                };

                let connection_string =
                    format!("postgres://postgres:postgres@{host}:{port}/postgres");
                let connect_deadline = tokio::time::Instant::now() + Duration::from_secs(30);
                let store = loop {
                    match PostgresStore::connect_with_options(
                        &connection_string,
                        PostgresStoreOptions {
                            initialize_schema: true,
                            max_connections: 2,
                        },
                    )
                    .await
                    {
                        Ok(store) => break Some(store),
                        Err(error) => {
                            if tokio::time::Instant::now() >= connect_deadline {
                                last_error = Some(format!(
                                    "timed out connecting to postgres container: {error}"
                                ));
                                break None;
                            }
                            tokio::time::sleep(Duration::from_millis(500)).await;
                        }
                    }
                };

                if let Some(store) = store {
                    return Self {
                        store: Arc::new(store),
                        _container: container,
                    };
                }
            }

            panic!(
                "{}",
                last_error.unwrap_or_else(|| "failed to start postgres fixture".to_string())
            );
        }
    }
}
