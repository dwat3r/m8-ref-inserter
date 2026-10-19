#![allow(dead_code)]

use m8_file_parser::Song;
use ref_inserter::{DEFAULT_CHAIN_PHRASES, DEFAULT_EMPTY_ID, DEFAULT_PHRASE_ID, Request, wav};
use std::path::PathBuf;
use tempfile::TempDir;

pub const PROJECT_FIXTURE: &str = "tests/fixtures/V6_6EMPTY.m8s";
pub const SAMPLE_RATE: u32 = 100;
pub const M8_DIR: &str = "/Samples/ref";

pub struct Workspace {
    _dir: TempDir,
    pub project: PathBuf,
    pub wav: PathBuf,
    pub dest: PathBuf,
    pub out: PathBuf,
}

pub fn fixture_song() -> Song {
    Song::read(&mut std::fs::read(PROJECT_FIXTURE).unwrap().as_slice()).unwrap()
}

pub fn slice_frames() -> u32 {
    wav::slice_frames(SAMPLE_RATE, fixture_song().tempo)
}

pub fn workspace_with(project_bytes: &[u8], wav_contents: &[u8]) -> Workspace {
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

pub fn request(workspace: &Workspace) -> Request {
    Request {
        project: workspace.project.clone(),
        reference_wav: workspace.wav.clone(),
        dest_dir: workspace.dest.clone(),
        m8_dir: M8_DIR.to_string(),
        out: workspace.out.clone(),
        empty_id: DEFAULT_EMPTY_ID,
        phrase_id: DEFAULT_PHRASE_ID,
        chain_phrases: DEFAULT_CHAIN_PHRASES,
        force: false,
        dry_run: false,
        bpm: None,
    }
}

pub fn wav_bytes(sample_rate: u32, frames: u32) -> Vec<u8> {
    let data_len = frames * 2;
    let mut bytes = b"RIFF".to_vec();
    bytes.extend((36 + data_len).to_le_bytes());
    bytes.extend(b"WAVEfmt ");
    bytes.extend(16u32.to_le_bytes());
    bytes.extend([1, 0, 1, 0]);
    bytes.extend(sample_rate.to_le_bytes());
    bytes.extend((sample_rate * 2).to_le_bytes());
    bytes.extend([2, 0, 16, 0]);
    bytes.extend(b"data");
    bytes.extend(data_len.to_le_bytes());
    bytes.extend(vec![0u8; data_len as usize]);
    bytes
}
