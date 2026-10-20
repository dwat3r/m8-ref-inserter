# Wasm build and web UI

Status: tasks 1 to 5 done (wasm compile check, core split, inspect split, wasm exports, web page). Task 2 committed as `c651efe`, task 3 as `bb2dd48`, task 4 as `d293fb8`, task 5 in the commit that adds `web/index.html`.

Local run: `wasm-pack build --target web --release --out-dir web/pkg`, then serve `web/` with any static server (for example `python3 -m http.server` inside `web/`). `web/pkg/` is gitignored build output.

**Goal:** use the reference inserter from a phone next to the M8. The tool is built to wasm and wrapped in a minimal static web page hosted on the repo's GitHub Pages. The page takes an `.m8s` and a WAV, runs insert in the browser, and offers two downloads: the new project and the cued WAV. Nothing is uploaded anywhere.

**Architecture:** same crate, `crate-type = ["cdylib", "rlib"]`. `run()` is split into a pure core that takes bytes and returns bytes, plus a thin fs wrapper that keeps the CLI behavior. `wasm-bindgen` exports call the pure core. One static `web/index.html` with inline vanilla JS and CSS, no framework, no bundler, no npm.

**Tech stack:** existing crate, plus `wasm-bindgen` (wasm32 only), built with `wasm-pack build --target web`. GitHub Actions deploys to Pages.

## Decisions

- Pure core in three stages: `Project::parse(bytes, name)`, `plan_insertion(project, &Reference { bytes, name }, &Settings) -> Plan` (all validation, cued WAV and file name known), `Plan::into_insertion() -> Insertion { project, wav, file_name, slices, summary, warnings }`. `insert_reference(&Source, &Settings)` chains the three and is what the wasm export calls. The fs wrapper in `run()` reads the files, runs the stages in the original order of checks (parse project, read reference, plan, copied-WAV refusal, apply), applies the existing overwrite checks (`--out` exists, copied WAV exists or is byte-identical, same-file check) and writes the files. CLI behavior, error messages and its tests stay unchanged.
- `Source`, `Reference` and `Project::parse` take display names (the file path on the CLI, the file name in the browser) so errors name the file.
- Browser has no dest-dir and no existing-file checks, so `reuses_wav` and the `--out` checks live only in the fs wrapper. `force` in the browser only covers a previous insertion and a used instrument 7F.
- Inspect gets the same split: `inspect_project(&Project, &Options) -> Vec<String>`, and the path version `inspect` parses the file and calls it. The wasm export is `Project::parse` plus `inspect_project`.
- Wasm exports (`src/web.rs`, wasm32 only): `insertReference(project, projectName, reference, referenceName, settings) -> Insertion` and `inspectProject(project, projectName, emptyId, phraseId) -> string[]`. `Insertion` is a `#[wasm_bindgen]` struct with getters (project bytes, wav bytes, `file_name`, `slices`, summary lines, warning lines). No serde.
- `Settings` is also a `#[wasm_bindgen]` struct: `new Settings()` has the CLI defaults (`impl Default for Settings`), JS sets its fields (`m8_dir`, `empty_id`, `phrase_id`, `chain_phrases`, `force`, `bpm`) before calling `insertReference`. The wasm attributes sit on the core types through `cfg_attr`, with `getter_with_clone` on each non-Copy field.
- `Options::for_inspect(empty, phrase)` builds the inspect options with the default chain phrases; the CLI, the wasm export and the tests share it.
- Errors from the core (`Err(String)`) become thrown JS errors, shown in the page.
- Inputs in the UI: `.m8s`, `.wav`, `m8-dir` (required, saved in `localStorage`), `bpm` (optional), `chain-phrases` select (4, 8, 12, 16), `force` checkbox, `empty-id` and `phrase-id` (hex text, defaults `FE` and `F0`).
- Flow: picking the `.m8s` runs inspect and shows its lines (and again when the empty or phrase id changes). Run shows the summary and warnings. Two buttons, "Download project" and "Download WAV", save the two files as plain browser downloads. The WAV name is the slugged name from the core, no rename field. The project downloads as `<original name>-ref.m8s`. Changing any input clears the result.
- Layout: single column, large tap targets, works at phone width.
- No dry-run: the summary is shown before anything is saved, so the page already is one.
- No size cap on the WAV. A memory failure surfaces as an error.

## Not in v1

- PWA, service worker, manifest, offline use (decided to add later if needed).
- Zip download, Web Share API.
- Browser tests (`wasm-bindgen-test`). The page is checked by hand on a phone.

## Tasks

1. Check that `m8-file-parser` compiles for `wasm32-unknown-unknown`. This is the main risk and goes first. Installing the wasm target and `wasm-pack` needs the user's approval. If it does not compile, stop and decide before anything else.
2. Split `run()` into the pure core and the fs wrapper. Run the existing tests, they must pass unchanged. Add a core test that calls it with fixture bytes and compares to the wrapper's output.
3. Split `inspect` the same way.
4. Add the wasm exports behind `cfg(target_arch = "wasm32")` with `wasm-bindgen` as a target-specific dependency, so the native build and the CLI do not pull it in. Add `crate-type`.
5. Write `web/index.html`: form, inspect on file pick, run, summary and warnings, two download buttons, `localStorage` for `m8-dir`.
6. CI: add a job that runs `wasm-pack build --target web --release --out-dir web/pkg` as the compile check. Add a Pages deploy job on push to master that builds the same way and publishes `web/`.
7. README: add the web UI link and a short usage section. Update `docs/ref-song-inserter-plan.md` status.

## Acceptance

- `cargo test` passes natively with no test changed except additions.
- `wasm-pack build --target web` succeeds in CI.
- On a phone, the Pages site takes a project and a WAV and downloads a project and WAV whose bytes equal what the CLI writes for the same inputs and options.

## Setup the user does

- Repo settings, Pages, source set to "GitHub Actions".
