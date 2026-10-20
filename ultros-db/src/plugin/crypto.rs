use super::{PluginError, Result};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use ring::hmac;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

#[derive(Clone)]
pub struct PluginKeys {
    pairing: hmac::Key,
    identity: hmac::Key,
}
impl PluginKeys {
    /// Separately provisioned deployment keys. Domain separation also binds
    /// development credentials and private identities to the configured origin.
    pub fn new(pairing: &[u8], identity: &[u8], origin: &str) -> Result<Self> {
        if pairing.len() < 32 || identity.len() < 32 {
            return Err(PluginError::Invalid("invalid_server_key"));
        }
        fn derive(key: &[u8], kind: &str, origin: &str) -> hmac::Key {
            let parent = hmac::Key::new(hmac::HMAC_SHA256, key);
            let derived = hmac::sign(
                &parent,
                format!("ultros-plugin-v1/{kind}/{origin}").as_bytes(),
            );
            hmac::Key::new(hmac::HMAC_SHA256, derived.as_ref())
        }
        Ok(Self {
            pairing: derive(pairing, "pairing", origin),
            identity: derive(identity, "identity", origin),
        })
    }
    pub fn human_hash(&self, code: &str) -> Result<Vec<u8>> {
        let code = normalize_human_code(code).ok_or(PluginError::Invalid("invalid_code"))?;
        Ok(hmac::sign(&self.pairing, code.as_bytes()).as_ref().to_vec())
    }
    pub fn verification_hash(&self, token: &str) -> Result<Vec<u8>> {
        let hash = verification_hash(token)?;
        let mut message = b"verification-link/".to_vec();
        message.extend_from_slice(&hash);
        Ok(hmac::sign(&self.pairing, &message).as_ref().to_vec())
    }
    pub fn private_fingerprint(&self, owner: i64, kind: &str, id: &str) -> Result<Vec<u8>> {
        let parsed: u64 = id
            .parse()
            .map_err(|_| PluginError::Invalid("invalid_content_id"))?;
        if parsed == 0 || id != parsed.to_string() {
            return Err(PluginError::Invalid("invalid_content_id"));
        }
        Ok(hmac::sign(
            &self.identity,
            format!("{owner}/{kind}/{parsed}").as_bytes(),
        )
        .as_ref()
        .to_vec())
    }
    pub fn ip_hash(&self, ip: &str) -> Vec<u8> {
        hmac::sign(&self.pairing, format!("ip/{ip}").as_bytes())
            .as_ref()
            .to_vec()
    }
}
pub fn normalize_human_code(code: &str) -> Option<String> {
    let code: String = code
        .chars()
        .filter(|c| *c != '-' && *c != ' ')
        .map(|c| match c.to_ascii_uppercase() {
            'O' => '0',
            'I' | 'L' => '1',
            c => c,
        })
        .collect();
    (code.len() == 8
        && code
            .bytes()
            .all(|b| b"0123456789ABCDEFGHJKMNPQRSTVWXYZ".contains(&b)))
    .then_some(code)
}
pub(crate) fn random(bytes: usize) -> Result<String> {
    let mut value = vec![0; bytes];
    getrandom::fill(&mut value).map_err(|_| PluginError::Storage)?;
    Ok(URL_SAFE_NO_PAD.encode(value))
}
pub(super) fn verification_hash(token: &str) -> Result<Vec<u8>> {
    let invalid = || PluginError::Invalid("invalid_or_expired_link");
    if token.len() != 43 {
        return Err(invalid());
    }
    let decoded = URL_SAFE_NO_PAD.decode(token).map_err(|_| invalid())?;
    if decoded.len() != 32 || URL_SAFE_NO_PAD.encode(&decoded) != token {
        return Err(invalid());
    }
    Ok(digest(token.as_bytes()))
}

/// A 40-bit, readable fingerprint of the independent 256-bit link token.
/// This code is for manual entry/comparison, never a polling credential.
pub(super) fn human_code(token: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(b"ultros-plugin-link-code-v1/");
    hash.update(token.as_bytes());
    let hash = hash.finalize();
    let bits = u64::from_be_bytes([0, 0, 0, hash[0], hash[1], hash[2], hash[3], hash[4]]);
    (0..8)
        .map(|index| {
            b"0123456789ABCDEFGHJKMNPQRSTVWXYZ"[((bits >> ((7 - index) * 5)) & 31) as usize] as char
        })
        .collect()
}
pub(super) fn digest(value: &[u8]) -> Vec<u8> {
    Sha256::digest(value).to_vec()
}
pub(super) fn equal(a: &[u8], b: &[u8]) -> bool {
    bool::from(a.ct_eq(b))
}

#[derive(Clone)]
pub(super) struct TokenProof {
    pub id: String,
    pub hash: Vec<u8>,
    pub refresh: bool,
}
impl TokenProof {
    pub fn parse(token: &str) -> Result<Self> {
        let mut parts = token.split('.');
        let kind = parts.next().ok_or(PluginError::Unauthorized)?;
        let id = parts.next().ok_or(PluginError::Unauthorized)?;
        let secret = parts.next().ok_or(PluginError::Unauthorized)?;
        if !matches!(kind, "ult_a1" | "ult_r1")
            || id.len() != 22
            || secret.len() != 43
            || parts.next().is_some()
        {
            return Err(PluginError::Unauthorized);
        }
        let decoded = URL_SAFE_NO_PAD
            .decode(secret)
            .map_err(|_| PluginError::Unauthorized)?;
        if decoded.len() != 32
            || URL_SAFE_NO_PAD.encode(&decoded) != secret
            || URL_SAFE_NO_PAD.decode(id).is_err()
        {
            return Err(PluginError::Unauthorized);
        }
        Ok(Self {
            id: id.into(),
            hash: digest(&decoded),
            refresh: kind == "ult_r1",
        })
    }
    pub fn mint(refresh: bool) -> Result<(String, Self)> {
        let id = random(16)?;
        let secret = random(32)?;
        let token = format!(
            "{}.{id}.{secret}",
            if refresh { "ult_r1" } else { "ult_a1" }
        );
        let proof = Self::parse(&token)?;
        Ok((token, proof))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn token_kinds_and_secrets_cannot_be_confused() {
        for refresh in [false, true] {
            let (token, proof) = TokenProof::mint(refresh).unwrap();
            assert_eq!(TokenProof::parse(&token).unwrap().refresh, refresh);
            assert_eq!(proof.hash.len(), 32);
            assert!(!format!("{:?}", PluginError::Unauthorized).contains(&token));
        }
        for value in ["", "ult_a1.foo.bar", "Bearer token", "ult_r1.a.b.c"] {
            assert!(TokenProof::parse(value).is_err());
        }
    }
    #[test]
    fn low_entropy_codes_are_keyed_and_identities_are_account_and_origin_bound() {
        let a = PluginKeys::new(&[1; 32], &[2; 32], "https://ultros.app").unwrap();
        let b = PluginKeys::new(&[1; 32], &[2; 32], "http://localhost:8080").unwrap();
        assert_eq!(
            a.human_hash("abcd-1234").unwrap(),
            a.human_hash("ABCD1234").unwrap()
        );
        assert_ne!(
            a.human_hash("abcd-1234").unwrap(),
            b.human_hash("abcd-1234").unwrap()
        );
        assert_ne!(
            a.private_fingerprint(1, "character", "42").unwrap(),
            a.private_fingerprint(2, "character", "42").unwrap()
        );
        assert!(a.private_fingerprint(1, "retainer", "00042").is_err());
        assert!(normalize_human_code("ABCDEFG$").is_none());
        for _ in 0..32 {
            assert!(normalize_human_code(&human_code(&random(32).unwrap())).is_some());
        }
    }

    #[test]
    fn verification_fingerprints_are_stable_and_tokens_are_canonical() {
        let token = random(32).unwrap();
        let code = human_code(&token);
        assert_eq!(code, human_code(&token));
        assert_eq!(verification_hash(&token).unwrap(), digest(token.as_bytes()));
        assert_eq!(code.len(), 8);
        assert_ne!(code, human_code(&random(32).unwrap()));
        let a = PluginKeys::new(&[1; 32], &[2; 32], "https://ultros.app").unwrap();
        let b = PluginKeys::new(&[1; 32], &[2; 32], "http://localhost:8080").unwrap();
        assert_ne!(
            a.verification_hash(&token).unwrap(),
            b.verification_hash(&token).unwrap()
        );
        for invalid in ["", "ABCD-EFGH", "ult_a1.secret", &format!("{token}=")] {
            assert!(verification_hash(invalid).is_err());
        }
    }
}
