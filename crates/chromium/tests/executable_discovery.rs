use std::path::{Path, PathBuf};

use localview_chromium::{
    discover_chromium_executable_with, ChromiumDiscoveryContext, ChromiumPlatform,
};

fn path(value: &str) -> PathBuf {
    PathBuf::from(value)
}

fn existing<'a>(paths: &'a [&'a str]) -> impl Fn(&Path) -> bool + 'a {
    move |candidate| paths.iter().any(|expected| candidate == Path::new(expected))
}

#[test]
fn explicit_process_configuration_has_highest_priority_but_must_exist() {
    let context = ChromiumDiscoveryContext {
        explicit_executable: Some(path("/opt/localview/chrome")),
        path_dirs: vec![path("/usr/bin")],
        ..Default::default()
    };

    let selected = discover_chromium_executable_with(
        ChromiumPlatform::Linux,
        &context,
        existing(&["/opt/localview/chrome", "/usr/bin/google-chrome"]),
    );
    assert_eq!(selected, Some(path("/opt/localview/chrome")));

    let fallback = discover_chromium_executable_with(
        ChromiumPlatform::Linux,
        &context,
        existing(&["/usr/bin/google-chrome"]),
    );
    assert_eq!(fallback, Some(path("/usr/bin/google-chrome")));
}

#[test]
fn linux_prefers_google_chrome_then_chromium_from_path() {
    let context = ChromiumDiscoveryContext {
        path_dirs: vec![path("/custom/bin"), path("/usr/bin")],
        ..Default::default()
    };

    let chrome = discover_chromium_executable_with(
        ChromiumPlatform::Linux,
        &context,
        existing(&[
            "/custom/bin/chromium",
            "/usr/bin/google-chrome",
            "/usr/bin/chromium",
        ]),
    );
    assert_eq!(chrome, Some(path("/usr/bin/google-chrome")));

    let chromium = discover_chromium_executable_with(
        ChromiumPlatform::Linux,
        &context,
        existing(&["/custom/bin/chromium", "/usr/bin/chromium"]),
    );
    assert_eq!(chromium, Some(path("/custom/bin/chromium")));
}

#[test]
fn macos_checks_system_and_user_application_bundles_before_path() {
    let context = ChromiumDiscoveryContext {
        path_dirs: vec![path("/usr/local/bin")],
        home_dir: Some(path("/Users/dev")),
        ..Default::default()
    };

    let system = discover_chromium_executable_with(
        ChromiumPlatform::Macos,
        &context,
        existing(&[
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            "/usr/local/bin/chromium",
        ]),
    );
    assert_eq!(
        system,
        Some(path(
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
        ))
    );

    let user = discover_chromium_executable_with(
        ChromiumPlatform::Macos,
        &context,
        existing(&[
            "/Users/dev/Applications/Chromium.app/Contents/MacOS/Chromium",
            "/usr/local/bin/chromium",
        ]),
    );
    assert_eq!(
        user,
        Some(path(
            "/Users/dev/Applications/Chromium.app/Contents/MacOS/Chromium"
        ))
    );
}

#[test]
fn windows_prefers_local_chrome_then_program_files_then_path() {
    let context = ChromiumDiscoveryContext {
        path_dirs: vec![path(r"C:\Tools")],
        local_app_data: Some(path(r"C:\Users\dev\AppData\Local")),
        program_files: Some(path(r"C:\Program Files")),
        program_files_x86: Some(path(r"C:\Program Files (x86)")),
        ..Default::default()
    };

    let local = discover_chromium_executable_with(
        ChromiumPlatform::Windows,
        &context,
        existing(&[
            r"C:\Users\dev\AppData\Local\Google\Chrome\Application\chrome.exe",
            r"C:\Program Files\Google\Chrome\Application\chrome.exe",
            r"C:\Tools\chrome.exe",
        ]),
    );
    assert_eq!(
        local,
        Some(path(
            r"C:\Users\dev\AppData\Local\Google\Chrome\Application\chrome.exe"
        ))
    );

    let system = discover_chromium_executable_with(
        ChromiumPlatform::Windows,
        &context,
        existing(&[
            r"C:\Program Files\Google\Chrome\Application\chrome.exe",
            r"C:\Tools\chrome.exe",
        ]),
    );
    assert_eq!(
        system,
        Some(path(r"C:\Program Files\Google\Chrome\Application\chrome.exe"))
    );
}

#[test]
fn discovery_fails_closed_when_no_candidate_exists() {
    let context = ChromiumDiscoveryContext {
        path_dirs: vec![path("/usr/bin")],
        ..Default::default()
    };

    assert_eq!(
        discover_chromium_executable_with(ChromiumPlatform::Linux, &context, |_| false),
        None
    );
}
