#![forbid(unsafe_code)]

use std::{
    collections::{BTreeMap, VecDeque},
    fs,
    hash::{Hash, Hasher},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[cfg(unix)]
use std::os::unix::fs::MetadataExt;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactMeta {
    pub id: String,
    pub kind: String,
    pub bytes: u64,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalArtifactMeta {
    pub physical: ArtifactMeta,
    pub canonical_hash: String,
}

pub struct ArtifactStore {
    root: PathBuf,
    max_bytes: u64,
    index: BTreeMap<String, ArtifactMeta>,
    lru: VecDeque<String>,
    used: u64,
}

impl ArtifactStore {
    pub async fn open(root: impl Into<PathBuf>, max_bytes: u64) -> Result<Self> {
        let root = root.into();
        match tokio::fs::symlink_metadata(&root).await {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                anyhow::bail!("artifact root must be a real directory")
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                tokio::fs::create_dir_all(&root).await?;
            }
            Err(error) => return Err(error.into()),
        }
        let metadata = tokio::fs::symlink_metadata(&root).await?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            anyhow::bail!("artifact root must remain a real directory");
        }
        let root = tokio::fs::canonicalize(&root).await?;
        let mut store = Self {
            root,
            max_bytes,
            index: BTreeMap::new(),
            lru: VecDeque::new(),
            used: 0,
        };
        store.restore_existing().await?;
        store.gc().await?;
        Ok(store)
    }

    pub fn used_bytes(&self) -> u64 {
        self.used
    }

    pub fn max_bytes(&self) -> u64 {
        self.max_bytes
    }

    pub async fn put_canonical(
        &mut self,
        kind: &str,
        canonical_hash: &str,
        bytes: &[u8],
    ) -> Result<CanonicalArtifactMeta> {
        if !valid_canonical_hash(canonical_hash) {
            anyhow::bail!("canonical artifact hash must be a sha256:<64 hex> digest");
        }
        let physical = self.put(kind, bytes).await?;
        let retained = read_regular_file(Path::new(&physical.path))?;
        if retained != bytes {
            anyhow::bail!(
                "physical artifact id collision detected; canonical artifact was not retained"
            );
        }
        Ok(CanonicalArtifactMeta {
            physical,
            canonical_hash: canonical_hash.to_owned(),
        })
    }

    pub fn projected_used_bytes_after_put(&self, bytes: &[u8]) -> Result<u64> {
        let id = content_id(bytes);
        if self.index.contains_key(&id) {
            return Ok(self.used);
        }

        let incoming = bytes.len() as u64;
        if incoming > self.max_bytes {
            anyhow::bail!(
                "artifact requires {incoming} retained bytes but store limit is {}",
                self.max_bytes
            );
        }

        let mut projected = self.used.saturating_add(incoming);
        for id in &self.lru {
            if projected <= self.max_bytes {
                break;
            }
            if let Some(meta) = self.index.get(id) {
                projected = projected.saturating_sub(meta.bytes);
            }
        }

        if projected > self.max_bytes {
            anyhow::bail!(
                "artifact store cannot project usage within {} retained bytes",
                self.max_bytes
            );
        }
        Ok(projected)
    }

    pub async fn put(&mut self, kind: &str, bytes: &[u8]) -> Result<ArtifactMeta> {
        self.projected_used_bytes_after_put(bytes)?;
        self.revalidate_root().await?;

        let id = content_id(bytes);
        if let Some(existing) = self.index.get_mut(&id) {
            let retained = read_regular_file(Path::new(&existing.path))
                .with_context(|| format!("validate retained artifact {}", existing.id))?;
            if retained != bytes {
                anyhow::bail!(
                    "physical artifact id collision detected; retained bytes differ for {id}"
                );
            }
            if existing.kind == "retained" {
                existing.kind = kind.into();
            }
            return Ok(existing.clone());
        }

        let path = self.root.join(&id);
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        let mut file = match options.open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                anyhow::bail!(
                    "artifact destination already exists outside the retained index; refusing overwrite"
                )
            }
            Err(error) => return Err(error.into()),
        };
        if let Err(error) = file.write_all(bytes).and_then(|_| file.sync_all()) {
            drop(file);
            let _ = fs::remove_file(&path);
            return Err(error.into());
        }
        drop(file);

        let meta = ArtifactMeta {
            id: id.clone(),
            kind: kind.into(),
            bytes: bytes.len() as u64,
            path: path.to_string_lossy().into_owned(),
        };
        self.used = self.used.saturating_add(meta.bytes);
        self.index.insert(id.clone(), meta.clone());
        self.lru.push_back(id);
        self.gc().await?;
        Ok(meta)
    }

    async fn revalidate_root(&self) -> Result<()> {
        let metadata = tokio::fs::symlink_metadata(&self.root).await?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            anyhow::bail!("artifact root identity is no longer a real directory");
        }
        let canonical = tokio::fs::canonicalize(&self.root).await?;
        if canonical != self.root {
            anyhow::bail!("artifact root identity changed after store open");
        }
        Ok(())
    }

    async fn restore_existing(&mut self) -> Result<()> {
        let mut entries = tokio::fs::read_dir(&self.root).await?;
        let mut restored = Vec::new();
        while let Some(entry) = entries.next_entry().await? {
            let file_type = entry.file_type().await?;
            let Some(id) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if !is_content_id(&id) {
                continue;
            }
            if file_type.is_symlink() || !file_type.is_file() {
                anyhow::bail!("artifact store contains non-regular retained entry {id}");
            }
            let metadata = entry.metadata().await?;
            let modified = metadata.modified().unwrap_or(UNIX_EPOCH);
            let meta = ArtifactMeta {
                id: id.clone(),
                kind: "retained".into(),
                bytes: metadata.len(),
                path: entry.path().to_string_lossy().into_owned(),
            };
            restored.push((modified, id, meta));
        }

        restored.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
        for (_, id, meta) in restored {
            self.used = self.used.saturating_add(meta.bytes);
            self.lru.push_back(id.clone());
            self.index.insert(id, meta);
        }
        Ok(())
    }

    async fn gc(&mut self) -> Result<()> {
        while self.used > self.max_bytes {
            let Some(id) = self.lru.front().cloned() else {
                break;
            };
            let Some(meta) = self.index.get(&id).cloned() else {
                self.lru.pop_front();
                continue;
            };

            self.revalidate_root().await?;
            match tokio::fs::symlink_metadata(&meta.path).await {
                Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
                    anyhow::bail!("artifact GC refuses non-regular retained entry {}", meta.id)
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
            match tokio::fs::remove_file(&meta.path).await {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
            self.lru.pop_front();
            self.index.remove(&id);
            self.used = self.used.saturating_sub(meta.bytes);
        }
        Ok(())
    }
}


fn read_regular_file(path: &Path) -> Result<Vec<u8>> {
    let before = fs::symlink_metadata(path).context("inspect artifact path")?;
    if before.file_type().is_symlink() || !before.is_file() {
        anyhow::bail!("artifact path must be a regular file");
    }

    let mut file = fs::File::open(path).context("open retained artifact")?;
    let opened = file.metadata().context("inspect retained artifact")?;
    let after = fs::symlink_metadata(path).context("revalidate retained artifact path")?;
    if after.file_type().is_symlink() || !after.is_file() {
        anyhow::bail!("artifact path changed during read");
    }

    #[cfg(unix)]
    if opened.dev() != after.dev() || opened.ino() != after.ino() {
        anyhow::bail!("artifact identity changed during read");
    }

    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).context("read retained artifact")?;
    Ok(bytes)
}

fn valid_canonical_hash(value: &str) -> bool {
    let Some(digest) = value.strip_prefix("sha256:") else {
        return false;
    };
    digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn is_content_id(value: &str) -> bool {
    value.len() == 19
        && value.starts_with("lv-")
        && value[3..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn content_id(bytes: &[u8]) -> String {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut h);
    format!("lv-{:016x}", h.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_dir(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        std::env::temp_dir().join(format!("lv-art-{name}-{}-{nonce}", std::process::id()))
    }

    async fn disk_bytes(dir: &PathBuf) -> u64 {
        let mut total = 0_u64;
        let mut entries = tokio::fs::read_dir(dir).await.unwrap();
        while let Some(entry) = entries.next_entry().await.unwrap() {
            if entry.file_type().await.unwrap().is_file() {
                total += entry.metadata().await.unwrap().len();
            }
        }
        total
    }

    #[tokio::test]
    async fn dedupes_bytes() {
        let dir = test_dir("dedupe");
        let mut store = ArtifactStore::open(&dir, 1024).await.unwrap();
        let a = store.put("text", b"same").await.unwrap();
        let b = store.put("text", b"same").await.unwrap();
        assert_eq!(a.id, b.id);
        let _ = tokio::fs::remove_dir_all(dir).await;
    }

    #[tokio::test]
    async fn canonical_hash_and_physical_storage_id_remain_distinct() {
        let dir = test_dir("canonical");
        let mut store = ArtifactStore::open(&dir, 1024).await.unwrap();
        let canonical = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let retained = store
            .put_canonical("baseline/json", canonical, b"baseline")
            .await
            .unwrap();
        assert!(retained.physical.id.starts_with("lv-"));
        assert_eq!(retained.canonical_hash, canonical);
        assert_ne!(retained.physical.id, retained.canonical_hash);
        let _ = tokio::fs::remove_dir_all(dir).await;
    }

    #[tokio::test]
    async fn reopen_keeps_disk_usage_inside_budget() {
        let dir = test_dir("reopen-budget");
        {
            let mut first = ArtifactStore::open(&dir, 5).await.unwrap();
            first.put("visual/png", b"1234").await.unwrap();
        }
        {
            let mut reopened = ArtifactStore::open(&dir, 5).await.unwrap();
            reopened.put("visual/png", b"5678").await.unwrap();
        }

        assert!(
            disk_bytes(&dir).await <= 5,
            "reopening the store must count and evict pre-existing artifacts"
        );
        let _ = tokio::fs::remove_dir_all(dir).await;
    }

    #[tokio::test]
    async fn existing_physical_id_with_wrong_bytes_is_collision_not_dedupe() {
        let dir = test_dir("collision");
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let id = content_id(b"expected");
        tokio::fs::write(dir.join(&id), b"wrong").await.unwrap();

        let mut store = ArtifactStore::open(&dir, 1024).await.unwrap();
        let error = store.put("text", b"expected").await.unwrap_err().to_string();
        assert!(error.contains("collision"));
        assert_eq!(tokio::fs::read(dir.join(id)).await.unwrap(), b"wrong");
        let _ = tokio::fs::remove_dir_all(dir).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn malicious_artifact_symlink_is_rejected_on_reopen() {
        use std::os::unix::fs::symlink;

        let dir = test_dir("symlink-reopen");
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let outside = test_dir("symlink-outside");
        tokio::fs::write(&outside, b"outside").await.unwrap();
        symlink(&outside, dir.join("lv-0123456789abcdef")).unwrap();

        assert!(ArtifactStore::open(&dir, 1024).await.is_err());
        assert_eq!(tokio::fs::read(&outside).await.unwrap(), b"outside");
        let _ = tokio::fs::remove_dir_all(dir).await;
        let _ = tokio::fs::remove_file(outside).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn new_put_refuses_preexisting_leaf_symlink_without_touching_target() {
        use std::os::unix::fs::symlink;

        let dir = test_dir("symlink-put");
        let mut store = ArtifactStore::open(&dir, 1024).await.unwrap();
        let bytes = b"artifact";
        let outside = test_dir("symlink-target");
        tokio::fs::write(&outside, b"outside").await.unwrap();
        symlink(&outside, dir.join(content_id(bytes))).unwrap();

        let error = store.put("text", bytes).await.unwrap_err().to_string();
        assert!(error.contains("refusing overwrite"));
        assert_eq!(tokio::fs::read(&outside).await.unwrap(), b"outside");
        let _ = tokio::fs::remove_dir_all(dir).await;
        let _ = tokio::fs::remove_file(outside).await;
    }

    #[tokio::test]
    async fn verified_dedupe_keeps_retained_accounting_stable() {
        let dir = test_dir("verified-dedupe");
        let mut store = ArtifactStore::open(&dir, 1024).await.unwrap();
        let first = store.put("visual/png", b"same").await.unwrap();
        let used = store.used_bytes();
        let second = store.put("visual/png", b"same").await.unwrap();
        assert_eq!(first.id, second.id);
        assert_eq!(store.used_bytes(), used);
        assert_eq!(disk_bytes(&dir).await, used);
        let _ = tokio::fs::remove_dir_all(dir).await;
    }

}
