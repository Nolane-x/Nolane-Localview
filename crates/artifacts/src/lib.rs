#![forbid(unsafe_code)]

use std::{
    collections::{BTreeMap, VecDeque},
    hash::{Hash, Hasher},
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactMeta {
    pub id: String,
    pub kind: String,
    pub bytes: u64,
    pub path: String,
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
        tokio::fs::create_dir_all(&root).await?;
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

    pub async fn put(&mut self, kind: &str, bytes: &[u8]) -> Result<ArtifactMeta> {
        let id = content_id(bytes);
        if let Some(existing) = self.index.get_mut(&id) {
            if existing.kind == "retained" {
                existing.kind = kind.into();
            }
            return Ok(existing.clone());
        }
        let path = self.root.join(&id);
        tokio::fs::write(&path, bytes).await?;
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

    async fn restore_existing(&mut self) -> Result<()> {
        let mut entries = tokio::fs::read_dir(&self.root).await?;
        let mut restored = Vec::new();
        while let Some(entry) = entries.next_entry().await? {
            let file_type = entry.file_type().await?;
            if !file_type.is_file() {
                continue;
            }
            let Some(id) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if !is_content_id(&id) {
                continue;
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

        restored.sort_by(|left, right| {
            left.0
                .cmp(&right.0)
                .then_with(|| left.1.cmp(&right.1))
        });
        for (_, id, meta) in restored {
            self.used = self.used.saturating_add(meta.bytes);
            self.lru.push_back(id.clone());
            self.index.insert(id, meta);
        }
        Ok(())
    }

    async fn gc(&mut self) -> Result<()> {
        while self.used > self.max_bytes {
            let Some(id) = self.lru.pop_front() else {
                break;
            };
            if let Some(meta) = self.index.remove(&id) {
                let _ = tokio::fs::remove_file(&meta.path).await;
                self.used = self.used.saturating_sub(meta.bytes);
            }
        }
        Ok(())
    }
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
}
