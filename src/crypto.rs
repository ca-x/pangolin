use std::{fs, io::Write as _, path::Path};

use anyhow::{Context, Result, bail};
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier, password_hash::SaltString};
use base64::{Engine as _, engine::general_purpose::STANDARD_NO_PAD};
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit},
};
use rand::RngCore;
use zeroize::Zeroizing;

#[derive(Clone)]
pub struct SecretBox {
    key: [u8; 32],
}

impl SecretBox {
    pub fn load(data_dir: &Path, configured: Option<&str>) -> Result<Self> {
        let key = if let Some(encoded) = configured {
            decode_key(encoded)
                .context("PANGOLIN_MASTER_KEY must be unpadded base64 for 32 bytes")?
        } else {
            let path = data_dir.join("master.key");
            if path.exists() {
                let encoded = fs::read_to_string(&path).context("failed to read master.key")?;
                decode_key(encoded.trim()).context("master.key is invalid")?
            } else {
                let mut key = [0_u8; 32];
                rand::rng().fill_bytes(&mut key);
                let encoded = STANDARD_NO_PAD.encode(key);
                let mut options = fs::OpenOptions::new();
                options.write(true).create_new(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt as _;
                    options.mode(0o600);
                }
                let mut file = options.open(&path).context("failed to create master.key")?;
                file.write_all(encoded.as_bytes())
                    .context("failed to write master.key")?;
                key
            }
        };
        Ok(Self { key })
    }

    pub fn encrypt(&self, plaintext: &str) -> Result<String> {
        let cipher = XChaCha20Poly1305::new((&self.key).into());
        let mut nonce = [0_u8; 24];
        rand::rng().fill_bytes(&mut nonce);
        let ciphertext = cipher
            .encrypt(XNonce::from_slice(&nonce), plaintext.as_bytes())
            .map_err(|_| anyhow::anyhow!("secret encryption failed"))?;
        let mut envelope = nonce.to_vec();
        envelope.extend(ciphertext);
        Ok(STANDARD_NO_PAD.encode(envelope))
    }

    pub fn decrypt(&self, envelope: &str) -> Result<Zeroizing<String>> {
        let bytes = STANDARD_NO_PAD
            .decode(envelope)
            .context("secret envelope is invalid")?;
        if bytes.len() < 25 {
            bail!("secret envelope is truncated");
        }
        let (nonce, ciphertext) = bytes.split_at(24);
        let cipher = XChaCha20Poly1305::new((&self.key).into());
        let plaintext = cipher
            .decrypt(XNonce::from_slice(nonce), ciphertext)
            .map_err(|_| anyhow::anyhow!("secret decryption failed"))?;
        Ok(Zeroizing::new(
            String::from_utf8(plaintext).context("secret is not UTF-8")?,
        ))
    }
}

pub fn hash_password(password: &str) -> Result<String> {
    let mut salt_bytes = [0_u8; 16];
    rand::rng().fill_bytes(&mut salt_bytes);
    let salt = SaltString::encode_b64(&salt_bytes).context("failed to encode password salt")?;
    Ok(Argon2::default()
        .hash_password(password.as_bytes(), &salt)?
        .to_string())
}

pub fn verify_password(password: &str, encoded: &str) -> bool {
    let Ok(hash) = PasswordHash::new(encoded) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &hash)
        .is_ok()
}

pub fn opaque_token(prefix: &str) -> String {
    let mut bytes = [0_u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    format!("{prefix}{}", STANDARD_NO_PAD.encode(bytes))
}

pub fn token_hash(token: &str) -> String {
    blake3::hash(token.as_bytes()).to_hex().to_string()
}

fn decode_key(encoded: &str) -> Result<[u8; 32]> {
    let bytes = STANDARD_NO_PAD.decode(encoded)?;
    bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("expected 32 bytes"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passwords_and_envelopes_round_trip() {
        let password_hash = hash_password("correct horse battery staple").unwrap();
        assert!(verify_password(
            "correct horse battery staple",
            &password_hash
        ));
        assert!(!verify_password("wrong", &password_hash));

        let secret_box = SecretBox { key: [7; 32] };
        let encrypted = secret_box.encrypt("sk-secret").unwrap();
        assert_ne!(encrypted, "sk-secret");
        assert_eq!(
            secret_box.decrypt(&encrypted).unwrap().as_str(),
            "sk-secret"
        );
    }
}
