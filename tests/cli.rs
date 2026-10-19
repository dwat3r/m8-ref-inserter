mod common;

use std::path::Path;
use std::process::{Command, Output};
use tempfile::TempDir;

const PROJECT_FIXTURE: &str = "tests/fixtures/V6_6EMPTY.m8s";
const SAMPLE_RATE: u32 = 100;
const FRAMES: u32 = 2000;

fn ref_inserter(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ref_inserter"))
        .args(args)
        .output()
        .unwrap()
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn insert_args<'a>(dir: &'a Path, project: &'a str, wav: &'a str, out: &'a str) -> Vec<String> {
    [
        project,
        wav,
        "--dest-dir",
        dir.join("samples").to_str().unwrap(),
        "--m8-dir",
        "/Samples/ref",
        "--out",
        out,
    ]
    .iter()
    .map(|arg| arg.to_string())
    .collect()
}

fn workspace() -> (TempDir, String, String, String) {
    let dir = TempDir::new().unwrap();
    let project = dir.path().join("project.m8s");
    let wav = dir.path().join("reference.wav");
    std::fs::copy(PROJECT_FIXTURE, &project).unwrap();
    std::fs::write(&wav, common::wav_bytes(SAMPLE_RATE, FRAMES)).unwrap();
    let out = dir.path().join("out.m8s");
    let paths = (
        project.to_str().unwrap().to_string(),
        wav.to_str().unwrap().to_string(),
        out.to_str().unwrap().to_string(),
    );
    (dir, paths.0, paths.1, paths.2)
}

#[test]
fn version_flag_prints_the_package_version() {
    let output = ref_inserter(&["--version"]);
    assert!(stdout(&output).contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn bare_invocation_still_inserts() {
    let (dir, project, wav, out) = workspace();
    let args = insert_args(dir.path(), &project, &wav, &out);
    let output = ref_inserter(&args.iter().map(String::as_str).collect::<Vec<_>>());
    assert!(output.status.success() && Path::new(&out).exists());
}

#[test]
fn bare_invocation_prints_the_summary() {
    let (dir, project, wav, out) = workspace();
    let args = insert_args(dir.path(), &project, &wav, &out);
    let output = ref_inserter(&args.iter().map(String::as_str).collect::<Vec<_>>());
    assert!(stdout(&output).contains("slices in"));
}

#[test]
fn missing_arguments_are_reported() {
    let output = ref_inserter(&[PROJECT_FIXTURE]);
    assert!(!output.status.success() && stderr(&output).contains("required"));
}

#[test]
fn inspect_subcommand_prints_the_tempo() {
    let output = ref_inserter(&["inspect", PROJECT_FIXTURE]);
    assert!(output.status.success() && stdout(&output).contains("tempo"));
}

#[test]
fn inspect_subcommand_fails_on_a_missing_file() {
    let output = ref_inserter(&["inspect", "tests/fixtures/does-not-exist.m8s"]);
    assert!(!output.status.success());
}

#[test]
fn help_lists_the_inspect_subcommand() {
    let output = ref_inserter(&["--help"]);
    assert!(stdout(&output).contains("inspect"));
}
