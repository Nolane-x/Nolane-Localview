#![forbid(unsafe_code)]

use std::{
    collections::{BTreeMap, VecDeque},
    hash::{Hash, Hasher},
    path::PathBuf,
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
        Ok(Self {
            root,
            max_bytes,
            index: BTreeMap::new(),
            lru: VecDeque::new(),
            used: 0,
        })
    }

    pub async fn put(&mut self, kind: &str, bytes: &[u8]) -> Result<ArtifactMeta> {
        let id = content_id(bytes);
        if let Some(existing) = self.index.get(&id) {
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
        self.used += meta.bytes;
        self.index.insert(id.clone(), meta.clone());
        self.lru.push_back(id);
        self.gc().await?;
        Ok(meta)
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
