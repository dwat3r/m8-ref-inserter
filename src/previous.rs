use crate::alloc::ID_LIMIT;
use crate::insert::{REF_INSTRUMENT_SLOT, REFERENCE_NAME};
use m8_file_parser::{Instrument, Song, SongSteps};

const EMPTY_CELL: u8 = 0xFF;
const TRACKS: usize = SongSteps::TRACK_COUNT;

fn reference_sampler_installed(song: &Song) -> bool {
    matches!(
        &song.instruments[usize::from(REF_INSTRUMENT_SLOT)],
        Instrument::Sampler(sampler) if sampler.name == REFERENCE_NAME
    )
}

fn is_reference_chain(song: &Song, id: usize, empty_id: u8) -> bool {
    let phrases: Vec<u8> = song.chains[id].steps.iter().map(|step| step.phrase).collect();
    let expected: Vec<u8> = [id as u8, empty_id, empty_id, empty_id]
        .into_iter()
        .chain(std::iter::repeat(EMPTY_CELL))
        .take(phrases.len())
        .collect();
    let rows = &song.phrases[id].steps;
    phrases == expected
        && rows[0].instrument == REF_INSTRUMENT_SLOT
        && !rows[0].note.is_empty()
        && rows[1..].iter().all(|row| row.is_empty())
}

pub fn previous_reference_chains(song: &Song, empty_id: u8) -> Vec<usize> {
    if !reference_sampler_installed(song) {
        return Vec::new();
    }
    (0..ID_LIMIT)
        .filter(|&id| is_reference_chain(song, id, empty_id))
        .collect()
}

pub fn clear_previous(song: &mut Song, ids: &[usize], empty_id: u8) {
    ids.iter().for_each(|&id| {
        song.chains[id].clear();
        song.phrases[id].clear();
    });
    let rows: Vec<usize> = (0..SongSteps::ROW_COUNT)
        .filter(|&row| ids.contains(&usize::from(song.song.steps[row * TRACKS])))
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
    use crate::alloc::{DEFAULT_START, last_used_row, used_ids};
    use crate::insert::{REFERENCE_NAME, apply, plan_layout};
    use m8_file_parser::{Instrument, Song, SongSteps};

    const EMPTY_ID: u8 = 0xFE;
    const SLICES: usize = 3;
    const PATH: &str = "/Samples/ref/song.wav";
    const FOREIGN_CHAIN: u8 = 5;
    const ROW_ZERO: usize = 0;
    const TRACKS: usize = SongSteps::TRACK_COUNT;
    const SLOT: usize = 0x7F;

    fn fixture() -> Song {
        let bytes = std::fs::read("tests/fixtures/V6_6EMPTY.m8s").unwrap();
        Song::read(&mut bytes.as_slice()).unwrap()
    }

    fn with_reference(mut song: Song) -> Song {
        let layout = plan_layout(&song, SLICES, EMPTY_ID).unwrap();
        apply(&mut song, &layout, PATH).unwrap();
        song
    }

    fn song_with_foreign_cell() -> Song {
        let mut song = fixture();
        song.song.steps[ROW_ZERO * TRACKS] = FOREIGN_CHAIN;
        song
    }

    #[test]
    fn inserted_reference_chains_are_found() {
        let song = with_reference(fixture());
        let first = usize::from(DEFAULT_START as u8);
        assert_eq!(previous_reference_chains(&song, EMPTY_ID), [first, first + 1, first + 2]);
    }

    #[test]
    fn song_without_reference_has_no_previous_chains() {
        assert!(previous_reference_chains(&fixture(), EMPTY_ID).is_empty());
    }

    #[test]
    fn chain_breaking_the_pattern_is_not_previous() {
        let mut song = with_reference(fixture());
        song.chains[DEFAULT_START].steps[1].phrase = 1;
        assert!(!previous_reference_chains(&song, EMPTY_ID).contains(&DEFAULT_START));
    }

    #[test]
    fn sampler_not_named_reference_means_no_previous_chains() {
        let mut song = with_reference(fixture());
        let Instrument::Sampler(sampler) = &mut song.instruments[SLOT] else {
            panic!("slot 7F is not a sampler");
        };
        sampler.name = format!("NOT {REFERENCE_NAME}");
        assert!(previous_reference_chains(&song, EMPTY_ID).is_empty());
    }

    #[test]
    fn clearing_leaves_no_used_chain_or_phrase_ids() {
        let mut song = with_reference(fixture());
        let ids = previous_reference_chains(&song, EMPTY_ID);
        clear_previous(&mut song, &ids, EMPTY_ID);
        let used = used_ids(&song);
        assert!(used.chains.iter().chain(&used.phrases).all(|used| !used));
    }

    #[test]
    fn clearing_empties_the_song_view() {
        let mut song = with_reference(fixture());
        let ids = previous_reference_chains(&song, EMPTY_ID);
        clear_previous(&mut song, &ids, EMPTY_ID);
        assert_eq!(last_used_row(&song.song), None);
    }

    #[test]
    fn clearing_keeps_foreign_song_cells() {
        let mut song = with_reference(song_with_foreign_cell());
        let ids = previous_reference_chains(&song, EMPTY_ID);
        clear_previous(&mut song, &ids, EMPTY_ID);
        assert_eq!(song.song.steps[ROW_ZERO * TRACKS], FOREIGN_CHAIN);
    }

    #[test]
    fn clearing_leaves_the_empty_chain_alone() {
        let mut song = fixture();
        song.chains[usize::from(EMPTY_ID)].steps[0].phrase = EMPTY_ID;
        let mut song = with_reference(song);
        let before = song.chains[usize::from(EMPTY_ID)].clone();
        let ids = previous_reference_chains(&song, EMPTY_ID);
        clear_previous(&mut song, &ids, EMPTY_ID);
        assert_eq!(song.chains[usize::from(EMPTY_ID)], before);
    }
}
