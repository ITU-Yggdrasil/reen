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
    assert!(output_text(&scaffold).contains("blocking ambiguit"));
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
fn help_lists_scaffold_and_build() {
    let output = Command::new(env!("CARGO_BIN_EXE_reen"))
        .arg("--help")
        .output()
        .expect("run help");
    assert!(output.status.success(), "{}", output_text(&output));
    let rendered = output_text(&output);
    assert!(
        rendered.contains("scaffold"),
        "help should list scaffold: {rendered}"
    );
    assert!(
        rendered.contains("build"),
        "help should list build: {rendered}"
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
