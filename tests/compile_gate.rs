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

use std::path::PathBuf;
use std::process::{Command, Output};

fn lupa_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/lupa")
}

fn run_reen(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_reen"))
        .args(args)
        .current_dir(lupa_path())
        // Skip LLM-backed test synthesis so the gate runs offline.
        .env_remove("ANTHROPIC_API_KEY")
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

#[test]
fn lupa_pipeline_compiles_end_to_end() {
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
