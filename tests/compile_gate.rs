//! Compile-parity gate.
//!
//! This is the lock-in step from the compile-parity sprint. It drives the
//! `tests/lupa` fixture end-to-end through the pipeline (`reen clean`,
//! `reen prepare`, `reen scaffold`) and then runs `cargo check` against the
//! generated multi-crate workspace. A non-zero exit at any stage fails the
//! test.
//!
//! The gate's value: every reen change runs through here. If a codegen,
//! prepare, or schema regression breaks the lupa scaffold's compilability,
//! this test catches it before review — the same regressions that Group 2.5
//! and Group 4 had to fix retroactively.
//!
//! ## Why lupa specifically
//!
//! Lupa is the canonical fixture in the project: it exercises every artefact
//! kind (data, projection, context, component, brand, routing table, web
//! app), the multi-crate layout, variant data types (ChatTurn,
//! TransferRequest), multiplicity inference (TransactionList.transactions →
//! Vec<Transaction>), and signature inference (Money.add binary, Session
//! state-transition with quoted-string parameter, etc.). Compiling that
//! end-to-end is a meaningful structural-correctness signal.
//!
//! ## Why these specific assertions
//!
//! The test asserts each pipeline step's exit code separately so the failure
//! message points at the right stage. A cargo-check failure with a multi-page
//! compiler diff is far harder to triage than "scaffold returned non-zero".
//!
//! ## What the test does NOT cover
//!
//! - It does not run `/implement`, so attribute placeholder fields stay at
//!   `()`. The wrapper-struct + payload-enum shape compiles with `()`, so
//!   that's fine.
//! - It does not run `cargo test` against the generated workspace. The
//!   scaffolded code calls `todo!()`, so any test would panic immediately.
//! - LLM-backed test synthesis is skipped (we clear `ANTHROPIC_API_KEY`),
//!   so synthesised invariant tests are not exercised. Bug 7 / Group 6 will
//!   land an offline validator.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::Mutex;

/// Serialise the lupa-driving tests. Both tests in this file mutate
/// `tests/lupa/{src,crates,drafts/prepare,.reen}` and would clobber each
/// other if `cargo test` ran them in parallel. The mutex is poison-tolerant
/// because either test failing leaves the lupa tree in a recoverable shape
/// for the next run.
static LUPA_LOCK: Mutex<()> = Mutex::new(());

fn lock_lupa() -> std::sync::MutexGuard<'static, ()> {
    LUPA_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn lupa_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/lupa")
}

fn run_reen(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_reen"))
        .args(args)
        .current_dir(lupa_path())
        // Skip LLM-backed test synthesis so the gate runs offline. Setting
        // the variable to empty rather than removing it stops the .env file
        // (loaded by dotenvy on startup) from re-introducing a real key.
        .env("ANTHROPIC_API_KEY", "")
        .output()
        .expect("failed to run reen")
}

fn pretty_output(stage: &str, out: &Output) -> String {
    format!(
        "{} exit={:?}\n--- stdout ---\n{}\n--- stderr ---\n{}",
        stage,
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    )
}

/// Recursively walk `root` and collect `(relative_path, file_bytes)` for
/// every file under it. Sorting comes for free from `BTreeMap`. Symlinks and
/// non-files are skipped.
fn snapshot_dir(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut out: BTreeMap<PathBuf, Vec<u8>> = BTreeMap::new();
    if !root.is_dir() {
        return out;
    }
    walk(root, root, &mut out);
    out
}

fn walk(base: &Path, current: &Path, into: &mut BTreeMap<PathBuf, Vec<u8>>) {
    let entries = match std::fs::read_dir(current) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        // Cargo's incremental cache is noisy across runs and isn't part of
        // what reen produces — skip it explicitly.
        if path.file_name().map(|n| n == "target").unwrap_or(false) {
            continue;
        }
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        if meta.is_dir() {
            walk(base, &path, into);
        } else if meta.is_file() {
            let rel = path.strip_prefix(base).unwrap_or(&path).to_path_buf();
            if let Ok(bytes) = std::fs::read(&path) {
                into.insert(rel, bytes);
            }
        }
    }
}

/// Format a side-by-side diff of two snapshots for the assertion message.
/// Names paths that appear in only one map and the byte length of mismatched
/// files. Returns an empty string when the snapshots are identical.
fn diff_snapshots(
    before: &BTreeMap<PathBuf, Vec<u8>>,
    after: &BTreeMap<PathBuf, Vec<u8>>,
) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let all_paths: std::collections::BTreeSet<&PathBuf> =
        before.keys().chain(after.keys()).collect();
    for path in all_paths {
        match (before.get(path), after.get(path)) {
            (Some(a), Some(b)) if a == b => {}
            (Some(a), Some(b)) => {
                writeln!(
                    out,
                    "  changed: {} ({} → {} bytes)",
                    path.display(),
                    a.len(),
                    b.len(),
                )
                .ok();
            }
            (Some(a), None) => {
                writeln!(out, "  removed: {} ({} bytes)", path.display(), a.len()).ok();
            }
            (None, Some(b)) => {
                writeln!(out, "  added:   {} ({} bytes)", path.display(), b.len()).ok();
            }
            (None, None) => {}
        }
    }
    out
}

#[test]
fn lupa_pipeline_compiles_end_to_end() {
    let _guard = lock_lupa();
    let lupa = lupa_path();

    // 1. Reset the generated tree so the scaffold runs from a clean slate.
    //    This avoids stale state from a previous test bleeding in.
    let clean = run_reen(&["clean", "--force"]);
    assert!(
        clean.status.success(),
        "{}",
        pretty_output("reen clean --force", &clean),
    );

    // 2. Prepare — must accept lupa's drafts without blocking ambiguities.
    let prepare = run_reen(&["prepare"]);
    assert!(
        prepare.status.success(),
        "{}",
        pretty_output("reen prepare", &prepare),
    );

    // 3. Scaffold — must produce a complete multi-crate workspace.
    let scaffold = run_reen(&["scaffold"]);
    assert!(
        scaffold.status.success(),
        "{}",
        pretty_output("reen scaffold", &scaffold),
    );

    // 4. cargo check — the structural lock-in. Every regression in codegen,
    //    prepare, or schema that breaks compilability shows up here.
    //    `--offline` keeps the gate fast and reproducible; if a dependency is
    //    missing locally the CI image must be primed beforehand.
    let cargo = std::env::var_os("CARGO")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("cargo"));
    let check = Command::new(&cargo)
        .args(["check", "--offline", "--quiet"])
        .current_dir(&lupa)
        .output()
        .expect("failed to run cargo check");
    assert!(
        check.status.success(),
        "{}",
        pretty_output("cargo check (lupa)", &check),
    );
}

/// Determinism gate. Runs the lupa pipeline twice and asserts every
/// generated file is byte-identical between the two runs. Pairs with
/// `lupa_pipeline_compiles_end_to_end`: that test asserts the pipeline
/// compiles, this one asserts the pipeline is reproducible.
///
/// Why we need this: bug 7 (test-synth validation) introduces retry logic,
/// finding emission, and per-test rejection paths. Any of those is a place
/// where a `HashMap` iteration order, a `SystemTime::now()`, or an unsorted
/// `Vec` could leak into the prepared YAML or scaffold output. A passing
/// idempotency test is the cheapest possible insurance against that whole
/// class of CI flake.
///
/// Snapshot scope: every file under `tests/lupa/` except `target/` (Cargo
/// owns its own cache).
#[test]
fn lupa_scaffold_is_deterministic() {
    let _guard = lock_lupa();
    let lupa = lupa_path();

    // First pass: clean → prepare → scaffold → snapshot.
    let clean = run_reen(&["clean", "--force"]);
    assert!(clean.status.success(), "{}", pretty_output("clean (run 1)", &clean));
    let prepare1 = run_reen(&["prepare"]);
    assert!(prepare1.status.success(), "{}", pretty_output("prepare (run 1)", &prepare1));
    let scaffold1 = run_reen(&["scaffold"]);
    assert!(scaffold1.status.success(), "{}", pretty_output("scaffold (run 1)", &scaffold1));
    let snapshot_1 = snapshot_dir(&lupa);

    // Second pass: prepare → scaffold without cleaning. The artefacts on disk
    // already carry the previous run's content, the prepared YAML is
    // unchanged, and the build tracker says everything is `not_implemented`.
    // Both prepare and scaffold should be idempotent under those inputs.
    let prepare2 = run_reen(&["prepare"]);
    assert!(prepare2.status.success(), "{}", pretty_output("prepare (run 2)", &prepare2));
    let scaffold2 = run_reen(&["scaffold"]);
    assert!(scaffold2.status.success(), "{}", pretty_output("scaffold (run 2)", &scaffold2));
    let snapshot_2 = snapshot_dir(&lupa);

    let diff = diff_snapshots(&snapshot_1, &snapshot_2);
    assert!(
        diff.is_empty(),
        "scaffold output differs across re-runs:\n{}\n\
         (the second prepare+scaffold pair should be a no-op given identical drafts)",
        diff,
    );
}
