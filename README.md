# m8-ref-inserter

Insert a reference song into a Dirtywave M8 project, sliced into chains for A/B listening.

## What it does

- Adds the WAV as sampler instrument 7F (`REFERENCE`, FWD, slice 01), sliced every 16 beats at the project BPM.
- Writes one chain per slice: a phrase that plays the slice, then three empty phrases. C-1 plays slice 1, C#-1 slice 2, and so on.
- Chain and phrase ids start at C0 and stay below F0. If the block is taken, it moves down in steps of 0x10 (B0, A0, ...).
- Chains go on track 1, 8 rows below the last used song row. Tracks 2 to 8 get the empty chain at the first row, so they stay silent.
- Never touches existing chains, phrases or song cells. The input project is never modified.

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
| `--force` | Replace instrument 7F if it is already used |

Copy the WAV and the new project to the M8's SD card.

## Limits

- FW 6.6 projects only.
- PCM WAV only, at the project BPM, starting on beat 1.
- Max 128 slices (17 min at 120 BPM). More is an error.
- Phrase `--empty-id` must be empty. Its chain must be empty or hold only that phrase.
- Never overwrites an existing `--out` file or copied WAV.
- The sample name is slugged to ASCII (`[A-Za-z0-9_-]`).

## Credits

- [m8-file-parser](https://github.com/Twinside/m8-file-parser) (MIT) reads and writes the project. Test fixtures in `tests/fixtures` come from it.
- Slice markers follow the cue chunk layout written by [DigiChain](https://github.com/brian3kb/digichain).

## License

MIT
