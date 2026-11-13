//! gcr's "secret exchange" (`[sx-aes-1]`): how the keyring daemon and its
//! prompter pass a password without it crossing the bus in the clear.
//!
//! Key agreement is Diffie-Hellman on the 1536-bit IKE group (RFC 3526 group
//! 5, generator 2), the key is HKDF-SHA256 of the shared secret (no salt, no
//! info) cut to 16 bytes, and the secret goes under AES-128-CBC with PKCS#7
//! padding and a random IV. The messages are GKeyFile text: the header
//! `[sx-aes-1]`, then `public=`, `secret=` and `iv=` in standard base64.
//!
//! It doesn't stand against an active attacker, as gcr's says; it only keeps
//! the password out of bus logs. This is the prompter's side: the daemon
//! sends its public key, and the reply carries the password.

use aes::Aes128;
use aes::cipher::block_padding::Pkcs7;
use aes::cipher::{BlockDecryptMut, BlockEncryptMut, KeyIvInit};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use hkdf::Hkdf;
use num_bigint::BigUint;
use sha2::Sha256;

const HEADER: &str = "[sx-aes-1]";
const PRIME_HEX: &str = include_str!("modp1536.hex");

fn prime() -> BigUint {
    BigUint::parse_bytes(PRIME_HEX.trim().as_bytes(), 16).expect("the group's prime")
}

/// One conversation's keys.
pub struct Exchange {
    private: BigUint,
    public: BigUint,
    key: Option<[u8; 16]>,
}

/// What a received message holds.
pub struct Received {
    /// The password, when the message carried one. The prompter only sends
    /// them; the tests and the interop example read what comes back.
    #[allow(dead_code)]
    pub secret: Option<Vec<u8>>,
}

impl Exchange {
    pub fn new() -> Exchange {
        let p = prime();
        let mut bytes = [0u8; 192];
        loop {
            getrandom::fill(&mut bytes).expect("random bytes");
            let private = BigUint::from_bytes_be(&bytes);
            // As gcr: below the prime's top bit, and never zero.
            let private = private % (BigUint::from(1u8) << 1535usize);
            if private.bits() > 1 {
                let public = BigUint::from(2u8).modpow(&private, &p);
                return Exchange {
                    private,
                    public,
                    key: None,
                };
            }
        }
    }

    /// The message that starts the conversation (or answers, with no
    /// secret): just our public key.
    pub fn begin(&self) -> String {
        format!(
            "{HEADER}\npublic={}\n",
            STANDARD.encode(self.public.to_bytes_be())
        )
    }

    /// Takes a message from the other side: derives the key from its public
    /// key the first time, and decrypts a secret if it has one.
    pub fn receive(&mut self, message: &str) -> Result<Received, String> {
        let fields = parse(message)?;
        if self.key.is_none() {
            let peer = field(&fields, "public")?.ok_or("no public key in the exchange")?;
            let peer = BigUint::from_bytes_be(&peer);
            let p = prime();
            if peer <= BigUint::from(1u8) || peer >= &p - 1u8 {
                return Err("a public key outside the group".into());
            }
            let shared = peer.modpow(&self.private, &p).to_bytes_be();
            let mut key = [0u8; 16];
            Hkdf::<Sha256>::new(None, &shared)
                .expand(&[], &mut key)
                .map_err(|e| e.to_string())?;
            self.key = Some(key);
        }
        let secret = match (field(&fields, "secret")?, field(&fields, "iv")?) {
            (Some(cipher), Some(iv)) => {
                let key = self.key.ok_or("no key yet")?;
                let iv: [u8; 16] = iv.try_into().map_err(|_| "a bad iv")?;
                let plain = cbc::Decryptor::<Aes128>::new(&key.into(), &iv.into())
                    .decrypt_padded_vec_mut::<Pkcs7>(&cipher)
                    .map_err(|_| "the secret didn't decrypt")?;
                Some(plain)
            }
            _ => None,
        };
        Ok(Received { secret })
    }

    /// The reply, after a message was received (it needs the key): our
    /// public key as gcr's `send` always puts, and `secret` if there is one.
    pub fn send(&self, secret: Option<&[u8]>) -> Result<String, String> {
        let key = self.key.ok_or("nothing received yet")?;
        let public = STANDARD.encode(self.public.to_bytes_be());
        let Some(secret) = secret else {
            return Ok(format!("{HEADER}\npublic={public}\n"));
        };
        let mut iv = [0u8; 16];
        getrandom::fill(&mut iv).map_err(|e| e.to_string())?;
        let cipher = cbc::Encryptor::<Aes128>::new(&key.into(), &iv.into())
            .encrypt_padded_vec_mut::<Pkcs7>(secret);
        Ok(format!(
            "{HEADER}\npublic={public}\nsecret={}\niv={}\n",
            STANDARD.encode(cipher),
            STANDARD.encode(iv)
        ))
    }
}

impl Default for Exchange {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Exchange {
    fn drop(&mut self) {
        if let Some(key) = &mut self.key {
            key.fill(0);
        }
    }
}

/// The `key=value` lines under `[sx-aes-1]`.
fn parse(message: &str) -> Result<Vec<(&str, &str)>, String> {
    let mut lines = message.lines().map(str::trim).filter(|l| !l.is_empty());
    if lines.next() != Some(HEADER) {
        return Err("not a secret exchange".into());
    }
    Ok(lines.filter_map(|l| l.split_once('=')).collect())
}

fn field(fields: &[(&str, &str)], name: &str) -> Result<Option<Vec<u8>>, String> {
    match fields.iter().find(|(k, _)| *k == name) {
        None => Ok(None),
        Some((_, v)) => STANDARD
            .decode(v.trim())
            .map(Some)
            .map_err(|e| format!("{name}: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_password_goes_from_one_side_to_the_other() {
        let mut daemon = Exchange::new();
        let mut prompter = Exchange::new();
        // The daemon begins; the prompter receives, then answers with the secret.
        prompter.receive(&daemon.begin()).unwrap();
        let reply = prompter.send(Some(b"hunter2 \xc3\xa5")).unwrap();
        let got = daemon.receive(&reply).unwrap();
        assert_eq!(got.secret.as_deref(), Some(&b"hunter2 \xc3\xa5"[..]));
    }

    #[test]
    fn the_secret_is_not_in_the_message() {
        let daemon = Exchange::new();
        let mut prompter = Exchange::new();
        prompter.receive(&daemon.begin()).unwrap();
        let reply = prompter.send(Some(b"hunter2")).unwrap();
        assert!(!reply.contains("hunter2"));
        assert!(reply.starts_with("[sx-aes-1]\n"));
    }

    #[test]
    fn a_message_without_the_header_is_refused() {
        let mut x = Exchange::new();
        assert!(x.receive("public=AAAA").is_err());
        assert!(x.receive("[sx-aes-1]\npublic=AQ==\n").is_err());
    }

    #[test]
    fn a_tampered_secret_is_refused() {
        let mut daemon = Exchange::new();
        let mut prompter = Exchange::new();
        prompter.receive(&daemon.begin()).unwrap();
        let reply = prompter.send(Some(b"hunter2")).unwrap();
        assert!(daemon.receive(&reply.replace("secret=", "secret=AAAA")).is_err());
    }
}
