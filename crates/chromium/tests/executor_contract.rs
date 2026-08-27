#![forbid(unsafe_code)]

use std::{
    env,
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use localview_chromium::{
    execute_ephemeral, validate_loopback_url, ChromiumExecutionPolicy, ChromiumExecutorError,
};
use url::Url;

fn nonce() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

fn test_dir(name: &str) -> PathBuf {
    env::temp_dir().join(format!(
        "localview-chromium-{name}-{}-{}",
        std::process::id(),
        nonce()
    ))
}

fn compile_fixture(name: &str, source: &str) -> (PathBuf, PathBuf) {
    let root = test_dir(name);
    fs::create_dir_all(&root).expect("fixture root");
    let source_path = root.join("fixture.rs");
    fs::write(&source_path, source).expect("fixture source");
    let executable = root.join(if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_owned()
    });
    let rustc = env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let status = Command::new(rustc)
        .arg(&source_path)
        .arg("-O")
        .arg("-o")
        .arg(&executable)
        .status()
        .expect("invoke rustc for deterministic fake Chromium");
    assert!(status.success(), "fake Chromium fixture must compile");
    (root, executable)
}

fn assert_empty_dir(path: &Path) {
    let mut entries = fs::read_dir(path).expect("profile temp root must exist");
    assert!(
        entries.next().is_none(),
        "ephemeral browser profiles must be removed after execution"
    );
}

#[test]
fn chromium_targets_are_strictly_loopback_http_or_https() {
    for accepted in [
        "http://127.0.0.1:5173/",
        "https://localhost:3000/settings",
        "http://[::1]:8080/",
    ] {
        assert!(
            validate_loopback_url(&Url::parse(accepted).expect("accepted URL")),
            "{accepted} should be admitted"
        );
    }

    for rejected in [
        "https://example.com/",
        "file:///tmp/index.html",
        "http://user@localhost:3000/",
        "http://192.168.1.50:5173/",
    ] {
        assert!(
            !validate_loopback_url(&Url::parse(rejected).expect("rejected URL")),
            "{rejected} must fail closed"
        );
    }
}

#[tokio::test]
async fn executor_uses_ephemeral_profile_safe_flags_and_bounded_output() {
    let source = r#"
fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    eprintln!("ARGS={}", args.join("|"));
    print!("{}", "x".repeat(16 * 1024));
    eprint!("{}", "e".repeat(4 * 1024));
}
"#;
    let (fixture_root, executable) = compile_fixture("bounded", source);
    let profile_root = test_dir("profiles-success");
    fs::create_dir_all(&profile_root).expect("profile root");
    let target = Url::parse("http://127.0.0.1:5173/settings").expect("loopback target");

    let receipt = execute_ephemeral(
        &executable,
        &target,
        &ChromiumExecutionPolicy {
            timeout: Duration::from_secs(3),
            max_stdout_bytes: 512,
            max_stderr_bytes: 8 * 1024,
            temp_root: profile_root.clone(),
        },
    )
    .await
    .expect("fake Chromium execution");

    assert_eq!(receipt.exit_code, Some(0));
    assert!(receipt.stdout.total_bytes >= 16 * 1024);
    assert_eq!(receipt.stdout.retained.len(), 512);
    assert!(receipt.stdout.truncated);
    let stderr = String::from_utf8_lossy(&receipt.stderr.retained);
    assert!(stderr.contains("--headless=new"));
    assert!(stderr.contains("--dump-dom"));
    assert!(stderr.contains("--user-data-dir="));
    assert!(stderr.contains(target.as_str()));
    assert!(
        !stderr.contains("--no-sandbox"),
        "executor must not weaken the browser sandbox"
    );
    assert_empty_dir(&profile_root);

    let _ = fs::remove_dir_all(fixture_root);
    let _ = fs::remove_dir_all(profile_root);
}

#[tokio::test]
async fn timeout_kills_child_and_removes_ephemeral_profile() {
    let source = r#"
fn main() {
    std::thread::sleep(std::time::Duration::from_secs(5));
}
"#;
    let (fixture_root, executable) = compile_fixture("timeout", source);
    let profile_root = test_dir("profiles-timeout");
    fs::create_dir_all(&profile_root).expect("profile root");
    let target = Url::parse("http://localhost:5173/").expect("loopback target");

    let error = execute_ephemeral(
        &executable,
        &target,
        &ChromiumExecutionPolicy {
            timeout: Duration::from_millis(75),
            max_stdout_bytes: 128,
            max_stderr_bytes: 128,
            temp_root: profile_root.clone(),
        },
    )
    .await
    .expect_err("sleeping child must time out");

    assert_eq!(error, ChromiumExecutorError::Timeout);
    assert_empty_dir(&profile_root);

    let _ = fs::remove_dir_all(fixture_root);
    let _ = fs::remove_dir_all(profile_root);
}

#[tokio::test]
async fn external_target_is_rejected_before_process_spawn() {
    let target = Url::parse("https://example.com/").expect("external target");
    let profile_root = test_dir("profiles-invalid-target");
    fs::create_dir_all(&profile_root).expect("profile root");

    let error = execute_ephemeral(
        Path::new("definitely-not-a-real-chromium-binary"),
        &target,
        &ChromiumExecutionPolicy {
            timeout: Duration::from_secs(1),
            max_stdout_bytes: 64,
            max_stderr_bytes: 64,
            temp_root: profile_root.clone(),
        },
    )
    .await
    .expect_err("external target must be denied before spawn");

    assert_eq!(error, ChromiumExecutorError::InvalidTarget);
    assert_empty_dir(&profile_root);
    let _ = fs::remove_dir_all(profile_root);
}
