use crate::alloc::ID_LIMIT;
use crate::insert::{Options, PHRASES_PER_SLICE, REF_INSTRUMENT_SLOT};
use m8_file_parser::{ChainStep, Instrument, Song, SongSteps};

const EMPTY_CELL: u8 = 0xFF;
const TRACKS: usize = SongSteps::TRACK_COUNT;

#[derive(Debug, PartialEq)]
pub struct PreviousInsertion {
    pub chains: Vec<usize>,
    pub phrases: Vec<usize>,
    pub stray_phrase: Option<usize>,
}

impl PreviousInsertion {
    pub fn is_empty(&self) -> bool {
        self.chains.is_empty()
    }
}

fn reference_sampler_installed(song: &Song) -> bool {
    matches!(
        &song.instruments[usize::from(REF_INSTRUMENT_SLOT)],
        Instrument::Sampler(_)
    )
}

fn phrase_plays_reference(song: &Song, id: usize) -> bool {
    let rows = &song.phrases[id].steps;
    rows[0].instrument == REF_INSTRUMENT_SLOT
        && !rows[0].note.is_empty()
        && rows[1..].iter().all(|row| row.is_empty())
}

fn chain_phrases_match(song: &Song, id: usize, first_phrase: u8, empty_id: u8) -> bool {
    let phrases: Vec<u8> = song.chains[id].steps.iter().map(|step| step.phrase).collect();
    let expected: Vec<u8> = [first_phrase, empty_id, empty_id, empty_id]
        .into_iter()
        .chain(std::iter::repeat(EMPTY_CELL))
        .take(phrases.len())
        .collect();
    phrases == expected
}

fn is_slice_group(group: &[ChainStep], options: &Options) -> bool {
    group[0].phrase == options.phrase
        && group[1..]
            .iter()
            .all(|step| step.phrase == options.empty && step.transpose == 0)
}

fn is_shared_phrase_chain(song: &Song, id: usize, options: &Options) -> bool {
    let steps = &song.chains[id].steps;
    let groups = steps
        .chunks(PHRASES_PER_SLICE)
        .take_while(|group| is_slice_group(group, options))
        .count();
    groups > 0
        && steps[groups * PHRASES_PER_SLICE..]
            .iter()
            .all(|step| step.phrase == EMPTY_CELL)
        && phrase_plays_reference(song, usize::from(options.phrase))
}

fn is_own_phrase_chain(song: &Song, id: usize, options: &Options) -> bool {
    chain_phrases_match(song, id, id as u8, options.empty) && phrase_plays_reference(song, id)
}

fn is_lone_reference_note(song: &Song, id: usize) -> bool {
    let filled: Vec<_> = song.phrases[id].steps.iter().filter(|row| !row.is_empty()).collect();
    matches!(
        filled.as_slice(),
        [only] if !only.note.is_empty() && only.instrument == REF_INSTRUMENT_SLOT
    )
}

fn owned_by_reference(song: &Song, options: &Options) -> (Vec<usize>, Vec<usize>) {
    if !reference_sampler_installed(song) {
        return (Vec::new(), Vec::new());
    }
    let shared: Vec<usize> = (0..ID_LIMIT)
        .filter(|&id| is_shared_phrase_chain(song, id, options))
        .collect();
    let own: Vec<usize> = (0..ID_LIMIT)
        .filter(|&id| is_own_phrase_chain(song, id, options))
        .collect();
    let shared_phrase = (!shared.is_empty()).then_some(usize::from(options.phrase));
    let mut chains = [shared, own.clone()].concat();
    chains.sort_unstable();
    (chains, shared_phrase.into_iter().chain(own).collect())
}

pub fn previous_insertion(song: &Song, options: &Options) -> PreviousInsertion {
    let (chains, phrases) = owned_by_reference(song, options);
    let phrase = usize::from(options.phrase);
    let stray_phrase = (!phrases.contains(&phrase) && is_lone_reference_note(song, phrase)).then_some(phrase);
    PreviousInsertion { chains, phrases, stray_phrase }
}

pub fn clear_previous(song: &mut Song, previous: &PreviousInsertion, empty_id: u8) {
    previous.chains.iter().for_each(|&id| song.chains[id].clear());
    previous.phrases.iter().for_each(|&id| song.phrases[id].clear());
    if let Some(phrase) = previous.stray_phrase {
        song.phrases[phrase].clear();
    }
    let rows: Vec<usize> = (0..SongSteps::ROW_COUNT)
        .filter(|&row| previous.chains.contains(&usize::from(song.song.steps[row * TRACKS])))
        .collect();
    rows.iter()
        .for_each(|&row| song.song.steps[row * TRACKS] = EMPTY_CELL);
    if let Some(&first_row) = rows.first() {
        let silent_tracks: Vec<usize> = (1..TRACKS)
            .filter(|&track| song.song.steps[first_row * TRACKS + track] == empty_id)
            .collect();
        silent_tracks
            .iter()
            .for_each(|&track| song.song.steps[first_row * TRACKS + track] = EMPTY_CELL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DEFAULT_CHAIN_PHRASES, DEFAULT_PHRASE_ID};
    use crate::alloc::{DEFAULT_START, last_used_row, used_ids};
    use crate::insert::{apply, blank_sampler, plan_layout};
    use m8_file_parser::{Note, Step};

    const EMPTY_ID: u8 = 0xFE;
    const PHRASE_ID: u8 = DEFAULT_PHRASE_ID;
    const OPTIONS: Options = Options { empty: EMPTY_ID, phrase: PHRASE_ID, chain_phrases: DEFAULT_CHAIN_PHRASES };
    const SLICES: usize = 3;
    const OLD_LAYOUT_CHAINS: usize = 2;
    const PACKED_PHRASES: usize = 8;
    const PACKED_SLICES: usize = 5;
    const PACKED_CHAINS: usize = 3;
    const PATH: &str = "/Samples/ref/song.wav";
    const FOREIGN_CHAIN: u8 = 5;
    const ROW_ZERO: usize = 0;
    const TRACKS: usize = SongSteps::TRACK_COUNT;
    const SLOT: usize = 0x7F;
    const UNSET_VELOCITY: u8 = 0xFF;
    const MANUAL_VELOCITY: u8 = 0x64;
    const OTHER_INSTRUMENT: u8 = 0x01;

    fn fixture() -> Song {
        let bytes = std::fs::read("tests/fixtures/V6_6EMPTY.m8s").unwrap();
        Song::read(&mut bytes.as_slice()).unwrap()
    }

    fn with_reference(mut song: Song) -> Song {
        let layout = plan_layout(&song, SLICES, &OPTIONS).unwrap();
        apply(&mut song, &layout, PATH).unwrap();
        song
    }

    fn song_with_old_layout() -> Song {
        let mut song = fixture();
        song.instruments[SLOT] = Instrument::Sampler(blank_sampler(PATH).unwrap());
        (0..OLD_LAYOUT_CHAINS).for_each(|slice| {
            let id = DEFAULT_START + slice;
            song.phrases[id].steps[0] = Step {
                note: Note(slice as u8),
                velocity: UNSET_VELOCITY,
                instrument: SLOT as u8,
                ..Step::default()
            };
            [id as u8, EMPTY_ID, EMPTY_ID, EMPTY_ID]
                .iter()
                .zip(song.chains[id].steps.iter_mut())
                .for_each(|(&phrase, step)| step.phrase = phrase);
            song.song.steps[slice * TRACKS] = id as u8;
        });
        (1..TRACKS).for_each(|track| song.song.steps[track] = EMPTY_ID);
        song
    }

    fn song_with_foreign_cell() -> Song {
        let mut song = fixture();
        song.song.steps[ROW_ZERO * TRACKS] = FOREIGN_CHAIN;
        song
    }

    fn chain_ids(first: usize, count: usize) -> Vec<usize> {
        (first..first + count).collect()
    }

    #[test]
    fn shared_phrase_chains_are_found() {
        let found = previous_insertion(&with_reference(fixture()), &OPTIONS);
        assert_eq!(found.chains, chain_ids(DEFAULT_START, SLICES));
    }

    #[test]
    fn shared_phrase_insertion_owns_only_the_shared_phrase() {
        let found = previous_insertion(&with_reference(fixture()), &OPTIONS);
        assert_eq!(found.phrases, [usize::from(PHRASE_ID)]);
    }

    #[test]
    fn old_own_phrase_chains_are_found() {
        let found = previous_insertion(&song_with_old_layout(), &OPTIONS);
        assert_eq!(found.chains, chain_ids(DEFAULT_START, OLD_LAYOUT_CHAINS));
    }

    #[test]
    fn old_own_phrase_insertion_owns_the_phrase_of_each_chain() {
        let found = previous_insertion(&song_with_old_layout(), &OPTIONS);
        assert_eq!(found.phrases, chain_ids(DEFAULT_START, OLD_LAYOUT_CHAINS));
    }

    fn with_packed_reference() -> Song {
        let mut song = fixture();
        let packed = Options { chain_phrases: PACKED_PHRASES, ..OPTIONS };
        let layout = plan_layout(&song, PACKED_SLICES, &packed).unwrap();
        apply(&mut song, &layout, PATH).unwrap();
        song
    }

    #[test]
    fn packed_chains_are_found_whatever_packing_is_configured() {
        let found = previous_insertion(&with_packed_reference(), &OPTIONS);
        assert_eq!(found.chains, chain_ids(DEFAULT_START, PACKED_CHAINS));
    }

    #[test]
    fn clearing_a_packed_insertion_leaves_no_used_ids() {
        let mut song = with_packed_reference();
        let previous = previous_insertion(&song, &OPTIONS);
        clear_previous(&mut song, &previous, EMPTY_ID);
        let used = used_ids(&song);
        assert!(used.chains.iter().chain(&used.phrases).all(|used| !used));
    }

    #[test]
    fn packed_chain_with_a_broken_group_is_not_previous() {
        let mut song = with_packed_reference();
        song.chains[DEFAULT_START].steps[5].phrase = 1;
        assert!(!previous_insertion(&song, &OPTIONS).chains.contains(&DEFAULT_START));
    }

    #[test]
    fn song_without_reference_has_no_previous_insertion() {
        assert!(previous_insertion(&fixture(), &OPTIONS).is_empty());
    }

    #[test]
    fn chain_breaking_the_pattern_is_not_previous() {
        let mut song = with_reference(fixture());
        song.chains[DEFAULT_START].steps[1].phrase = 1;
        assert!(!previous_insertion(&song, &OPTIONS).chains.contains(&DEFAULT_START));
    }

    #[test]
    fn transposed_empty_step_breaks_the_pattern() {
        let mut song = with_reference(fixture());
        song.chains[DEFAULT_START].steps[1].transpose = 1;
        assert!(!previous_insertion(&song, &OPTIONS).chains.contains(&DEFAULT_START));
    }

    #[test]
    fn sampler_with_another_name_is_still_a_previous_insertion() {
        let mut song = with_reference(fixture());
        let Instrument::Sampler(sampler) = &mut song.instruments[SLOT] else {
            panic!("slot 7F is not a sampler");
        };
        sampler.name = String::new();
        assert_eq!(previous_insertion(&song, &OPTIONS).chains, chain_ids(DEFAULT_START, SLICES));
    }

    #[test]
    fn missing_sampler_means_no_previous_insertion() {
        let mut song = with_reference(fixture());
        song.instruments[SLOT] = Instrument::None;
        assert!(previous_insertion(&song, &OPTIONS).is_empty());
    }

    fn song_with_phrase_rows(rows: &[(usize, u8)]) -> Song {
        let mut song = fixture();
        rows.iter().for_each(|&(row, instrument)| {
            song.phrases[usize::from(PHRASE_ID)].steps[row] = Step {
                note: Note(0),
                velocity: MANUAL_VELOCITY,
                instrument,
                ..Step::default()
            };
        });
        song
    }

    #[test]
    fn lone_note_on_instrument_7f_in_the_phrase_slot_is_a_stray_phrase() {
        let song = song_with_phrase_rows(&[(0, SLOT as u8)]);
        assert_eq!(previous_insertion(&song, &OPTIONS).stray_phrase, Some(usize::from(PHRASE_ID)));
    }

    #[test]
    fn lone_note_on_a_later_row_is_still_a_stray_phrase() {
        let song = song_with_phrase_rows(&[(3, SLOT as u8)]);
        assert_eq!(previous_insertion(&song, &OPTIONS).stray_phrase, Some(usize::from(PHRASE_ID)));
    }

    #[test]
    fn phrase_with_two_notes_is_not_a_stray_phrase() {
        let song = song_with_phrase_rows(&[(0, SLOT as u8), (4, SLOT as u8)]);
        assert_eq!(previous_insertion(&song, &OPTIONS).stray_phrase, None);
    }

    #[test]
    fn lone_note_on_another_instrument_is_not_a_stray_phrase() {
        let song = song_with_phrase_rows(&[(0, OTHER_INSTRUMENT)]);
        assert_eq!(previous_insertion(&song, &OPTIONS).stray_phrase, None);
    }

    #[test]
    fn empty_phrase_slot_is_not_a_stray_phrase() {
        assert_eq!(previous_insertion(&fixture(), &OPTIONS).stray_phrase, None);
    }

    #[test]
    fn phrase_owned_by_a_detected_insertion_is_not_a_stray_phrase() {
        let found = previous_insertion(&with_reference(fixture()), &OPTIONS);
        assert_eq!(found.stray_phrase, None);
    }

    #[test]
    fn clearing_empties_a_stray_phrase() {
        let mut song = song_with_phrase_rows(&[(0, SLOT as u8)]);
        let previous = previous_insertion(&song, &OPTIONS);
        clear_previous(&mut song, &previous, EMPTY_ID);
        assert!(song.phrases[usize::from(PHRASE_ID)].is_empty());
    }

    #[test]
    fn clearing_a_shared_phrase_insertion_leaves_no_used_ids() {
        let mut song = with_reference(fixture());
        let previous = previous_insertion(&song, &OPTIONS);
        clear_previous(&mut song, &previous, EMPTY_ID);
        let used = used_ids(&song);
        assert!(used.chains.iter().chain(&used.phrases).all(|used| !used));
    }

    #[test]
    fn clearing_an_old_layout_insertion_leaves_no_used_ids() {
        let mut song = song_with_old_layout();
        let previous = previous_insertion(&song, &OPTIONS);
        clear_previous(&mut song, &previous, EMPTY_ID);
        let used = used_ids(&song);
        assert!(used.chains.iter().chain(&used.phrases).all(|used| !used));
    }

    #[test]
    fn clearing_empties_the_song_view() {
        let mut song = with_reference(fixture());
        let previous = previous_insertion(&song, &OPTIONS);
        clear_previous(&mut song, &previous, EMPTY_ID);
        assert_eq!(last_used_row(&song.song), None);
    }

    #[test]
    fn clearing_keeps_foreign_song_cells() {
        let mut song = with_reference(song_with_foreign_cell());
        let previous = previous_insertion(&song, &OPTIONS);
        clear_previous(&mut song, &previous, EMPTY_ID);
        assert_eq!(song.song.steps[ROW_ZERO * TRACKS], FOREIGN_CHAIN);
    }

    #[test]
    fn clearing_leaves_the_empty_chain_alone() {
        let mut song = fixture();
        song.chains[usize::from(EMPTY_ID)].steps[0].phrase = EMPTY_ID;
        let mut song = with_reference(song);
        let before = song.chains[usize::from(EMPTY_ID)].clone();
        let previous = previous_insertion(&song, &OPTIONS);
        clear_previous(&mut song, &previous, EMPTY_ID);
        assert_eq!(song.chains[usize::from(EMPTY_ID)], before);
    }
}
