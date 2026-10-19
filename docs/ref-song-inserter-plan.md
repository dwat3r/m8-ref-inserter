# Reference Song Inserter

Status: built, tested (157 tests), pushed to https://github.com/dwat3r/m8-ref-inserter (MIT).

**Goal:** CLI that inserts a reference WAV into a Dirtywave M8 project (FW 6.6) as a sliced sampler instrument (7F) plus chains that play the slices, placed after the existing song, written to a new `.m8s`.

**Architecture:** Rust lib + bin. The project is read with `m8-file-parser`, mutated in memory, and written by patching the original bytes (`Writer::new(original_bytes)`, `Song::write` seeks to each section). All validation and serialization happen in memory before any file is written.

**Tech stack:** Rust 2024, `m8-file-parser` (MIT, Twinside, git dependency pinned to rev `08d68bc`), `clap` (derive), `tempfile` (dev only).

## Hardware status

- Verified on an M8 by the user: the default layout (shared phrase F0, one chain per slice, chain transpose selects the slice), WAV cue-chunk slices, instrument 7F settings, `--force` on a project with an unfinished manual attempt.
- Not verified on hardware: packed layouts (`--chain-phrases 8`, `12`, `16`). Covered only by tests and byte-level checks.

## CLI

```
ref_inserter <project.m8s> <reference.wav> --dest-dir <local dir> --m8-dir <dir as the M8 sees it> --out <new.m8s>
             [--empty-id <hex, default FE>] [--phrase-id <hex, default F0>]
             [--chain-phrases <4|8|12|16, default 4>] [--force] [--dry-run] [--bpm <f32>]
```

## Behavior

- **Input:** FW 6.6 projects only (error otherwise). The crate's `Song::write` panics on files older than 6.5. BPM comes from `Song.tempo` unless `--bpm` is given (must be finite and positive, else error). The reference is trusted to match and starts at beat 1. A length-based tempo check was rejected: real tracks have silence and tails, so their length is never reliably a whole number of slices. Instead the summary states the slice length in seconds, the tempo used and the reference length so a wrong tempo is visible. PCM WAV only.
- **Slices:** length 16 beats = `round(960 * sample_rate / bpm)` frames. Count = `ceil(frames / slice_frames)`, the partial tail is a slice. 0 slices or more than 128 is an error.
- **Output WAV:** copied to `<dest-dir>/<slug>.wav` with a `cue ` chunk, one point per slice. `slug` is the WAV file stem with each run of characters outside `[A-Za-z0-9_-]` replaced by one `_`, trimmed of leading and trailing `_` (error if empty). The instrument stores `<m8-dir>/<slug>.wav` (max 127 characters).
- **Cue chunk** (layout from DigiChain's M8-compatible writer): size field `4 + 24 * slices`, slice count, then per point 24 bytes: id (0-based), position 0, `data`, chunk start 0, block start 0, sample offset = start frame. The chunk follows `data`.
- **Instrument 7F:** the sampler template `assets/sampler.m8i` (untouched FW 6.6 export, TRANSP on) with name `REFERENCE`, play mode FWD, slice 01, and the sample path.
- **Shared phrase P** (`--phrase-id`, default F0, must differ from the empty id): row 0 = note C-1 (slice 1), velocity unset (0xFF), instrument 7F, other rows empty.
- **Chains:** each slice takes a 4-step group `[P with transpose = slice index, E, E, E]` where E is the empty id. `--chain-phrases n` (4, 8, 12 or 16, else error) puts n/4 groups in a chain, the rest of the chain is empty, and the last chain may be partly filled. Chains needed = `ceil(slices / (n/4))`.
- **Chain ids:** block `[start, start + chains)` with `start = min(0xC0, (0xF0 - chains) / 0x10 * 0x10)`: C0 up to 48 chains, then B0, A0, 90, 80, 70 (128 slices at 4 phrases per chain starts at 70). There is no shifting for used ids: a used chain id in the block aborts with a `--force` hint. A used phrase P (content or referenced by a chain) aborts the same way. "Used" chain = non-empty or referenced by a song cell; "used" phrase = non-empty or referenced by a chain step.
- **Empty id E** (default FE): phrase E must be empty, chain E must be silent (every step is phrase E or empty; an existing `[E, E, E, E]` chain is accepted and never touched). E must be below 0xFF and outside the chain block.
- **Song view:** chains go on track 1, one song row per chain, starting 8 rows below the last used row across all tracks (row 0 on an empty song). Tracks 2 to 8 get chain E at the first reference row only. The rows must fit within 0x100.
- **Overwriting:** chains, phrases and song cells that are not this tool's are never overwritten, even with `--force`. The input project is never overwritten, even with `--force` (same-file check on canonical paths). `--out` and the copied WAV are refused when they exist, unless `--force`.
- **Instrument 7F already used:** error unless `--force`.

## `--force`

- **Previous insertion** (`previous.rs`): instrument 7F is a Sampler (any name), and chains are recognized in two layouts. Shared phrase layout: one or more `[P, E, E, E]` groups (zero transpose on the E steps) followed by empty steps, with phrase P playing the reference, whatever `--chain-phrases` was. Old layout: chain c is `[c, E, E, E]` and phrase c plays the reference. Phrase "plays the reference" = row 0 has a note on instrument 7F and all other rows are empty.
- **Clearing:** those chains and phrases, their track 1 song cells, and the E cells on tracks 2 to 8 on the first reference row. Then it inserts fresh, normally at C0 and the same rows.
- **Stray phrase:** if phrase P holds exactly one non-empty step and it is a note on instrument 7F, it is cleared even if no chain owns it, with a warning.
- **Also with `--force`:** replaces a used instrument 7F, overwrites a differing copied WAV and an existing `--out`.
- **Warnings** (printed to stderr, tense-neutral so they read correctly in a dry run): replacing a previous insertion (with chain count), overwriting the stray phrase, replacing a foreign instrument 7F, overwriting an existing `--out`.

## File structure

- `src/main.rs`: clap args, builds a `Request`, prints warnings and errors. No logic.
- `src/lib.rs`: `Request`, `Report` (warnings plus summary data, `summary()` lines), `run`, constants `DEFAULT_EMPTY_ID`, `DEFAULT_PHRASE_ID`, `DEFAULT_CHAIN_PHRASES`, `MAX_SAMPLE_PATH_LEN`, file helpers, `slug`.
- `src/wav.rs`: `Wav`, `parse`, `slice_frames`, `slice_count`, `with_cue_points`.
- `src/alloc.rs`: `UsedIds`, `used_ids`, `last_used_row`, `block_start`, `block_is_free` (chain ids only), constants `ID_LIMIT`, `DEFAULT_START`, `ALIGN`, `ROW_GAP`.
- `src/insert.rs`: `Options { empty, phrase, chain_phrases }`, `Layout`, `blank_sampler`, `reference_slot_is_free`, `plan_layout`, `apply`, `REF_INSTRUMENT_SLOT`, `REFERENCE_NAME`, `MAX_SLICES`, `PHRASES_PER_SLICE`.
- `src/inspect.rs`: `inspect(project, options) -> Result<Vec<String>, String>` for the `inspect` subcommand: FW and tempo, instrument 7F, previous insertion (chain count and range, slice count, track 1 song rows, layout and phrases per chain), stray phrase.
- `src/previous.rs`: `PreviousInsertion { chains, phrases, stray_phrase }`, `previous_insertion`, `slice_groups`, `clear_previous`.
- `assets/sampler.m8i`: sampler template.
- `tests/fixtures/V6_6EMPTY.m8s`, `tests/fixtures/V6EMPTY.m8s`: from the crate repo; the FW 6.0 one only tests the version check.
- `tests/roundtrip.rs`: `Song` read then write is byte identical.
- `tests/e2e.rs`: `run` and `inspect` end to end on generated WAVs in temp dirs.
- `tests/cli.rs`: the built binary: `--version`, bare insert still works, missing arguments, `inspect`, `--help`.
- `tests/common/mod.rs`: the shared WAV generator.
- `README.md`, `LICENSE` (MIT).
- `.github/workflows/ci.yml`: on push and pull request to `master`, and on `v*` tags. Matrix: macOS arm64 (tests, build), macOS x86_64 (cross-compiled on the arm64 runner, build only, to avoid paid Intel runners), Linux x86_64 and Windows x86_64 (tests, build). Each job packages `ref_inserter-<target>.tar.gz` (binary, README, LICENSE) as a workflow artifact; a tag push also creates a GitHub Release with `gh release create` and attaches the archives.

## `run` order

1. Refuse `--out` that is the input project; refuse an existing `--out` unless `--force`.
2. Read and parse the project, parse the WAV, compute slices.
3. Detect a previous insertion; note whether instrument 7F is used; with `--force` clear the previous insertion and stray phrase.
4. `plan_layout` (version, slice count, chain phrases, ids, silent E, free chain block, free phrase P, rows).
5. Error if 7F is used without `--force`; build the slug and sample path (length check); an existing copied WAV that is byte-identical to the one that would be written is reused (nothing is written for it, the summary says "already up to date"); a differing one is refused unless `--force`.
6. `apply` and serialize the song in memory.
7. Only then (and only without `--dry-run`) create `--dest-dir`, write the WAV, write `--out`.
8. Return a `Report`; `main` prints its warnings to stderr and its summary (slice and chain counts, chain ids, song rows, shared phrase, sample path, written or dry run) to stdout.

## `inspect`

`ref_inserter inspect <project> [--empty-id] [--phrase-id]` is an optional subcommand. The bare insert form keeps working (`args_conflicts_with_subcommands` and `subcommand_negates_reqs`, with the insert arguments as an optional flattened group). It is read only.

## `--dry-run`

Runs every check and the full in-memory build, including all refusals (existing files, too many slices, foreign content) and all warnings, then writes nothing and prints `dry run: would write ...`.

## Tests

- Unit tests in each module cover the behavior above, one concept per test, including boundaries: 48 and 49 chains, 128 slices, an exact row fit, 127 and 128 character sample paths.
- E2E tests cover output files, input untouched, reruns with and without `--force`, packed layouts, custom phrase id, an unfinished manual attempt, foreign content refusals, existing file refusals.
- A mutation check (breaking the chain transpose) was caught by exactly the matching test.

## Decisions and history

- Instrument 7F is not split across a second instrument; more than 128 slices is an error.
- A used id aborts instead of shifting the block to B0, A0 and so on (earlier design).
- The sampler comes from a template file because the crate has no `Default` for `Sampler` or `SynthParams`.
- The tool writes the `cue ` marker layout that DigiChain uses; the M8 reads it as slices, confirmed on the user's M8.
- The commit history uses the GitHub noreply address; the work email never reached the remote.

## Open items

- Packed layouts need an M8 check (see Hardware status).
- Empty-song start row is 0 with no gap, an assumption nobody objected to.
- The CI workflow has not run yet: the Windows and Linux test runs and the Intel macOS cross-build are unverified until the first run (locally only the arm64 commands were run).
- The `Song::write` panic on files older than FW 6.5 is a crate bug and has not been reported upstream.
- The sampler template's instrument file version header reads 6.0.2, while projects read 6.6.2. It loads fine on the M8.
