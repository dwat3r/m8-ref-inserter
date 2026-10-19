use m8_file_parser::{Instrument, SamplePlayMode, Sampler, Song, SongSteps};
use ref_inserter::alloc::DEFAULT_START;
use ref_inserter::insert::{MAX_SLICES, REF_INSTRUMENT_SLOT, blank_sampler};
use ref_inserter::{DEFAULT_EMPTY_ID, MAX_SAMPLE_PATH_LEN, Request, run, wav};
use std::path::{Path, PathBuf};
use tempfile::TempDir;

const PROJECT_FIXTURE: &str = "tests/fixtures/V6_6EMPTY.m8s";
const SAMPLE_RATE: u32 = 100;
const M8_DIR: &str = "/Samples/ref";
const EMPTY_ID: u8 = DEFAULT_EMPTY_ID;
const SLICES: usize = 3;
const DEFAULT_START_ID: u8 = DEFAULT_START as u8;

struct Workspace {
    _dir: TempDir,
    project: PathBuf,
    wav: PathBuf,
    dest: PathBuf,
    out: PathBuf,
}

fn wav_bytes(frames: u32) -> Vec<u8> {
    let data_len = frames * 2;
    let mut bytes = b"RIFF".to_vec();
    bytes.extend((36 + data_len).to_le_bytes());
    bytes.extend(b"WAVEfmt ");
    bytes.extend(16u32.to_le_bytes());
    bytes.extend([1, 0, 1, 0]);
    bytes.extend(SAMPLE_RATE.to_le_bytes());
    bytes.extend((SAMPLE_RATE * 2).to_le_bytes());
    bytes.extend([2, 0, 16, 0]);
    bytes.extend(b"data");
    bytes.extend(data_len.to_le_bytes());
    bytes.extend(vec![0u8; data_len as usize]);
    bytes
}

fn fixture_song() -> Song {
    Song::read(&mut std::fs::read(PROJECT_FIXTURE).unwrap().as_slice()).unwrap()
}

fn slice_frames() -> u32 {
    wav::slice_frames(SAMPLE_RATE, fixture_song().tempo)
}

fn frames_for_slices(slices: usize) -> u32 {
    slice_frames() * slices as u32 - slice_frames() / 2
}

fn workspace_with(project_bytes: &[u8], wav_contents: &[u8]) -> Workspace {
    let dir = TempDir::new().unwrap();
    let workspace = Workspace {
        project: dir.path().join("project.m8s"),
        wav: dir.path().join("reference.wav"),
        dest: dir.path().join("samples"),
        out: dir.path().join("out.m8s"),
        _dir: dir,
    };
    std::fs::write(&workspace.project, project_bytes).unwrap();
    std::fs::write(&workspace.wav, wav_contents).unwrap();
    workspace
}

fn workspace(slices: usize) -> Workspace {
    workspace_with(
        &std::fs::read(PROJECT_FIXTURE).unwrap(),
        &wav_bytes(frames_for_slices(slices)),
    )
}

fn request(workspace: &Workspace) -> Request {
    Request {
        project: workspace.project.clone(),
        reference_wav: workspace.wav.clone(),
        dest_dir: workspace.dest.clone(),
        m8_dir: M8_DIR.to_string(),
        out: workspace.out.clone(),
        empty_id: EMPTY_ID,
        force: false,
    }
}

fn run_in(workspace: &Workspace) -> Result<Vec<String>, String> {
    run(&request(workspace))
}

fn m8_dir_for_path_len(path_len: usize) -> String {
    "a".repeat(path_len - "/reference.wav".len())
}

fn reference_sampler(workspace: &Workspace) -> Sampler {
    let song = read_out(workspace);
    let Instrument::Sampler(sampler) = &song.instruments[usize::from(REF_INSTRUMENT_SLOT)] else {
        panic!("reference slot is not a sampler");
    };
    sampler.clone()
}

fn read_song(path: &Path) -> Song {
    Song::read(&mut std::fs::read(path).unwrap().as_slice()).unwrap()
}

fn read_out(workspace: &Workspace) -> Song {
    read_song(&workspace.out)
}

fn serialized(song: &Song) -> Vec<u8> {
    let mut writer = m8_file_parser::writer::Writer::new(std::fs::read(PROJECT_FIXTURE).unwrap());
    song.write(&mut writer).unwrap();
    writer.finish()
}

fn second_run(workspace: &Workspace, force: bool) -> Result<Vec<String>, String> {
    run(&Request {
        project: workspace.out.clone(),
        out: workspace.out.with_file_name("second.m8s"),
        force,
        ..request(workspace)
    })
}

#[test]
fn output_project_is_written() {
    let workspace = workspace(SLICES);
    run_in(&workspace).unwrap();
    assert!(workspace.out.exists());
}

#[test]
fn input_project_is_unchanged() {
    let workspace = workspace(SLICES);
    run_in(&workspace).unwrap();
    assert_eq!(
        std::fs::read(&workspace.project).unwrap(),
        std::fs::read(PROJECT_FIXTURE).unwrap()
    );
}

#[test]
fn instrument_points_at_the_m8_side_sample_path() {
    let workspace = workspace(SLICES);
    run_in(&workspace).unwrap();
    assert_eq!(reference_sampler(&workspace).sample_path, "/Samples/ref/reference.wav");
}

#[test]
fn instrument_is_named_reference() {
    let workspace = workspace(SLICES);
    run_in(&workspace).unwrap();
    assert_eq!(reference_sampler(&workspace).name, "REFERENCE");
}

#[test]
fn instrument_plays_forward() {
    let workspace = workspace(SLICES);
    run_in(&workspace).unwrap();
    assert_eq!(reference_sampler(&workspace).play_mode, SamplePlayMode::FWD);
}

#[test]
fn instrument_selects_the_first_slice() {
    let workspace = workspace(SLICES);
    run_in(&workspace).unwrap();
    assert_eq!(reference_sampler(&workspace).slice, 1);
}

#[test]
fn track_one_holds_one_chain_per_slice() {
    let workspace = workspace(SLICES);
    run_in(&workspace).unwrap();
    let song = read_out(&workspace);
    let cells: Vec<u8> = (0..SLICES)
        .map(|row| song.song.steps[row * SongSteps::TRACK_COUNT])
        .collect();
    assert_eq!(
        cells,
        [DEFAULT_START_ID, DEFAULT_START_ID + 1, DEFAULT_START_ID + 2]
    );
}

#[test]
fn copied_wav_has_one_cue_point_per_slice() {
    let workspace = workspace(SLICES);
    run_in(&workspace).unwrap();
    let copied = std::fs::read(workspace.dest.join("reference.wav")).unwrap();
    let cue = copied.windows(4).position(|w| w == b"cue ").unwrap();
    let count = u32::from_le_bytes(copied[cue + 8..cue + 12].try_into().unwrap());
    assert_eq!(count as usize, SLICES);
}

#[test]
fn too_long_reference_is_rejected() {
    let workspace = workspace(MAX_SLICES + 1);
    assert!(run_in(&workspace).is_err());
}

#[test]
fn rejected_run_writes_no_files() {
    let workspace = workspace(MAX_SLICES + 1);
    run_in(&workspace).unwrap_err();
    assert!(!workspace.out.exists() && !workspace.dest.exists());
}

#[test]
fn existing_output_is_not_overwritten() {
    let workspace = workspace(SLICES);
    std::fs::write(&workspace.out, b"keep me").unwrap();
    run_in(&workspace).unwrap_err();
    assert_eq!(std::fs::read(&workspace.out).unwrap(), b"keep me");
}

#[test]
fn existing_output_is_overwritten_with_force() {
    let workspace = workspace(SLICES);
    std::fs::write(&workspace.out, b"replace me").unwrap();
    run(&Request { force: true, ..request(&workspace) }).unwrap();
    assert_eq!(read_out(&workspace).tempo, fixture_song().tempo);
}

#[test]
fn overwriting_an_existing_output_produces_a_warning() {
    let workspace = workspace(SLICES);
    std::fs::write(&workspace.out, b"replace me").unwrap();
    let warnings = run(&Request { force: true, ..request(&workspace) }).unwrap();
    assert!(warnings.iter().any(|warning| warning.contains("overwrote")));
}

#[test]
fn output_equal_to_input_is_rejected_even_with_force() {
    let workspace = workspace(SLICES);
    let result = run(&Request { out: workspace.project.clone(), force: true, ..request(&workspace) });
    assert!(result.is_err());
}

#[test]
fn input_project_is_unchanged_when_output_equals_it_with_force() {
    let workspace = workspace(SLICES);
    run(&Request { out: workspace.project.clone(), force: true, ..request(&workspace) }).unwrap_err();
    assert_eq!(std::fs::read(&workspace.project).unwrap(), std::fs::read(PROJECT_FIXTURE).unwrap());
}

#[test]
fn output_equal_to_input_is_rejected() {
    let workspace = workspace(SLICES);
    let result = run(&Request { out: workspace.project.clone(), ..request(&workspace) });
    assert!(result.is_err());
}

#[test]
fn existing_copied_wav_is_not_overwritten() {
    let workspace = workspace(SLICES);
    let copied = workspace.dest.join("reference.wav");
    std::fs::create_dir_all(&workspace.dest).unwrap();
    std::fs::write(&copied, b"keep me").unwrap();
    run_in(&workspace).unwrap_err();
    assert_eq!(std::fs::read(&copied).unwrap(), b"keep me");
}

#[test]
fn sample_path_of_maximum_length_is_accepted() {
    let workspace = workspace(SLICES);
    let m8_dir = m8_dir_for_path_len(MAX_SAMPLE_PATH_LEN);
    assert!(run(&Request { m8_dir, ..request(&workspace) }).is_ok());
}

#[test]
fn sample_path_one_over_maximum_length_is_rejected() {
    let workspace = workspace(SLICES);
    let m8_dir = m8_dir_for_path_len(MAX_SAMPLE_PATH_LEN + 1);
    assert!(run(&Request { m8_dir, ..request(&workspace) }).is_err());
}

#[test]
fn non_wav_reference_is_rejected() {
    let workspace = workspace_with(&std::fs::read(PROJECT_FIXTURE).unwrap(), b"ID3 not a wav");
    assert!(run_in(&workspace).is_err());
}

fn workspace_with_foreign_default_block() -> Workspace {
    let mut song = fixture_song();
    song.chains[usize::from(DEFAULT_START_ID)].steps[0].phrase = 1;
    workspace_with(&serialized(&song), &wav_bytes(frames_for_slices(SLICES)))
}

#[test]
fn used_default_block_is_rejected() {
    assert!(run_in(&workspace_with_foreign_default_block()).is_err());
}

#[test]
fn used_default_block_that_is_not_a_previous_insertion_is_rejected_even_with_force() {
    let workspace = workspace_with_foreign_default_block();
    assert!(run(&Request { force: true, ..request(&workspace) }).is_err());
}

#[test]
fn rerun_on_own_output_is_rejected_without_force() {
    let workspace = workspace(SLICES);
    run_in(&workspace).unwrap();
    assert!(second_run(&workspace, false).is_err());
}

#[test]
fn rerun_with_force_rebuilds_the_reference_chains() {
    let workspace = workspace(SLICES);
    run_in(&workspace).unwrap();
    second_run(&workspace, true).unwrap();
    let song = read_song(&workspace.out.with_file_name("second.m8s"));
    let cells: Vec<u8> = (0..SLICES)
        .map(|row| song.song.steps[row * SongSteps::TRACK_COUNT])
        .collect();
    assert_eq!(
        cells,
        [DEFAULT_START_ID, DEFAULT_START_ID + 1, DEFAULT_START_ID + 2]
    );
}

#[test]
fn rerun_with_force_warns_about_the_replacement() {
    let workspace = workspace(SLICES);
    run_in(&workspace).unwrap();
    let warnings = second_run(&workspace, true).unwrap();
    assert!(warnings.iter().any(|warning| warning.contains("previous")));
}

#[test]
fn rerun_with_force_overwrites_the_copied_wav() {
    let workspace = workspace(SLICES);
    run_in(&workspace).unwrap();
    std::fs::write(workspace.dest.join("reference.wav"), b"old").unwrap();
    second_run(&workspace, true).unwrap();
    assert!(std::fs::read(workspace.dest.join("reference.wav")).unwrap().starts_with(b"RIFF"));
}

#[test]
fn default_start_id_produces_no_warning() {
    let workspace = workspace(SLICES);
    assert!(run_in(&workspace).unwrap().is_empty());
}

fn run_with_reference_named(workspace: &Workspace, file_name: &str) -> Result<Vec<String>, String> {
    let renamed = workspace.wav.with_file_name(file_name);
    std::fs::rename(&workspace.wav, &renamed).unwrap();
    run(&Request { reference_wav: renamed, ..request(workspace) })
}

#[test]
fn copied_wav_name_is_an_ascii_slug() {
    let workspace = workspace(SLICES);
    run_with_reference_named(&workspace, "My Song, v2 \u{2060}final.wav").unwrap();
    assert!(workspace.dest.join("My_Song_v2_final.wav").exists());
}

#[test]
fn instrument_path_uses_the_slug() {
    let workspace = workspace(SLICES);
    run_with_reference_named(&workspace, "My Song, v2 \u{2060}final.wav").unwrap();
    assert_eq!(reference_sampler(&workspace).sample_path, "/Samples/ref/My_Song_v2_final.wav");
}

#[test]
fn reference_name_without_any_ascii_letters_or_digits_is_rejected() {
    let workspace = workspace(SLICES);
    assert!(run_with_reference_named(&workspace, "!!!.wav").is_err());
}

fn workspace_with_used_reference_slot() -> Workspace {
    let mut song = fixture_song();
    let mut old_sampler = blank_sampler("/old.wav").unwrap();
    old_sampler.name = "OLD".to_string();
    song.instruments[usize::from(REF_INSTRUMENT_SLOT)] = Instrument::Sampler(old_sampler);
    workspace_with(&serialized(&song), &wav_bytes(frames_for_slices(SLICES)))
}

#[test]
fn used_reference_slot_is_rejected_without_force() {
    let workspace = workspace_with_used_reference_slot();
    assert!(run_in(&workspace).is_err());
}

#[test]
fn force_replaces_a_used_reference_slot() {
    let workspace = workspace_with_used_reference_slot();
    run(&Request { force: true, ..request(&workspace) }).unwrap();
    assert_eq!(reference_sampler(&workspace).name, "REFERENCE");
}

#[test]
fn replacing_a_used_reference_slot_produces_a_warning() {
    let workspace = workspace_with_used_reference_slot();
    let warnings = run(&Request { force: true, ..request(&workspace) }).unwrap();
    assert!(warnings.iter().any(|warning| warning.contains("7F")));
}

#[test]
fn force_on_a_free_reference_slot_produces_no_warning() {
    let workspace = workspace(SLICES);
    let warnings = run(&Request { force: true, ..request(&workspace) }).unwrap();
    assert!(warnings.is_empty());
}
