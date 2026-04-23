use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_project_dir(prefix: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("reen_tracker_{prefix}_{stamp}_{seq}"))
}

fn run_reen(project_root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_reen"))
        .args(args)
        .current_dir(project_root)
        .output()
        .expect("failed to run reen")
}

fn write_file(root: &Path, relative: &str, content: &str) {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("mkdir");
    }
    fs::write(&path, content).expect("write");
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

fn read_tracker(root: &Path) -> serde_json::Value {
    let path = root.join(".reen/build.json");
    let raw = fs::read_to_string(&path).expect("read build.json");
    serde_json::from_str(&raw).expect("parse build.json")
}

// ── Drafts ───────────────────────────────────────────────────────

const ACCOUNT_DRAFT: &str = r#"# Data: Account

## Purpose
Represents a bank account with a balance.

## Attributes
- balance: the current amount of money in the account

## Invariants
- an Account balance is never negative

## Operations
### deposit
Adds an amount to the balance.

### withdraw
Subtracts an amount from the balance.

## Scenarios
### depositing money
Given: an account with balance 100
When: deposit is invoked with amount 50
Then: balance is 150
"#;

const TRANSFER_DRAFT: &str = r#"# Context: MoneyTransfer

## Purpose
Enacts a transfer of money between two accounts.

## Role Players
- source: Account — the account money is withdrawn from
- sink: Account — the account money is deposited into

## Props
- amount: the quantity of money to transfer

## Role Methods
### source
- withdraw: removes the transfer amount from the source balance

### sink
- deposit: adds the transfer amount to the sink balance

## Functionalities
### transfer
The source withdraws the amount via its withdraw role method,
then the sink deposits the amount via its deposit role method.

## Scenarios
### successful transfer
Given: source has balance 100, sink has balance 0, amount is 30
When: transfer is invoked
Then: source has balance 70, sink has balance 30
"#;

// ── Tests ────────────────────────────────────────────────────────

#[test]
fn prepare_creates_build_tracker() {
    let root = temp_project_dir("tracker_create");
    write_file(&root, "drafts/data/Account.md", ACCOUNT_DRAFT);
    write_file(&root, "drafts/contexts/money_transfer.md", TRANSFER_DRAFT);

    let output = run_reen(&root, &["prepare"]);
    assert!(output.status.success(), "stderr: {}", stderr(&output));

    // .reen/build.json should exist
    let tracker_path = root.join(".reen/build.json");
    assert!(tracker_path.is_file(), "build.json should be created");

    let tracker = read_tracker(&root);
    assert_eq!(tracker["schema_version"], 1);

    // Should have draft hashes
    let drafts = tracker["drafts"].as_object().unwrap();
    assert!(drafts.contains_key("drafts/data/Account.md"));
    assert!(drafts.contains_key("drafts/contexts/money_transfer.md"));

    // Should have artefact entries
    let artefacts = tracker["artefacts"].as_object().unwrap();
    assert!(
        artefacts.contains_key("data/account/type"),
        "should have data type entry"
    );
    assert!(
        artefacts.contains_key("data/account/operation/deposit"),
        "should have data operation entry"
    );
    assert!(
        artefacts.contains_key("contexts/money_transfer/functionality/transfer"),
        "should have functionality entry"
    );

    // All entries should be not_implemented on first run
    for (_key, entry) in artefacts {
        assert_eq!(
            entry["state"].as_str().unwrap(),
            "not_implemented",
            "first prepare should set all entries to not_implemented"
        );
    }

    let _ = fs::remove_dir_all(root);
}

#[test]
fn prepare_unchanged_is_idempotent() {
    let root = temp_project_dir("tracker_idempotent");
    write_file(&root, "drafts/data/Account.md", ACCOUNT_DRAFT);
    write_file(&root, "drafts/contexts/money_transfer.md", TRANSFER_DRAFT);

    // First prepare
    let output = run_reen(&root, &["prepare"]);
    assert!(output.status.success(), "stderr: {}", stderr(&output));

    let tracker1 = read_tracker(&root);

    // Second prepare — no changes
    let output = run_reen(&root, &["prepare", "--verbose"]);
    assert!(output.status.success(), "stderr: {}", stderr(&output));

    let tracker2 = read_tracker(&root);

    // Trackers should be identical
    assert_eq!(tracker1, tracker2, "unchanged prepare should produce identical tracker");

    // Verbose output should say 0 drafts changed
    let out = stdout(&output);
    assert!(
        out.contains("0 drafts changed"),
        "should report 0 drafts changed, got: {}",
        out
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn prepare_mutation_cascades_stale() {
    let root = temp_project_dir("tracker_cascade");
    write_file(&root, "drafts/data/Account.md", ACCOUNT_DRAFT);
    write_file(&root, "drafts/contexts/money_transfer.md", TRANSFER_DRAFT);

    // Initial prepare
    let output = run_reen(&root, &["prepare"]);
    assert!(output.status.success(), "stderr: {}", stderr(&output));

    // Mutate the Account draft — add a new attribute
    let modified_account = ACCOUNT_DRAFT.replace(
        "## Attributes\n- balance:",
        "## Attributes\n- currency: the currency denomination\n- balance:",
    );
    write_file(&root, "drafts/data/Account.md", &modified_account);

    // Re-prepare
    let output = run_reen(&root, &["prepare"]);
    assert!(output.status.success(), "stderr: {}", stderr(&output));

    let tracker = read_tracker(&root);
    let artefacts = tracker["artefacts"].as_object().unwrap();

    // Data type should be stale (direct change)
    let data_type = &artefacts["data/account/type"];
    assert_eq!(data_type["state"].as_str().unwrap(), "stale");
    assert_eq!(data_type["reason"].as_str().unwrap(), "draft_changed");

    // Data operations should be stale (upstream_changed via data type)
    let deposit = &artefacts["data/account/operation/deposit"];
    assert_eq!(deposit["state"].as_str().unwrap(), "stale");

    // The stdout should report stale methods
    let out = stdout(&output);
    assert!(
        out.contains("stale"),
        "should report stale methods, got: {}",
        out
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn prepare_dry_run_does_not_write_tracker() {
    let root = temp_project_dir("tracker_dry_run");
    write_file(&root, "drafts/data/Account.md", ACCOUNT_DRAFT);

    let output = run_reen(&root, &["prepare", "--dry-run"]);
    assert!(output.status.success(), "stderr: {}", stderr(&output));

    // Tracker should not exist
    assert!(
        !root.join(".reen/build.json").exists(),
        "dry-run should not create build.json"
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn clean_removes_tracker_prepared_and_owned_source() {
    let root = temp_project_dir("tracker_clean");
    write_file(&root, "drafts/data/Account.md", ACCOUNT_DRAFT);
    write_file(&root, "drafts/contexts/money_transfer.md", TRANSFER_DRAFT);

    // Prepare and scaffold
    let output = run_reen(&root, &["prepare"]);
    assert!(output.status.success(), "prepare: {}", stderr(&output));
    let output = run_reen(&root, &["scaffold"]);
    assert!(output.status.success(), "scaffold: {}", stderr(&output));

    // Verify things exist
    assert!(root.join(".reen/build.json").is_file());
    assert!(root.join("drafts/prepare").is_dir());
    assert!(root.join("src/contexts").is_dir());
    assert!(root.join("src/data").is_dir());

    // Clean with --force
    let output = run_reen(&root, &["clean", "--force"]);
    assert!(output.status.success(), "clean: {}", stderr(&output));

    // Everything should be gone
    assert!(!root.join(".reen/build.json").exists(), "tracker should be removed");
    assert!(!root.join("drafts/prepare").exists(), "prepared dir should be removed");
    assert!(!root.join("src/contexts").exists(), "contexts dir should be removed");
    assert!(!root.join("src/data").exists(), "data dir should be removed");

    // Drafts themselves should still exist
    assert!(root.join("drafts/data/Account.md").is_file(), "drafts should be preserved");
    assert!(root.join("drafts/contexts/money_transfer.md").is_file(), "drafts should be preserved");

    let out = stdout(&output);
    assert!(out.contains("Clean complete"), "should report completion, got: {}", out);

    let _ = fs::remove_dir_all(root);
}
