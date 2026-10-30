//! Local agent keys. PublicWallet is the only serializable registration value.
use crate::{
    error::{Error, Result},
    routing,
};
use allowit_client_core::store::Store;
use serde::{Deserialize, Serialize};
use std::path::Path;
use zeroize::Zeroizing;
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PublicWallet {
    pub network: String,
    pub address: String,
    pub public_key: String,
    pub key_type: String,
}
fn public(network: &str, seed: &[u8; 32]) -> Result<PublicWallet> {
    let normalized = routing::normalize(network)?;
    let (address, public_key, key_type) = if normalized.starts_with("tempo:") {
        let (address, public_key) = allowit_tempo::wallet::public_key(seed)
            .map_err(|_| Error::config("Invalid Tempo wallet seed"))?;
        (address, public_key, "secp256k1".to_owned())
    } else {
        let key = ed25519_dalek::SigningKey::from_bytes(seed)
            .verifying_key()
            .to_bytes();
        let address = if normalized.starts_with("stellar:") {
            stellar_address(&key)
        } else {
            bs58::encode(key).into_string()
        };
        (
            address,
            bs58::encode(key).into_string(),
            "ed25519".to_owned(),
        )
    };
    Ok(PublicWallet {
        network: routing::caip2(network)?.into(),
        address,
        public_key,
        key_type,
    })
}
fn stellar_address(key: &[u8; 32]) -> String {
    let mut bytes = vec![6 << 3];
    bytes.extend_from_slice(key);
    let mut crc = 0u16;
    for b in &bytes {
        crc ^= (*b as u16) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x1021
            } else {
                crc << 1
            };
        }
    }
    bytes.extend_from_slice(&crc.to_le_bytes());
    data_encoding::BASE32_NOPAD.encode(&bytes)
}
fn storage(path: &Path) -> Result<(Store, String)> {
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| Error::usage("Choose a private wallet file"))?;
    if !name
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        || name.is_empty()
    {
        return Err(Error::usage("Invalid wallet filename"));
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    Ok((Store::open(parent)?, name.into()))
}
/// Generates an executor key in the rail's existing private-file format.
pub fn generate_wallet(network: &str, path: &Path) -> Result<PublicWallet> {
    let network = routing::normalize(network)?;
    let (_lock, _name) = storage(path)?;
    let mut seed = Zeroizing::new([0u8; 32]);
    let metadata = loop {
        getrandom::fill(seed.as_mut())
            .map_err(|_| Error::config("Operating-system randomness unavailable"))?;
        if let Ok(p) = public(network, &seed) {
            break p;
        }
    };
    let secret = if network.starts_with("tempo:") {
        seed.iter().map(|b| format!("{b:02x}")).collect::<String>()
    } else if network.starts_with("solana:") {
        let key = ed25519_dalek::SigningKey::from_bytes(&seed);
        serde_json::to_string(&key.to_keypair_bytes().as_slice())
            .map_err(|_| Error::config("Cannot encode signer"))?
    } else {
        let mut bytes = vec![18 << 3];
        bytes.extend_from_slice(seed.as_ref());
        let mut crc = 0u16;
        for b in &bytes {
            crc ^= (*b as u16) << 8;
            for _ in 0..8 {
                crc = if crc & 0x8000 != 0 {
                    (crc << 1) ^ 0x1021
                } else {
                    crc << 1
                };
            }
        }
        bytes.extend_from_slice(&crc.to_le_bytes());
        let out = data_encoding::BASE32_NOPAD.encode(&bytes);
        use zeroize::Zeroize;
        bytes.zeroize();
        out
    };
    let secret = Zeroizing::new(secret);
    write_new(path, secret.as_bytes())?;
    Ok(metadata)
}
#[cfg(unix)]
fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::{io::Write, os::unix::fs::OpenOptionsExt};
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|_| {
            Error::config("Cannot create new private signer file; existing keys are never replaced")
        })?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| Error::config("Cannot durably save private signer"))?;
    Ok(())
}
#[cfg(not(unix))]
fn write_new(_: &Path, _: &[u8]) -> Result<()> {
    Err(Error::unsupported(
        "Private wallet generation requires a supported POSIX filesystem",
    ))
}
/// Read a private native signer into a zeroizing buffer under its storage lock.
fn read_signer(path: &Path) -> Result<Zeroizing<String>> {
    let (_lock, _) = storage(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let m = std::fs::symlink_metadata(path)
            .map_err(|_| Error::config("Cannot read private signer"))?;
        if !m.is_file()
            || m.mode() & 0o7777 != 0o600
            || m.nlink() != 1
            || m.uid() != unsafe { libc::geteuid() }
        {
            return Err(Error::config("Unsafe private signer file"));
        }
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options
        .open(path)
        .map_err(|_| Error::config("Cannot open private signer"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let m = file
            .metadata()
            .map_err(|_| Error::config("Cannot inspect signer"))?;
        if !m.is_file()
            || m.mode() & 0o7777 != 0o600
            || m.nlink() != 1
            || m.uid() != unsafe { libc::geteuid() }
        {
            return Err(Error::config("Unsafe private signer file"));
        }
    }
    use std::io::Read;
    let mut text = Zeroizing::new(String::new());
    file.take(4097)
        .read_to_string(&mut text)
        .map_err(|_| Error::config("Cannot read signer"))?;
    if text.len() > 4096 {
        return Err(Error::config("Signer file too large"));
    }
    Ok(text)
}
pub(crate) fn stellar_signing_seed(path: &Path) -> Result<Zeroizing<[u8; 32]>> {
    let text = read_signer(path)?;
    allowit_stellar_wallet::decode_seed(text.trim())
        .map(Zeroizing::new)
        .map_err(|_| Error::config("Invalid local Stellar signer"))
}
/// Inspect a native signer file without returning its secret.
pub fn inspect_wallet(network: &str, path: &Path) -> Result<PublicWallet> {
    let text = read_signer(path)?;
    let n = routing::normalize(network)?;
    let mut bytes = Zeroizing::new(if n.starts_with("solana:") {
        serde_json::from_str::<Vec<u8>>(&text)
            .map_err(|_| Error::config("Invalid Solana signer"))?
    } else if n.starts_with("tempo:") {
        if text.len() != 64 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(Error::config("Invalid Tempo signer"));
        }
        (0..32)
            .map(|i| {
                u8::from_str_radix(&text[i * 2..i * 2 + 2], 16)
                    .map_err(|_| Error::config("Invalid Tempo signer"))
            })
            .collect::<Result<Vec<_>>>()?
    } else {
        allowit_stellar_wallet::decode_seed(text.trim())
            .map_err(|_| Error::config("Invalid Stellar seed"))?
            .to_vec()
    });
    let seed = Zeroizing::new(
        <[u8; 32]>::try_from(
            bytes
                .get(..32)
                .ok_or_else(|| Error::config("Invalid seed"))?,
        )
        .map_err(|_| Error::config("Invalid seed"))?,
    );
    let metadata = public(network, &seed)?;
    if n.starts_with("solana:") {
        let expected = ed25519_dalek::SigningKey::from_bytes(&seed).to_keypair_bytes();
        if bytes.as_slice() != expected {
            return Err(Error::config("Invalid Solana key pair"));
        }
    }
    use zeroize::Zeroize;
    bytes.zeroize();
    Ok(metadata)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn public_metadata_cannot_serialize_secret() {
        let seed = [7u8; 32];
        for network in ["solana:devnet", "stellar:testnet", "eip155:42431"] {
            let p = public(network, &seed).unwrap();
            let v = serde_json::to_value(p).unwrap();
            assert_eq!(v.as_object().unwrap().len(), 4);
            assert!(v.get("seed").is_none());
            assert!(v.get("privateKey").is_none());
        }
    }
    #[test]
    fn known_stellar_and_evm_addresses() {
        assert_eq!(
            stellar_address(&[0; 32]),
            "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF"
        );
        let mut seed = [0; 32];
        seed[31] = 1;
        assert_eq!(
            public("eip155:42431", &seed).unwrap().address,
            "0x7e5f4552091a69125d5dfcb7b8c2659029395bdf"
        );
    }
}

impl PublicWallet {
    pub fn validate(&self) -> Result<()> {
        if routing::caip2(&self.network)? != self.network {
            return Err(Error::usage("Registration requires a canonical network"));
        }
        let normalized = routing::normalize(&self.network)?;
        let expected = if normalized.starts_with("tempo:") {
            if self.key_type != "secp256k1" {
                return Err(Error::usage("Invalid public key type"));
            }
            allowit_tempo::wallet::validate_public(&self.address, &self.public_key)
                .map_err(|_| Error::usage("Invalid Tempo public wallet"))?;
            self.address.clone()
        } else {
            if self.key_type != "ed25519" {
                return Err(Error::usage("Invalid public key type"));
            }
            let key: [u8; 32] = bs58::decode(&self.public_key)
                .into_vec()
                .map_err(|_| Error::usage("Invalid public key"))?
                .try_into()
                .map_err(|_| Error::usage("Invalid public key"))?;
            ed25519_dalek::VerifyingKey::from_bytes(&key)
                .map_err(|_| Error::usage("Invalid public point"))?;
            if normalized.starts_with("stellar:") {
                stellar_address(&key)
            } else {
                bs58::encode(key).into_string()
            }
        };
        if expected != self.address {
            return Err(Error::usage("Address differs from public key"));
        }
        Ok(())
    }
}

#[cfg(all(test, unix))]
mod storage_tests {
    use super::*;
    use std::{
        os::unix::fs::{PermissionsExt, symlink},
        sync::atomic::{AtomicU64, Ordering},
    };
    static NEXT: AtomicU64 = AtomicU64::new(0);
    fn path() -> std::path::PathBuf {
        std::env::temp_dir()
            .canonicalize()
            .expect("Test temporary directory must exist")
            .join(format!(
                "allowit-local-key-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ))
            .join("agent.key")
    }
    #[test]
    fn generated_files_are_usable_private_and_never_replaced() {
        for n in ["solana:devnet", "stellar:testnet", "eip155:42431"] {
            let p = path();
            let public = generate_wallet(n, &p).unwrap();
            public.validate().unwrap();
            assert_eq!(inspect_wallet(n, &p).unwrap(), public);
            assert_eq!(
                std::fs::metadata(&p).unwrap().permissions().mode() & 0o777,
                0o600
            );
            let before = std::fs::read(&p).unwrap();
            assert!(generate_wallet(n, &p).is_err());
            assert_eq!(before, std::fs::read(&p).unwrap());
            if n.starts_with("solana:") {
                let signer = allowit_native::crypto::LocalSigner::load(&p).unwrap();
                assert_eq!(signer.public_key().to_string(), public.address);
                let signature = signer.sign(b"local-agent-proof");
                allowit_native::crypto::verify(
                    signer.public_key(),
                    b"local-agent-proof",
                    &signature,
                )
                .unwrap();
                assert!(
                    allowit_native::crypto::verify(
                        signer.public_key(),
                        b"different-intent",
                        &signature
                    )
                    .is_err()
                );
            }
            std::fs::remove_dir_all(p.parent().unwrap()).unwrap();
        }
    }
    #[test]
    fn symlink_hardlink_and_insecure_files_are_refused() {
        let p = path();
        generate_wallet("solana:devnet", &p).unwrap();
        let alias = p.with_file_name("alias");
        symlink(&p, &alias).unwrap();
        assert!(inspect_wallet("solana:devnet", &alias).is_err());
        assert!(generate_wallet("solana:devnet", &alias).is_err());
        std::fs::remove_file(&alias).unwrap();
        std::fs::hard_link(&p, &alias).unwrap();
        assert!(inspect_wallet("solana:devnet", &p).is_err());
        std::fs::remove_file(&alias).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(inspect_wallet("solana:devnet", &p).is_err());
        std::fs::remove_dir_all(p.parent().unwrap()).unwrap();
    }
    #[test]
    fn concurrent_generation_has_exactly_one_winner() {
        let p = path();
        let mut threads = vec![];
        for _ in 0..8 {
            let p = p.clone();
            threads.push(std::thread::spawn(move || {
                generate_wallet("solana:devnet", &p).is_ok()
            }));
        }
        let wins = threads
            .into_iter()
            .map(|t| usize::from(t.join().unwrap()))
            .sum::<usize>();
        assert_eq!(wins, 1);
        inspect_wallet("solana:devnet", &p).unwrap();
        std::fs::remove_dir_all(p.parent().unwrap()).unwrap();
    }
}
