#![forbid(unsafe_code)]

use std::{
    env,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use thiserror::Error;
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::Command,
    task::JoinHandle,
    time::timeout,
};
use url::{Host, Url};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChromiumExecutionPolicy {
    pub timeout: Duration,
    pub max_stdout_bytes: usize,
    pub max_stderr_bytes: usize,
    pub temp_root: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundedProcessOutput {
    pub retained: Vec<u8>,
    pub total_bytes: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChromiumExecutionReceipt {
    pub exit_code: Option<i32>,
    pub stdout: BoundedProcessOutput,
    pub stderr: BoundedProcessOutput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChromiumPlatform {
    Linux,
    Macos,
    Windows,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChromiumDiscoveryContext {
    pub explicit_executable: Option<PathBuf>,
    pub path_dirs: Vec<PathBuf>,
    pub home_dir: Option<PathBuf>,
    pub program_files: Option<PathBuf>,
    pub program_files_x86: Option<PathBuf>,
    pub local_app_data: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, Error, PartialEq, Eq)]
pub enum ChromiumExecutorError {
    #[error("Chromium target must be loopback HTTP(S)")]
    InvalidTarget,
    #[error("failed to prepare ephemeral Chromium profile")]
    Profile,
    #[error("failed to spawn Chromium")]
    Spawn,
    #[error("Chromium process I/O failed")]
    Io,
    #[error("Chromium process timed out")]
    Timeout,
    #[error("failed to remove ephemeral Chromium profile")]
    Cleanup,
}

pub fn discover_chromium_executable_with<F>(
    platform: ChromiumPlatform,
    context: &ChromiumDiscoveryContext,
    mut is_file: F,
) -> Option<PathBuf>
where
    F: FnMut(&Path) -> bool,
{
    chromium_executable_candidates(platform, context)
        .into_iter()
        .find(|candidate| is_file(candidate.as_path()))
}

pub fn discover_chromium_executable() -> Option<PathBuf> {
    let platform = current_platform()?;
    let context = ChromiumDiscoveryContext {
        explicit_executable: env::var_os("LOCALVIEW_CHROMIUM_EXECUTABLE").map(PathBuf::from),
        path_dirs: env::var_os("PATH")
            .map(|value| env::split_paths(&value).collect())
            .unwrap_or_default(),
        home_dir: env::var_os("HOME")
            .or_else(|| env::var_os("USERPROFILE"))
            .map(PathBuf::from),
        program_files: env::var_os("PROGRAMFILES").map(PathBuf::from),
        program_files_x86: env::var_os("PROGRAMFILES(X86)").map(PathBuf::from),
        local_app_data: env::var_os("LOCALAPPDATA").map(PathBuf::from),
    };
    discover_chromium_executable_with(platform, &context, Path::is_file)
}

fn current_platform() -> Option<ChromiumPlatform> {
    if cfg!(target_os = "linux") {
        Some(ChromiumPlatform::Linux)
    } else if cfg!(target_os = "macos") {
        Some(ChromiumPlatform::Macos)
    } else if cfg!(target_os = "windows") {
        Some(ChromiumPlatform::Windows)
    } else {
        None
    }
}

fn chromium_executable_candidates(
    platform: ChromiumPlatform,
    context: &ChromiumDiscoveryContext,
) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(explicit) = context.explicit_executable.clone() {
        push_unique(&mut candidates, explicit);
    }

    match platform {
        ChromiumPlatform::Linux => {
            push_path_candidates(
                &mut candidates,
                platform,
                &context.path_dirs,
                &[
                    "google-chrome",
                    "google-chrome-stable",
                    "chromium",
                    "chromium-browser",
                ],
            );
        }
        ChromiumPlatform::Macos => {
            for bundle in [
                "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
                "/Applications/Chromium.app/Contents/MacOS/Chromium",
            ] {
                push_unique(&mut candidates, PathBuf::from(bundle));
            }
            if let Some(home) = context.home_dir.as_deref() {
                for relative in [
                    "Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
                    "Applications/Chromium.app/Contents/MacOS/Chromium",
                ] {
                    push_unique(
                        &mut candidates,
                        platform_join(ChromiumPlatform::Macos, home, relative),
                    );
                }
            }
            push_path_candidates(
                &mut candidates,
                platform,
                &context.path_dirs,
                &["google-chrome", "chromium"],
            );
        }
        ChromiumPlatform::Windows => {
            for root in [
                context.local_app_data.as_deref(),
                context.program_files.as_deref(),
                context.program_files_x86.as_deref(),
            ]
            .into_iter()
            .flatten()
            {
                push_unique(
                    &mut candidates,
                    platform_join(
                        ChromiumPlatform::Windows,
                        root,
                        r"Google\Chrome\Application\chrome.exe",
                    ),
                );
            }
            for root in [
                context.local_app_data.as_deref(),
                context.program_files.as_deref(),
                context.program_files_x86.as_deref(),
            ]
            .into_iter()
            .flatten()
            {
                push_unique(
                    &mut candidates,
                    platform_join(
                        ChromiumPlatform::Windows,
                        root,
                        r"Chromium\Application\chrome.exe",
                    ),
                );
            }
            push_path_candidates(
                &mut candidates,
                platform,
                &context.path_dirs,
                &["chrome.exe", "chromium.exe"],
            );
        }
    }

    candidates
}

fn push_path_candidates(
    candidates: &mut Vec<PathBuf>,
    platform: ChromiumPlatform,
    path_dirs: &[PathBuf],
    executable_names: &[&str],
) {
    for executable in executable_names {
        for directory in path_dirs {
            push_unique(
                candidates,
                platform_join(platform, directory.as_path(), executable),
            );
        }
    }
}

fn platform_join(platform: ChromiumPlatform, base: &Path, relative: &str) -> PathBuf {
    let base = base.to_string_lossy();
    match platform {
        ChromiumPlatform::Windows => PathBuf::from(format!(
            "{}\\{}",
            base.trim_end_matches(['\\', '/']),
            relative.replace('/', "\\")
        )),
        ChromiumPlatform::Linux | ChromiumPlatform::Macos => PathBuf::from(format!(
            "{}/{}",
            base.trim_end_matches('/'),
            relative.replace('\\', "/")
        )),
    }
}

fn push_unique(candidates: &mut Vec<PathBuf>, candidate: PathBuf) {
    if !candidates.contains(&candidate) {
        candidates.push(candidate);
    }
}

pub fn validate_loopback_url(url: &Url) -> bool {
    if !matches!(url.scheme(), "http" | "https") {
        return false;
    }
    if !url.username().is_empty() || url.password().is_some() {
        return false;
    }
    match url.host() {
        Some(Host::Domain(host)) => host.eq_ignore_ascii_case("localhost"),
        Some(Host::Ipv4(address)) => address.is_loopback(),
        Some(Host::Ipv6(address)) => address.is_loopback(),
        None => false,
    }
}

pub async fn execute_ephemeral(
    executable: &Path,
    target: &Url,
    policy: &ChromiumExecutionPolicy,
) -> Result<ChromiumExecutionReceipt, ChromiumExecutorError> {
    if !validate_loopback_url(target) {
        return Err(ChromiumExecutorError::InvalidTarget);
    }

    tokio::fs::create_dir_all(&policy.temp_root)
        .await
        .map_err(|_| ChromiumExecutorError::Profile)?;
    let profile_dir = policy
        .temp_root
        .join(format!("localview-chromium-{}", Uuid::new_v4()));
    tokio::fs::create_dir(&profile_dir)
        .await
        .map_err(|_| ChromiumExecutorError::Profile)?;

    let mut child = match Command::new(executable)
        .arg("--headless=new")
        .arg("--dump-dom")
        .arg(format!("--user-data-dir={}", profile_dir.display()))
        .arg(target.as_str())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
    {
        Ok(child) => child,
        Err(_) => {
            cleanup_profile(&profile_dir).await?;
            return Err(ChromiumExecutorError::Spawn);
        }
    };

    let stdout = child.stdout.take().ok_or(ChromiumExecutorError::Io)?;
    let stderr = child.stderr.take().ok_or(ChromiumExecutorError::Io)?;
    let stdout_task = tokio::spawn(read_bounded(stdout, policy.max_stdout_bytes));
    let stderr_task = tokio::spawn(read_bounded(stderr, policy.max_stderr_bytes));

    let wait_result = timeout(policy.timeout, child.wait()).await;
    let execution_result = match wait_result {
        Ok(Ok(status)) => Ok(status.code()),
        Ok(Err(_)) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            Err(ChromiumExecutorError::Io)
        }
        Err(_) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            Err(ChromiumExecutorError::Timeout)
        }
    };

    let stdout = join_output(stdout_task).await;
    let stderr = join_output(stderr_task).await;
    let cleanup = cleanup_profile(&profile_dir).await;

    let exit_code = execution_result?;
    let stdout = stdout?;
    let stderr = stderr?;
    cleanup?;

    Ok(ChromiumExecutionReceipt {
        exit_code,
        stdout,
        stderr,
    })
}

async fn read_bounded<R>(
    mut reader: R,
    max_retained_bytes: usize,
) -> Result<BoundedProcessOutput, ChromiumExecutorError>
where
    R: AsyncRead + Unpin,
{
    let mut retained = Vec::with_capacity(max_retained_bytes.min(16 * 1024));
    let mut total_bytes = 0usize;
    let mut buffer = [0u8; 4096];
    loop {
        let read = reader
            .read(&mut buffer)
            .await
            .map_err(|_| ChromiumExecutorError::Io)?;
        if read == 0 {
            break;
        }
        total_bytes = total_bytes.saturating_add(read);
        let remaining = max_retained_bytes.saturating_sub(retained.len());
        if remaining > 0 {
            retained.extend_from_slice(&buffer[..read.min(remaining)]);
        }
    }
    Ok(BoundedProcessOutput {
        truncated: total_bytes > retained.len(),
        retained,
        total_bytes,
    })
}

async fn join_output(
    task: JoinHandle<Result<BoundedProcessOutput, ChromiumExecutorError>>,
) -> Result<BoundedProcessOutput, ChromiumExecutorError> {
    task.await.map_err(|_| ChromiumExecutorError::Io)?
}

async fn cleanup_profile(profile_dir: &Path) -> Result<(), ChromiumExecutorError> {
    match tokio::fs::remove_dir_all(profile_dir).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(ChromiumExecutorError::Cleanup),
    }
}
