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
    std::env::temp_dir().join(format!("reen_v2_{prefix}_{stamp}_{seq}"))
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

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

// ── Minimal context draft ─────────────────────────────────────────

const AMOUNT_DRAFT: &str = r#"# Data: Amount

## Purpose
Represents a monetary amount that is never negative.

## Attributes
- value: the numeric quantity in the smallest currency unit

## Invariants
- an Amount is never negative

## Operations
### add
Adds two amounts together, returning a new Amount.

## Scenarios
### adding two amounts
Given: an amount of 30 and an amount of 50
When: add is invoked
Then: the result is 80
"#;

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
Subtracts an amount from the balance. Fails if the result would be negative.

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

// ── Tests ─────────────────────────────────────────────────────────

#[test]
fn prepare_success_writes_yaml() {
    let root = temp_project_dir("prepare_ok");
    write_file(&root, "drafts/data/Amount.md", AMOUNT_DRAFT);
    write_file(&root, "drafts/data/Account.md", ACCOUNT_DRAFT);
    write_file(&root, "drafts/contexts/money_transfer.md", TRANSFER_DRAFT);

    let output = run_reen(&root, &["prepare", "--verbose"]);
    assert!(output.status.success(), "stderr: {}", stderr(&output));

    let prepared_path = root.join("drafts/prepare/contexts/money_transfer.yml");
    assert!(prepared_path.is_file(), "prepared YAML should exist");

    let yaml = fs::read_to_string(&prepared_path).unwrap();
    assert!(yaml.contains("reen.prepare/v2"));
    assert!(yaml.contains("kind: context"));
    assert!(yaml.contains("source_withdraw"));
    assert!(yaml.contains("sink_deposit"));

    let amount_prepared = root.join("drafts/prepare/data/Amount.yml");
    assert!(amount_prepared.is_file());
    let amount_yaml = fs::read_to_string(&amount_prepared).unwrap();
    assert!(amount_yaml.contains("kind: data"));
    assert!(amount_yaml.contains("source: invariant"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn prepare_flags_unresolved_vocabulary() {
    let root = temp_project_dir("prepare_unresolved");
    // Context references "Widget" which doesn't exist as a data or projection draft
    let draft = r#"# Context: WidgetHandler

## Purpose
Handles widgets.

## Role Players
- widget: Widget — the widget being handled

## Props

## Role Methods
### widget
- handle: processes the widget

## Functionalities
### process
Calls the widget's handle role method.

## Scenarios
### process a widget
Given: a widget
When: process is invoked
Then: widget is handled
"#;
    write_file(&root, "drafts/contexts/widget_handler.md", draft);

    let output = run_reen(&root, &["prepare"]);
    assert!(!output.status.success());
    let err = stderr(&output);
    assert!(
        err.contains("Widget") && err.contains("not declared"),
        "expected unresolved reference error about Widget, got: {err}"
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn prepare_data_only_flag_filters_correctly() {
    let root = temp_project_dir("prepare_data_only");
    write_file(&root, "drafts/data/Amount.md", AMOUNT_DRAFT);
    write_file(&root, "drafts/contexts/money_transfer.md", TRANSFER_DRAFT);

    let output = run_reen(&root, &["prepare", "--data"]);
    assert!(output.status.success(), "stderr: {}", stderr(&output));

    // Data artifact should exist
    assert!(root.join("drafts/prepare/data/Amount.yml").is_file());
    // Context should NOT exist
    assert!(!root.join("drafts/prepare/contexts/money_transfer.yml").is_file());

    let _ = fs::remove_dir_all(root);
}

#[test]
fn refine_catches_incomplete_context() {
    let root = temp_project_dir("refine_incomplete");
    let bad_draft = r#"# Context: Empty

## Purpose
Does nothing.

## Role Players

## Props

## Role Methods
### nobody
- nothing: does nothing

## Functionalities
### do_nothing
Does nothing.

## Scenarios
"#;
    write_file(&root, "drafts/contexts/empty.md", bad_draft);

    let output = run_reen(&root, &["refine"]);
    assert!(!output.status.success());
    let err = stderr(&output);
    assert!(err.contains("at least one role player") || err.contains("at least one scenario"),
        "expected completeness findings, got: {err}");

    let _ = fs::remove_dir_all(root);
}

#[test]
fn scaffold_succeeds_after_prepare() {
    let root = temp_project_dir("scaffold_ok");
    write_file(&root, "drafts/data/Amount.md", AMOUNT_DRAFT);

    let output = run_reen(&root, &["prepare"]);
    assert!(output.status.success(), "prepare: {}", stderr(&output));

    let output = run_reen(&root, &["scaffold"]);
    assert!(output.status.success(), "scaffold should succeed: {}", stderr(&output));

    assert!(root.join("src/data/amount.rs").is_file());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn clear_prepared_removes_directory() {
    let root = temp_project_dir("clear");
    write_file(&root, "drafts/data/Amount.md", AMOUNT_DRAFT);

    let output = run_reen(&root, &["prepare"]);
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert!(root.join("drafts/prepare").is_dir());

    let output = run_reen(&root, &["clear", "prepared"]);
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert!(!root.join("drafts/prepare").is_dir());

    let _ = fs::remove_dir_all(root);
}

#[test]
fn manifest_capabilities_add_provider_works() {
    let root = temp_project_dir("manifest_cap");
    fs::create_dir_all(root.join("drafts")).unwrap();

    let output = run_reen(
        &root,
        &["manifest", "capabilities", "add", "randomness", "rand"],
    );
    assert!(output.status.success(), "stderr: {}", stderr(&output));

    let registry = fs::read_to_string(root.join("drafts/capability_registry.yml")).unwrap();
    assert!(registry.contains("randomness"));
    assert!(registry.contains("rand"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn manifest_types_add_prefix_works() {
    let root = temp_project_dir("manifest_types");
    fs::create_dir_all(root.join("drafts")).unwrap();

    let output = run_reen(&root, &["manifest", "types", "add-prefix", "rand::"]);
    assert!(output.status.success(), "stderr: {}", stderr(&output));

    let manifest = fs::read_to_string(root.join("drafts/types-manifest.yml")).unwrap();
    assert!(manifest.contains("rand::"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn help_lists_expected_subcommands() {
    let root = temp_project_dir("help");
    fs::create_dir_all(&root).unwrap();
    let output = run_reen(&root, &["--help"]);
    let out = stdout(&output);
    assert!(out.contains("prepare"), "should list prepare");
    assert!(out.contains("refine"), "should list refine");
    assert!(out.contains("scaffold"), "should list scaffold");
    assert!(out.contains("manifest"), "should list manifest");
    assert!(out.contains("clear"), "should list clear");
    assert!(out.contains("init"), "should list init");
    assert!(!out.contains(" build"), "should not list build (removed)");
    assert!(!out.contains(" fix"), "should not list fix (removed)");
    // "compile" as a subcommand should not appear (it's fine in the description)
    assert!(!out.contains("  compile "), "should not list compile subcommand (removed)");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn init_installs_bundle_in_empty_directory() {
    let root = temp_project_dir("init_fresh");
    fs::create_dir_all(&root).unwrap();
    let output = run_reen(&root, &["init"]);
    assert!(output.status.success(), "reen init should succeed in empty dir");
    assert!(root.join(".claude/hooks.json").exists());
    assert!(root.join(".reen-bundle.yml").exists());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn vocabulary_file_is_respected() {
    let root = temp_project_dir("vocab_file");
    // Create a vocabulary file that declares "Widget" as data
    write_file(
        &root,
        "drafts/vocabulary.yml",
        "schema: reen.vocabulary/v1\ndata:\n  - Widget\nprojections: []\n",
    );
    let draft = r#"# Context: WidgetHandler

## Purpose
Handles widgets.

## Role Players
- widget: Widget — the widget being handled

## Props

## Role Methods
### widget
- handle: processes the widget

## Functionalities
### process
Calls the widget's handle role method.

## Scenarios
### process a widget
Given: a widget
When: process is invoked
Then: widget is handled
"#;
    write_file(&root, "drafts/contexts/widget_handler.md", draft);

    let output = run_reen(&root, &["prepare"]);
    // Should succeed because Widget is declared in vocabulary
    assert!(output.status.success(), "stderr: {}", stderr(&output));

    let _ = fs::remove_dir_all(root);
}

// ── Phase 2: scaffold + verify integration tests ──────────────────

#[test]
fn scaffold_then_verify_accepts() {
    let root = temp_project_dir("scaffold_verify_ok");
    write_file(&root, "drafts/data/Account.md", ACCOUNT_DRAFT);
    write_file(&root, "drafts/contexts/money_transfer.md", TRANSFER_DRAFT);

    // Prepare
    let output = run_reen(&root, &["prepare"]);
    assert!(output.status.success(), "prepare failed: {}", stderr(&output));

    // Scaffold
    let output = run_reen(&root, &["scaffold"]);
    assert!(output.status.success(), "scaffold failed: {}", stderr(&output));

    // The scaffolded context file should exist
    let rs_path = root.join("src/contexts/money_transfer.rs");
    assert!(rs_path.is_file(), "scaffolded file should exist");

    // Verify should accept it
    let output = run_reen(&root, &["verify", "src/contexts/money_transfer.rs"]);
    assert!(output.status.success(), "verify should accept scaffolded file: {}", stderr(&output));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn verify_rejects_dropped_field() {
    let root = temp_project_dir("verify_dropped_field");
    write_file(&root, "drafts/data/Account.md", ACCOUNT_DRAFT);
    write_file(&root, "drafts/contexts/money_transfer.md", TRANSFER_DRAFT);

    // Prepare + scaffold
    let output = run_reen(&root, &["prepare"]);
    assert!(output.status.success(), "prepare: {}", stderr(&output));
    let output = run_reen(&root, &["scaffold"]);
    assert!(output.status.success(), "scaffold: {}", stderr(&output));

    // Modify the scaffolded file: remove the "sink" field
    let rs_path = root.join("src/contexts/money_transfer.rs");
    let content = fs::read_to_string(&rs_path).unwrap();
    let modified = content.replace("    sink: Account,\n", "");
    fs::write(&rs_path, &modified).unwrap();

    // Verify should reject with exit code 1
    let output = run_reen(&root, &["verify", "src/contexts/money_transfer.rs"]);
    assert_eq!(output.status.code(), Some(1), "verify should exit 1 on drift");
    let err = stderr(&output);
    assert!(err.contains("sink"), "diff should mention the missing field: {err}");

    let _ = fs::remove_dir_all(root);
}

#[test]
fn verify_rejects_renamed_method() {
    let root = temp_project_dir("verify_renamed_method");
    write_file(&root, "drafts/data/Account.md", ACCOUNT_DRAFT);
    write_file(&root, "drafts/contexts/money_transfer.md", TRANSFER_DRAFT);

    // Prepare + scaffold
    let output = run_reen(&root, &["prepare"]);
    assert!(output.status.success(), "prepare: {}", stderr(&output));
    let output = run_reen(&root, &["scaffold"]);
    assert!(output.status.success(), "scaffold: {}", stderr(&output));

    // Modify: rename "transfer" to "execute"
    let rs_path = root.join("src/contexts/money_transfer.rs");
    let content = fs::read_to_string(&rs_path).unwrap();
    let modified = content.replace("pub fn transfer(", "pub fn execute(");
    fs::write(&rs_path, &modified).unwrap();

    // Verify should reject
    let output = run_reen(&root, &["verify", "src/contexts/money_transfer.rs"]);
    assert_eq!(output.status.code(), Some(1), "verify should exit 1 on drift");
    let err = stderr(&output);
    assert!(err.contains("transfer"), "diff should mention missing functionality: {err}");

    let _ = fs::remove_dir_all(root);
}

#[test]
fn verify_rejects_missing_rs_file() {
    let root = temp_project_dir("verify_no_file");
    fs::create_dir_all(&root).unwrap();
    let output = run_reen(&root, &["verify", "src/contexts/nonexistent.rs"]);
    assert_eq!(output.status.code(), Some(2), "should exit 2 for missing file");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn scaffold_data_then_verify_accepts() {
    let root = temp_project_dir("scaffold_verify_data");
    write_file(&root, "drafts/data/Amount.md", AMOUNT_DRAFT);

    let output = run_reen(&root, &["prepare"]);
    assert!(output.status.success(), "prepare: {}", stderr(&output));
    let output = run_reen(&root, &["scaffold"]);
    assert!(output.status.success(), "scaffold: {}", stderr(&output));

    let rs_path = root.join("src/data/amount.rs");
    assert!(rs_path.is_file(), "scaffolded data file should exist");

    let output = run_reen(&root, &["verify", "src/data/amount.rs"]);
    assert!(output.status.success(), "verify should accept data scaffold: {}", stderr(&output));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn help_lists_verify() {
    let root = temp_project_dir("help_verify");
    fs::create_dir_all(&root).unwrap();
    let output = run_reen(&root, &["--help"]);
    let out = stdout(&output);
    assert!(out.contains("verify"), "help should list verify subcommand");
    let _ = fs::remove_dir_all(root);
}
