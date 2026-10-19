mod common;

use common::{
    PROJECT_FIXTURE, SAMPLE_RATE, Workspace, request, slice_frames, wav_bytes, workspace_with,
};
use ref_inserter::{
    Insertion, Project, Reference, Report, Request, Settings, Source, insert_reference,
    plan_insertion, run,
};

const SLICES: u32 = 3;
const PROJECT_NAME: &str = "song.m8s";
const REFERENCE_NAME: &str = "reference.wav";

struct Inputs {
    project: Vec<u8>,
    reference: Vec<u8>,
}

fn inputs() -> Inputs {
    Inputs {
        project: std::fs::read(PROJECT_FIXTURE).unwrap(),
        reference: wav_bytes(SAMPLE_RATE, slice_frames() * SLICES),
    }
}

fn settings(workspace: &Workspace) -> Settings {
    Settings::from(&request(workspace))
}

fn insert_named(
    inputs: &Inputs,
    project_name: &str,
    reference_name: &str,
) -> Result<Insertion, String> {
    let workspace = workspace_with(&inputs.project, &inputs.reference);
    insert_reference(
        &Source {
            project: &inputs.project,
            project_name,
            reference: &inputs.reference,
            reference_name,
        },
        &settings(&workspace),
    )
}

fn insert(inputs: &Inputs) -> Insertion {
    insert_named(inputs, PROJECT_NAME, REFERENCE_NAME).unwrap()
}

fn run_on_disk(inputs: &Inputs) -> (Workspace, Report) {
    let workspace = workspace_with(&inputs.project, &inputs.reference);
    let report = run(&request(&workspace)).unwrap();
    (workspace, report)
}

#[test]
fn core_project_bytes_equal_the_file_run_writes() {
    let inputs = inputs();
    let (workspace, _) = run_on_disk(&inputs);
    let written = std::fs::read(&workspace.out).unwrap();
    assert_eq!(insert(&inputs).project, written);
}

#[test]
fn core_wav_bytes_equal_the_file_run_copies() {
    let inputs = inputs();
    let (workspace, _) = run_on_disk(&inputs);
    let copied = std::fs::read(workspace.dest.join(REFERENCE_NAME)).unwrap();
    assert_eq!(insert(&inputs).wav, copied);
}

#[test]
fn core_names_the_wav_after_the_slugged_reference_name() {
    let inputs = inputs();
    let insertion = insert_named(&inputs, PROJECT_NAME, "My Song, v2.wav").unwrap();
    assert_eq!(insertion.file_name, "My_Song_v2.wav");
}

#[test]
fn core_summary_is_the_start_of_the_run_summary() {
    let inputs = inputs();
    let (_, report) = run_on_disk(&inputs);
    let core_summary = insert(&inputs).summary;
    assert_eq!(report.summary()[..core_summary.len()], core_summary[..]);
}

#[test]
fn plan_knows_the_wav_and_name_before_the_project_is_applied() {
    let inputs = inputs();
    let workspace = workspace_with(&inputs.project, &inputs.reference);
    let project = Project::parse(&inputs.project, PROJECT_NAME).unwrap();
    let plan = plan_insertion(
        project,
        &Reference {
            bytes: &inputs.reference,
            name: REFERENCE_NAME,
        },
        &settings(&workspace),
    )
    .unwrap();
    let insertion = insert(&inputs);
    assert_eq!(
        (plan.file_name.as_str(), plan.wav.as_slice()),
        (insertion.file_name.as_str(), insertion.wav.as_slice())
    );
}

#[test]
fn run_reports_an_unparseable_project_before_a_missing_reference() {
    let workspace = workspace_with(b"not a project", &[]);
    let missing_reference = workspace.wav.with_file_name("missing.wav");
    let error = run(&Request {
        reference_wav: missing_reference,
        ..request(&workspace)
    })
    .unwrap_err();
    assert!(error.starts_with("cannot parse"));
}

#[test]
fn core_rejects_a_reference_name_without_ascii_letters_or_digits() {
    let error = insert_named(&inputs(), PROJECT_NAME, "!!!.wav").unwrap_err();
    assert!(error.contains("!!!.wav has no ASCII letters or digits"));
}

#[test]
fn core_rejects_a_reference_name_without_a_file_name() {
    let error = insert_named(&inputs(), PROJECT_NAME, "..").unwrap_err();
    assert!(error.contains(".. has no file name"));
}

#[test]
fn core_names_the_project_when_it_cannot_parse_it() {
    let inputs = Inputs {
        project: b"not a project".to_vec(),
        ..inputs()
    };
    let error = insert_named(&inputs, PROJECT_NAME, REFERENCE_NAME).unwrap_err();
    assert!(error.starts_with("cannot parse song.m8s:"));
}
