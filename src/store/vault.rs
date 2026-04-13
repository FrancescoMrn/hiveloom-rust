use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use rand::RngCore;
use std::path::Path;

pub struct Vault {
    cipher: Aes256Gcm,
}

const NONCE_LEN: usize = 12;
const KEY_LEN: usize = 32;
const MASTER_KEY_FILE: &str = "master.key";

impl Vault {
    /// Load or generate master key from `<data_dir>/master.key`.
    ///
    /// If the file does not exist, 32 random bytes are generated and written
    /// with `0600` permissions (owner read/write only).
    pub fn open(data_dir: &Path) -> anyhow::Result<Self> {
        let key_path = data_dir.join(MASTER_KEY_FILE);
        let key_bytes = if key_path.exists() {
            let bytes = std::fs::read(&key_path)?;
            if bytes.len() != KEY_LEN {
                anyhow::bail!(
                    "master.key has invalid length {} (expected {})",
                    bytes.len(),
                    KEY_LEN
                );
            }
            bytes
        } else {
            // Ensure directory exists
            std::fs::create_dir_all(data_dir)?;

            let mut key = vec![0u8; KEY_LEN];
            rand::thread_rng().fill_bytes(&mut key);

            std::fs::write(&key_path, &key)?;

            // Set file permissions to 0600 (owner read/write only)
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600))?;
            }

            key
        };

        let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
        let cipher = Aes256Gcm::new(key);

        Ok(Self { cipher })
    }

    /// Encrypt a plaintext value.
    ///
    /// Returns the 12-byte nonce prepended to the ciphertext.
    pub fn encrypt(&self, plaintext: &[u8]) -> anyhow::Result<Vec<u8>> {
        let mut nonce_bytes = [0u8; NONCE_LEN];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = self
            .cipher
            .encrypt(nonce, plaintext)
            .map_err(|e| anyhow::anyhow!("encryption failed: {e}"))?;

        let mut out = Vec::with_capacity(NONCE_LEN + ciphertext.len());
        out.extend_from_slice(&nonce_bytes);
        out.extend_from_slice(&ciphertext);
        Ok(out)
    }

    /// Decrypt a value previously produced by [`Self::encrypt`].
    ///
    /// Expects the first 12 bytes to be the nonce, followed by ciphertext.
    pub fn decrypt(&self, encrypted: &[u8]) -> anyhow::Result<Vec<u8>> {
        if encrypted.len() < NONCE_LEN {
            anyhow::bail!("encrypted data too short (missing nonce)");
        }

        let (nonce_bytes, ciphertext) = encrypted.split_at(NONCE_LEN);
        let nonce = Nonce::from_slice(nonce_bytes);

        let plaintext = self
            .cipher
            .decrypt(nonce, ciphertext)
            .map_err(|e| anyhow::anyhow!("decryption failed: {e}"))?;

        Ok(plaintext)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn round_trip() {
        let dir = TempDir::new().unwrap();
        let vault = Vault::open(dir.path()).unwrap();
        let plaintext = b"secret-api-key-12345";
        let encrypted = vault.encrypt(plaintext).unwrap();
        assert_ne!(&encrypted, plaintext);
        let decrypted = vault.decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn reopen_same_key() {
        let dir = TempDir::new().unwrap();
        let vault1 = Vault::open(dir.path()).unwrap();
        let encrypted = vault1.encrypt(b"hello").unwrap();
        // Re-open — should load the same key
        let vault2 = Vault::open(dir.path()).unwrap();
        let decrypted = vault2.decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, b"hello");
    }

    #[cfg(unix)]
    #[test]
    fn key_file_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = TempDir::new().unwrap();
        let _ = Vault::open(dir.path()).unwrap();
        let meta = std::fs::metadata(dir.path().join("master.key")).unwrap();
        assert_eq!(meta.permissions().mode() & 0o777, 0o600);
    }
}
