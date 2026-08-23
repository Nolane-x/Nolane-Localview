#![forbid(unsafe_code)]

use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    process::Output,
};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectRevision {
    pub root: String,
    pub branch: Option<String>,
    pub commit: String,
    pub detached: bool,
    pub dirty_files: Vec<String>,
    pub working_tree_id: String,
}

impl ProjectRevision {
    pub fn dirty(&self) -> bool {
        !self.dirty_files.is_empty()
    }
}

#[derive(Debug, Error)]
pub enum ProjectStateError {
    #[error("git executable is unavailable: {0}")]
    GitUnavailable(#[source] std::io::Error),
    #[error("git command failed: {command}: {stderr}")]
    GitCommand { command: String, stderr: String },
    #[error("git repository has no readable HEAD revision")]
    MissingHead,
}

pub async fn inspect_git(root_hint: impl AsRef<Path>) -> Result<ProjectRevision, ProjectStateError> {
    let root_hint = root_hint.as_ref();
    let root = git_text(root_hint, &["rev-parse", "--show-toplevel"]).await?;
    let root_path = PathBuf::from(root.trim());
    let commit = git_text(&root_path, &["rev-parse", "HEAD"]).await?;
    let commit = commit.trim().to_owned();
    if commit.is_empty() {
        return Err(ProjectStateError::MissingHead);
    }

    let branch_text = git_text(&root_path, &["rev-parse", "--abbrev-ref", "HEAD"]).await?;
    let branch_text = branch_text.trim();
    let detached = branch_text == "HEAD";
    let branch = (!detached && !branch_text.is_empty()).then(|| branch_text.to_owned());

    let mut dirty_files = BTreeSet::new();
    let changed = git_bytes(&root_path, &["diff", "--name-only", "-z", "HEAD", "--"]).await?;
    dirty_files.extend(parse_nul_names(&changed));
    let untracked = git_bytes(
        &root_path,
        &["ls-files", "--others", "--exclude-standard", "-z"],
    )
    .await?;
    dirty_files.extend(parse_nul_names(&untracked));

    let dirty_files = dirty_files.into_iter().collect::<Vec<_>>();
    let working_tree_id = working_tree_id(&commit, dirty_files.len());
    Ok(ProjectRevision {
        root: root_path.to_string_lossy().into_owned(),
        branch,
        commit,
        detached,
        dirty_files,
        working_tree_id,
    })
}

pub fn working_tree_id(commit: &str, dirty_count: usize) -> String {
    if dirty_count == 0 {
        format!("wt:{commit}")
    } else {
        format!("wt:{commit}+dirty.{dirty_count}")
    }
}

fn parse_nul_names(bytes: &[u8]) -> Vec<String> {
    bytes
        .split(|byte| *byte == 0)
        .filter(|item| !item.is_empty())
        .map(|item| String::from_utf8_lossy(item).into_owned())
        .collect()
}

async fn git_text(root: &Path, args: &[&str]) -> Result<String, ProjectStateError> {
    let output = git_output(root, args).await?;
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

async fn git_bytes(root: &Path, args: &[&str]) -> Result<Vec<u8>, ProjectStateError> {
    Ok(git_output(root, args).await?.stdout)
}

async fn git_output(root: &Path, args: &[&str]) -> Result<Output, ProjectStateError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .await
        .map_err(ProjectStateError::GitUnavailable)?;
    if output.status.success() {
        return Ok(output);
    }
    Err(ProjectStateError::GitCommand {
        command: format!("git -C {} {}", root.display(), args.join(" ")),
        stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn working_tree_identity_is_explicit_about_dirty_state() {
        assert_eq!(working_tree_id("abc123", 0), "wt:abc123");
        assert_eq!(working_tree_id("abc123", 4), "wt:abc123+dirty.4");
    }

    #[test]
    fn nul_name_parser_preserves_spaces_and_drops_empty_tail() {
        let names = parse_nul_names(b"src/App.tsx\0docs/a file.md\0");
        assert_eq!(names, vec!["src/App.tsx", "docs/a file.md"]);
    }
}
