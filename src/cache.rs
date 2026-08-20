//! Local cache of the last HTTP configuration payload.

use std::fs;
use std::path::PathBuf;

use crate::error::Error;
use crate::options::CacheOptions;

pub(crate) fn cache_path(cache: &CacheOptions, app_id: &str) -> PathBuf {
    cache
        .directory
        .join(format!("{app_id}.agileconfig.client.configs.cache"))
}

pub(crate) fn write_cache(
    cache: &CacheOptions,
    app_id: &str,
    secret: &str,
    json: &str,
) -> Result<(), Error> {
    if !cache.enabled || json.is_empty() {
        return Ok(());
    }
    ensure_encrypt_available(cache.encrypt)?;
    if !cache.directory.as_os_str().is_empty() {
        fs::create_dir_all(&cache.directory).map_err(Error::cache)?;
    }
    let body = encode_body(cache.encrypt, secret, json)?;
    fs::write(cache_path(cache, app_id), body).map_err(Error::cache)?;
    Ok(())
}

pub(crate) fn read_cache(
    cache: &CacheOptions,
    app_id: &str,
    secret: &str,
) -> Result<Option<String>, Error> {
    if !cache.enabled {
        return Ok(None);
    }
    ensure_encrypt_available(cache.encrypt)?;
    let path = cache_path(cache, app_id);
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&path).map_err(Error::cache)?;
    if raw.is_empty() {
        return Ok(None);
    }
    decode_body(cache.encrypt, secret, &raw).map(Some)
}

fn ensure_encrypt_available(encrypt: bool) -> Result<(), Error> {
    if encrypt && cfg!(not(feature = "cache-encrypt")) {
        return Err(Error::CacheEncryptDisabled);
    }
    Ok(())
}

#[allow(clippy::unnecessary_wraps)]
fn encode_body(encrypt: bool, secret: &str, json: &str) -> Result<String, Error> {
    if !encrypt {
        return Ok(json.to_string());
    }
    #[cfg(feature = "cache-encrypt")]
    {
        Ok(crypto::encrypt(secret, json))
    }
    #[cfg(not(feature = "cache-encrypt"))]
    {
        let _ = secret;
        Err(Error::CacheEncryptDisabled)
    }
}

fn decode_body(encrypt: bool, secret: &str, raw: &str) -> Result<String, Error> {
    if !encrypt {
        return Ok(raw.to_string());
    }
    #[cfg(feature = "cache-encrypt")]
    {
        crypto::decrypt(secret, raw)
    }
    #[cfg(not(feature = "cache-encrypt"))]
    {
        let _ = secret;
        Err(Error::CacheEncryptDisabled)
    }
}

#[cfg(feature = "cache-encrypt")]
mod crypto {
    use aes::Aes128;
    use base64::Engine as _;
    use cipher::{BlockModeDecrypt, BlockModeEncrypt, KeyInit, block_padding::Pkcs7};
    use ecb::{Decryptor, Encryptor};
    use sha1::{Digest, Sha1};

    use crate::error::Error;

    pub(super) fn encrypt(secret: &str, plaintext: &str) -> String {
        let key = aes_key(secret);
        let encryptor = Encryptor::<Aes128>::new(&key.into());
        let ciphertext = encryptor.encrypt_padded_vec::<Pkcs7>(plaintext.as_bytes());
        base64::engine::general_purpose::STANDARD.encode(ciphertext)
    }

    pub(super) fn decrypt(secret: &str, payload: &str) -> Result<String, Error> {
        let key = aes_key(secret);
        let ciphertext = base64::engine::general_purpose::STANDARD
            .decode(payload.trim())
            .map_err(Error::cache)?;
        let decryptor = Decryptor::<Aes128>::new(&key.into());
        let plaintext = decryptor
            .decrypt_padded_vec::<Pkcs7>(&ciphertext)
            .map_err(Error::cache)?;
        String::from_utf8(plaintext).map_err(Error::cache)
    }

    fn aes_key(secret: &str) -> [u8; 16] {
        let first = Sha1::digest(secret.as_bytes());
        let second = Sha1::digest(first);
        let mut key = [0_u8; 16];
        key.copy_from_slice(&second[..16]);
        key
    }
}

#[cfg(test)]
mod tests {
    use super::{read_cache, write_cache};
    use crate::options::CacheOptions;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn write_and_read_plain_cache() {
        let dir = TempDir::new().unwrap();
        let cache = CacheOptions {
            enabled: true,
            directory: dir.path().to_path_buf(),
            encrypt: false,
        };
        write_cache(&cache, "app", "secret", "[1]").unwrap();
        assert_eq!(
            read_cache(&cache, "app", "secret").unwrap().as_deref(),
            Some("[1]")
        );
        let stored =
            fs::read_to_string(dir.path().join("app.agileconfig.client.configs.cache")).unwrap();
        assert_eq!(stored, "[1]");
    }

    #[cfg(feature = "cache-encrypt")]
    #[test]
    fn encrypt_roundtrip_matches_c_sharp_key_derivation() {
        let secret = "test-secret";
        let json = r#"[{"key":"a","value":"b","group":""}]"#;
        let encoded = super::crypto::encrypt(secret, json);
        assert_eq!(super::crypto::decrypt(secret, &encoded).unwrap(), json);
    }

    #[cfg(feature = "cache-encrypt")]
    #[test]
    fn write_and_read_encrypted_cache() {
        let dir = TempDir::new().unwrap();
        let cache = CacheOptions {
            enabled: true,
            directory: dir.path().to_path_buf(),
            encrypt: true,
        };
        write_cache(&cache, "app", "secret", "[1]").unwrap();
        assert_eq!(
            read_cache(&cache, "app", "secret").unwrap().as_deref(),
            Some("[1]")
        );
        let stored =
            fs::read_to_string(dir.path().join("app.agileconfig.client.configs.cache")).unwrap();
        assert_ne!(stored, "[1]");
    }

    #[cfg(not(feature = "cache-encrypt"))]
    #[test]
    fn encrypt_without_feature_is_rejected() {
        let dir = TempDir::new().unwrap();
        let cache = CacheOptions {
            enabled: true,
            directory: dir.path().to_path_buf(),
            encrypt: true,
        };
        let error = write_cache(&cache, "app", "secret", "[1]").unwrap_err();
        assert_eq!(
            error.to_string(),
            "cache encryption requires the `cache-encrypt` cargo feature"
        );
    }

    #[test]
    fn disabled_cache_skips_io() {
        let dir = TempDir::new().unwrap();
        let cache = CacheOptions {
            enabled: false,
            directory: dir.path().to_path_buf(),
            encrypt: false,
        };
        write_cache(&cache, "app", "secret", "[1]").unwrap();
        assert!(read_cache(&cache, "app", "secret").unwrap().is_none());
        assert!(dir.path().read_dir().unwrap().next().is_none());
    }
}
