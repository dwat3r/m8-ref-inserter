# Reference Song Inserter Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. Do not commit: the user commits.

**Goal:** CLI that inserts a reference WAV into an M8 project as a sliced sampler instrument (7f) plus one chain per slice, placed after the existing song, written to a new `.m8s`.

**Architecture:** Single Rust binary. `wav.rs` (RIFF parse, slice points, cue chunk), `alloc.rs` (pure id/row allocation), `insert.rs` (validates + mutates `Song`), `main.rs` (clap, file IO). `Song` is read with `m8-file-parser`, mutated in memory, written by patching the original bytes in place (`Writer::new(original_bytes)`, `Song::write` seeks to each section).

**Tech Stack:** Rust 2021, `m8-file-parser` (MIT, Twinside; git main, pinned rev, because crates.io 0.7 may lack song writing), `clap` (derive).

## Global Constraints

- CLI: `ref_inserter <project.m8s> <ref.wav> --dest-dir <local dir> --m8-dir <dir as the M8 sees it, e.g. /Samples/ref> --out <new.m8s> [--empty-id <hex, default FE>]`
- FW 6.6 only. A project with another version is an error. The crate's `Song::write` panics on files older than 6.5 anyway.
- Input project is never modified. Only `--out` and the copied WAV are written.
- BPM comes from `Song.tempo`. Reference is trusted to match. Audio starts at beat 0, no offset.
- WAV only (PCM). Anything else: error telling the user to convert.
- Slice length = 16 beats = round(960 * sample_rate / bpm) frames. Slices N = ceil(total_frames / slice_frames); the partial tail is a slice.
- N > 128: error, before any write. Error if N == 0.
- Instrument 7f: Sampler, name `REFERENCE`, play mode FWD, slice param 01, other params default. Error if 7f is already used.
- Ids: chains and phrases use block [start, 0xF0), N ids each. Start aligned to 0x10, candidates C0, B0, A0, 90, ... descending. For N > 48 first candidate is the highest aligned start with start + N <= 0xF0. Pick first block where every chain id and phrase id is unused. Warn when start != C0. Error if none fits.
- "Used" chain = non-empty chain or referenced by a song cell. "Used" phrase = non-empty phrase or referenced by a chain step.
- Empty id E (default FE): chain E and phrase E must both be empty, else error. Error if E is inside the chosen block.
- Phrase k (k = 0..N-1): row 0 = note k (C-1 = byte 0 = slice 1, +1 per semitone), instrument 7f. Chain k = [phrase k, E, E, E, rest 0xFF].
- Song view: reference on track 1 (index 0). Start row = last used row across all tracks + 1 + 8 (empty song: row 0). Track 1 rows start_row..start_row+N get chain ids start..start+N. Tracks 2 to 8 get chain E at start_row only. Error if start_row + N > 0x100.
- Never overwrite existing chains, phrases, song cells, instrument 7f.
- WAV is copied to `<dest-dir>/<wav-stem>_1.wav`. Sample path stored in the instrument: `<m8-dir>/<wav-stem>_1.wav` (max 127 chars).
- Split files and 7e spill are out of scope (error at N > 128 instead).

## File Structure

- `Cargo.toml`: deps.
- `src/main.rs`: args, IO, orchestration. No logic.
- `src/wav.rs`: `Wav`, `parse`, `slice_frames`, `slice_count`, `with_cue_points`.
- `src/alloc.rs`: `used_chains`, `used_phrases`, `last_used_row`, `pick_start`.
- `src/insert.rs`: `plan_layout`, `apply`.
- `tests/fixtures/V6_6EMPTY.m8s`: copied from the crate repo `examples/songs/`.
- `tests/e2e.rs`: end-to-end test via the library functions.

Make the crate a lib + bin (`src/lib.rs` re-exporting modules) so `tests/e2e.rs` can call it.

---

### Task 1: Scaffold and API spike

**Files:**
- Create: `Cargo.toml`, `src/lib.rs`, `src/main.rs`, `tests/fixtures/V6_6EMPTY.m8s`, `tests/roundtrip.rs`

**Interfaces:**
- Produces: `fn blank_sampler(version: Version, name: &str, sample_path: &str) -> Sampler` in `src/insert.rs`, FWD play mode, slice 1.

- [ ] **Step 1: Scaffold.** `cargo init --name ref_inserter` in the project dir. Add deps via `cargo add clap --features derive` and `cargo add m8-file-parser --git https://github.com/Twinside/m8-file-parser` (then pin `rev` in Cargo.toml to the resolved commit). Copy `examples/songs/V6_6EMPTY.m8s` from the crate repo into `tests/fixtures/`.
- [ ] **Step 2: Failing round-trip test.**

```rust
use m8_file_parser::{Song, Writer};

#[test]
fn song_roundtrip_is_byte_identical() {
    let bytes = std::fs::read("tests/fixtures/V6_6EMPTY.m8s").unwrap();
    let song = Song::read(&mut bytes.as_slice()).unwrap();
    let mut w = Writer::new(bytes.clone());
    song.write(&mut w).unwrap();
    assert_eq!(w.finish(), bytes);
}
```

- [ ] **Step 3:** `cargo test roundtrip`. Expected: PASS. If it fails, stop: the crate cannot patch-write this firmware; report the first differing offset to the user.
- [ ] **Step 4: Sampler from template.** `Sampler`/`SynthParams` have no `Default`. The user exports one untouched FW 6.6 Sampler instrument from the M8 to `assets/sampler.m8i`. `blank_sampler` embeds it (`include_bytes!`), parses it with `Instrument::read`, errors if it is not a `Sampler`, then sets name, sample path, `play_mode = FWD`, `slice = 1`. Test asserts `play_mode == FWD`, `slice == 1`, `name == "REFERENCE"`, the path, and that `Instrument::Sampler(..).write` + re-read returns equal values.
- [ ] **Step 5:** Run `cargo test`. Expected: PASS.

### Task 2: WAV slicing

**Files:**
- Create: `src/wav.rs`
- Test: inline `#[cfg(test)]` in `src/wav.rs`

**Interfaces:**
- Produces:
  - `pub struct Wav { pub sample_rate: u32, pub block_align: u16, pub fmt: Vec<u8>, pub data: Vec<u8> }`
  - `pub fn parse(bytes: &[u8]) -> Result<Wav, String>`: accepts RIFF/WAVE with `fmt ` format tag 1 (PCM) or 0xFFFE; ignores other chunks.
  - `pub fn slice_frames(sample_rate: u32, bpm: f32) -> u32` = round(960 * sample_rate / bpm)
  - `pub fn slice_count(wav: &Wav, slice_frames: u32) -> usize` = ceil(frames / slice_frames)
  - `pub fn with_cue_points(wav: &Wav, slice_frames: u32, slices: usize) -> Vec<u8>`: RIFF with `fmt `, `data`, then `cue ` chunk with `slices` points, point k starts at frame k * slice_frames. Layout copied from DigiChain's M8-compatible writer (https://github.com/brian3kb/digichain/blob/main/src/resources.js): chunk size field = 4 + 24 * slices, then slice count u32, then 24 bytes per point: id u32 (k, 0-based), position u32 = 0, `data` fourcc, chunk_start u32 0, block_start u32 0, sample_offset u32 = start frame. `cue ` chunk goes after `data`.

- [x] **Step 1: Verify the marker format.** Done: DigiChain documents "wav CUE point markers, compatible with DirtyWave M8 slice sampler" and its source gives the exact layout above. Not confirmed by Dirtywave docs or hardware; Task 5 step 5 is the proof.
- [ ] **Step 2: Failing tests.**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn wav_bytes(frames: u32) -> Vec<u8> {
        let data_len = frames * 2;
        let mut v = b"RIFF".to_vec();
        v.extend((36 + data_len).to_le_bytes());
        v.extend(b"WAVEfmt ");
        v.extend(16u32.to_le_bytes());
        v.extend([1, 0, 1, 0]);
        v.extend(44100u32.to_le_bytes());
        v.extend(88200u32.to_le_bytes());
        v.extend([2, 0, 16, 0]);
        v.extend(b"data");
        v.extend(data_len.to_le_bytes());
        v.extend(vec![0u8; data_len as usize]);
        v
    }

    #[test]
    fn slice_frames_at_120_bpm_is_8_seconds() {
        assert_eq!(slice_frames(44100, 120.0), 352800);
    }

    #[test]
    fn slice_count_counts_partial_tail() {
        let wav = parse(&wav_bytes(352800 + 1)).unwrap();
        assert_eq!(slice_count(&wav, 352800), 2);
    }

    #[test]
    fn slice_count_exact_multiple_has_no_extra() {
        let wav = parse(&wav_bytes(352800 * 2)).unwrap();
        assert_eq!(slice_count(&wav, 352800), 2);
    }

    #[test]
    fn cue_points_roundtrip_through_parse_and_land_at_slice_starts() {
        let wav = parse(&wav_bytes(352800 * 2)).unwrap();
        let out = with_cue_points(&wav, 352800, 2);
        let cue = out.windows(4).position(|w| w == b"cue ").unwrap();
        let count = u32::from_le_bytes(out[cue + 8..cue + 12].try_into().unwrap());
        let second_start = u32::from_le_bytes(out[cue + 12 + 24 + 20..cue + 12 + 24 + 24].try_into().unwrap());
        assert_eq!((count, second_start), (2, 352800));
        assert_eq!(parse(&out).unwrap().data, wav.data);
    }

    #[test]
    fn non_wav_is_rejected() {
        assert!(parse(b"ID3 not a wav").is_err());
    }
}
```

- [ ] **Step 3:** `cargo test wav`. Expected: FAIL, functions undefined.
- [ ] **Step 4: Implement** `parse` (walk chunks: 4-byte id, u32 LE size, pad to even), `slice_frames`, `slice_count` (`frames = data.len() / block_align`, integer ceil division), `with_cue_points` per the interfaces above. `Result<_, String>` errors name the problem ("not a RIFF/WAVE file", "not PCM", "missing fmt/data chunk").
- [ ] **Step 5:** `cargo test wav`. Expected: PASS.

### Task 3: Id and row allocation

**Files:**
- Create: `src/alloc.rs`
- Test: inline `#[cfg(test)]`

**Interfaces:**
- Consumes: `m8_file_parser::{Song, SongSteps}`
- Produces:
  - `pub const ID_LIMIT: usize = 0xF0; pub const DEFAULT_START: usize = 0xC0; pub const ALIGN: usize = 0x10; pub const ROW_GAP: usize = 8;`
  - `pub fn used_chains(song: &Song) -> Vec<bool>` (len `Song::N_CHAINS`)
  - `pub fn used_phrases(song: &Song) -> Vec<bool>` (len `Song::N_PHRASES`)
  - `pub fn last_used_row(steps: &SongSteps) -> Option<usize>`
  - `pub fn pick_start(n: usize, used_chains: &[bool], used_phrases: &[bool]) -> Option<usize>`

- [ ] **Step 1: Failing tests** (pure helpers take plain slices, so no `Song` fixture needed for the pick logic):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn none_used() -> Vec<bool> { vec![false; 255] }

    #[test]
    fn small_block_starts_at_c0() {
        assert_eq!(pick_start(10, &none_used(), &none_used()), Some(0xC0));
    }

    #[test]
    fn block_of_48_still_starts_at_c0() {
        assert_eq!(pick_start(48, &none_used(), &none_used()), Some(0xC0));
    }

    #[test]
    fn block_of_49_starts_at_b0() {
        assert_eq!(pick_start(49, &none_used(), &none_used()), Some(0xB0));
    }

    #[test]
    fn used_chain_in_block_shifts_down_one_step() {
        let mut chains = none_used();
        chains[0xC5] = true;
        assert_eq!(pick_start(10, &chains, &none_used()), Some(0xB0));
    }

    #[test]
    fn used_phrase_in_block_shifts_down_one_step() {
        let mut phrases = none_used();
        phrases[0xC0] = true;
        assert_eq!(pick_start(10, &none_used(), &phrases), Some(0xB0));
    }

    #[test]
    fn max_128_slices_start_at_70() {
        assert_eq!(pick_start(128, &none_used(), &none_used()), Some(0x70));
    }

    #[test]
    fn everything_used_yields_none() {
        assert_eq!(pick_start(1, &vec![true; 255], &none_used()), None);
    }
}
```

Also add tests for `last_used_row` using a `SongSteps` built by `Song::read` of the fixture then setting `steps[row * 8 + track] = chain`: empty song -> `None`; a chain at row 3 track 5 -> `Some(3)`.

- [ ] **Step 2:** `cargo test alloc`. Expected: FAIL.
- [ ] **Step 3: Implement.**

```rust
pub fn candidate_starts(n: usize) -> Vec<usize> {
    let first = DEFAULT_START.min((ID_LIMIT - n) / ALIGN * ALIGN);
    (0..=first / ALIGN).rev().map(|i| i * ALIGN).collect()
}

pub fn pick_start(n: usize, used_chains: &[bool], used_phrases: &[bool]) -> Option<usize> {
    candidate_starts(n)
        .into_iter()
        .find(|&s| (s..s + n).all(|i| !used_chains[i] && !used_phrases[i]))
}

pub fn last_used_row(steps: &SongSteps) -> Option<usize> {
    steps
        .steps
        .chunks(SongSteps::TRACK_COUNT)
        .rposition(|row| row.iter().any(|&c| c != 0xFF))
}
```

`used_chains`: `referenced` = set of song cells != 0xFF; chain i used = `!song.chains[i].is_empty() || referenced.contains(&(i as u8))`. `used_phrases`: `referenced` = every `ChainStep.phrase != 0xFF` across all chains; same shape. Do not use `SongSteps::last_modified_row` (returns 256 for an empty song).
- [ ] **Step 4:** `cargo test alloc`. Expected: PASS.

### Task 4: Validate and apply to Song

**Files:**
- Create: `src/insert.rs` (extends Task 1's `blank_sampler`)
- Test: inline `#[cfg(test)]` using `tests/fixtures/V6_6EMPTY.m8s`

**Interfaces:**
- Consumes: Task 3 helpers.
- Produces:
  - `pub struct Layout { pub start_id: usize, pub start_row: usize, pub slices: usize }`
  - `pub fn plan_layout(song: &Song, slices: usize, empty_id: u8) -> Result<Layout, String>`
  - `pub fn apply(song: &mut Song, layout: &Layout, empty_id: u8, sample_path: &str)`

- [ ] **Step 1: Failing tests** on the fixture: (0) `V6EMPTY.m8s` (FW 6.0, read-only fixture copied for this test) returns Err mentioning FW 6.6, and the 6.6 fixture passes the check; (a) empty song, 3 slices, E=FE: layout `{start_id: 0xC0, start_row: 0, slices: 3}`; (b) slices = 129 returns Err mentioning 128; (c) slices = 0 returns Err; (d) instrument 7f non-empty returns Err; (e) chain FE non-empty returns Err; (f) phrase FE non-empty returns Err; (g) E = 0xC1 with block C0..C3 returns Err; (h) song with chain at row 255 returns Err (start_row + N > 0x100); (i) after `apply` on (a): `song.song.steps[0]` = 0xC0, `steps[1*8]` = 0xC1, `steps[2*8]` = 0xC2, `steps[1]` (track 2, row 0) = 0xFE, `steps[7]` = 0xFE, `steps[8 + 1]` (track 2, row 1) = 0xFF; `chains[0xC0].steps[0..4]` phrases = [0xC0, 0xFE, 0xFE, 0xFE], `steps[4].phrase == 0xFF`; `phrases[0xC1].steps[0].note == Note(1)` and `.instrument == 0x7F`, other steps of that phrase empty; `instruments[0x7F]` is a Sampler named `REFERENCE` with the given path; (j) song with last used row 4: start_row == 13.
- [ ] **Step 2:** `cargo test insert`. Expected: FAIL.
- [ ] **Step 3: Implement.** `plan_layout` checks in this order: `song.version.major == 6 && song.version.minor == 6` else Err("only FW 6.6 projects are supported, found X.Y"), slices in 1..=128, instrument 7f is `Instrument::None`, chain E and phrase E empty (`Chain::is_empty`, `Phrase::is_empty`), `pick_start` or Err("no free block of N chain+phrase ids below 0xF0"), E not in block, `start_row = last_used_row.map_or(0, |r| r + 1 + ROW_GAP)`, `start_row + slices <= SongSteps::ROW_COUNT`. `apply`: set `instruments[0x7F] = Instrument::Sampler(blank_sampler(...))`; for k in 0..slices set phrase `start_id + k` step 0 to `Step { note: Note(k as u8), velocity: <crate default for an empty step, copy from Step::default()>, instrument: 0x7F, ..Default::default() }`, set chain `start_id + k` steps 0..4 phrases to `[start_id + k, E, E, E]`; set `steps[(start_row + k) * 8] = (start_id + k) as u8`; for tracks 1..8 set `steps[start_row * 8 + t] = E`. Check the velocity: an unset velocity on a note row may need 0xFF (empty) or a real value; confirm against how the fixture stores a normal note and match it. Build with iterator `for_each`/`map`, no mutable counters.
- [ ] **Step 4:** `cargo test insert`. Expected: PASS.

### Task 5: CLI and end-to-end

**Files:**
- Create/Modify: `src/main.rs`, `src/lib.rs`
- Test: `tests/e2e.rs`

**Interfaces:**
- Consumes: Tasks 2 to 4.
- Produces: `pub fn run(project: &Path, wav: &Path, dest_dir: &Path, m8_dir: &str, out: &Path, empty_id: u8) -> Result<String, String>` in `src/lib.rs` (returns warning text, may be empty). `main` only parses args (`--empty-id` parsed as hex via `u8::from_str_radix`) and prints.

- [ ] **Step 1: Failing e2e test** on the fixture plus a generated WAV (synthesised silence, sample_rate 44100, length 2.5 slices at the fixture's tempo; the generator lives in the test file): run into a temp dir, then assert (a) `out` exists, input bytes unchanged; (b) re-read `out` with `Song::read`: instrument 7f is a Sampler with path ending `_1.wav`, 3 chains at the expected ids; (c) the copied WAV in `dest_dir` parses, has a `cue ` chunk with 3 points; (d) a second run with a WAV of 129 slices returns Err and creates no files.
- [ ] **Step 2:** `cargo test --test e2e`. Expected: FAIL.
- [ ] **Step 3: Implement `run`** in order: read project bytes; `Song::read`; read + `parse` wav; `slice_frames(rate, song.tempo)`; `slice_count`; `plan_layout` (all validation before any write); sample path = `m8_dir/<stem>_1.wav`, Err when over 127 chars; `apply`; create `dest_dir`, write WAV with cue points; `Song::write` into `Writer::new(project_bytes.clone())`, write `out`. Warning string when `start_id != 0xC0`. Refuse `out == project`.
- [ ] **Step 4:** `cargo test`. Expected: all PASS.
- [ ] **Step 5: Manual check (user).** Load `--out` on a real M8 or the M8 headless/emulator: instrument 7f plays slice 1 on C-1, chains show at the expected rows, the WAV slices appear. This is the only proof the cue-chunk format matches the firmware.

---

## Open items

- **Cue chunk format** (Task 2, step 1) must be confirmed from the manual and on hardware.
- **Empty-song start row** is 0 with no gap, assumed.
