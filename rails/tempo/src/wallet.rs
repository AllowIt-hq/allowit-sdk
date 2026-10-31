//! Local secp256k1 wallet preparation. These functions perform no I/O.
use crate::{Result, address, hex, unhex};
use k256::{SecretKey, ecdsa::VerifyingKey, elliptic_curve::sec1::ToEncodedPoint};
use sha3::{Digest, Keccak256};

fn identity(key: &VerifyingKey) -> (String, String) {
    let point = key.to_encoded_point(false);
    let bytes = point.as_bytes();
    let digest = Keccak256::digest(&bytes[1..]);
    (hex(&digest[12..]), hex(bytes)[2..].to_owned())
}

/// Derive the public address and uncompressed SEC1 public key from a local seed.
/// The caller retains the seed. No secret is returned or serialized.
pub fn public_key(seed: &[u8; 32]) -> Result<(String, String)> {
    let secret = SecretKey::from_slice(seed).map_err(|_| "Invalid local secp256k1 scalar")?;
    let point = secret.public_key().to_encoded_point(false);
    let key = VerifyingKey::from_sec1_bytes(point.as_bytes())
        .map_err(|_| "Invalid local secp256k1 public key")?;
    Ok(identity(&key))
}

/// Check a public registration without reading or receiving its private key.
pub fn validate_public(account: &str, public_key_hex: &str) -> Result<()> {
    if public_key_hex.len() != 130 || !public_key_hex.starts_with("04") {
        return Err("Tempo public key must be uncompressed SEC1 hex".into());
    }
    let bytes = unhex(&format!("0x{public_key_hex}"))?;
    let key = VerifyingKey::from_sec1_bytes(&bytes).map_err(|_| "Invalid Tempo public key")?;
    if identity(&key).0 != address(account)? {
        return Err("Tempo public key does not match the wallet address".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_identity_matches_standard_secp256k1_vector() {
        let mut seed = [0u8; 32];
        seed[31] = 1;
        let (account, public) = public_key(&seed).unwrap();
        assert_eq!(account, "0x7e5f4552091a69125d5dfcb7b8c2659029395bdf");
        assert_eq!(public.len(), 130);
        validate_public(&account, &public).unwrap();
        assert!(validate_public(crate::ZERO, &public).is_err());
    }

    #[test]
    fn malformed_or_secret_scalar_input_cannot_be_public_registration() {
        assert!(public_key(&[0u8; 32]).is_err());
        for public in ["00".repeat(32), "04".repeat(65), "02".repeat(33)] {
            assert!(validate_public(crate::ZERO, &public).is_err());
        }
    }
}
