use std::{fs, path::PathBuf};

const SHIPPING_MANIFESTS: &[&str] = &[
    "apps/daemon/Cargo.toml",
    "apps/cli/Cargo.toml",
    "apps/desktop/src-tauri/Cargo.toml",
    "crates/native-provider/Cargo.toml",
    "crates/windows-uia-provider/Cargo.toml",
    "crates/windows-observe-runtime/Cargo.toml",
    "crates/observation/Cargo.toml",
    "crates/control/Cargo.toml",
];

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("validation-lab must live at <repo>/crates/validation-lab")
        .to_path_buf()
}

#[test]
fn shipping_manifests_do_not_depend_on_validation_lab() {
    let root = repo_root();
    for relative in SHIPPING_MANIFESTS {
        let path = root.join(relative);
        let text = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("failed reading {}: {error}", path.display()));
        assert!(
            !text.contains("localview-validation-lab"),
            "shipping manifest {} must not depend on localview-validation-lab",
            path.display()
        );
    }
}

#[test]
fn validation_lab_is_a_workspace_member_not_a_shipping_dependency() {
    let root = repo_root();
    let workspace = fs::read_to_string(root.join("Cargo.toml")).unwrap();
    assert!(
        workspace.contains("\"crates/validation-lab\""),
        "workspace must include crates/validation-lab"
    );
}
