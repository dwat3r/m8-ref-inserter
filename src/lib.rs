pub mod alloc;
pub mod insert;
pub mod wav;

use alloc::DEFAULT_START;
use insert::{apply, plan_layout};
use m8_file_parser::Song;
use m8_file_parser::writer::Writer;
use std::path::{Path, PathBuf};

pub const MAX_SAMPLE_PATH_LEN: usize = 127;
pub const DEFAULT_EMPTY_ID: u8 = 0xFE;

pub struct Request {
    pub project: PathBuf,
    pub reference_wav: PathBuf,
    pub dest_dir: PathBuf,
    pub m8_dir: String,
    pub out: PathBuf,
    pub empty_id: u8,
}

fn read_file(path: &Path) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    std::fs::write(path, bytes).map_err(|e| format!("cannot write {}: {e}", path.display()))
}

fn create_dir(path: &Path) -> Result<(), String> {
    std::fs::create_dir_all(path).map_err(|e| format!("cannot create {}: {e}", path.display()))
}

fn slug(stem: &str) -> String {
    stem.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_'))
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("_")
        .trim_matches('_')
        .to_string()
}

fn refuse_existing(path: &Path) -> Result<(), String> {
    if path.exists() {
        return Err(format!("{} already exists, refusing to overwrite", path.display()));
    }
    Ok(())
}

pub fn run(request: &Request) -> Result<Option<String>, String> {
    refuse_existing(&request.out)?;
    let project_bytes = read_file(&request.project)?;
    let mut song = Song::read(&mut project_bytes.as_slice())
        .map_err(|e| format!("cannot parse {}: {e:?}", request.project.display()))?;
    let reference = wav::parse(&read_file(&request.reference_wav)?)?;
    let slice_frames = wav::slice_frames(reference.sample_rate, song.tempo);
    let slices = wav::slice_count(&reference, slice_frames);
    let layout = plan_layout(&song, slices, request.empty_id)?;

    let stem = request
        .reference_wav
        .file_stem()
        .ok_or_else(|| format!("{} has no file name", request.reference_wav.display()))?
        .to_string_lossy();
    let stem = slug(&stem);
    if stem.is_empty() {
        return Err(format!(
            "{} has no ASCII letters or digits to name the sample after",
            request.reference_wav.display()
        ));
    }
    let file_name = format!("{stem}.wav");
    let sample_path = format!("{}/{file_name}", request.m8_dir.trim_end_matches('/'));
    if sample_path.len() > MAX_SAMPLE_PATH_LEN {
        return Err(format!("sample path {sample_path} is longer than {MAX_SAMPLE_PATH_LEN} characters"));
    }
    let copied_wav = request.dest_dir.join(&file_name);
    refuse_existing(&copied_wav)?;

    apply(&mut song, &layout, &sample_path)?;
    let mut writer = Writer::new(project_bytes);
    song.write(&mut writer)?;

    create_dir(&request.dest_dir)?;
    write_file(&copied_wav, &wav::with_cue_points(&reference, slice_frames, slices))?;
    write_file(&request.out, &writer.finish())?;

    Ok((layout.start_id != DEFAULT_START).then(|| {
        format!(
            "warning: reference ids start at {:02X} instead of {DEFAULT_START:02X}, that block is in use",
            layout.start_id
        )
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spaces_commas_and_invisible_characters_become_one_underscore() {
        assert_eq!(
            slug("Modestep, Cassyette - \u{2060}Limerence"),
            "Modestep_Cassyette_-_Limerence"
        );
    }

    #[test]
    fn plain_name_is_unchanged() {
        assert_eq!(slug("Limerence_v2-final"), "Limerence_v2-final");
    }

    #[test]
    fn leading_and_trailing_symbols_are_trimmed() {
        assert_eq!(slug("  ??song!! "), "song");
    }

    #[test]
    fn non_ascii_letters_are_dropped() {
        assert_eq!(slug("caf\u{e9}"), "caf");
    }

    #[test]
    fn name_of_only_symbols_is_empty() {
        assert_eq!(slug("!!!"), "");
    }
}
