//! What a run actually did: the dataset it read, the seed it used, the code
//! that ran it. Fairness and publication rules (Plan 25) require every run
//! to carry this before any result is compared against another.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunManifest {
    pub generated_at: DateTime<Utc>,
    pub seed: u64,
    pub cognoscenti_commit: Option<String>,
    pub dataset_path: String,
    pub dataset_sha256: String,
    pub item_count: usize,
    pub systems: Vec<String>,
    pub top_k: usize,
}

impl RunManifest {
    pub fn build(
        dataset_path: &str,
        item_count: usize,
        systems: &[String],
        top_k: usize,
        seed: u64,
    ) -> Result<Self> {
        Ok(Self {
            generated_at: Utc::now(),
            seed,
            cognoscenti_commit: current_commit(),
            dataset_path: dataset_path.to_string(),
            dataset_sha256: sha256_file(dataset_path)?,
            item_count,
            systems: systems.to_vec(),
            top_k,
        })
    }

    pub fn write(&self, output_dir: &Path) -> Result<()> {
        let manifest_path = output_dir.join("run_manifest.json");
        std::fs::write(&manifest_path, serde_json::to_string_pretty(self)?)
            .with_context(|| format!("failed to write {}", manifest_path.display()))?;
        println!("Run manifest saved to: {}", manifest_path.display());
        Ok(())
    }
}

/// Hex-encoded SHA-256 of a file's exact bytes, so a dataset can be cited by
/// checksum rather than by trust.
pub fn sha256_file(path: &str) -> Result<String> {
    let bytes =
        std::fs::read(path).with_context(|| format!("failed to read {path} for checksum"))?;
    Ok(sha256_hex(&bytes))
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// The Cognoscenti commit a run was built from, when run inside a git
/// checkout. `None` outside one (e.g. a packaged release) rather than a
/// placeholder SHA.
fn current_commit() -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let sha = String::from_utf8(output.stdout).ok()?;
    Some(sha.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn sha256_hex_matches_known_test_vectors() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn sha256_file_matches_content_and_changes_with_it() {
        let mut file_a = tempfile_with(b"one");
        let mut file_b = tempfile_with(b"two");

        let checksum_a = sha256_file(file_a.path_str()).unwrap();
        let checksum_b = sha256_file(file_b.path_str()).unwrap();

        assert_eq!(checksum_a, sha256_hex(b"one"));
        assert_ne!(checksum_a, checksum_b);

        // Changing the file's content changes its checksum.
        file_a.overwrite(b"two");
        assert_eq!(sha256_file(file_a.path_str()).unwrap(), checksum_b);

        file_b.overwrite(b"");
    }

    #[test]
    fn manifest_build_records_the_dataset_checksum_and_seed() {
        let file = tempfile_with(b"{\"id\":\"1\"}\n");
        let manifest =
            RunManifest::build(file.path_str(), 1, &["local".to_string()], 3, 42).unwrap();

        assert_eq!(manifest.seed, 42);
        assert_eq!(manifest.item_count, 1);
        assert_eq!(manifest.systems, vec!["local".to_string()]);
        assert_eq!(manifest.top_k, 3);
        assert_eq!(
            manifest.dataset_sha256,
            sha256_file(file.path_str()).unwrap()
        );
    }

    struct TempFile {
        path: std::path::PathBuf,
    }

    impl TempFile {
        fn path_str(&self) -> &str {
            self.path.to_str().unwrap()
        }

        fn overwrite(&mut self, content: &[u8]) {
            std::fs::write(&self.path, content).unwrap();
        }
    }

    impl Drop for TempFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
        }
    }

    fn tempfile_with(content: &[u8]) -> TempFile {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);

        let mut path = std::env::temp_dir();
        path.push(format!(
            "cognoscenti-manifest-test-{}-{:?}-{unique}",
            std::process::id(),
            std::thread::current().id()
        ));
        let mut file = std::fs::File::create(&path).unwrap();
        file.write_all(content).unwrap();
        TempFile { path }
    }
}
