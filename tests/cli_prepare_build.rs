use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

fn fixture_root(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn temp_project_dir(prefix: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    std::env::temp_dir().join(format!("reen_{prefix}_{stamp}"))
}

fn copy_fixture(name: &str) -> PathBuf {
    let source = fixture_root(name);
    let target = temp_project_dir(name);
    copy_dir_recursive(&source, &target);
    target
}

fn copy_dir_recursive(source: &Path, target: &Path) {
    fs::create_dir_all(target).expect("mkdir target");
    for entry in fs::read_dir(source).expect("read_dir source") {
        let entry = entry.expect("entry");
        let path = entry.path();
        let destination = target.join(entry.file_name());
        if path.is_dir() {
            copy_dir_recursive(&path, &destination);
        } else {
            fs::copy(&path, &destination).expect("copy file");
        }
    }
}

fn copy_file(source: &Path, target: &Path) {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).expect("mkdir parent");
    }
    fs::copy(source, target).expect("copy file");
}

fn run_reen<I, S>(project_root: &Path, args: I) -> Output
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    Command::new(env!("CARGO_BIN_EXE_reen"))
        .current_dir(project_root)
        .args(args)
        .output()
        .expect("run reen")
}

fn output_text(output: &Output) -> String {
    format!(
        "stdout:\n{}\n\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn prepare_success_writes_yaml() {
    let project = copy_fixture("success");
    let output = run_reen(&project, ["prepare"]);
    assert!(output.status.success(), "{}", output_text(&output));
    assert!(
        project.join("drafts/prepare/data/Message.yml").is_file(),
        "prepared data YAML missing"
    );
    assert!(
        project
            .join("drafts/prepare/contexts/Greeter.yml")
            .is_file(),
        "prepared context YAML missing"
    );
    assert!(
        project.join("drafts/prepare/app.yml").is_file(),
        "prepared app YAML missing"
    );
}

#[test]
fn prepare_writes_yaml_and_exits_nonzero_on_ambiguity() {
    let project = copy_fixture("ambiguity");
    let output = run_reen(&project, ["prepare"]);
    assert!(!output.status.success(), "{}", output_text(&output));
    let prepared = project.join("drafts/prepare/data/Mystery.yml");
    assert!(prepared.is_file(), "prepared YAML missing after ambiguity");
    let yaml = fs::read_to_string(prepared).expect("read prepared yaml");
    assert!(yaml.contains("status: missing") || yaml.contains("status: ambiguous"));
    assert!(yaml.contains("ambiguities:"));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("error[prepare]:"),
        "stderr should print each ambiguity: {stderr}"
    );
    assert!(
        stderr.contains("blocking ambiguit"),
        "stderr should contain summary: {stderr}"
    );
}

#[test]
fn scaffold_fails_when_prepared_is_missing() {
    let project = copy_fixture("success");
    let output = run_reen(&project, ["scaffold"]);
    assert!(!output.status.success(), "{}", output_text(&output));
    let rendered = output_text(&output);
    assert!(rendered.contains("run `reen prepare` first"));
}

#[test]
fn scaffold_fails_when_prepared_has_blocking_ambiguity() {
    let project = copy_fixture("ambiguity");
    let prepare = run_reen(&project, ["prepare"]);
    assert!(!prepare.status.success(), "{}", output_text(&prepare));
    let scaffold = run_reen(&project, ["scaffold"]);
    assert!(!scaffold.status.success(), "{}", output_text(&scaffold));
    let rendered = output_text(&scaffold);
    assert!(
        rendered.contains("blocking ambiguit")
            || rendered.contains("contains blocking ambiguities"),
        "{rendered}"
    );
}

#[test]
fn refine_succeeds_on_good_raw_drafts_before_prepare_and_writes_report() {
    let project = copy_fixture("success");

    // `--skip-llm-review` keeps this test deterministic regardless of whether the developer
    // running it has `ANTHROPIC_API_KEY` set in their environment.
    let refine = run_reen(&project, ["refine", "--skip-llm-review"]);
    assert!(refine.status.success(), "{}", output_text(&refine));

    let report_path = project.join(".reen/refine/report.md");
    assert!(report_path.is_file(), "refine report missing");
    let report = fs::read_to_string(report_path).expect("read refine report");
    assert!(report.contains("## Draft Review"), "{report}");
    assert!(report.contains("Prepared review: skipped"), "{report}");
}

#[test]
fn refine_succeeds_when_prepared_artifacts_are_deterministic() {
    let project = copy_fixture("success");
    let prepare = run_reen(&project, ["prepare"]);
    assert!(prepare.status.success(), "{}", output_text(&prepare));

    let refine = run_reen(&project, ["refine", "--skip-llm-review"]);
    assert!(refine.status.success(), "{}", output_text(&refine));

    let report =
        fs::read_to_string(project.join(".reen/refine/report.md")).expect("read refine report");
    assert!(report.contains("## Prepared Review"), "{report}");
    assert!(
        report.contains("No blocking prepared-review findings."),
        "{report}"
    );
}

#[test]
fn refine_drafts_only_skips_prepared_review() {
    let project = copy_fixture("success");

    let refine = run_reen(&project, ["refine", "--drafts-only", "--skip-llm-review"]);
    assert!(refine.status.success(), "{}", output_text(&refine));

    let report =
        fs::read_to_string(project.join(".reen/refine/report.md")).expect("read refine report");
    assert!(report.contains("skipped by `--drafts-only`"), "{report}");
}

#[test]
fn refine_rejects_behavioral_ambiguity_in_raw_drafts() {
    let project = copy_fixture("behavioral_ambiguity");

    let refine = run_reen(&project, ["refine"]);
    assert!(!refine.status.success(), "{}", output_text(&refine));
    let rendered = output_text(&refine);
    assert!(
        rendered.contains("current situation") || rendered.contains("appropriate"),
        "{rendered}"
    );

    let report =
        fs::read_to_string(project.join(".reen/refine/report.md")).expect("read refine report");
    assert!(report.contains("Line 25 [concreteness]"), "{report}");
}

#[test]
fn refine_prepared_only_accepts_prose_only_app_flow() {
    let project = copy_fixture("behavioral_ambiguity");
    let prepare = run_reen(&project, ["prepare"]);
    assert!(prepare.status.success(), "{}", output_text(&prepare));

    let refine = run_reen(&project, ["refine", "--prepared-only"]);
    assert!(refine.status.success(), "{}", output_text(&refine));
}

#[test]
fn scaffold_accepts_prose_only_app_flow() {
    let project = copy_fixture("behavioral_ambiguity");
    let prepare = run_reen(&project, ["prepare"]);
    assert!(prepare.status.success(), "{}", output_text(&prepare));

    let scaffold = run_reen(&project, ["scaffold"]);
    assert!(scaffold.status.success(), "{}", output_text(&scaffold));
}

#[test]
fn refine_and_scaffold_reject_prepared_contract_mismatch() {
    let project = copy_fixture("success");
    let prepare = run_reen(&project, ["prepare"]);
    assert!(prepare.status.success(), "{}", output_text(&prepare));

    let prepared_path = project.join("drafts/prepare/app.yml");
    let yaml = fs::read_to_string(&prepared_path).expect("read prepared app");
    let broken = yaml.replacen(
        "kind: string\n          value: Hello, world!",
        "kind: integer\n          value: '42'",
        1,
    );
    assert_ne!(broken, yaml, "expected to corrupt the prepared app body");
    fs::write(&prepared_path, broken).expect("write broken prepared app");

    let refine = run_reen(&project, ["refine"]);
    assert!(!refine.status.success(), "{}", output_text(&refine));
    let refine_text = output_text(&refine);
    assert!(
        refine_text.contains("contract mismatch")
            || refine_text.contains("expects `String` but got integer literal"),
        "{refine_text}"
    );

    let scaffold = run_reen(&project, ["scaffold"]);
    assert!(!scaffold.status.success(), "{}", output_text(&scaffold));
    let scaffold_text = output_text(&scaffold);
    assert!(
        scaffold_text.contains("contract mismatches")
            || scaffold_text.contains("expects `String` but got integer literal"),
        "{scaffold_text}"
    );
}

#[test]
fn refine_rejects_explicit_draft_contract_mismatch() {
    let project = copy_fixture("draft_review_mismatch");

    let refine = run_reen(&project, ["refine"]);
    assert!(!refine.status.success(), "{}", output_text(&refine));
    let rendered = output_text(&refine);
    assert!(
        rendered.contains("renderer.render(board_picture)")
            || rendered.contains("explicitly expects `Board`")
            || rendered.contains("std::collections::HashMap<Position, char>"),
        "{rendered}"
    );
}

#[test]
fn refine_accepts_aligned_snake_board_contract_subset() {
    let project = temp_project_dir("snake_contracts");
    let drafts_root = project.join("drafts");

    let repo_drafts = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("snake")
        .join("drafts");

    for relative in [
        "data/Board.md",
        "data/GameState.md",
        "data/Snake.md",
        "data/Food.md",
        "contexts/game_loop.md",
        "contexts/terminal_renderer.md",
        "projections/string_renderer.md",
        "prepare/data/Board.yml",
        "prepare/data/GameState.yml",
        "prepare/data/Snake.yml",
        "prepare/data/Food.yml",
        "prepare/contexts/game_loop.yml",
        "prepare/contexts/terminal_renderer.yml",
        "prepare/projections/string_renderer.yml",
    ] {
        copy_file(&repo_drafts.join(relative), &drafts_root.join(relative));
    }
    fs::write(project.join("reen.yml"), "fix: false\n").expect("write reen.yml");

    let refine = run_reen(
        &project,
        [
            "refine",
            "--skip-llm-review",
            "Board",
            "GameState",
            "Snake",
            "Food",
            "game_loop",
            "string_renderer",
            "terminal_renderer",
        ],
    );
    assert!(refine.status.success(), "{}", output_text(&refine));
}

#[test]
fn refine_skip_llm_review_flag_is_accepted() {
    // Guard against accidental removal of the flag: a plain `--skip-llm-review` invocation
    // must parse cleanly so callers and automation don't start failing after a refactor.
    let output = Command::new(env!("CARGO_BIN_EXE_reen"))
        .arg("refine")
        .arg("--help")
        .output()
        .expect("run reen refine --help");
    let rendered = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        rendered.contains("--skip-llm-review"),
        "refine --help must mention --skip-llm-review: {rendered}"
    );
    assert!(
        rendered.contains("--min-severity"),
        "refine --help must mention --min-severity: {rendered}"
    );
    assert!(
        rendered.contains("--require-llm-review"),
        "refine --help must mention --require-llm-review: {rendered}"
    );
}

#[test]
fn refine_require_llm_review_without_api_key_errors() {
    // `--require-llm-review` promotes a missing API key / network failure from a warn-skip
    // into a hard error. This is the main safety net for CI configurations that want to fail
    // loudly if the behavioral review did not actually run.
    let project = copy_fixture("success");
    let output = Command::new(env!("CARGO_BIN_EXE_reen"))
        .current_dir(&project)
        .args(["refine", "--require-llm-review"])
        .env_remove("ANTHROPIC_API_KEY")
        .output()
        .expect("run reen refine --require-llm-review");
    assert!(!output.status.success(), "{}", output_text(&output));
    let rendered = output_text(&output);
    assert!(
        rendered.contains("ANTHROPIC_API_KEY") || rendered.contains("require-llm-review"),
        "expected error to mention the missing key or the flag: {rendered}"
    );
}

#[test]
fn scaffold_succeeds_from_prepared_yaml() {
    let project = copy_fixture("success");
    let prepare = run_reen(&project, ["prepare"]);
    assert!(prepare.status.success(), "{}", output_text(&prepare));
    let scaffold = run_reen(&project, ["scaffold"]);
    assert!(scaffold.status.success(), "{}", output_text(&scaffold));

    assert!(project.join("Cargo.toml").is_file());
    assert!(project.join("src/lib.rs").is_file());
    assert!(project.join("src/main.rs").is_file());
    assert!(project.join("src/data/message.rs").is_file());
    assert!(project.join("src/contexts/greeter.rs").is_file());
}

#[test]
fn help_lists_refine_scaffold_and_build() {
    let output = Command::new(env!("CARGO_BIN_EXE_reen"))
        .arg("--help")
        .output()
        .expect("run help");
    assert!(output.status.success(), "{}", output_text(&output));
    let rendered = output_text(&output);
    assert!(
        rendered.contains("refine"),
        "help should list refine: {rendered}"
    );
    assert!(
        rendered.contains("scaffold"),
        "help should list scaffold: {rendered}"
    );
    assert!(
        rendered.contains("build"),
        "help should list build: {rendered}"
    );
    assert!(
        rendered.contains("init"),
        "help should list init: {rendered}"
    );
    assert!(
        rendered.contains("manifest"),
        "help should list manifest: {rendered}"
    );
    assert!(!rendered.contains("create"));
    assert!(!rendered.contains("check"));
    assert!(!rendered.contains("capabilities"));
}

#[test]
fn build_help_lists_fix_flag() {
    let output = Command::new(env!("CARGO_BIN_EXE_reen"))
        .args(["build", "--help"])
        .output()
        .expect("run build help");
    assert!(output.status.success(), "{}", output_text(&output));
    let rendered = output_text(&output);
    assert!(
        rendered.contains("--fix"),
        "build --help should list --fix: {rendered}"
    );
}

#[test]
fn prepare_resolves_explicit_role_player_types() {
    let project = copy_fixture("explicit_role_type");
    let output = run_reen(&project, ["prepare"]);
    assert!(output.status.success(), "{}", output_text(&output));

    let prepared_path = project.join("drafts/prepare/contexts/Processor.yml");
    assert!(prepared_path.is_file(), "prepared Processor YAML missing");
    let yaml = fs::read_to_string(&prepared_path).expect("read prepared yaml");

    assert!(
        yaml.contains("std::io::Stdin"),
        "explicit Rust path type should appear in prepared YAML:\n{yaml}"
    );
    assert!(
        yaml.contains("source: draft.role_player_type"),
        "explicit types should have source draft.role_player_type:\n{yaml}"
    );
    assert!(
        yaml.contains("rust: i64"),
        "English 'integer' should normalise to i64:\n{yaml}"
    );
    assert!(
        yaml.contains("rust: String"),
        "English 'string' should normalise to String:\n{yaml}"
    );
    // The 'token' role has an empty Type column — it should fall back to name-based resolution.
    assert!(
        yaml.contains("source: name_match"),
        "empty Type column should fall back to name-based resolution:\n{yaml}"
    );
    // Role method signatures should include the role player parameter.
    assert!(
        yaml.contains("reader_: &std::io::Stdin"),
        "role method signature should include the role player parameter:\n{yaml}"
    );
    // Role method parameters should have the role player type resolved.
    assert!(
        yaml.contains("rust: '&std::io::Stdin'")
            || yaml.contains("rust: \"&std::io::Stdin\"")
            || yaml.contains("rust: \'&std::io::Stdin\'"),
        "role player parameter should carry the resolved type:\n{yaml}"
    );
}

#[test]
fn prepare_rejects_unsupported_api_drafts() {
    let project = copy_fixture("unsupported_api");
    let output = run_reen(&project, ["prepare"]);
    assert!(!output.status.success(), "{}", output_text(&output));
    assert!(output_text(&output).contains("does not support drafts/apis"));
}

#[test]
fn prepare_fix_without_api_key_gives_helpful_error() {
    let project = copy_fixture("ambiguity");
    let output = Command::new(env!("CARGO_BIN_EXE_reen"))
        .current_dir(&project)
        .args(["prepare", "--fix"])
        .env_remove("ANTHROPIC_API_KEY")
        .output()
        .expect("run reen");
    assert!(!output.status.success(), "{}", output_text(&output));
    let rendered = output_text(&output);
    assert!(
        rendered.contains("ANTHROPIC_API_KEY"),
        "should mention ANTHROPIC_API_KEY requirement: {rendered}"
    );
}

#[test]
fn build_fails_without_scaffold() {
    let project = copy_fixture("success");
    let prepare = run_reen(&project, ["prepare"]);
    assert!(prepare.status.success(), "{}", output_text(&prepare));
    let build = run_reen(&project, ["build"]);
    assert!(!build.status.success(), "{}", output_text(&build));
    let rendered = output_text(&build);
    assert!(
        rendered.contains("reen scaffold"),
        "should tell user to scaffold first: {rendered}"
    );
}

#[test]
fn build_without_api_key_gives_helpful_error() {
    let project = copy_fixture("success");
    let prepare = run_reen(&project, ["prepare"]);
    assert!(prepare.status.success(), "{}", output_text(&prepare));
    let scaffold = run_reen(&project, ["scaffold"]);
    assert!(scaffold.status.success(), "{}", output_text(&scaffold));
    let build = Command::new(env!("CARGO_BIN_EXE_reen"))
        .current_dir(&project)
        .args(["build"])
        .env_remove("ANTHROPIC_API_KEY")
        .output()
        .expect("run reen");
    let rendered = output_text(&build);
    if !build.status.success() {
        assert!(
            rendered.contains("ANTHROPIC_API_KEY") || rendered.contains("No todo!() sites"),
            "should mention ANTHROPIC_API_KEY or report no sites: {rendered}"
        );
    }
}

#[test]
fn root_fix_from_reen_yml_applies_to_prepare() {
    let project = copy_fixture("ambiguity");
    fs::write(project.join("reen.yml"), "fix: true\n").expect("write reen.yml");

    let output = Command::new(env!("CARGO_BIN_EXE_reen"))
        .current_dir(&project)
        .args(["prepare"])
        .env_remove("ANTHROPIC_API_KEY")
        .output()
        .expect("run reen");

    assert!(!output.status.success(), "{}", output_text(&output));
    assert!(
        output_text(&output).contains("ANTHROPIC_API_KEY"),
        "{}",
        output_text(&output)
    );
}

#[test]
fn command_fix_false_in_reen_yml_overrides_root_fix() {
    let project = copy_fixture("ambiguity");
    fs::write(
        project.join("reen.yml"),
        "fix: true\nprepare:\n  fix: false\n",
    )
    .expect("write reen.yml");

    let output = Command::new(env!("CARGO_BIN_EXE_reen"))
        .current_dir(&project)
        .args(["prepare"])
        .env_remove("ANTHROPIC_API_KEY")
        .output()
        .expect("run reen");

    assert!(!output.status.success(), "{}", output_text(&output));
    let rendered = output_text(&output);
    assert!(
        !rendered.contains("ANTHROPIC_API_KEY"),
        "command override should disable root fix: {rendered}"
    );
    assert!(
        rendered.contains("error[prepare]:"),
        "prepare should still run and report ambiguities: {rendered}"
    );
}

#[test]
fn init_prepare_fix_writes_reen_yml_before_running_command() {
    let project = copy_fixture("ambiguity");

    let output = Command::new(env!("CARGO_BIN_EXE_reen"))
        .current_dir(&project)
        .args(["init", "prepare", "--fix"])
        .env_remove("ANTHROPIC_API_KEY")
        .output()
        .expect("run reen");

    assert!(!output.status.success(), "{}", output_text(&output));
    let config = fs::read_to_string(project.join("reen.yml")).expect("read reen.yml");
    assert!(
        config.contains("prepare:\n  fix: true") || config.contains("prepare:\r\n  fix: true"),
        "init should persist prepare.fix into reen.yml:\n{config}"
    );
}

#[test]
fn init_root_fix_writes_reen_yml_without_running_command() {
    let project = copy_fixture("success");

    let output = run_reen(&project, ["init", "--fix"]);

    assert!(output.status.success(), "{}", output_text(&output));
    let config = fs::read_to_string(project.join("reen.yml")).expect("read reen.yml");
    assert!(
        config.lines().any(|line| line.trim() == "fix: true"),
        "init --fix should persist root fix:\n{config}"
    );
}

#[test]
fn init_refine_min_severity_persists_without_running_refine() {
    let project = copy_fixture("success");

    let output = Command::new(env!("CARGO_BIN_EXE_reen"))
        .current_dir(&project)
        .args(["init", "refine", "--min-severity", "90"])
        .env_remove("ANTHROPIC_API_KEY")
        .output()
        .expect("run reen");

    assert!(output.status.success(), "{}", output_text(&output));
    let rendered = output_text(&output);
    assert!(
        !rendered.contains("refine"),
        "init refine should NOT execute refine:\n{rendered}"
    );
    let config = fs::read_to_string(project.join("reen.yml")).expect("read reen.yml");
    assert!(
        config.contains("refine:\n  min-severity: 90")
            || config.contains("refine:\r\n  min-severity: 90"),
        "init refine --min-severity should persist refine.min-severity into reen.yml:\n{config}"
    );
}

#[test]
fn refine_picks_up_root_min_severity_from_reen_yml() {
    // A root-level `min-severity: 90` in reen.yml (outside the `refine:` section) must be
    // honoured as a fallback for the refine command's `--min-severity` dial. Verified via
    // `--verbose`: the refine command echoes the effective min-severity on startup.
    let project = copy_fixture("success");
    fs::write(project.join("reen.yml"), "min-severity: 90\n").expect("write reen.yml");

    let output = Command::new(env!("CARGO_BIN_EXE_reen"))
        .current_dir(&project)
        .args(["refine", "--skip-llm-review", "--verbose"])
        .env_remove("ANTHROPIC_API_KEY")
        .output()
        .expect("run reen");

    assert!(output.status.success(), "{}", output_text(&output));
    let rendered = output_text(&output);
    assert!(
        rendered.contains("min-severity=90"),
        "refine should report the root-level min-severity 90 from reen.yml:\n{rendered}"
    );
}

#[test]
fn refine_prefers_refine_section_min_severity_over_root() {
    // When both root-level `min-severity` and `refine.min-severity` are set, the nested
    // refine-section value wins (matching the precedence used for other overlapping keys).
    let project = copy_fixture("success");
    fs::write(
        project.join("reen.yml"),
        "min-severity: 10\nrefine:\n  min-severity: 90\n",
    )
    .expect("write reen.yml");

    let output = Command::new(env!("CARGO_BIN_EXE_reen"))
        .current_dir(&project)
        .args(["refine", "--skip-llm-review", "--verbose"])
        .env_remove("ANTHROPIC_API_KEY")
        .output()
        .expect("run reen");

    assert!(output.status.success(), "{}", output_text(&output));
    let rendered = output_text(&output);
    assert!(
        rendered.contains("min-severity=90"),
        "refine should prefer refine.min-severity over root:\n{rendered}"
    );
}

#[test]
fn manifest_types_add_prefix_creates_manifest_file() {
    let project = copy_fixture("success");

    let output = run_reen(&project, ["manifest", "types", "add-prefix", "rand::"]);

    assert!(output.status.success(), "{}", output_text(&output));
    let manifest =
        fs::read_to_string(project.join("drafts/types-manifest.yml")).expect("read manifest");
    assert!(manifest.contains("external_path_prefixes:"));
    assert!(
        manifest.contains("rand::"),
        "manifest should contain rand:::\n{manifest}"
    );
}

#[test]
fn manifest_types_add_prefix_is_idempotent() {
    let project = copy_fixture("success");
    let manifest_path = project.join("drafts/types-manifest.yml");
    fs::write(&manifest_path, "external_path_prefixes:\n  - 'std::'\n").expect("write manifest");

    let first = run_reen(&project, ["manifest", "types", "add-prefix", "rand::"]);
    assert!(first.status.success(), "{}", output_text(&first));
    let second = run_reen(&project, ["manifest", "types", "add-prefix", "rand::"]);
    assert!(second.status.success(), "{}", output_text(&second));

    let manifest = fs::read_to_string(manifest_path).expect("read manifest");
    assert_eq!(manifest.matches("rand::").count(), 1, "{manifest}");
    assert_eq!(manifest.matches("std::").count(), 1, "{manifest}");
}

#[test]
fn manifest_capabilities_help_lists_add_provider() {
    let output = Command::new(env!("CARGO_BIN_EXE_reen"))
        .args(["manifest", "capabilities", "--help"])
        .output()
        .expect("run manifest capabilities help");
    assert!(output.status.success(), "{}", output_text(&output));
    let rendered = output_text(&output);
    assert!(
        rendered.contains("add"),
        "manifest capabilities --help should list add: {rendered}"
    );
}

#[test]
fn manifest_capabilities_add_provider_writes_registry_dependencies_and_types() {
    let project = copy_fixture("success");

    let output = run_reen(
        &project,
        [
            "manifest",
            "capabilities",
            "add",
            "randomness",
            "rand",
            "--feature",
            "std_rng",
        ],
    );

    assert!(output.status.success(), "{}", output_text(&output));

    let registry = fs::read_to_string(project.join("drafts/capability_registry.yml"))
        .expect("read capability registry");
    assert!(registry.contains("schema: reen.capability-registry/v1"));
    assert!(registry.contains("domain: randomness"), "{registry}");
    assert!(registry.contains("crate: rand"), "{registry}");
    assert!(registry.contains("- randomness"), "{registry}");
    assert!(registry.contains("rand::"), "{registry}");
    assert!(
        registry.contains("version: '*'") || registry.contains("version: \"*\""),
        "{registry}"
    );

    let dependencies =
        fs::read_to_string(project.join("drafts/dependencies.yml")).expect("read dependencies");
    assert!(dependencies.contains("schema: reen.dependencies/v1"));
    assert!(dependencies.contains("name: rand"), "{dependencies}");
    assert!(dependencies.contains("- randomness"), "{dependencies}");
    assert!(
        dependencies.contains("features = [\"std_rng\"]"),
        "{dependencies}"
    );
    assert!(
        dependencies.contains("version = \"*\"") || dependencies.contains("version: '*'"),
        "{dependencies}"
    );

    let manifest =
        fs::read_to_string(project.join("drafts/types-manifest.yml")).expect("read types manifest");
    assert!(manifest.contains("external_path_prefixes:"), "{manifest}");
    assert!(manifest.contains("rand::"), "{manifest}");
    assert!(!manifest.contains("allowlists:"), "{manifest}");
}

#[test]
fn manifest_capabilities_add_provider_is_idempotent() {
    let project = copy_fixture("success");

    let first = run_reen(
        &project,
        ["manifest", "capabilities", "add", "randomness", "rand"],
    );
    assert!(first.status.success(), "{}", output_text(&first));

    let second = run_reen(
        &project,
        ["manifest", "capabilities", "add", "randomness", "rand"],
    );
    assert!(second.status.success(), "{}", output_text(&second));

    let registry = fs::read_to_string(project.join("drafts/capability_registry.yml"))
        .expect("read capability registry");
    assert_eq!(
        registry.matches("domain: randomness").count(),
        1,
        "{registry}"
    );
    assert_eq!(registry.matches("- randomness").count(), 1, "{registry}");

    let dependencies =
        fs::read_to_string(project.join("drafts/dependencies.yml")).expect("read dependencies");
    assert_eq!(
        dependencies.matches("name: rand").count(),
        1,
        "{dependencies}"
    );
    assert_eq!(
        dependencies.matches("- randomness").count(),
        1,
        "{dependencies}"
    );

    let manifest: serde_yaml::Value = serde_yaml::from_str(
        &fs::read_to_string(project.join("drafts/types-manifest.yml"))
            .expect("read types manifest"),
    )
    .expect("parse types manifest");
    let mapping = manifest.as_mapping().expect("types manifest mapping");
    let prefixes = mapping
        .get(serde_yaml::Value::String(
            "external_path_prefixes".to_string(),
        ))
        .and_then(serde_yaml::Value::as_sequence)
        .expect("prefix sequence");
    assert_eq!(prefixes.len(), 1);
    assert_eq!(prefixes[0].as_str(), Some("rand::"));
    assert!(
        mapping
            .get(serde_yaml::Value::String("allowlists".to_string()))
            .is_none(),
        "simple capability add should only extend namespace prefixes"
    );
}

#[test]
#[ignore]
fn build_implements_todo_bodies() {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("snake");
    let project = temp_project_dir("build_agent");
    copy_dir_recursive(&source, &project);
    let prepare_dir = project.join("drafts").join("prepare");
    if prepare_dir.exists() {
        fs::remove_dir_all(&prepare_dir).expect("remove old prepared");
    }
    let tracker = project.join(".reen").join("build_tracker.json");
    if tracker.exists() {
        fs::remove_file(&tracker).expect("remove tracker");
    }

    let prepare = run_reen(&project, ["prepare", "--fix", "--verbose"]);
    assert!(
        prepare.status.success(),
        "prepare failed: {}",
        output_text(&prepare)
    );
    let scaffold = run_reen(&project, ["scaffold", "--verbose"]);
    assert!(
        scaffold.status.success(),
        "scaffold failed: {}",
        output_text(&scaffold)
    );
    let build = run_reen(&project, ["build", "--verbose"]);
    let rendered = output_text(&build);
    eprintln!("{}", rendered);
    assert!(build.status.success(), "build failed: {}", rendered);
}

#[test]
#[ignore]
fn prepare_fix_resolves_ambiguities_with_llm() {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("snake");
    let project = temp_project_dir("fix_llm");
    copy_dir_recursive(&source, &project);
    let prepare_dir = project.join("drafts").join("prepare");
    if prepare_dir.exists() {
        fs::remove_dir_all(&prepare_dir).expect("remove old prepared");
    }
    let tracker = project.join(".reen").join("build_tracker.json");
    if tracker.exists() {
        fs::remove_file(&tracker).expect("remove tracker");
    }

    let output = run_reen(&project, ["prepare", "--fix", "--verbose"]);
    let rendered = output_text(&output);
    eprintln!("{}", rendered);

    let command_input = project.join("drafts/prepare/contexts/command_input.yml");
    assert!(
        command_input.is_file(),
        "prepared YAML for command_input missing after --fix"
    );
    let yaml = fs::read_to_string(command_input).expect("read yaml");
    assert!(
        yaml.contains("status: fixed"),
        "expected at least one fixed status in command_input.yml"
    );
    assert!(
        yaml.contains("source: fix.agent"),
        "expected fix.agent source in command_input.yml"
    );
}
