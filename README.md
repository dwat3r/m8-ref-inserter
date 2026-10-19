# m8-ref-inserter

Insert a reference song into a Dirtywave M8 project, sliced into chains for A/B listening.

## What it does

- Adds the WAV as sampler instrument 7F (`REFERENCE`, FWD, slice 01), sliced every 16 beats at the project BPM.
- Writes one shared phrase (C-1 on instrument 7F, plays slice 1) and one chain per slice: that phrase transposed by the slice index, then three empty phrases. C-1 plays slice 1, C#-1 slice 2, and so on, so the chain transpose picks the slice.
- Chain ids start at C0 and stay below F0 (longer references start lower: B0 for 49 to 64 slices, and so on). The shared phrase is F0. If any chain id in the block or the phrase is used, it aborts.
- With `--chain-phrases 16`, four slices share a chain (8 gives two), so a 45-slice reference needs 12 chains and 12 song rows instead of 45. The cost is that playback can only start at chain boundaries.
- Chains go on track 1, 8 rows below the last used song row. Tracks 2 to 8 get the empty chain at the first row, so they stay silent.
- Never touches existing chains, phrases or song cells. The input project is never modified.

## Binaries

Prebuilt binaries for macOS (Apple silicon and Intel), Linux x86_64 and Windows x86_64 are attached to each [GitHub release](https://github.com/dwat3r/m8-ref-inserter/releases) as `ref_inserter-<target>.tar.gz`. CI also uploads them as workflow artifacts on every push.

## Usage

```
cargo run --release -- <project.m8s> <reference.wav> \
  --dest-dir <local dir> --m8-dir /Samples/Reference --out <new.m8s>
```

| Option | Meaning |
|---|---|
| `--dest-dir` | Local directory the sliced WAV is copied into |
| `--m8-dir` | Same directory as the M8 sees it, stored in the instrument |
| `--out` | New project file to write |
| `--empty-id` | Hex id of the empty chain and phrase (default `FE`) |
| `--phrase-id` | Hex id of the shared phrase (default `F0`) |
| `--chain-phrases` | Phrases per chain: 4, 8, 12 or 16 (default 4) |
| `--force` | Replace a previous insertion, an already used instrument 7F, and existing `--out` and copied WAV files |
| `--bpm` | Tempo of the reference, if it differs from the project tempo |
| `--dry-run` | Run all checks and print what would change, without writing any file |

The tool prints a summary (slices, chain ids, song rows, slice length in seconds at the tempo used, reference length, shared phrase, sample path). Check the slice length and tempo line: the tool trusts that the reference matches the tempo, and a wrong tempo shows up there as unexpected seconds per slice. Warnings are printed for anything it replaces. Use `--dry-run` with `--force` to preview a replacement first.

Copy the WAV and the new project to the M8's SD card.

## Limits

- FW 6.6 projects only.
- PCM WAV only, at the project BPM, starting on beat 1.
- Max 128 slices (17 min at 120 BPM). More is an error.
- Phrase `--empty-id` must be empty. Its chain must be empty or hold only that phrase.
- Running it on its own output aborts. `--force` removes the previous insertion (found by its pattern: a sampler at 7F, whatever its name, and chains made of `[phrase, FE, FE, FE]` groups, whatever `--chain-phrases` was; the older one-phrase-per-chain layout is recognized too) and inserts again, overwriting the old copied WAV.
- `--force` also overwrites the shared phrase (F0 by default) when it holds exactly one note on instrument 7F, and warns.
- Anything else using the C0 chain ids or the phrase id aborts, even with `--force`.
- Never overwrites an existing `--out` file, or a copied WAV that differs, without `--force`. An existing copied WAV that is byte-identical is reused, so several projects can share one reference WAV. The input project is never overwritten, even with `--force`.
- The sample name is slugged to ASCII (`[A-Za-z0-9_-]`).

## Credits

- [m8-file-parser](https://github.com/Twinside/m8-file-parser) (MIT) reads and writes the project. Test fixtures in `tests/fixtures` come from it.
- Slice markers follow the cue chunk layout written by [DigiChain](https://github.com/brian3kb/digichain).

## License

MIT
