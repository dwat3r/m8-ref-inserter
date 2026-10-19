use crate::alloc::ID_LIMIT;
use crate::insert::{REF_INSTRUMENT_SLOT, REFERENCE_NAME, SpecialIds};
use m8_file_parser::{Instrument, Song, SongSteps};

const EMPTY_CELL: u8 = 0xFF;
const TRACKS: usize = SongSteps::TRACK_COUNT;

#[derive(Debug, PartialEq)]
pub struct PreviousInsertion {
    pub chains: Vec<usize>,
    pub phrases: Vec<usize>,
}

impl PreviousInsertion {
    pub fn is_empty(&self) -> bool {
        self.chains.is_empty()
    }
}

fn reference_sampler_installed(song: &Song) -> bool {
    matches!(
        &song.instruments[usize::from(REF_INSTRUMENT_SLOT)],
        Instrument::Sampler(sampler) if sampler.name == REFERENCE_NAME
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

fn is_shared_phrase_chain(song: &Song, id: usize, ids: &SpecialIds) -> bool {
    chain_phrases_match(song, id, ids.phrase, ids.empty)
        && song.chains[id].steps[1..].iter().all(|step| step.transpose == 0)
        && phrase_plays_reference(song, usize::from(ids.phrase))
}

fn is_own_phrase_chain(song: &Song, id: usize, ids: &SpecialIds) -> bool {
    chain_phrases_match(song, id, id as u8, ids.empty) && phrase_plays_reference(song, id)
}

pub fn previous_insertion(song: &Song, ids: &SpecialIds) -> PreviousInsertion {
    if !reference_sampler_installed(song) {
        return PreviousInsertion { chains: Vec::new(), phrases: Vec::new() };
    }
    let shared: Vec<usize> = (0..ID_LIMIT)
        .filter(|&id| is_shared_phrase_chain(song, id, ids))
        .collect();
    let own: Vec<usize> = (0..ID_LIMIT)
        .filter(|&id| is_own_phrase_chain(song, id, ids))
        .collect();
    let shared_phrase = (!shared.is_empty()).then_some(usize::from(ids.phrase));
    let mut chains = [shared, own.clone()].concat();
    chains.sort_unstable();
    PreviousInsertion {
        chains,
        phrases: shared_phrase.into_iter().chain(own).collect(),
    }
}

pub fn clear_previous(song: &mut Song, previous: &PreviousInsertion, empty_id: u8) {
    previous.chains.iter().for_each(|&id| song.chains[id].clear());
    previous.phrases.iter().for_each(|&id| song.phrases[id].clear());
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
    use crate::DEFAULT_PHRASE_ID;
    use crate::alloc::{DEFAULT_START, last_used_row, used_ids};
    use crate::insert::{apply, blank_sampler, plan_layout};
    use m8_file_parser::{Note, Step};

    const EMPTY_ID: u8 = 0xFE;
    const PHRASE_ID: u8 = DEFAULT_PHRASE_ID;
    const IDS: SpecialIds = SpecialIds { empty: EMPTY_ID, phrase: PHRASE_ID };
    const SLICES: usize = 3;
    const OLD_LAYOUT_CHAINS: usize = 2;
    const PATH: &str = "/Samples/ref/song.wav";
    const FOREIGN_CHAIN: u8 = 5;
    const ROW_ZERO: usize = 0;
    const TRACKS: usize = SongSteps::TRACK_COUNT;
    const SLOT: usize = 0x7F;
    const UNSET_VELOCITY: u8 = 0xFF;

    fn fixture() -> Song {
        let bytes = std::fs::read("tests/fixtures/V6_6EMPTY.m8s").unwrap();
        Song::read(&mut bytes.as_slice()).unwrap()
    }

    fn with_reference(mut song: Song) -> Song {
        let layout = plan_layout(&song, SLICES, &IDS).unwrap();
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
        let found = previous_insertion(&with_reference(fixture()), &IDS);
        assert_eq!(found.chains, chain_ids(DEFAULT_START, SLICES));
    }

    #[test]
    fn shared_phrase_insertion_owns_only_the_shared_phrase() {
        let found = previous_insertion(&with_reference(fixture()), &IDS);
        assert_eq!(found.phrases, [usize::from(PHRASE_ID)]);
    }

    #[test]
    fn old_own_phrase_chains_are_found() {
        let found = previous_insertion(&song_with_old_layout(), &IDS);
        assert_eq!(found.chains, chain_ids(DEFAULT_START, OLD_LAYOUT_CHAINS));
    }

    #[test]
    fn old_own_phrase_insertion_owns_the_phrase_of_each_chain() {
        let found = previous_insertion(&song_with_old_layout(), &IDS);
        assert_eq!(found.phrases, chain_ids(DEFAULT_START, OLD_LAYOUT_CHAINS));
    }

    #[test]
    fn song_without_reference_has_no_previous_insertion() {
        assert!(previous_insertion(&fixture(), &IDS).is_empty());
    }

    #[test]
    fn chain_breaking_the_pattern_is_not_previous() {
        let mut song = with_reference(fixture());
        song.chains[DEFAULT_START].steps[1].phrase = 1;
        assert!(!previous_insertion(&song, &IDS).chains.contains(&DEFAULT_START));
    }

    #[test]
    fn transposed_empty_step_breaks_the_pattern() {
        let mut song = with_reference(fixture());
        song.chains[DEFAULT_START].steps[1].transpose = 1;
        assert!(!previous_insertion(&song, &IDS).chains.contains(&DEFAULT_START));
    }

    #[test]
    fn sampler_not_named_reference_means_no_previous_insertion() {
        let mut song = with_reference(fixture());
        let Instrument::Sampler(sampler) = &mut song.instruments[SLOT] else {
            panic!("slot 7F is not a sampler");
        };
        sampler.name = format!("NOT {REFERENCE_NAME}");
        assert!(previous_insertion(&song, &IDS).is_empty());
    }

    #[test]
    fn clearing_a_shared_phrase_insertion_leaves_no_used_ids() {
        let mut song = with_reference(fixture());
        let previous = previous_insertion(&song, &IDS);
        clear_previous(&mut song, &previous, EMPTY_ID);
        let used = used_ids(&song);
        assert!(used.chains.iter().chain(&used.phrases).all(|used| !used));
    }

    #[test]
    fn clearing_an_old_layout_insertion_leaves_no_used_ids() {
        let mut song = song_with_old_layout();
        let previous = previous_insertion(&song, &IDS);
        clear_previous(&mut song, &previous, EMPTY_ID);
        let used = used_ids(&song);
        assert!(used.chains.iter().chain(&used.phrases).all(|used| !used));
    }

    #[test]
    fn clearing_empties_the_song_view() {
        let mut song = with_reference(fixture());
        let previous = previous_insertion(&song, &IDS);
        clear_previous(&mut song, &previous, EMPTY_ID);
        assert_eq!(last_used_row(&song.song), None);
    }

    #[test]
    fn clearing_keeps_foreign_song_cells() {
        let mut song = with_reference(song_with_foreign_cell());
        let previous = previous_insertion(&song, &IDS);
        clear_previous(&mut song, &previous, EMPTY_ID);
        assert_eq!(song.song.steps[ROW_ZERO * TRACKS], FOREIGN_CHAIN);
    }

    #[test]
    fn clearing_leaves_the_empty_chain_alone() {
        let mut song = fixture();
        song.chains[usize::from(EMPTY_ID)].steps[0].phrase = EMPTY_ID;
        let mut song = with_reference(song);
        let before = song.chains[usize::from(EMPTY_ID)].clone();
        let previous = previous_insertion(&song, &IDS);
        clear_previous(&mut song, &previous, EMPTY_ID);
        assert_eq!(song.chains[usize::from(EMPTY_ID)], before);
    }
}
