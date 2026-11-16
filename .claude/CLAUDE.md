# Trinity Model Viewer: project guide for Claude

A Rust desktop viewer (three-d + egui) for assets of Game Freak's Trinity engine, developed against
*Pokémon Scarlet & Violet*. It reads the game's packed archive straight from the user's own RomFS dump.

**Goal:** view every model, animation and scene in the game: Pokémon (done), characters (player, trainers,
NPCs, props), and maps (the Paldea overworld, Area Zero, towns, interiors such as the academy and Pokémon Centers).

Read these before starting work:

- [docs/PROGRESS.md](docs/PROGRESS.md): what works, what is in flight, what is next. **Update it at the end of
  every work session.**
- [docs/ROADMAP.md](docs/ROADMAP.md): the plan for characters, maps and scenes, in phases.
- [docs/FORMATS.md](docs/FORMATS.md): every Trinity file type, how well it is understood, and where it lives.
- [docs/ARCHIVE_PATHS.md](docs/ARCHIVE_PATHS.md): how archive paths are recovered and the naming conventions seen.

Skills in [skills/](skills/): `run-viewer` (build, headless check, launch), `study-format` (reverse a new file
type with the probe), `find-assets` (list and search the archive's pack names).

## Hard rules

- **Never commit or publish game data**: no files from the dump, no decoded textures or meshes, no screenshots
  of game content in the repo. Lists of archive names go in `target/` (ignored), not in tracked files.
  `.gitignore` already blocks the common dump folders and Trinity extensions; keep it that way when adding
  new formats (add their extensions there).
- Never write to the dump. The archive is opened read-only.
- Don't send paths, names or file content from the dump to outside services.
- Commit only when the user asks.

## Layout

| File | Role |
| --- | --- |
| `src/main.rs` | app, UI (egui panels), scene building, skinning, clips and crossfades |
| `src/archive.rs` | `data.trpfd`/`data.trpfs` reader, FNV-1a 64 path hashes, Oodle Kraken; `pokemon_models()` discovery |
| `src/fb.rs` | schemaless, bounds-checked FlatBuffers reader (fields by slot index) |
| `src/model.rs` | `.trmdl` / `.trmsh` / `.trmbf` (+ `.trmtr` slots) |
| `src/materials.rs` | bakes `.trmtr` layers into one albedo + one normal map |
| `src/texture.rs` | `.bntx` (BC1–7, ASTC, Tegra block-linear) |
| `src/skeleton.rs` | `.trskl`, rest pose vs. bind pose |
| `src/anim.rs` | `.tranm` tracks, `_base.tracr` animation lists |
| `src/demo.rs` | play mode (F7): locomotion blend space and jump states |
| `src/player.rs` | player character: part catalogue by slot, assembly of parts on the base skeleton |
| `src/terrain.rs` | overworld terrain tiles: `.trtrr` and the height file in its pack |
| `src/world.rs` | World tab: terrain meshes per tile, splat `TerrainMaterial` (shader `src/shaders/terrain.frag`), minimap panel, right-drag pan |
| `src/config.rs` | remembers the RomFS path (`%APPDATA%\trinity-model-viewer\romfs_path.txt`) |
| `src/bin/probe.rs` | dev tool: slot dumps of any archive file and whole-dump checks |

## Conventions

- Each format module starts with a doc comment giving the **field slots as checked against the game's files**.
  Keep that up to date whenever a slot's meaning is learned; it is the project's format documentation.
- Prove format assumptions over the whole dump (a probe mode that runs over every file of the type and counts
  failures) before relying on them, and write the result into the doc comment (e.g. "958 of the 976 Pokémon").
- Errors use `anyhow`; parsing is bounds-checked and never panics on bad data. A file that fails to load becomes
  a warning in the UI, not a crash.
- Rust edition 2024. Before calling work done, pass what CI (`.github/workflows/ci.yml`) runs: `cargo fmt --all -- --check` and `cargo clippy --locked --all-targets --all-features -- -D warnings`; run
  `--check` (headless load of every model) after touching any loader.
- Plain, direct wording in docs and UI text; the README is written for users who own the game.

## Environment notes

- Windows; Git Bash and PowerShell both available.
- The user's dump is a RomFS **image** (`romfs.bin` + `index.json`), not a folder; `archive.rs` handles both.
  Its path is in the config file above, or set `TRINITY_ROMFS`.
- `data.trpfs` is about 8 GB; reading the pack table is quick, but whole-dump probe runs take minutes, so run
  them in the background.
