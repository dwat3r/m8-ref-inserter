pub mod alloc;
pub mod insert;
pub mod inspect;
pub mod previous;
pub mod wav;
#[cfg(target_arch = "wasm32")]
mod web;

use insert::{Layout, Options, REF_INSTRUMENT_SLOT, apply, plan_layout, reference_slot_is_free};
use m8_file_parser::Song;
use m8_file_parser::writer::Writer;
use previous::{clear_previous, previous_insertion};
use std::path::{Path, PathBuf};

pub const MAX_SAMPLE_PATH_LEN: usize = 127;
pub const DEFAULT_EMPTY_ID: u8 = 0xFE;
pub const DEFAULT_PHRASE_ID: u8 = 0xF0;
pub const DEFAULT_CHAIN_PHRASES: usize = 4;

pub struct Request {
    pub project: PathBuf,
    pub reference_wav: PathBuf,
    pub dest_dir: PathBuf,
    pub m8_dir: String,
    pub out: PathBuf,
    pub empty_id: u8,
    pub phrase_id: u8,
    pub chain_phrases: usize,
    pub force: bool,
    pub dry_run: bool,
    pub bpm: Option<f32>,
}

pub struct Source<'a> {
    pub project: &'a [u8],
    pub project_name: &'a str,
    pub reference: &'a [u8],
    pub reference_name: &'a str,
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen::prelude::wasm_bindgen)]
pub struct Settings {
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter_with_clone))]
    pub m8_dir: String,
    pub empty_id: u8,
    pub phrase_id: u8,
    pub chain_phrases: usize,
    pub force: bool,
    pub bpm: Option<f32>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            m8_dir: String::new(),
            empty_id: DEFAULT_EMPTY_ID,
            phrase_id: DEFAULT_PHRASE_ID,
            chain_phrases: DEFAULT_CHAIN_PHRASES,
            force: false,
            bpm: None,
        }
    }
}

impl From<&Request> for Settings {
    fn from(request: &Request) -> Self {
        Settings {
            m8_dir: request.m8_dir.clone(),
            empty_id: request.empty_id,
            phrase_id: request.phrase_id,
            chain_phrases: request.chain_phrases,
            force: request.force,
            bpm: request.bpm,
        }
    }
}

#[derive(Debug)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen::prelude::wasm_bindgen)]
pub struct Insertion {
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter_with_clone))]
    pub project: Vec<u8>,
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter_with_clone))]
    pub wav: Vec<u8>,
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter_with_clone))]
    pub file_name: String,
    pub slices: usize,
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter_with_clone))]
    pub summary: Vec<String>,
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter_with_clone))]
    pub warnings: Vec<String>,
}

#[derive(Debug)]
pub struct Report {
    pub warnings: Vec<String>,
    pub slices: usize,
    pub copied_wav: PathBuf,
    pub out: PathBuf,
    pub dry_run: bool,
    pub reused_wav: bool,
    insertion_summary: Vec<String>,
}

struct Summary {
    slices: usize,
    chains: usize,
    first_chain: usize,
    first_row: usize,
    phrase_id: u8,
    sample_path: String,
    bpm: f32,
    slice_seconds: f64,
    reference_seconds: f64,
}

impl Summary {
    fn lines(&self) -> Vec<String> {
        let last_chain = self.first_chain + self.chains - 1;
        let last_row = self.first_row + self.chains - 1;
        vec![
            format!(
                "{} slices in {} chains ({:02X} to {:02X}) on track 1, song rows {:02X} to {:02X}",
                self.slices, self.chains, self.first_chain, last_chain, self.first_row, last_row
            ),
            format!(
                "slice length {:.2} s ({} beats at {} BPM), reference {}:{:02}",
                self.slice_seconds,
                wav::BEATS_PER_SLICE,
                self.bpm,
                self.reference_seconds as u64 / 60,
                self.reference_seconds as u64 % 60
            ),
            format!(
                "shared phrase {:02X}, instrument {REF_INSTRUMENT_SLOT:02X} plays {}",
                self.phrase_id, self.sample_path
            ),
        ]
    }
}

impl Report {
    pub fn summary(&self) -> Vec<String> {
        let written = if self.dry_run {
            "dry run: would write"
        } else {
            "wrote"
        };
        let files = if self.reused_wav {
            format!(
                "{written} {}; {} already up to date",
                self.out.display(),
                self.copied_wav.display()
            )
        } else {
            format!(
                "{written} {} and {}",
                self.out.display(),
                self.copied_wav.display()
            )
        };
        [self.insertion_summary.clone(), vec![files]].concat()
    }
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

fn is_same_file(first: &Path, second: &Path) -> bool {
    matches!((first.canonicalize(), second.canonicalize()), (Ok(a), Ok(b)) if a == b)
}

fn refuse_existing(path: &Path) -> Result<(), String> {
    if path.exists() {
        return Err(format!(
            "{} already exists, refusing to overwrite",
            path.display()
        ));
    }
    Ok(())
}

pub struct Project {
    bytes: Vec<u8>,
    song: Song,
}

impl Project {
    pub fn parse(bytes: &[u8], name: &str) -> Result<Self, String> {
        let song =
            Song::read(&mut &bytes[..]).map_err(|e| format!("cannot parse {name}: {e:?}"))?;
        Ok(Project {
            bytes: bytes.to_vec(),
            song,
        })
    }
}

pub struct Reference<'a> {
    pub bytes: &'a [u8],
    pub name: &'a str,
}

pub struct Plan {
    project: Project,
    layout: Layout,
    sample_path: String,
    pub file_name: String,
    pub wav: Vec<u8>,
    slices: usize,
    summary: Vec<String>,
    warnings: Vec<String>,
}

impl Plan {
    pub fn into_insertion(mut self) -> Result<Insertion, String> {
        apply(&mut self.project.song, &self.layout, &self.sample_path)?;
        let mut writer = Writer::new(self.project.bytes);
        self.project.song.write(&mut writer)?;
        Ok(Insertion {
            project: writer.finish(),
            wav: self.wav,
            file_name: self.file_name,
            slices: self.slices,
            summary: self.summary,
            warnings: self.warnings,
        })
    }
}

pub fn insert_reference(source: &Source, settings: &Settings) -> Result<Insertion, String> {
    let project = Project::parse(source.project, source.project_name)?;
    let reference = Reference {
        bytes: source.reference,
        name: source.reference_name,
    };
    plan_insertion(project, &reference, settings)?.into_insertion()
}

pub fn plan_insertion(
    project: Project,
    reference: &Reference,
    settings: &Settings,
) -> Result<Plan, String> {
    let Project { bytes, mut song } = project;
    let reference_name = reference.name;
    let reference = wav::parse(reference.bytes)?;
    let bpm = settings.bpm.unwrap_or(song.tempo);
    if !bpm.is_finite() || bpm <= 0.0 {
        return Err(format!("bpm must be a positive number, got {bpm}"));
    }
    let slice_frames = wav::slice_frames(reference.sample_rate, bpm);
    let slices = wav::slice_count(&reference, slice_frames);
    let options = Options {
        empty: settings.empty_id,
        phrase: settings.phrase_id,
        chain_phrases: settings.chain_phrases,
    };
    let previous = previous_insertion(&song, &options);
    let replaces_instrument = !reference_slot_is_free(&song);
    if settings.force {
        clear_previous(&mut song, &previous, options.empty);
    }
    let layout = plan_layout(&song, slices, &options)?;
    if replaces_instrument && !settings.force {
        return Err(format!(
            "instrument {REF_INSTRUMENT_SLOT:02X} is already used, pass --force to replace it"
        ));
    }

    let stem = Path::new(reference_name)
        .file_stem()
        .ok_or_else(|| format!("{reference_name} has no file name"))?
        .to_string_lossy();
    let stem = slug(&stem);
    if stem.is_empty() {
        return Err(format!(
            "{reference_name} has no ASCII letters or digits to name the sample after"
        ));
    }
    let file_name = format!("{stem}.wav");
    let sample_path = format!("{}/{file_name}", settings.m8_dir.trim_end_matches('/'));
    if sample_path.len() > MAX_SAMPLE_PATH_LEN {
        return Err(format!(
            "sample path {sample_path} is longer than {MAX_SAMPLE_PATH_LEN} characters"
        ));
    }
    let cued_wav = wav::with_cue_points(&reference, slice_frames, slices);

    let previous_warning = (!previous.is_empty()).then(|| {
        format!(
            "warning: replacing a previous reference insertion ({} chains)",
            previous.chains.len()
        )
    });
    let instrument_warning = (replaces_instrument && previous.is_empty())
        .then(|| format!("warning: replacing existing instrument {REF_INSTRUMENT_SLOT:02X}"));
    let stray_warning = previous.stray_phrase.filter(|_| settings.force).map(|phrase| {
        format!("warning: overwriting phrase {phrase:02X}, it holds a single note on instrument {REF_INSTRUMENT_SLOT:02X}")
    });
    let warnings = [previous_warning, stray_warning, instrument_warning]
        .into_iter()
        .flatten()
        .collect();
    let summary = Summary {
        slices,
        chains: layout.chains(),
        first_chain: layout.start_id,
        first_row: layout.start_row,
        phrase_id: options.phrase,
        sample_path: sample_path.clone(),
        bpm,
        slice_seconds: f64::from(slice_frames) / f64::from(reference.sample_rate),
        reference_seconds: reference.frames() as f64 / f64::from(reference.sample_rate),
    };
    Ok(Plan {
        project: Project { bytes, song },
        layout,
        sample_path,
        file_name,
        wav: cued_wav,
        slices,
        summary: summary.lines(),
        warnings,
    })
}

pub fn run(request: &Request) -> Result<Report, String> {
    if is_same_file(&request.project, &request.out) {
        return Err(format!(
            "{} is the input project, refusing to overwrite it",
            request.out.display()
        ));
    }
    if !request.force {
        refuse_existing(&request.out)?;
    }
    let overwrites_output = request.out.exists();
    let project = Project::parse(
        &read_file(&request.project)?,
        &request.project.display().to_string(),
    )?;
    let reference_bytes = read_file(&request.reference_wav)?;
    let plan = plan_insertion(
        project,
        &Reference {
            bytes: &reference_bytes,
            name: &request.reference_wav.display().to_string(),
        },
        &Settings::from(request),
    )?;

    let copied_wav = request.dest_dir.join(&plan.file_name);
    let reuses_wav = copied_wav.exists() && read_file(&copied_wav)? == plan.wav;
    if !reuses_wav && !request.force {
        refuse_existing(&copied_wav)?;
    }
    let insertion = plan.into_insertion()?;

    if !request.dry_run {
        create_dir(&request.dest_dir)?;
        if !reuses_wav {
            write_file(&copied_wav, &insertion.wav)?;
        }
        write_file(&request.out, &insertion.project)?;
    }

    let output_warning = overwrites_output
        .then(|| format!("warning: overwriting existing {}", request.out.display()));
    Ok(Report {
        warnings: [insertion.warnings, output_warning.into_iter().collect()].concat(),
        slices: insertion.slices,
        copied_wav,
        out: request.out.clone(),
        dry_run: request.dry_run,
        reused_wav: reuses_wav,
        insertion_summary: insertion.summary,
    })
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
