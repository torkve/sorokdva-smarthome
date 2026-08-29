//! Minimal Fernet (spec v0x80) implementation: AES-128-CBC + HMAC-SHA256
//! over urlsafe base64, used to encrypt the session cookie.

use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut, BlockEncryptMut, KeyIvInit};
use base64::engine::general_purpose::URL_SAFE;
use base64::Engine;
use hmac::{Hmac, Mac};
use sha2::Sha256;

type Aes128CbcEnc = cbc::Encryptor<aes::Aes128>;
type Aes128CbcDec = cbc::Decryptor<aes::Aes128>;
type HmacSha256 = Hmac<Sha256>;

pub struct Fernet {
    signing_key: [u8; 16],
    encryption_key: [u8; 16],
}

impl Fernet {
    /// Build from the raw 32-byte key (the server_settings cookie_key blob).
    pub fn new(key: &[u8]) -> Option<Self> {
        if key.len() != 32 {
            return None;
        }
        let mut signing_key = [0u8; 16];
        let mut encryption_key = [0u8; 16];
        signing_key.copy_from_slice(&key[..16]);
        encryption_key.copy_from_slice(&key[16..]);
        Some(Fernet {
            signing_key,
            encryption_key,
        })
    }

    pub fn encrypt(&self, plaintext: &[u8], timestamp: u64, iv: [u8; 16]) -> String {
        let ciphertext = Aes128CbcEnc::new(&self.encryption_key.into(), &iv.into())
            .encrypt_padded_vec_mut::<Pkcs7>(plaintext);

        let mut token = Vec::with_capacity(1 + 8 + 16 + ciphertext.len() + 32);
        token.push(0x80);
        token.extend_from_slice(&timestamp.to_be_bytes());
        token.extend_from_slice(&iv);
        token.extend_from_slice(&ciphertext);

        let mut mac = HmacSha256::new_from_slice(&self.signing_key).expect("hmac accepts any key");
        mac.update(&token);
        token.extend_from_slice(&mac.finalize().into_bytes());

        URL_SAFE.encode(token)
    }

    pub fn decrypt(&self, token: &str) -> Option<Vec<u8>> {
        let raw = URL_SAFE.decode(token.as_bytes()).ok()?;
        if raw.len() < 1 + 8 + 16 + 32 || raw[0] != 0x80 {
            return None;
        }
        let (signed, signature) = raw.split_at(raw.len() - 32);

        let mut mac = HmacSha256::new_from_slice(&self.signing_key).expect("hmac accepts any key");
        mac.update(signed);
        mac.verify_slice(signature).ok()?;

        let iv: [u8; 16] = signed[9..25].try_into().ok()?;
        let ciphertext = &signed[25..];
        Aes128CbcDec::new(&self.encryption_key.into(), &iv.into())
            .decrypt_padded_vec_mut::<Pkcs7>(ciphertext)
            .ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let fernet = Fernet::new(&[7u8; 32]).unwrap();
        let token = fernet.encrypt(b"hello world", 1_700_000_000, [3u8; 16]);
        assert_eq!(fernet.decrypt(&token).unwrap(), b"hello world");
    }

    #[test]
    fn known_token_decrypts() {
        // A fixed reference token for key = bytes 0..32; decrypting it
        // guards the exact wire format (version byte, ts, IV, HMAC).
        // Never regenerate it with this implementation: a self-produced
        // token would make the check circular.
        let key: Vec<u8> = (0..32).collect();
        let fernet = Fernet::new(&key).unwrap();
        let token = "gAAAAABlU_EA8OSk1szHUprMcozLGzd8IhvjKh0MTZTYfvdDA1EKoy5oltupByrKE3aMl6o35\
                     OXhpsjPKbWedpx7Bt1pmQTHfUdrmgfmqLy_zMgBRWhE4FaNVwpJT3OLxrMtzFqQlhyHBcPNu5\
                     iYrDJCRUomyy4Zdw==";
        assert_eq!(
            fernet.decrypt(token).unwrap(),
            b"{\"created\": 1700000000, \"session\": {\"AIOHTTP_SECURITY\": \"1\"}}",
        );

        // tamper -> reject
        let good = fernet.encrypt(b"{\"a\": 1}", 1_700_000_000, [9u8; 16]);
        let mut broken = good.clone();
        broken.replace_range(10..11, if &good[10..11] == "A" { "B" } else { "A" });
        assert!(fernet.decrypt(&broken).is_none());
    }
}
