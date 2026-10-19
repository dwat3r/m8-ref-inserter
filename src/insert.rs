use crate::alloc::{ROW_GAP, block_is_free, block_start, last_used_row, used_ids};
use m8_file_parser::{Chain, Instrument, Note, SamplePlayMode, Sampler, Song, SongSteps, Step, Version};

const SAMPLER_TEMPLATE: &[u8] = include_bytes!("../assets/sampler.m8i");
pub const REF_INSTRUMENT_SLOT: u8 = 0x7F;
pub const REFERENCE_NAME: &str = "REFERENCE";
const FIRST_SLICE: u8 = 1;
const SUPPORTED_FIRMWARE: (u8, u8) = (6, 6);
pub const MAX_SLICES: usize = 128;
const UNSET_VELOCITY: u8 = 0xFF;
const EMPTY_PHRASE_CELL: u8 = 0xFF;

#[derive(Debug, PartialEq)]
pub struct Layout {
    pub start_id: usize,
    pub start_row: usize,
    pub slices: usize,
    pub empty_id: u8,
}

pub fn blank_sampler(sample_path: &str) -> Result<Sampler, String> {
    let template = Instrument::read(&mut &SAMPLER_TEMPLATE[..]).map_err(|e| format!("{e:?}"))?;
    match template.instrument {
        Instrument::Sampler(sampler) => Ok(Sampler {
            number: REF_INSTRUMENT_SLOT,
            name: REFERENCE_NAME.to_string(),
            sample_path: sample_path.to_string(),
            play_mode: SamplePlayMode::FWD,
            slice: FIRST_SLICE,
            ..sampler
        }),
        _ => Err("assets/sampler.m8i is not a Sampler instrument".to_string()),
    }
}

fn is_silent_chain(chain: &Chain, empty_id: u8) -> bool {
    chain
        .steps
        .iter()
        .all(|step| step.phrase == empty_id || step.phrase == EMPTY_PHRASE_CELL)
}

pub fn reference_slot_is_free(song: &Song) -> bool {
    song.instruments[usize::from(REF_INSTRUMENT_SLOT)].is_empty()
}

pub fn plan_layout(song: &Song, slices: usize, empty_id: u8) -> Result<Layout, String> {
    let Version { major, minor, .. } = song.version;
    if (major, minor) != SUPPORTED_FIRMWARE {
        return Err(format!("only FW 6.6 projects are supported, found {major}.{minor}"));
    }
    if !(1..=MAX_SLICES).contains(&slices) {
        return Err(format!("reference needs {slices} slices, supported range is 1 to {MAX_SLICES}"));
    }
    let empty = usize::from(empty_id);
    if empty >= Song::N_CHAINS {
        return Err(format!("empty id {empty_id:02X} is out of range"));
    }
    if !is_silent_chain(&song.chains[empty], empty_id) || !song.phrases[empty].is_empty() {
        return Err(format!("chain {empty_id:02X} is not silent or phrase {empty_id:02X} is not empty"));
    }
    let start_id = block_start(slices);
    if !block_is_free(start_id, slices, &used_ids(song)) {
        return Err(format!(
            "chain or phrase ids {start_id:02X} to {:02X} are already used, the reference may already be inserted, pass --force to replace a previous insertion",
            start_id + slices - 1
        ));
    }
    if (start_id..start_id + slices).contains(&empty) {
        return Err(format!("empty id {empty_id:02X} lies inside the reference id block"));
    }
    let start_row = last_used_row(&song.song).map_or(0, |row| row + 1 + ROW_GAP);
    if start_row + slices > SongSteps::ROW_COUNT {
        return Err(format!("song view has no room for {slices} rows from row {start_row}"));
    }
    Ok(Layout { start_id, start_row, slices, empty_id })
}

pub fn apply(song: &mut Song, layout: &Layout, sample_path: &str) -> Result<(), String> {
    song.instruments[usize::from(REF_INSTRUMENT_SLOT)] = Instrument::Sampler(blank_sampler(sample_path)?);
    (0..layout.slices).for_each(|slice| {
        let id = layout.start_id + slice;
        song.phrases[id].steps[0] = Step {
            note: Note(slice as u8),
            velocity: UNSET_VELOCITY,
            instrument: REF_INSTRUMENT_SLOT,
            ..Step::default()
        };
        [id as u8, layout.empty_id, layout.empty_id, layout.empty_id]
            .iter()
            .zip(song.chains[id].steps.iter_mut())
            .for_each(|(&phrase, step)| step.phrase = phrase);
        song.song.steps[(layout.start_row + slice) * SongSteps::TRACK_COUNT] = id as u8;
    });
    (1..SongSteps::TRACK_COUNT).for_each(|track| {
        song.song.steps[layout.start_row * SongSteps::TRACK_COUNT + track] = layout.empty_id;
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alloc::ALIGN;
    use m8_file_parser::{Note, SamplePlayMode, SongSteps};

    const PATH: &str = "/Samples/ref/song.wav";
    const EMPTY_ID: u8 = 0xFE;
    const SLICES: usize = 3;
    const DEFAULT_START_ID: usize = 0xC0;
    const EMPTY_CELL: u8 = 0xFF;
    const LAST_USED_ROW: usize = 4;
    const LAST_ROW: usize = SongSteps::ROW_COUNT - 1;
    const TRACKS: usize = SongSteps::TRACK_COUNT;
    const SLICE_CHAIN_LEN: usize = 4;
    const SECOND_SLICE: usize = 1;
    const SECOND_ROW: usize = 1;
    const SECOND_TRACK: usize = 1;
    const ANY_PHRASE: u8 = 1;
    const ANY_CHAIN: u8 = 1;
    const ANY_NOTE: Note = Note(0);

    fn id(offset: usize) -> u8 {
        (DEFAULT_START_ID + offset) as u8
    }

    fn read_song(fixture: &str) -> Song {
        let bytes = std::fs::read(format!("tests/fixtures/{fixture}")).unwrap();
        Song::read(&mut bytes.as_slice()).unwrap()
    }

    fn empty_song() -> Song {
        read_song("V6_6EMPTY.m8s")
    }

    fn applied_song() -> Song {
        let mut song = empty_song();
        let layout = plan_layout(&song, SLICES, EMPTY_ID).unwrap();
        apply(&mut song, &layout, PATH).unwrap();
        song
    }

    fn silent_chain_song() -> Song {
        let mut song = empty_song();
        song.chains[usize::from(EMPTY_ID)].steps[..SLICE_CHAIN_LEN]
            .iter_mut()
            .for_each(|step| step.phrase = EMPTY_ID);
        song
    }

    fn song_cell(song: &Song, row: usize, track: usize) -> u8 {
        song.song.steps[row * TRACKS + track]
    }

    #[test]
    fn firmware_other_than_6_6_is_rejected() {
        let err = plan_layout(&read_song("V6EMPTY.m8s"), SLICES, EMPTY_ID).unwrap_err();
        assert!(err.contains("6.6"));
    }

    #[test]
    fn firmware_6_6_is_accepted() {
        assert!(plan_layout(&empty_song(), SLICES, EMPTY_ID).is_ok());
    }

    #[test]
    fn empty_song_layout_starts_at_default_id_and_row_zero() {
        let layout = plan_layout(&empty_song(), SLICES, EMPTY_ID).unwrap();
        assert_eq!(
            layout,
            Layout { start_id: DEFAULT_START_ID, start_row: 0, slices: SLICES, empty_id: EMPTY_ID }
        );
    }

    #[test]
    fn used_default_block_is_rejected_with_a_force_hint() {
        let mut song = empty_song();
        song.chains[DEFAULT_START_ID].steps[0].phrase = ANY_PHRASE;
        let err = plan_layout(&song, SLICES, EMPTY_ID).unwrap_err();
        assert!(err.contains("--force"));
    }

    #[test]
    fn more_slices_than_the_default_range_start_one_step_down() {
        let layout = plan_layout(&empty_song(), 49, EMPTY_ID).unwrap();
        assert_eq!(layout.start_id, DEFAULT_START_ID - ALIGN);
    }

    #[test]
    fn too_many_slices_are_rejected() {
        let err = plan_layout(&empty_song(), MAX_SLICES + 1, EMPTY_ID).unwrap_err();
        assert!(err.contains("128"));
    }

    #[test]
    fn zero_slices_are_rejected() {
        assert!(plan_layout(&empty_song(), 0, EMPTY_ID).is_err());
    }

    fn song_with_used_reference_slot() -> Song {
        let mut song = empty_song();
        song.instruments[usize::from(REF_INSTRUMENT_SLOT)] = Instrument::Sampler(blank_sampler(PATH).unwrap());
        song
    }

    #[test]
    fn used_reference_instrument_slot_is_not_free() {
        assert!(!reference_slot_is_free(&song_with_used_reference_slot()));
    }

    #[test]
    fn unused_reference_instrument_slot_is_free() {
        assert!(reference_slot_is_free(&empty_song()));
    }

    #[test]
    fn layout_planning_does_not_judge_the_reference_slot() {
        assert!(plan_layout(&song_with_used_reference_slot(), SLICES, EMPTY_ID).is_ok());
    }

    #[test]
    fn chain_with_an_audible_phrase_is_rejected() {
        let mut song = empty_song();
        song.chains[usize::from(EMPTY_ID)].steps[0].phrase = ANY_PHRASE;
        assert!(plan_layout(&song, SLICES, EMPTY_ID).is_err());
    }

    #[test]
    fn chain_of_only_empty_phrases_is_accepted() {
        assert!(plan_layout(&silent_chain_song(), SLICES, EMPTY_ID).is_ok());
    }

    #[test]
    fn silent_chain_is_left_untouched_by_apply() {
        let mut song = silent_chain_song();
        let before = song.chains[usize::from(EMPTY_ID)].clone();
        let layout = plan_layout(&song, SLICES, EMPTY_ID).unwrap();
        apply(&mut song, &layout, PATH).unwrap();
        assert_eq!(song.chains[usize::from(EMPTY_ID)], before);
    }

    #[test]
    fn non_empty_empty_phrase_is_rejected() {
        let mut song = empty_song();
        song.phrases[usize::from(EMPTY_ID)].steps[0].note = ANY_NOTE;
        assert!(plan_layout(&song, SLICES, EMPTY_ID).is_err());
    }

    #[test]
    fn empty_id_inside_the_block_is_rejected() {
        let inside_block = (DEFAULT_START_ID + 1) as u8;
        assert!(plan_layout(&empty_song(), SLICES, inside_block).is_err());
    }

    #[test]
    fn empty_id_outside_chain_range_is_rejected() {
        assert!(plan_layout(&empty_song(), SLICES, EMPTY_CELL).is_err());
    }

    #[test]
    fn start_row_leaves_gap_after_last_used_row() {
        let mut song = empty_song();
        song.song.steps[LAST_USED_ROW * TRACKS] = ANY_CHAIN;
        let layout = plan_layout(&song, SLICES, EMPTY_ID).unwrap();
        assert_eq!(layout.start_row, LAST_USED_ROW + 1 + ROW_GAP);
    }

    #[test]
    fn reference_not_fitting_in_remaining_rows_is_rejected() {
        let mut song = empty_song();
        song.song.steps[LAST_ROW * TRACKS] = ANY_CHAIN;
        assert!(plan_layout(&song, SLICES, EMPTY_ID).is_err());
    }

    #[test]
    fn reference_chains_fill_track_one_from_start_row() {
        let song = applied_song();
        let cells: Vec<u8> = (0..SLICES).map(|row| song_cell(&song, row, 0)).collect();
        assert_eq!(cells, [id(0), id(1), id(2)]);
    }

    #[test]
    fn other_tracks_get_empty_chain_at_start_row() {
        let song = applied_song();
        let cells: Vec<u8> = (1..TRACKS).map(|track| song_cell(&song, 0, track)).collect();
        assert_eq!(cells, [EMPTY_ID; TRACKS - 1]);
    }

    #[test]
    fn other_tracks_stay_empty_after_start_row() {
        assert_eq!(song_cell(&applied_song(), SECOND_ROW, SECOND_TRACK), EMPTY_CELL);
    }

    #[test]
    fn chain_is_slice_phrase_then_three_empty_phrases() {
        let song = applied_song();
        let phrases: Vec<u8> = song.chains[DEFAULT_START_ID].steps[..SLICE_CHAIN_LEN].iter().map(|step| step.phrase).collect();
        assert_eq!(phrases, [id(0), EMPTY_ID, EMPTY_ID, EMPTY_ID]);
    }

    #[test]
    fn chain_steps_after_the_fourth_stay_empty() {
        let song = applied_song();
        assert_eq!(song.chains[DEFAULT_START_ID].steps[SLICE_CHAIN_LEN].phrase, EMPTY_CELL);
    }

    #[test]
    fn phrase_row_zero_plays_the_slice_note() {
        let song = applied_song();
        assert_eq!(song.phrases[DEFAULT_START_ID + SECOND_SLICE].steps[0].note, Note(SECOND_SLICE as u8));
    }

    #[test]
    fn phrase_row_zero_uses_the_reference_instrument() {
        let song = applied_song();
        assert_eq!(song.phrases[DEFAULT_START_ID].steps[0].instrument, REF_INSTRUMENT_SLOT);
    }

    #[test]
    fn phrase_rows_after_row_zero_stay_empty() {
        let song = applied_song();
        assert!(song.phrases[DEFAULT_START_ID].steps[SECOND_ROW].is_empty());
    }

    #[test]
    fn reference_instrument_is_installed_in_slot_7f() {
        let song = applied_song();
        let Instrument::Sampler(sampler) = &song.instruments[usize::from(REF_INSTRUMENT_SLOT)] else {
            panic!("slot 7F is not a sampler");
        };
        assert_eq!(sampler.sample_path, PATH);
    }

    #[test]
    fn blank_sampler_is_named_reference() {
        assert_eq!(blank_sampler(PATH).unwrap().name, "REFERENCE");
    }

    #[test]
    fn blank_sampler_points_at_sample_path() {
        assert_eq!(blank_sampler(PATH).unwrap().sample_path, PATH);
    }

    #[test]
    fn blank_sampler_plays_forward() {
        assert_eq!(blank_sampler(PATH).unwrap().play_mode, SamplePlayMode::FWD);
    }

    #[test]
    fn blank_sampler_selects_first_slice() {
        assert_eq!(blank_sampler(PATH).unwrap().slice, 1);
    }

    #[test]
    fn blank_sampler_is_instrument_7f() {
        assert_eq!(blank_sampler(PATH).unwrap().number, 0x7F);
    }

    #[test]
    fn max_slices_are_accepted() {
        assert!(plan_layout(&empty_song(), MAX_SLICES, EMPTY_ID).is_ok());
    }

    #[test]
    fn reference_filling_the_last_rows_exactly_is_accepted() {
        let mut song = empty_song();
        song.song.steps[(SongSteps::ROW_COUNT - ROW_GAP - SLICES - 1) * TRACKS] = ANY_CHAIN;
        assert!(plan_layout(&song, SLICES, EMPTY_ID).is_ok());
    }
}
