use crate::Project;
use crate::insert::{Options, PHRASES_PER_SLICE, REF_INSTRUMENT_SLOT};
use crate::previous::{PreviousInsertion, previous_insertion, slice_groups};
use m8_file_parser::{Instrument, Song, SongSteps};
use std::path::Path;

const TRACKS: usize = SongSteps::TRACK_COUNT;

fn instrument_line(song: &Song) -> String {
    let slot = REF_INSTRUMENT_SLOT;
    match &song.instruments[usize::from(slot)] {
        Instrument::None => format!("instrument {slot:02X}: empty"),
        Instrument::Sampler(sampler) => format!(
            "instrument {slot:02X}: sampler {:?} plays {}",
            sampler.name, sampler.sample_path
        ),
        _ => format!("instrument {slot:02X}: not a sampler"),
    }
}

fn placement(song: &Song, previous: &PreviousInsertion) -> String {
    let rows: Vec<usize> = (0..SongSteps::ROW_COUNT)
        .filter(|&row| {
            previous
                .chains
                .contains(&usize::from(song.song.steps[row * TRACKS]))
        })
        .collect();
    match (rows.first(), rows.last()) {
        (Some(first), Some(last)) => format!("track 1 song rows {first:02X} to {last:02X}"),
        _ => "not placed in the song view".to_string(),
    }
}

fn previous_lines(song: &Song, options: &Options) -> Vec<String> {
    let previous = previous_insertion(song, options);
    let (Some(first), Some(last)) = (previous.chains.first(), previous.chains.last()) else {
        return vec!["previous insertion: none".to_string()];
    };
    let shared = previous.phrases == [usize::from(options.phrase)];
    let groups: Vec<usize> = previous
        .chains
        .iter()
        .map(|&chain| {
            if shared {
                slice_groups(song, chain, options)
            } else {
                1
            }
        })
        .collect();
    let layout = if shared {
        let phrases_per_chain = groups.iter().max().copied().unwrap_or(0) * PHRASES_PER_SLICE;
        format!(
            "layout: shared phrase {:02X}, {phrases_per_chain} phrases per chain",
            options.phrase
        )
    } else {
        "layout: one phrase per chain (older layout)".to_string()
    };
    vec![
        format!(
            "previous insertion: {} chains ({first:02X} to {last:02X}), {} slices, {}",
            previous.chains.len(),
            groups.iter().sum::<usize>(),
            placement(song, &previous)
        ),
        layout,
    ]
}

fn stray_line(song: &Song, options: &Options) -> Option<String> {
    previous_insertion(song, options)
        .stray_phrase
        .map(|phrase| {
            format!(
                "stray phrase {phrase:02X}: a single note on instrument {REF_INSTRUMENT_SLOT:02X}"
            )
        })
}

pub fn inspect_project(project: &Project, options: &Options) -> Vec<String> {
    let song = &project.song;
    [
        vec![
            format!("FW {}, tempo {}", song.version, song.tempo),
            instrument_line(song),
        ],
        previous_lines(song, options),
        stray_line(song, options).into_iter().collect(),
    ]
    .concat()
}

pub fn inspect(project: &Path, options: &Options) -> Result<Vec<String>, String> {
    let bytes =
        std::fs::read(project).map_err(|e| format!("cannot read {}: {e}", project.display()))?;
    let parsed = Project::parse(&bytes, &project.display().to_string())?;
    Ok(inspect_project(&parsed, options))
}
