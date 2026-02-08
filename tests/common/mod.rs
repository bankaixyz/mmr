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
