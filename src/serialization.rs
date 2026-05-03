//! Serialization and encoding helpers.
//!
//! The encoding pipeline is: **codec → compress → encrypt**.
//! The encoding tag stored in the database reflects all active layers:
//!
//! | Tag                     | Codec   | Compress | Encrypt   |
//! |-------------------------|---------|----------|-----------|
//! | `"raw"`                 | bincode | —        | —         |
//! | `"zstd"`                | bincode | zstd     | —         |
//! | `"json"`                | json    | —        | —         |
//! | `"json-zstd"`           | json    | zstd     | —         |
//! | `"raw-aes256gcm"`       | bincode | —        | AES-256   |
//! | `"zstd-aes256gcm"`      | bincode | zstd     | AES-256   |
//! | `"json-aes256gcm"`      | json    | —        | AES-256   |
//! | `"json-zstd-aes256gcm"` | json    | zstd     | AES-256   |

use std::io::Cursor;

use serde::{Serialize, de::DeserializeOwned};

use crate::cache::options::Codec;
use crate::error::LocalFileCacheError;

// ---------------------------------------------------------------------------
// Encryption suffix
// ---------------------------------------------------------------------------

const ENC_SUFFIX: &str = "-aes256gcm";

// ---------------------------------------------------------------------------
// Public encode / decode
// ---------------------------------------------------------------------------

/// Encode `payload` for storage, returning `(bytes, encoding_tag)`.
pub(crate) fn encode_payload<T>(
    payload: &T,
    compress: bool,
    codec: Codec,
    #[cfg(feature = "encryption")] encryption_key: Option<&[u8; 32]>,
) -> Result<(Vec<u8>, &'static str), LocalFileCacheError>
where
    T: Serialize,
{
    let serialized = serialize_with_codec(payload, codec)?;

    // Compression step.
    #[allow(unused_mut)]
    let (mut bytes, mut base_tag): (Vec<u8>, &'static str) = {
        #[cfg(feature = "compression")]
        if compress {
            let compressed = compress_bytes(&serialized)?;
            let tag = match codec {
                Codec::Bincode => "zstd",
                #[cfg(feature = "json")]
                Codec::Json => "json-zstd",
            };
            (compressed, tag)
        } else {
            let tag = match codec {
                Codec::Bincode => "raw",
                #[cfg(feature = "json")]
                Codec::Json => "json",
            };
            (serialized, tag)
        }
        #[cfg(not(feature = "compression"))]
        {
            let _ = compress;
            let tag = match codec {
                Codec::Bincode => "raw",
                #[cfg(feature = "json")]
                Codec::Json => "json",
            };
            (serialized, tag)
        }
    };

    // Encryption step.
    #[cfg(feature = "encryption")]
    if let Some(key) = encryption_key {
        bytes = encrypt_bytes(&bytes, key)?;
        base_tag = match base_tag {
            "raw" => "raw-aes256gcm",
            "zstd" => "zstd-aes256gcm",
            #[cfg(feature = "json")]
            "json" => "json-aes256gcm",
            #[cfg(all(feature = "json", feature = "compression"))]
            "json-zstd" => "json-zstd-aes256gcm",
            other => {
                return Err(LocalFileCacheError::UnsupportedFeature(format!(
                    "no encrypted tag for base tag '{other}'"
                )));
            }
        };
    }

    Ok((bytes, base_tag))
}

/// Decode stored bytes back into `T` given the `encoding` tag.
pub(crate) fn decode_payload<T>(
    bytes: &[u8],
    encoding: &str,
    #[cfg(feature = "encryption")] encryption_key: Option<&[u8; 32]>,
) -> Result<T, LocalFileCacheError>
where
    T: DeserializeOwned,
{
    // Decryption step (if the tag ends with the encryption suffix).
    let (inner_bytes_cow, inner_tag) = if let Some(base) = encoding.strip_suffix(ENC_SUFFIX) {
        #[cfg(feature = "encryption")]
        {
            let key = encryption_key.ok_or_else(|| {
                LocalFileCacheError::UnsupportedFeature(
                    "entry is encrypted but no encryption key was provided".into(),
                )
            })?;
            let decrypted = decrypt_bytes(bytes, key)?;
            (std::borrow::Cow::Owned(decrypted), base)
        }
        #[cfg(not(feature = "encryption"))]
        {
            let _ = (bytes, base);
            return Err(LocalFileCacheError::UnknownEncoding(encoding.to_owned()));
        }
    } else {
        (std::borrow::Cow::Borrowed(bytes), encoding)
    };

    let data: &[u8] = &inner_bytes_cow;

    // Decompression + deserialization.
    match inner_tag {
        "raw" => deserialize_bincode(data),

        #[cfg(feature = "compression")]
        "zstd" => {
            let d = decompress_bytes(data)?;
            deserialize_bincode(&d)
        }

        #[cfg(feature = "json")]
        "json" => deserialize_json(data),

        #[cfg(all(feature = "json", feature = "compression"))]
        "json-zstd" => {
            let d = decompress_bytes(data)?;
            deserialize_json(&d)
        }

        other => Err(LocalFileCacheError::UnknownEncoding(other.to_owned())),
    }
}

// ---------------------------------------------------------------------------
// Codec helpers
// ---------------------------------------------------------------------------

fn serialize_with_codec<T: Serialize>(
    payload: &T,
    codec: Codec,
) -> Result<Vec<u8>, LocalFileCacheError> {
    match codec {
        Codec::Bincode => serialize_bincode(payload),
        #[cfg(feature = "json")]
        Codec::Json => serialize_json(payload),
    }
}

pub(crate) fn serialize_bincode<T: Serialize>(payload: &T) -> Result<Vec<u8>, LocalFileCacheError> {
    let capacity = bincode::serialized_size(payload).unwrap_or(256) as usize;
    let mut buf = Vec::with_capacity(capacity);
    bincode::serialize_into(&mut buf, payload).map_err(LocalFileCacheError::Serialization)?;
    Ok(buf)
}

pub(crate) fn deserialize_bincode<T: DeserializeOwned>(
    bytes: &[u8],
) -> Result<T, LocalFileCacheError> {
    bincode::deserialize_from(Cursor::new(bytes)).map_err(LocalFileCacheError::Serialization)
}

#[cfg(feature = "json")]
fn serialize_json<T: Serialize>(payload: &T) -> Result<Vec<u8>, LocalFileCacheError> {
    serde_json::to_vec(payload).map_err(|e| {
        LocalFileCacheError::UnsupportedFeature(format!("json serialization error: {e}"))
    })
}

#[cfg(feature = "json")]
fn deserialize_json<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, LocalFileCacheError> {
    serde_json::from_slice(bytes).map_err(|e| {
        LocalFileCacheError::UnsupportedFeature(format!("json deserialization error: {e}"))
    })
}

// ---------------------------------------------------------------------------
// Compression
// ---------------------------------------------------------------------------

#[cfg(feature = "compression")]
fn compress_bytes(data: &[u8]) -> Result<Vec<u8>, LocalFileCacheError> {
    zstd::encode_all(Cursor::new(data), 3).map_err(LocalFileCacheError::Io)
}

#[cfg(feature = "compression")]
fn decompress_bytes(data: &[u8]) -> Result<Vec<u8>, LocalFileCacheError> {
    zstd::decode_all(Cursor::new(data)).map_err(LocalFileCacheError::Io)
}

// ---------------------------------------------------------------------------
// Encryption (AES-256-GCM)
// ---------------------------------------------------------------------------

#[cfg(feature = "encryption")]
fn encrypt_bytes(plaintext: &[u8], key: &[u8; 32]) -> Result<Vec<u8>, LocalFileCacheError> {
    use aes_gcm::{
        Aes256Gcm, KeyInit,
        aead::{Aead, AeadCore, OsRng},
    };

    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|e| LocalFileCacheError::EncryptionError(format!("key init failed: {e}")))?;
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let ciphertext = cipher
        .encrypt(&nonce, plaintext)
        .map_err(|e| LocalFileCacheError::EncryptionError(format!("encryption failed: {e}")))?;

    // Store as: nonce (12 bytes) ‖ ciphertext.
    let mut result = Vec::with_capacity(nonce.len() + ciphertext.len());
    result.extend_from_slice(&nonce);
    result.extend_from_slice(&ciphertext);
    Ok(result)
}

#[cfg(feature = "encryption")]
fn decrypt_bytes(data: &[u8], key: &[u8; 32]) -> Result<Vec<u8>, LocalFileCacheError> {
    use aes_gcm::{Aes256Gcm, KeyInit, Nonce, aead::Aead};

    const NONCE_LEN: usize = 12;
    if data.len() < NONCE_LEN {
        return Err(LocalFileCacheError::EncryptionError(
            "ciphertext too short to contain nonce".into(),
        ));
    }

    let (nonce_bytes, ciphertext) = data.split_at(NONCE_LEN);
    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|e| LocalFileCacheError::EncryptionError(format!("key init failed: {e}")))?;
    let nonce = Nonce::from_slice(nonce_bytes);
    cipher.decrypt(nonce, ciphertext).map_err(|_| {
        LocalFileCacheError::EncryptionError(
            "decryption failed — wrong key or corrupted data".into(),
        )
    })
}

// ---------------------------------------------------------------------------
// Key-rotation helpers (pub(crate), feature-gated)
// ---------------------------------------------------------------------------

/// Decrypt `data` that was previously encrypted with `key`.
///
/// This is the same as the internal `decrypt_bytes` but exposed at
/// `pub(crate)` so `CacheEngine::rotate_encryption_key` can use it.
#[cfg(feature = "encryption")]
pub(crate) fn decrypt_for_rotation(
    data: &[u8],
    key: &[u8; 32],
) -> Result<Vec<u8>, crate::error::LocalFileCacheError> {
    decrypt_bytes(data, key)
}

/// Encrypt `plaintext` with `key` for use in key rotation.
#[cfg(feature = "encryption")]
pub(crate) fn encrypt_for_rotation(
    plaintext: &[u8],
    key: &[u8; 32],
) -> Result<Vec<u8>, crate::error::LocalFileCacheError> {
    encrypt_bytes(plaintext, key)
}
