use crate::alloc::{ROW_GAP, block_start, block_is_free, last_used_row, used_ids};
use m8_file_parser::{Chain, Instrument, Note, SamplePlayMode, Sampler, Song, SongSteps, Step, Version};

const SAMPLER_TEMPLATE: &[u8] = include_bytes!("../assets/sampler.m8i");
pub const REF_INSTRUMENT_SLOT: u8 = 0x7F;
pub const REFERENCE_NAME: &str = "REFERENCE";
const FIRST_SLICE: u8 = 1;
const SUPPORTED_FIRMWARE: (u8, u8) = (6, 6);
pub const MAX_SLICES: usize = 128;
const UNSET_VELOCITY: u8 = 0xFF;
const EMPTY_PHRASE_CELL: u8 = 0xFF;
const FIRST_SLICE_NOTE: u8 = 0;
pub const PHRASES_PER_SLICE: usize = 4;
const MAX_CHAIN_PHRASES: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Options {
    pub empty: u8,
    pub phrase: u8,
    pub chain_phrases: usize,
}

#[derive(Debug, PartialEq)]
pub struct Layout {
    pub start_id: usize,
    pub start_row: usize,
    pub slices: usize,
    pub options: Options,
}

impl Layout {
    fn slices_per_chain(&self) -> usize {
        self.options.chain_phrases / PHRASES_PER_SLICE
    }

    pub fn chains(&self) -> usize {
        self.slices.div_ceil(self.slices_per_chain())
    }
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

pub fn plan_layout(song: &Song, slices: usize, options: &Options) -> Result<Layout, String> {
    let Version { major, minor, .. } = song.version;
    if (major, minor) != SUPPORTED_FIRMWARE {
        return Err(format!("only FW 6.6 projects are supported, found {major}.{minor}"));
    }
    if !(1..=MAX_SLICES).contains(&slices) {
        return Err(format!("reference needs {slices} slices, supported range is 1 to {MAX_SLICES}"));
    }
    let chain_phrases = options.chain_phrases;
    if chain_phrases == 0 || chain_phrases > MAX_CHAIN_PHRASES || chain_phrases % PHRASES_PER_SLICE != 0 {
        return Err(format!("chain phrases must be 4, 8, 12 or 16, got {chain_phrases}"));
    }
    let chains = slices.div_ceil(chain_phrases / PHRASES_PER_SLICE);
    let empty = usize::from(options.empty);
    if empty >= Song::N_CHAINS {
        return Err(format!("empty id {:02X} is out of range", options.empty));
    }
    if usize::from(options.phrase) >= Song::N_PHRASES {
        return Err(format!("phrase id {:02X} is out of range", options.phrase));
    }
    if options.phrase == options.empty {
        return Err(format!("phrase id {:02X} must differ from the empty id", options.phrase));
    }
    if !is_silent_chain(&song.chains[empty], options.empty) || !song.phrases[empty].is_empty() {
        return Err(format!(
            "chain {:02X} is not silent or phrase {:02X} is not empty",
            options.empty, options.empty
        ));
    }
    let used = used_ids(song);
    let start_id = block_start(chains);
    if !block_is_free(start_id, chains, &used) {
        return Err(format!(
            "chain ids {start_id:02X} to {:02X} are already used, the reference may already be inserted, pass --force to replace a previous insertion",
            start_id + chains - 1
        ));
    }
    if used.phrases[usize::from(options.phrase)] {
        return Err(format!(
            "phrase {:02X} is already used, the reference may already be inserted, pass --force to replace a previous insertion",
            options.phrase
        ));
    }
    if (start_id..start_id + chains).contains(&empty) {
        return Err(format!("empty id {:02X} lies inside the reference chain block", options.empty));
    }
    let start_row = last_used_row(&song.song).map_or(0, |row| row + 1 + ROW_GAP);
    if start_row + chains > SongSteps::ROW_COUNT {
        return Err(format!("song view has no room for {chains} rows from row {start_row}"));
    }
    Ok(Layout { start_id, start_row, slices, options: *options })
}

pub fn apply(song: &mut Song, layout: &Layout, sample_path: &str) -> Result<(), String> {
    song.instruments[usize::from(REF_INSTRUMENT_SLOT)] = Instrument::Sampler(blank_sampler(sample_path)?);
    song.phrases[usize::from(layout.options.phrase)].steps[0] = Step {
        note: Note(FIRST_SLICE_NOTE),
        velocity: UNSET_VELOCITY,
        instrument: REF_INSTRUMENT_SLOT,
        ..Step::default()
    };
    let slices_per_chain = layout.slices_per_chain();
    (0..layout.slices).for_each(|slice| {
        let steps = &mut song.chains[layout.start_id + slice / slices_per_chain].steps;
        let first = (slice % slices_per_chain) * PHRASES_PER_SLICE;
        steps[first].phrase = layout.options.phrase;
        steps[first].transpose = slice as u8;
        steps[first + 1..first + PHRASES_PER_SLICE]
            .iter_mut()
            .for_each(|step| step.phrase = layout.options.empty);
    });
    (0..layout.chains()).for_each(|chain| {
        song.song.steps[(layout.start_row + chain) * SongSteps::TRACK_COUNT] = (layout.start_id + chain) as u8;
    });
    (1..SongSteps::TRACK_COUNT).for_each(|track| {
        song.song.steps[layout.start_row * SongSteps::TRACK_COUNT + track] = layout.options.empty;
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alloc::{ALIGN, last_used_row};
    use crate::{DEFAULT_CHAIN_PHRASES, DEFAULT_PHRASE_ID};
    use m8_file_parser::SamplePlayMode;

    const PATH: &str = "/Samples/ref/song.wav";
    const EMPTY_ID: u8 = 0xFE;
    const PHRASE_ID: u8 = DEFAULT_PHRASE_ID;
    const OPTIONS: Options = Options { empty: EMPTY_ID, phrase: PHRASE_ID, chain_phrases: DEFAULT_CHAIN_PHRASES };
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

    fn plan(song: &Song, slices: usize) -> Result<Layout, String> {
        plan_layout(song, slices, &OPTIONS)
    }

    fn applied_song() -> Song {
        let mut song = empty_song();
        let layout = plan(&song, SLICES).unwrap();
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

    fn song_with_used_reference_slot() -> Song {
        let mut song = empty_song();
        song.instruments[usize::from(REF_INSTRUMENT_SLOT)] = Instrument::Sampler(blank_sampler(PATH).unwrap());
        song
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
        assert_eq!(blank_sampler(PATH).unwrap().number, REF_INSTRUMENT_SLOT);
    }

    #[test]
    fn blank_sampler_follows_chain_transpose() {
        assert!(blank_sampler(PATH).unwrap().transpose);
    }

    #[test]
    fn firmware_other_than_6_6_is_rejected() {
        let err = plan(&read_song("V6EMPTY.m8s"), SLICES).unwrap_err();
        assert!(err.contains("6.6"));
    }

    #[test]
    fn firmware_6_6_is_accepted() {
        assert!(plan(&empty_song(), SLICES).is_ok());
    }

    #[test]
    fn empty_song_layout_starts_at_default_id_and_row_zero() {
        let layout = plan(&empty_song(), SLICES).unwrap();
        assert_eq!(
            layout,
            Layout { start_id: DEFAULT_START_ID, start_row: 0, slices: SLICES, options: OPTIONS }
        );
    }

    #[test]
    fn used_default_chain_block_is_rejected_with_a_force_hint() {
        let mut song = empty_song();
        song.chains[DEFAULT_START_ID].steps[0].phrase = ANY_PHRASE;
        let err = plan(&song, SLICES).unwrap_err();
        assert!(err.contains("--force"));
    }

    #[test]
    fn used_reference_phrase_is_rejected_with_a_force_hint() {
        let mut song = empty_song();
        song.phrases[usize::from(PHRASE_ID)].steps[0].note = ANY_NOTE;
        let err = plan(&song, SLICES).unwrap_err();
        assert!(err.contains("--force"));
    }

    #[test]
    fn used_phrase_among_the_chain_ids_does_not_block_the_layout() {
        let mut song = empty_song();
        song.phrases[DEFAULT_START_ID].steps[0].note = ANY_NOTE;
        assert!(plan(&song, SLICES).is_ok());
    }

    #[test]
    fn phrase_id_equal_to_empty_id_is_rejected() {
        let ids = Options { empty: EMPTY_ID, phrase: EMPTY_ID, chain_phrases: DEFAULT_CHAIN_PHRASES };
        assert!(plan_layout(&empty_song(), SLICES, &ids).is_err());
    }

    #[test]
    fn phrase_id_outside_phrase_range_is_rejected() {
        let ids = Options { empty: EMPTY_ID, phrase: EMPTY_CELL, chain_phrases: DEFAULT_CHAIN_PHRASES };
        assert!(plan_layout(&empty_song(), SLICES, &ids).is_err());
    }

    #[test]
    fn more_slices_than_the_default_range_start_one_step_down() {
        let layout = plan(&empty_song(), 49).unwrap();
        assert_eq!(layout.start_id, DEFAULT_START_ID - ALIGN);
    }

    #[test]
    fn too_many_slices_are_rejected() {
        let err = plan(&empty_song(), MAX_SLICES + 1).unwrap_err();
        assert!(err.contains("128"));
    }

    #[test]
    fn zero_slices_are_rejected() {
        assert!(plan(&empty_song(), 0).is_err());
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
        assert!(plan(&song_with_used_reference_slot(), SLICES).is_ok());
    }

    #[test]
    fn chain_with_an_audible_phrase_is_rejected() {
        let mut song = empty_song();
        song.chains[usize::from(EMPTY_ID)].steps[0].phrase = ANY_PHRASE;
        assert!(plan(&song, SLICES).is_err());
    }

    #[test]
    fn chain_of_only_empty_phrases_is_accepted() {
        assert!(plan(&silent_chain_song(), SLICES).is_ok());
    }

    #[test]
    fn silent_chain_is_left_untouched_by_apply() {
        let mut song = silent_chain_song();
        let before = song.chains[usize::from(EMPTY_ID)].clone();
        let layout = plan(&song, SLICES).unwrap();
        apply(&mut song, &layout, PATH).unwrap();
        assert_eq!(song.chains[usize::from(EMPTY_ID)], before);
    }

    #[test]
    fn non_empty_empty_phrase_is_rejected() {
        let mut song = empty_song();
        song.phrases[usize::from(EMPTY_ID)].steps[0].note = ANY_NOTE;
        assert!(plan(&song, SLICES).is_err());
    }

    #[test]
    fn empty_id_inside_the_block_is_rejected() {
        let ids = Options { empty: id(1), phrase: PHRASE_ID, chain_phrases: DEFAULT_CHAIN_PHRASES };
        assert!(plan_layout(&empty_song(), SLICES, &ids).is_err());
    }

    #[test]
    fn empty_id_outside_chain_range_is_rejected() {
        let ids = Options { empty: EMPTY_CELL, phrase: PHRASE_ID, chain_phrases: DEFAULT_CHAIN_PHRASES };
        assert!(plan_layout(&empty_song(), SLICES, &ids).is_err());
    }

    #[test]
    fn start_row_leaves_gap_after_last_used_row() {
        let mut song = empty_song();
        song.song.steps[LAST_USED_ROW * TRACKS] = ANY_CHAIN;
        let layout = plan(&song, SLICES).unwrap();
        assert_eq!(layout.start_row, LAST_USED_ROW + 1 + ROW_GAP);
    }

    #[test]
    fn reference_not_fitting_in_remaining_rows_is_rejected() {
        let mut song = empty_song();
        song.song.steps[LAST_ROW * TRACKS] = ANY_CHAIN;
        assert!(plan(&song, SLICES).is_err());
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
    fn every_chain_plays_the_shared_phrase_then_three_empty_phrases() {
        let song = applied_song();
        let phrases: Vec<u8> = song.chains[DEFAULT_START_ID + SECOND_SLICE].steps[..SLICE_CHAIN_LEN]
            .iter()
            .map(|step| step.phrase)
            .collect();
        assert_eq!(phrases, [PHRASE_ID, EMPTY_ID, EMPTY_ID, EMPTY_ID]);
    }

    #[test]
    fn chain_steps_after_the_fourth_stay_empty() {
        let song = applied_song();
        assert_eq!(song.chains[DEFAULT_START_ID].steps[SLICE_CHAIN_LEN].phrase, EMPTY_CELL);
    }

    #[test]
    fn chain_transposes_the_shared_phrase_by_its_slice_index() {
        let song = applied_song();
        assert_eq!(song.chains[DEFAULT_START_ID + SECOND_SLICE].steps[0].transpose, SECOND_SLICE as u8);
    }

    #[test]
    fn empty_phrase_steps_of_a_chain_are_not_transposed() {
        let song = applied_song();
        assert_eq!(song.chains[DEFAULT_START_ID + SECOND_SLICE].steps[SECOND_ROW].transpose, 0);
    }

    #[test]
    fn shared_phrase_row_zero_plays_the_first_slice_note() {
        let song = applied_song();
        assert_eq!(song.phrases[usize::from(PHRASE_ID)].steps[0].note, Note(0));
    }

    #[test]
    fn shared_phrase_row_zero_uses_the_reference_instrument() {
        let song = applied_song();
        assert_eq!(song.phrases[usize::from(PHRASE_ID)].steps[0].instrument, REF_INSTRUMENT_SLOT);
    }

    #[test]
    fn shared_phrase_rows_after_row_zero_stay_empty() {
        let song = applied_song();
        assert!(song.phrases[usize::from(PHRASE_ID)].steps[SECOND_ROW].is_empty());
    }

    #[test]
    fn only_the_shared_phrase_is_written() {
        let used = used_ids(&applied_song());
        let written: Vec<usize> = (0..Song::N_PHRASES)
            .filter(|&phrase| used.phrases[phrase] && phrase != usize::from(EMPTY_ID))
            .collect();
        assert_eq!(written, [usize::from(PHRASE_ID)]);
    }

    #[test]
    fn reference_instrument_is_installed_in_slot_7f() {
        let song = applied_song();
        let Instrument::Sampler(sampler) = &song.instruments[usize::from(REF_INSTRUMENT_SLOT)] else {
            panic!("slot 7F is not a sampler");
        };
        assert_eq!(sampler.sample_path, PATH);
    }

    const PACKED_PHRASES: usize = 8;
    const PACKED_SLICES: usize = 5;
    const PACKED_OPTIONS: Options = Options {
        empty: EMPTY_ID,
        phrase: PHRASE_ID,
        chain_phrases: PACKED_PHRASES,
    };
    const FULLY_PACKED_PHRASES: usize = 16;
    const SLICE_PHRASES: usize = 4;

    fn packed_song() -> Song {
        let mut song = empty_song();
        let layout = plan_layout(&song, PACKED_SLICES, &PACKED_OPTIONS).unwrap();
        apply(&mut song, &layout, PATH).unwrap();
        song
    }

    fn with_chain_phrases(chain_phrases: usize) -> Options {
        Options { chain_phrases, ..OPTIONS }
    }

    #[test]
    fn chain_phrase_counts_that_are_multiples_of_four_up_to_sixteen_are_accepted() {
        let all_accepted = [4, 8, 12, 16]
            .into_iter()
            .all(|count| plan_layout(&empty_song(), SLICES, &with_chain_phrases(count)).is_ok());
        assert!(all_accepted);
    }

    #[test]
    fn chain_phrase_count_that_is_not_a_multiple_of_four_is_rejected() {
        assert!(plan_layout(&empty_song(), SLICES, &with_chain_phrases(6)).is_err());
    }

    #[test]
    fn chain_phrase_count_above_sixteen_is_rejected() {
        assert!(plan_layout(&empty_song(), SLICES, &with_chain_phrases(20)).is_err());
    }

    #[test]
    fn zero_chain_phrases_are_rejected() {
        assert!(plan_layout(&empty_song(), SLICES, &with_chain_phrases(0)).is_err());
    }

    #[test]
    fn packed_layout_uses_one_song_row_per_chain() {
        let song = packed_song();
        let cells: Vec<u8> = (0..3).map(|row| song_cell(&song, row, 0)).collect();
        assert_eq!(cells, [id(0), id(1), id(2)]);
    }

    #[test]
    fn packed_layout_ends_the_song_rows_after_the_last_chain() {
        assert_eq!(last_used_row(&packed_song().song), Some(2));
    }

    #[test]
    fn packed_chain_holds_slice_groups_back_to_back() {
        let song = packed_song();
        let phrases: Vec<u8> = song.chains[DEFAULT_START_ID].steps[..PACKED_PHRASES]
            .iter()
            .map(|step| step.phrase)
            .collect();
        assert_eq!(
            phrases,
            [PHRASE_ID, EMPTY_ID, EMPTY_ID, EMPTY_ID, PHRASE_ID, EMPTY_ID, EMPTY_ID, EMPTY_ID]
        );
    }

    #[test]
    fn packed_chain_ends_after_its_groups() {
        assert_eq!(packed_song().chains[DEFAULT_START_ID].steps[PACKED_PHRASES].phrase, EMPTY_CELL);
    }

    #[test]
    fn packed_second_group_transposes_to_the_next_slice() {
        assert_eq!(packed_song().chains[DEFAULT_START_ID].steps[SLICE_PHRASES].transpose, 1);
    }

    #[test]
    fn packed_next_chain_continues_the_slice_numbering() {
        assert_eq!(packed_song().chains[DEFAULT_START_ID + 1].steps[0].transpose, 2);
    }

    #[test]
    fn partly_filled_last_chain_holds_only_the_remaining_slice() {
        let last = &packed_song().chains[DEFAULT_START_ID + 2];
        assert_eq!((last.steps[0].transpose, last.steps[SLICE_PHRASES].phrase), (4, EMPTY_CELL));
    }

    #[test]
    fn packed_layout_leaves_unneeded_chains_untouched() {
        assert!(packed_song().chains[DEFAULT_START_ID + 3].is_empty());
    }

    #[test]
    fn fully_packed_maximum_slices_still_start_at_the_default_id() {
        let layout = plan_layout(&empty_song(), MAX_SLICES, &with_chain_phrases(FULLY_PACKED_PHRASES)).unwrap();
        assert_eq!(layout.start_id, DEFAULT_START_ID);
    }

    #[test]
    fn max_slices_are_accepted() {
        assert!(plan(&empty_song(), MAX_SLICES).is_ok());
    }

    #[test]
    fn reference_filling_the_last_rows_exactly_is_accepted() {
        let mut song = empty_song();
        song.song.steps[(SongSteps::ROW_COUNT - ROW_GAP - SLICES - 1) * TRACKS] = ANY_CHAIN;
        assert!(plan(&song, SLICES).is_ok());
    }
}
