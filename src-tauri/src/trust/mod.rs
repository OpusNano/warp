use std::{fs, path::PathBuf};

use anyhow::{Context, Result};
use russh::keys::{HashAlg, PublicKey};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnownHostEntry {
    pub host: String,
    pub port: u16,
    pub key_algorithm: String,
    pub public_key: String,
    pub fingerprint_sha256: String,
}

#[derive(Debug, Clone)]
pub enum TrustCheck {
    Unknown,
    Verified(KnownHostEntry),
    Mismatch(KnownHostEntry),
}

#[derive(Debug, Clone)]
pub struct TrustModel {
    known_hosts_path: PathBuf,
}

impl TrustModel {
    pub fn new(base_dir: PathBuf) -> Self {
        Self {
            known_hosts_path: base_dir.join("known_hosts.json"),
        }
    }

    pub fn verify_host(&self, host: &str, port: u16, key: &PublicKey) -> Result<TrustCheck> {
        let stored = self
            .load_entries()?
            .into_iter()
            .find(|entry| entry.host == host && entry.port == port);

        Ok(match stored {
            None => TrustCheck::Unknown,
            Some(entry) if entry.public_key == key.to_string() => TrustCheck::Verified(entry),
            Some(entry) => TrustCheck::Mismatch(entry),
        })
    }

    pub fn remember_host(&self, host: &str, port: u16, key: &PublicKey) -> Result<KnownHostEntry> {
        let mut entries = self.load_entries()?;
        entries.retain(|entry| !(entry.host == host && entry.port == port));

        let entry = KnownHostEntry {
            host: host.into(),
            port,
            key_algorithm: key.algorithm().to_string(),
            public_key: key.to_string(),
            fingerprint_sha256: fingerprint_sha256(key),
        };

        entries.push(entry.clone());
        self.save_entries(&entries)?;
        Ok(entry)
    }

    fn load_entries(&self) -> Result<Vec<KnownHostEntry>> {
        if !self.known_hosts_path.exists() {
            return Ok(Vec::new());
        }

        let contents = fs::read_to_string(&self.known_hosts_path)
            .with_context(|| format!("failed to read {}", self.known_hosts_path.display()))?;

        serde_json::from_str(&contents)
            .with_context(|| format!("failed to parse {}", self.known_hosts_path.display()))
    }

    fn save_entries(&self, entries: &[KnownHostEntry]) -> Result<()> {
        if let Some(parent) = self.known_hosts_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }

        let contents = serde_json::to_string_pretty(entries)?;
        fs::write(&self.known_hosts_path, format!("{contents}\n"))
            .with_context(|| format!("failed to write {}", self.known_hosts_path.display()))
    }
}

pub fn fingerprint_sha256(key: &PublicKey) -> String {
    key.fingerprint(HashAlg::Sha256).to_string()
}

#[cfg(test)]
mod tests {
    use super::{fingerprint_sha256, TrustCheck, TrustModel};
    use russh::keys::PublicKey;

    fn example_key() -> PublicKey {
        PublicKey::from_openssh(
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIEJ8Oe0Rb114F8nGMD4HzyBbs6k8ZZrVSu2Ce279b9Ec",
        )
        .expect("valid key")
    }

    #[test]
    fn remembers_and_verifies_host() {
        let temp = std::env::temp_dir().join(format!("warp-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp);
        let model = TrustModel::new(temp.clone());
        let key = example_key();

        model
            .remember_host("example.com", 22, &key)
            .expect("save host");

        match model
            .verify_host("example.com", 22, &key)
            .expect("verify host")
        {
            TrustCheck::Verified(entry) => {
                assert_eq!(entry.fingerprint_sha256, fingerprint_sha256(&key));
            }
            _ => panic!("expected verified host"),
        }

        let _ = std::fs::remove_dir_all(temp);
    }
}
