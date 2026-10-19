use m8_file_parser::{Chain, Phrase, Song, SongSteps};
use std::collections::HashSet;

pub const ID_LIMIT: usize = 0xF0;
pub const DEFAULT_START: usize = 0xC0;
pub const ALIGN: usize = 0x10;
pub const ROW_GAP: usize = 8;
const EMPTY_CELL: u8 = 0xFF;

pub fn block_start(block_size: usize) -> usize {
    DEFAULT_START.min((ID_LIMIT - block_size) / ALIGN * ALIGN)
}

pub struct UsedIds {
    pub chains: Vec<bool>,
    pub phrases: Vec<bool>,
}

pub fn block_is_free(start: usize, block_size: usize, used: &UsedIds) -> bool {
    (start..start + block_size).all(|id| !used.chains[id] && !used.phrases[id])
}

fn referenced_ids(cells: impl Iterator<Item = u8>) -> HashSet<u8> {
    cells.filter(|&id| id != EMPTY_CELL).collect()
}

fn flag_used(is_empty: impl Iterator<Item = bool>, referenced: &HashSet<u8>) -> Vec<bool> {
    is_empty
        .enumerate()
        .map(|(id, empty)| !empty || referenced.contains(&(id as u8)))
        .collect()
}

pub fn used_ids(song: &Song) -> UsedIds {
    let chains_in_song = referenced_ids(song.song.steps.iter().copied());
    let phrases_in_chains = referenced_ids(
        song.chains
            .iter()
            .flat_map(|chain| chain.steps.iter().map(|step| step.phrase)),
    );
    UsedIds {
        chains: flag_used(song.chains.iter().map(Chain::is_empty), &chains_in_song),
        phrases: flag_used(song.phrases.iter().map(Phrase::is_empty), &phrases_in_chains),
    }
}

pub fn last_used_row(steps: &SongSteps) -> Option<usize> {
    steps
        .steps
        .chunks(SongSteps::TRACK_COUNT)
        .rposition(|row| row.iter().any(|&chain| chain != EMPTY_CELL))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SMALL_BLOCK: usize = 10;
    const IDS_BELOW_LIMIT_FROM_DEFAULT_START: usize = ID_LIMIT - DEFAULT_START;
    const MAX_SLICES: usize = 128;
    const ONE_STEP_DOWN: usize = DEFAULT_START - ALIGN;
    const LAST_ID_IN_SMALL_BLOCK: usize = DEFAULT_START + SMALL_BLOCK - 1;
    const FIRST_ID_PAST_SMALL_BLOCK: usize = DEFAULT_START + SMALL_BLOCK;
    const CHAIN_WITH_CONTENT: usize = 5;
    const PHRASE_IN_CHAIN: u8 = 3;
    const CHAIN_IN_SONG_CELL: u8 = 9;
    const FILLED_ROW: usize = 3;
    const FILLED_TRACK: usize = 5;

    fn empty_song() -> Song {
        let bytes = std::fs::read("tests/fixtures/V6_6EMPTY.m8s").unwrap();
        Song::read(&mut bytes.as_slice()).unwrap()
    }

    fn only_used(chains: &[usize], phrases: &[usize]) -> UsedIds {
        let flags = |ids: &[usize], len: usize| -> Vec<bool> {
            (0..len).map(|id| ids.contains(&id)).collect()
        };
        UsedIds {
            chains: flags(chains, Song::N_CHAINS),
            phrases: flags(phrases, Song::N_PHRASES),
        }
    }

    fn none_used() -> UsedIds {
        only_used(&[], &[])
    }

    #[test]
    fn small_block_starts_at_default_start() {
        assert_eq!(block_start(SMALL_BLOCK), DEFAULT_START);
    }

    #[test]
    fn block_filling_default_range_still_starts_at_default_start() {
        assert_eq!(block_start(IDS_BELOW_LIMIT_FROM_DEFAULT_START), DEFAULT_START);
    }

    #[test]
    fn block_one_id_over_default_range_starts_one_step_down() {
        assert_eq!(block_start(IDS_BELOW_LIMIT_FROM_DEFAULT_START + 1), ONE_STEP_DOWN);
    }

    #[test]
    fn max_slices_block_ends_exactly_at_id_limit() {
        assert_eq!(block_start(MAX_SLICES), ID_LIMIT - MAX_SLICES);
    }

    #[test]
    fn untouched_block_is_free() {
        assert!(block_is_free(DEFAULT_START, SMALL_BLOCK, &none_used()));
    }

    #[test]
    fn used_chain_in_block_makes_it_not_free() {
        let used = only_used(&[LAST_ID_IN_SMALL_BLOCK], &[]);
        assert!(!block_is_free(DEFAULT_START, SMALL_BLOCK, &used));
    }

    #[test]
    fn used_phrase_in_block_makes_it_not_free() {
        let used = only_used(&[], &[DEFAULT_START]);
        assert!(!block_is_free(DEFAULT_START, SMALL_BLOCK, &used));
    }

    #[test]
    fn used_id_just_past_the_block_leaves_it_free() {
        let used = only_used(&[FIRST_ID_PAST_SMALL_BLOCK], &[]);
        assert!(block_is_free(DEFAULT_START, SMALL_BLOCK, &used));
    }

    #[test]
    fn empty_song_has_no_used_chains() {
        assert!(used_ids(&empty_song()).chains.iter().all(|used| !used));
    }

    #[test]
    fn empty_song_has_no_used_phrases() {
        assert!(used_ids(&empty_song()).phrases.iter().all(|used| !used));
    }

    #[test]
    fn chain_with_content_is_used() {
        let mut song = empty_song();
        song.chains[CHAIN_WITH_CONTENT].steps[0].phrase = PHRASE_IN_CHAIN;
        assert!(used_ids(&song).chains[CHAIN_WITH_CONTENT]);
    }

    #[test]
    fn chain_referenced_by_song_cell_is_used() {
        let mut song = empty_song();
        song.song.steps[0] = CHAIN_IN_SONG_CELL;
        assert!(used_ids(&song).chains[usize::from(CHAIN_IN_SONG_CELL)]);
    }

    #[test]
    fn phrase_referenced_by_chain_step_is_used() {
        let mut song = empty_song();
        song.chains[CHAIN_WITH_CONTENT].steps[0].phrase = PHRASE_IN_CHAIN;
        assert!(used_ids(&song).phrases[usize::from(PHRASE_IN_CHAIN)]);
    }

    #[test]
    fn empty_song_has_no_last_used_row() {
        assert_eq!(last_used_row(&empty_song().song), None);
    }

    #[test]
    fn last_used_row_is_the_lowest_row_with_any_track_filled() {
        let mut song = empty_song();
        song.song.steps[FILLED_ROW * SongSteps::TRACK_COUNT + FILLED_TRACK] = 1;
        assert_eq!(last_used_row(&song.song), Some(FILLED_ROW));
    }
}
