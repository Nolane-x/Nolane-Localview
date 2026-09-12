#![forbid(unsafe_code)]

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use localview_chromium::{
    execute_rendered_screenshot, ChromiumExecutionPolicy, ChromiumExecutorError,
    ChromiumScreenshotRequest,
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
        "localview-chromium-rendered-{name}-{}-{}",
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

fn policy(profile_root: PathBuf, timeout: Duration) -> ChromiumExecutionPolicy {
    ChromiumExecutionPolicy {
        timeout,
        max_stdout_bytes: 256,
        max_stderr_bytes: 8 * 1024,
        temp_root: profile_root,
    }
}

fn assert_empty_dir(path: &Path) {
    let mut entries = fs::read_dir(path).expect("profile temp root must exist");
    assert!(
        entries.next().is_none(),
        "rendered-pixel execution must remove ephemeral browser state and screenshot files"
    );
}

#[tokio::test]
async fn rendered_executor_returns_bounded_png_with_verified_dimensions_and_safe_flags() {
    let source = r#"
use std::{env, fs};

fn main() {
    let args = env::args().skip(1).collect::<Vec<_>>();
    eprintln!("ARGS={}", args.join("|"));
    let screenshot = args.iter()
        .find_map(|arg| arg.strip_prefix("--screenshot="))
        .expect("screenshot path");
    let mut png = vec![137, 80, 78, 71, 13, 10, 26, 10];
    png.extend_from_slice(&13u32.to_be_bytes());
    png.extend_from_slice(b"IHDR");
    png.extend_from_slice(&320u32.to_be_bytes());
    png.extend_from_slice(&180u32.to_be_bytes());
    png.extend_from_slice(&[8, 6, 0, 0, 0]);
    png.extend_from_slice(&[0, 0, 0, 0]);
    fs::write(screenshot, png).expect("write deterministic PNG fixture");
}
"#;
    let (fixture_root, executable) = compile_fixture("pixels", source);
    let profile_root = test_dir("profiles-success");
    fs::create_dir_all(&profile_root).expect("profile root");
    let target = Url::parse("http://127.0.0.1:5173/settings").expect("loopback target");

    let receipt = execute_rendered_screenshot(
        &executable,
        &target,
        &policy(profile_root.clone(), Duration::from_secs(3)),
        &ChromiumScreenshotRequest {
            pixel_width: 320,
            pixel_height: 180,
            max_png_bytes: 64 * 1024,
        },
    )
    .await
    .expect("rendered screenshot execution");

    assert_eq!(receipt.exit_code, Some(0));
    assert_eq!(receipt.pixel_width, 320);
    assert_eq!(receipt.pixel_height, 180);
    assert!(receipt.png.starts_with(&[137, 80, 78, 71, 13, 10, 26, 10]));
    assert!(receipt.png.len() <= 64 * 1024);
    let stderr = String::from_utf8_lossy(&receipt.stderr.retained);
    assert!(stderr.contains("--headless=new"));
    assert!(stderr.contains("--window-size=320,180"));
    assert!(stderr.contains("--screenshot="));
    assert!(stderr.contains("--user-data-dir="));
    assert!(stderr.contains(target.as_str()));
    assert!(!stderr.contains("--dump-dom"));
    assert!(!stderr.contains("--no-sandbox"));
    assert_empty_dir(&profile_root);

    let _ = fs::remove_dir_all(fixture_root);
    let _ = fs::remove_dir_all(profile_root);
}

#[tokio::test]
async fn rendered_executor_rejects_oversized_screenshot_and_cleans_profile() {
    let source = r#"
use std::{env, fs};
fn main() {
    let args = env::args().skip(1).collect::<Vec<_>>();
    let screenshot = args.iter()
        .find_map(|arg| arg.strip_prefix("--screenshot="))
        .expect("screenshot path");
    fs::write(screenshot, vec![7u8; 4096]).expect("write oversized fixture");
}
"#;
    let (fixture_root, executable) = compile_fixture("oversized", source);
    let profile_root = test_dir("profiles-oversized");
    fs::create_dir_all(&profile_root).expect("profile root");
    let target = Url::parse("http://localhost:5173/").expect("loopback target");

    let error = execute_rendered_screenshot(
        &executable,
        &target,
        &policy(profile_root.clone(), Duration::from_secs(3)),
        &ChromiumScreenshotRequest {
            pixel_width: 320,
            pixel_height: 180,
            max_png_bytes: 512,
        },
    )
    .await
    .expect_err("oversized screenshot must fail closed");

    assert_eq!(error, ChromiumExecutorError::ScreenshotTooLarge);
    assert_empty_dir(&profile_root);

    let _ = fs::remove_dir_all(fixture_root);
    let _ = fs::remove_dir_all(profile_root);
}

#[tokio::test]
async fn rendered_executor_rejects_invalid_or_wrong_dimension_png() {
    let source = r#"
use std::{env, fs};
fn main() {
    let args = env::args().skip(1).collect::<Vec<_>>();
    let screenshot = args.iter()
        .find_map(|arg| arg.strip_prefix("--screenshot="))
        .expect("screenshot path");
    let mut png = vec![137, 80, 78, 71, 13, 10, 26, 10];
    png.extend_from_slice(&13u32.to_be_bytes());
    png.extend_from_slice(b"IHDR");
    png.extend_from_slice(&640u32.to_be_bytes());
    png.extend_from_slice(&360u32.to_be_bytes());
    png.extend_from_slice(&[8, 6, 0, 0, 0]);
    png.extend_from_slice(&[0, 0, 0, 0]);
    fs::write(screenshot, png).expect("write mismatched PNG fixture");
}
"#;
    let (fixture_root, executable) = compile_fixture("wrong-size", source);
    let profile_root = test_dir("profiles-wrong-size");
    fs::create_dir_all(&profile_root).expect("profile root");
    let target = Url::parse("http://localhost:5173/").expect("loopback target");

    let error = execute_rendered_screenshot(
        &executable,
        &target,
        &policy(profile_root.clone(), Duration::from_secs(3)),
        &ChromiumScreenshotRequest {
            pixel_width: 320,
            pixel_height: 180,
            max_png_bytes: 64 * 1024,
        },
    )
    .await
    .expect_err("mismatched rendered dimensions must fail closed");

    assert_eq!(error, ChromiumExecutorError::InvalidScreenshot);
    assert_empty_dir(&profile_root);

    let _ = fs::remove_dir_all(fixture_root);
    let _ = fs::remove_dir_all(profile_root);
}

#[tokio::test]
async fn rendered_executor_timeout_kills_child_and_cleans_profile() {
    let source = r#"
fn main() {
    std::thread::sleep(std::time::Duration::from_secs(5));
}
"#;
    let (fixture_root, executable) = compile_fixture("timeout", source);
    let profile_root = test_dir("profiles-timeout");
    fs::create_dir_all(&profile_root).expect("profile root");
    let target = Url::parse("http://localhost:5173/").expect("loopback target");

    let error = execute_rendered_screenshot(
        &executable,
        &target,
        &policy(profile_root.clone(), Duration::from_millis(75)),
        &ChromiumScreenshotRequest {
            pixel_width: 320,
            pixel_height: 180,
            max_png_bytes: 64 * 1024,
        },
    )
    .await
    .expect_err("sleeping rendered child must time out");

    assert_eq!(error, ChromiumExecutorError::Timeout);
    assert_empty_dir(&profile_root);

    let _ = fs::remove_dir_all(fixture_root);
    let _ = fs::remove_dir_all(profile_root);
}
