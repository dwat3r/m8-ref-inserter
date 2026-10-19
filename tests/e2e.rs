mod common;

use m8_file_parser::{Instrument, Note, SamplePlayMode, Sampler, Song, SongSteps, Step};
use ref_inserter::alloc::DEFAULT_START;
use ref_inserter::insert::{MAX_SLICES, Options, REF_INSTRUMENT_SLOT, blank_sampler};
use ref_inserter::inspect::inspect;
use ref_inserter::{
    DEFAULT_CHAIN_PHRASES, DEFAULT_EMPTY_ID, DEFAULT_PHRASE_ID, MAX_SAMPLE_PATH_LEN, Report,
    Request, run, wav,
};
use std::path::{Path, PathBuf};
use tempfile::TempDir;

const PROJECT_FIXTURE: &str = "tests/fixtures/V6_6EMPTY.m8s";
const SAMPLE_RATE: u32 = 100;
const M8_DIR: &str = "/Samples/ref";
const EMPTY_ID: u8 = DEFAULT_EMPTY_ID;
const PHRASE_ID: u8 = DEFAULT_PHRASE_ID;
const CUSTOM_PHRASE_ID: u8 = 0xEF;
const SECOND_SLICE: usize = 1;
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
    common::wav_bytes(SAMPLE_RATE, frames)
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
        phrase_id: PHRASE_ID,
        chain_phrases: DEFAULT_CHAIN_PHRASES,
        force: false,
        dry_run: false,
        bpm: None,
    }
}

fn run_in(workspace: &Workspace) -> Result<Vec<String>, String> {
    run(&request(workspace)).map(|report| report.warnings)
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
    .map(|report| report.warnings)
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
    assert_eq!(
        reference_sampler(&workspace).sample_path,
        "/Samples/ref/reference.wav"
    );
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
    run(&Request {
        force: true,
        ..request(&workspace)
    })
    .unwrap();
    assert_eq!(read_out(&workspace).tempo, fixture_song().tempo);
}

#[test]
fn overwriting_an_existing_output_produces_a_warning() {
    let workspace = workspace(SLICES);
    std::fs::write(&workspace.out, b"replace me").unwrap();
    let warnings = run(&Request {
        force: true,
        ..request(&workspace)
    })
    .unwrap()
    .warnings;
    assert!(
        warnings
            .iter()
            .any(|warning| warning.contains("overwriting"))
    );
}

#[test]
fn output_equal_to_input_is_rejected_even_with_force() {
    let workspace = workspace(SLICES);
    let result = run(&Request {
        out: workspace.project.clone(),
        force: true,
        ..request(&workspace)
    });
    assert!(result.is_err());
}

#[test]
fn input_project_is_unchanged_when_output_equals_it_with_force() {
    let workspace = workspace(SLICES);
    run(&Request {
        out: workspace.project.clone(),
        force: true,
        ..request(&workspace)
    })
    .unwrap_err();
    assert_eq!(
        std::fs::read(&workspace.project).unwrap(),
        std::fs::read(PROJECT_FIXTURE).unwrap()
    );
}

#[test]
fn output_equal_to_input_is_rejected() {
    let workspace = workspace(SLICES);
    let result = run(&Request {
        out: workspace.project.clone(),
        ..request(&workspace)
    });
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
    assert!(
        run(&Request {
            m8_dir,
            ..request(&workspace)
        })
        .is_ok()
    );
}

#[test]
fn sample_path_one_over_maximum_length_is_rejected() {
    let workspace = workspace(SLICES);
    let m8_dir = m8_dir_for_path_len(MAX_SAMPLE_PATH_LEN + 1);
    assert!(
        run(&Request {
            m8_dir,
            ..request(&workspace)
        })
        .is_err()
    );
}

#[test]
fn non_wav_reference_is_rejected() {
    let workspace = workspace_with(&std::fs::read(PROJECT_FIXTURE).unwrap(), b"ID3 not a wav");
    assert!(run_in(&workspace).is_err());
}

fn workspace_with_unfinished_manual_attempt() -> Workspace {
    let mut song = fixture_song();
    let mut manual_sampler = blank_sampler("/Samples/Reference/manual.wav").unwrap();
    manual_sampler.name = String::new();
    song.instruments[usize::from(REF_INSTRUMENT_SLOT)] = Instrument::Sampler(manual_sampler);
    let lone_note = |note: u8| Step {
        note: Note(note),
        velocity: 0x64,
        instrument: REF_INSTRUMENT_SLOT,
        ..Step::default()
    };
    (0..2usize).for_each(|slice| {
        let id = usize::from(DEFAULT_START_ID) + slice;
        song.phrases[id].steps[0] = lone_note(slice as u8);
        [id as u8, EMPTY_ID, EMPTY_ID, EMPTY_ID]
            .iter()
            .zip(song.chains[id].steps.iter_mut())
            .for_each(|(&phrase, step)| step.phrase = phrase);
        song.song.steps[slice * SongSteps::TRACK_COUNT] = id as u8;
    });
    song.phrases[usize::from(PHRASE_ID)].steps[0] = lone_note(0);
    workspace_with(&serialized(&song), &wav_bytes(frames_for_slices(SLICES)))
}

#[test]
fn unfinished_manual_attempt_is_rejected_without_force() {
    assert!(run_in(&workspace_with_unfinished_manual_attempt()).is_err());
}

#[test]
fn force_replaces_an_unfinished_manual_attempt() {
    let workspace = workspace_with_unfinished_manual_attempt();
    run(&Request {
        force: true,
        ..request(&workspace)
    })
    .unwrap();
    let step = read_out(&workspace).chains[usize::from(DEFAULT_START_ID)].steps[0];
    assert_eq!(step.phrase, PHRASE_ID);
}

#[test]
fn force_resets_the_manual_phrase_to_the_reference_note() {
    let workspace = workspace_with_unfinished_manual_attempt();
    run(&Request {
        force: true,
        ..request(&workspace)
    })
    .unwrap();
    let velocity = read_out(&workspace).phrases[usize::from(PHRASE_ID)].steps[0].velocity;
    assert_eq!(velocity, 0xFF);
}

#[test]
fn overwriting_the_manual_phrase_produces_a_warning() {
    let workspace = workspace_with_unfinished_manual_attempt();
    let warnings = run(&Request {
        force: true,
        ..request(&workspace)
    })
    .unwrap()
    .warnings;
    assert!(warnings.iter().any(|warning| warning.contains("F0")));
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
    assert!(
        run(&Request {
            force: true,
            ..request(&workspace)
        })
        .is_err()
    );
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
    assert!(
        std::fs::read(workspace.dest.join("reference.wav"))
            .unwrap()
            .starts_with(b"RIFF")
    );
}

#[test]
fn default_start_id_produces_no_warning() {
    let workspace = workspace(SLICES);
    assert!(run_in(&workspace).unwrap().is_empty());
}

fn run_with_reference_named(workspace: &Workspace, file_name: &str) -> Result<Vec<String>, String> {
    let renamed = workspace.wav.with_file_name(file_name);
    std::fs::rename(&workspace.wav, &renamed).unwrap();
    run(&Request {
        reference_wav: renamed,
        ..request(workspace)
    })
    .map(|report| report.warnings)
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
    assert_eq!(
        reference_sampler(&workspace).sample_path,
        "/Samples/ref/My_Song_v2_final.wav"
    );
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
    run(&Request {
        force: true,
        ..request(&workspace)
    })
    .unwrap();
    assert_eq!(reference_sampler(&workspace).name, "REFERENCE");
}

#[test]
fn replacing_a_used_reference_slot_produces_a_warning() {
    let workspace = workspace_with_used_reference_slot();
    let warnings = run(&Request {
        force: true,
        ..request(&workspace)
    })
    .unwrap()
    .warnings;
    assert!(warnings.iter().any(|warning| warning.contains("7F")));
}

#[test]
fn force_on_a_free_reference_slot_produces_no_warning() {
    let workspace = workspace(SLICES);
    let warnings = run(&Request {
        force: true,
        ..request(&workspace)
    })
    .unwrap()
    .warnings;
    assert!(warnings.is_empty());
}

#[test]
fn chains_play_the_shared_phrase_transposed_by_slice_index() {
    let workspace = workspace(SLICES);
    run_in(&workspace).unwrap();
    let step = read_out(&workspace).chains[usize::from(DEFAULT_START_ID) + SECOND_SLICE].steps[0];
    assert_eq!(
        (step.phrase, usize::from(step.transpose)),
        (PHRASE_ID, SECOND_SLICE)
    );
}

#[test]
fn custom_phrase_id_is_used_by_the_chains() {
    let workspace = workspace(SLICES);
    run(&Request {
        phrase_id: CUSTOM_PHRASE_ID,
        ..request(&workspace)
    })
    .unwrap();
    let step = read_out(&workspace).chains[usize::from(DEFAULT_START_ID)].steps[0];
    assert_eq!(step.phrase, CUSTOM_PHRASE_ID);
}

#[test]
fn used_phrase_id_is_rejected() {
    let mut song = fixture_song();
    song.phrases[usize::from(PHRASE_ID)].steps[0].instrument = 1;
    let workspace = workspace_with(&serialized(&song), &wav_bytes(frames_for_slices(SLICES)));
    assert!(run_in(&workspace).is_err());
}

const PACKED_PHRASES: usize = 8;
const PACKED_SLICES: usize = 5;
const PACKED_CHAINS: usize = 3;

fn track_one_cells(song: &Song, rows: usize) -> Vec<u8> {
    (0..rows)
        .map(|row| song.song.steps[row * SongSteps::TRACK_COUNT])
        .collect()
}

#[test]
fn packed_run_writes_one_song_row_per_chain() {
    let workspace = workspace(PACKED_SLICES);
    run(&Request {
        chain_phrases: PACKED_PHRASES,
        ..request(&workspace)
    })
    .unwrap();
    let cells = track_one_cells(&read_out(&workspace), PACKED_CHAINS + 1);
    assert_eq!(
        cells,
        [
            DEFAULT_START_ID,
            DEFAULT_START_ID + 1,
            DEFAULT_START_ID + 2,
            0xFF
        ]
    );
}

#[test]
fn rerun_with_force_replaces_a_packed_insertion_with_a_different_packing() {
    let workspace = workspace(PACKED_SLICES);
    run(&Request {
        chain_phrases: PACKED_PHRASES,
        ..request(&workspace)
    })
    .unwrap();
    second_run(&workspace, true).unwrap();
    let song = read_song(&workspace.out.with_file_name("second.m8s"));
    assert_eq!(
        track_one_cells(&song, PACKED_SLICES)[PACKED_SLICES - 1],
        DEFAULT_START_ID + 4
    );
}

#[test]
fn chain_phrase_count_that_is_not_a_multiple_of_four_is_rejected() {
    let workspace = workspace(SLICES);
    assert!(
        run(&Request {
            chain_phrases: 6,
            ..request(&workspace)
        })
        .is_err()
    );
}

fn dry_run_request(workspace: &Workspace) -> Request {
    Request {
        dry_run: true,
        ..request(workspace)
    }
}

#[test]
fn dry_run_writes_no_files() {
    let workspace = workspace(SLICES);
    run(&dry_run_request(&workspace)).unwrap();
    assert!(!workspace.out.exists() && !workspace.dest.exists());
}

#[test]
fn dry_run_rejects_what_a_real_run_would_reject() {
    let workspace = workspace(MAX_SLICES + 1);
    assert!(run(&dry_run_request(&workspace)).is_err());
}

#[test]
fn dry_run_refuses_an_existing_output_like_a_real_run() {
    let workspace = workspace(SLICES);
    std::fs::write(&workspace.out, b"keep me").unwrap();
    assert!(run(&dry_run_request(&workspace)).is_err());
}

#[test]
fn dry_run_reports_the_same_warnings_as_a_real_run() {
    let workspace = workspace_with_unfinished_manual_attempt();
    let request = Request {
        force: true,
        dry_run: true,
        ..request(&workspace)
    };
    let warnings = run(&request).unwrap().warnings;
    assert!(warnings.iter().any(|warning| warning.contains("F0")));
}

#[test]
fn dry_run_with_force_leaves_the_output_file_absent() {
    let workspace = workspace_with_unfinished_manual_attempt();
    run(&Request {
        force: true,
        dry_run: true,
        ..request(&workspace)
    })
    .unwrap();
    assert!(!workspace.out.exists());
}

fn summary_of(request: &Request) -> String {
    run(request).unwrap().summary().join("\n")
}

#[test]
fn summary_names_slices_and_chain_ids() {
    let workspace = workspace(SLICES);
    assert!(summary_of(&request(&workspace)).contains("3 slices in 3 chains (C0 to C2)"));
}

#[test]
fn summary_names_the_song_rows() {
    let workspace = workspace(SLICES);
    assert!(summary_of(&request(&workspace)).contains("song rows 00 to 02"));
}

#[test]
fn summary_names_the_shared_phrase_and_sample_path() {
    let workspace = workspace(SLICES);
    let summary = summary_of(&request(&workspace));
    assert!(summary.contains("phrase F0") && summary.contains("/Samples/ref/reference.wav"));
}

#[test]
fn summary_says_written_after_a_real_run() {
    let workspace = workspace(SLICES);
    assert!(summary_of(&request(&workspace)).contains("wrote"));
}

#[test]
fn summary_says_dry_run_when_nothing_is_written() {
    let workspace = workspace(SLICES);
    assert!(summary_of(&dry_run_request(&workspace)).contains("dry run"));
}

fn rerun_from_original(workspace: &Workspace) -> Result<Report, String> {
    run(&Request {
        out: workspace.out.with_file_name("again.m8s"),
        ..request(workspace)
    })
}

#[test]
fn identical_existing_wav_is_reused_without_force() {
    let workspace = workspace(SLICES);
    run_in(&workspace).unwrap();
    assert!(rerun_from_original(&workspace).is_ok());
}

#[test]
fn reused_wav_is_reported_as_up_to_date() {
    let workspace = workspace(SLICES);
    run_in(&workspace).unwrap();
    let summary = rerun_from_original(&workspace)
        .unwrap()
        .summary()
        .join("\n");
    assert!(summary.contains("already up to date"));
}

#[test]
fn dry_run_reuses_an_identical_wav_too() {
    let workspace = workspace(SLICES);
    run_in(&workspace).unwrap();
    let again = Request {
        out: workspace.out.with_file_name("again.m8s"),
        dry_run: true,
        ..request(&workspace)
    };
    assert!(run(&again).is_ok());
}

fn fixture_tempo() -> f32 {
    fixture_song().tempo
}

fn with_bpm(bpm: f32) -> Request {
    Request {
        bpm: Some(bpm),
        ..request(&workspace(SLICES))
    }
}

#[test]
fn bpm_override_changes_the_slice_count() {
    let workspace = workspace(SLICES);
    let doubled = fixture_tempo() * 2.0;
    let expected = (frames_for_slices(SLICES) as usize)
        .div_ceil(wav::slice_frames(SAMPLE_RATE, doubled) as usize);
    let report = run(&Request {
        bpm: Some(doubled),
        ..request(&workspace)
    })
    .unwrap();
    assert_eq!(report.slices, expected);
}

#[test]
fn zero_bpm_is_rejected() {
    assert!(run(&with_bpm(0.0)).is_err());
}

#[test]
fn negative_bpm_is_rejected() {
    assert!(run(&with_bpm(-120.0)).is_err());
}

#[test]
fn non_finite_bpm_is_rejected() {
    assert!(run(&with_bpm(f32::NAN)).is_err());
}

#[test]
fn summary_states_the_slice_length_and_tempo() {
    let workspace = workspace(SLICES);
    let summary = summary_of(&request(&workspace));
    assert!(summary.contains(&format!("16 beats at {} BPM", fixture_tempo())));
}

#[test]
fn summary_shows_the_overridden_tempo() {
    let workspace = workspace(SLICES);
    let summary = summary_of(&Request {
        bpm: Some(200.0),
        ..request(&workspace)
    });
    assert!(summary.contains("16 beats at 200 BPM"));
}

#[test]
fn summary_states_the_slice_length_in_seconds() {
    let workspace = workspace(SLICES);
    let seconds =
        f64::from(wav::slice_frames(SAMPLE_RATE, fixture_tempo())) / f64::from(SAMPLE_RATE);
    let summary = summary_of(&request(&workspace));
    assert!(summary.contains(&format!("slice length {seconds:.2} s")));
}

#[test]
fn summary_states_the_reference_length_in_minutes_and_seconds() {
    let workspace = workspace(SLICES);
    let seconds = (f64::from(frames_for_slices(SLICES)) / f64::from(SAMPLE_RATE)) as u64;
    let summary = summary_of(&request(&workspace));
    assert!(summary.contains(&format!("reference {}:{:02}", seconds / 60, seconds % 60)));
}

const INSPECT_OPTIONS: Options = Options {
    empty: EMPTY_ID,
    phrase: PHRASE_ID,
    chain_phrases: DEFAULT_CHAIN_PHRASES,
};

fn inspect_text(project: &Path) -> String {
    inspect(project, &INSPECT_OPTIONS).unwrap().join("\n")
}

#[test]
fn inspect_reports_the_firmware_and_tempo() {
    let workspace = workspace(SLICES);
    let text = inspect_text(&workspace.project);
    assert!(text.contains("FW 6.6") && text.contains(&format!("tempo {}", fixture_tempo())));
}

#[test]
fn inspect_reports_an_empty_instrument_slot() {
    let workspace = workspace(SLICES);
    assert!(inspect_text(&workspace.project).contains("instrument 7F: empty"));
}

#[test]
fn inspect_reports_no_previous_insertion_on_a_clean_project() {
    let workspace = workspace(SLICES);
    assert!(inspect_text(&workspace.project).contains("previous insertion: none"));
}

#[test]
fn inspect_names_the_sampler_and_its_path() {
    let workspace = workspace(SLICES);
    run_in(&workspace).unwrap();
    let text = inspect_text(&workspace.out);
    assert!(text.contains("\"REFERENCE\"") && text.contains("/Samples/ref/reference.wav"));
}

#[test]
fn inspect_describes_a_previous_insertion() {
    let workspace = workspace(SLICES);
    run_in(&workspace).unwrap();
    let text = inspect_text(&workspace.out);
    assert!(text.contains("3 chains (C0 to C2), 3 slices"));
}

#[test]
fn inspect_reports_the_song_rows_of_a_previous_insertion() {
    let workspace = workspace(SLICES);
    run_in(&workspace).unwrap();
    assert!(inspect_text(&workspace.out).contains("track 1 song rows 00 to 02"));
}

#[test]
fn inspect_reports_the_shared_phrase_layout() {
    let workspace = workspace(SLICES);
    run_in(&workspace).unwrap();
    assert!(inspect_text(&workspace.out).contains("shared phrase F0, 4 phrases per chain"));
}

#[test]
fn inspect_reports_the_packing() {
    let workspace = workspace(PACKED_SLICES);
    run(&Request {
        chain_phrases: PACKED_PHRASES,
        ..request(&workspace)
    })
    .unwrap();
    let text = inspect_text(&workspace.out);
    assert!(text.contains("3 chains (C0 to C2), 5 slices") && text.contains("8 phrases per chain"));
}

#[test]
fn inspect_reports_the_older_one_phrase_per_chain_layout() {
    let workspace = workspace_with_unfinished_manual_attempt();
    assert!(inspect_text(&workspace.project).contains("one phrase per chain (older layout)"));
}

#[test]
fn inspect_reports_a_stray_phrase() {
    let workspace = workspace_with_unfinished_manual_attempt();
    assert!(inspect_text(&workspace.project).contains("stray phrase F0"));
}

#[test]
fn inspect_rejects_a_missing_file() {
    let workspace = workspace(SLICES);
    assert!(inspect(&workspace.out, &INSPECT_OPTIONS).is_err());
}

const SLICE_COUNTS: [usize; 5] = [1, 4, 5, 49, 128];
const PACKINGS: [usize; 4] = [4, 8, 12, 16];
const FOREIGN_CHAIN: u8 = 5;

fn slices_and_packings() -> impl Iterator<Item = (usize, usize)> {
    SLICE_COUNTS
        .iter()
        .flat_map(|&slices| PACKINGS.iter().map(move |&packing| (slices, packing)))
}

fn insert_with(workspace: &Workspace, chain_phrases: usize) -> Vec<u8> {
    run(&Request {
        chain_phrases,
        ..request(workspace)
    })
    .unwrap();
    std::fs::read(&workspace.out).unwrap()
}

fn force_over_output(workspace: &Workspace, chain_phrases: usize) -> Vec<u8> {
    let second = workspace.out.with_file_name("second.m8s");
    run(&Request {
        project: workspace.out.clone(),
        out: second.clone(),
        chain_phrases,
        force: true,
        ..request(workspace)
    })
    .unwrap();
    std::fs::read(second).unwrap()
}

fn rerun_reproduces(workspace: &Workspace, chain_phrases: usize) -> bool {
    let first = insert_with(workspace, chain_phrases);
    force_over_output(workspace, chain_phrases) == first
}

#[test]
fn force_rerun_reproduces_the_original_for_every_slice_count_and_packing() {
    let mismatches: Vec<(usize, usize)> = slices_and_packings()
        .filter(|&(slices, packing)| !rerun_reproduces(&workspace(slices), packing))
        .collect();
    assert!(
        mismatches.is_empty(),
        "differing (slices, packing): {mismatches:?}"
    );
}

#[test]
fn force_rerun_reproduces_the_original_next_to_existing_song_content() {
    let mut song = fixture_song();
    song.song.steps[0] = FOREIGN_CHAIN;
    let mismatches: Vec<(usize, usize)> = slices_and_packings()
        .filter(|&(slices, packing)| {
            let wav = wav_bytes(frames_for_slices(slices));
            !rerun_reproduces(&workspace_with(&serialized(&song), &wav), packing)
        })
        .collect();
    assert!(
        mismatches.is_empty(),
        "differing (slices, packing): {mismatches:?}"
    );
}

#[test]
fn changing_the_packing_with_force_matches_a_fresh_insert_of_the_new_packing() {
    let mismatches: Vec<(usize, usize, usize)> = [5usize, 49, 128]
        .into_iter()
        .flat_map(|slices| {
            PACKINGS.iter().flat_map(move |&old| {
                PACKINGS
                    .iter()
                    .filter(move |&&new| new != old)
                    .map(move |&new| (slices, old, new))
            })
        })
        .filter(|&(slices, old, new)| {
            let converted = workspace(slices);
            insert_with(&converted, old);
            let fresh = workspace(slices);
            force_over_output(&converted, new) != insert_with(&fresh, new)
        })
        .collect();
    assert!(
        mismatches.is_empty(),
        "differing (slices, old, new): {mismatches:?}"
    );
}
