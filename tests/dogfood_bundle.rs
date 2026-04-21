use std::path::Path;
use std::process::Command;

/// After `reen init --upgrade` in the reen repo, all managed files at the repo
/// root must be byte-identical to their counterparts in `templates/`. This is the
/// self-consistency gate: templates/ is the source of truth.
#[test]
fn dogfood_is_clean() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));

    // Run reen init --upgrade in the repo root.
    let output = Command::new(env!("CARGO_BIN_EXE_reen"))
        .args(["init", "--upgrade"])
        .current_dir(repo_root)
        .output()
        .expect("failed to run reen init --upgrade");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // The command should succeed (exit 0 — no blocked files).
    assert!(
        output.status.success(),
        "reen init --upgrade failed.\nstdout: {stdout}\nstderr: {stderr}"
    );

    // Now verify that every managed file at the repo root matches templates/.
    let bundle = reen::bundle::Bundle::load().expect("bundle loads");
    for file in bundle.managed_files() {
        let repo_path = repo_root.join(&file.path);
        assert!(
            repo_path.exists(),
            "managed file missing from repo root: {}",
            file.path
        );
        let disk_contents = std::fs::read(&repo_path).unwrap_or_else(|e| {
            panic!("failed to read {}: {e}", repo_path.display());
        });
        assert_eq!(
            disk_contents,
            file.contents,
            "repo root file {} differs from templates/ — edit templates/ first, then run `reen init --upgrade`",
            file.path
        );
    }
}

/// `reen init --check` should exit 0 when the repo is up to date.
#[test]
fn dogfood_check_clean() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));

    // First ensure we're up to date.
    let _ = Command::new(env!("CARGO_BIN_EXE_reen"))
        .args(["init", "--upgrade"])
        .current_dir(repo_root)
        .output()
        .expect("failed to run reen init --upgrade");

    // Now check should report up-to-date.
    let output = Command::new(env!("CARGO_BIN_EXE_reen"))
        .args(["init", "--check"])
        .current_dir(repo_root)
        .output()
        .expect("failed to run reen init --check");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "reen init --check reported pending changes.\nstdout: {stdout}"
    );
    assert!(
        stdout.contains("Up to date"),
        "expected 'Up to date' in output, got: {stdout}"
    );
}

/// Fresh init in a clean temp directory produces a working layout.
#[test]
fn fresh_init_produces_layout() {
    let tmpdir = tempfile::tempdir().expect("create tempdir");

    let output = Command::new(env!("CARGO_BIN_EXE_reen"))
        .args(["init", "--path", tmpdir.path().to_str().unwrap()])
        .output()
        .expect("failed to run reen init");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "reen init failed.\nstdout: {stdout}\nstderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Check key files exist.
    assert!(tmpdir.path().join(".claude/hooks.json").exists());
    assert!(tmpdir.path().join("CLAUDE.md").exists());
    assert!(tmpdir.path().join("DCI.md").exists());
    assert!(tmpdir.path().join("DRAFT_FORMAT.md").exists());
    assert!(tmpdir.path().join("reen.manifest.yml").exists());
    assert!(tmpdir.path().join(".reen-bundle.yml").exists());
    assert!(tmpdir.path().join(".gitignore").exists());
    assert!(tmpdir.path().join("drafts/contexts/.gitkeep").exists());
}
