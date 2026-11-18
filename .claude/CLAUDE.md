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
| `src/texture.rs` | `.bntx` (BC1–7, ASTC, raw and float formats, Tegra block-linear); `info` reads the header only |
| `src/textures.rs` | the texture catalogue: pack names → paths (slash recovery), groups (UI, Pokédex, ...), folders, pack siblings |
| `src/wwise.rs` | Wwise banks (`.bnk`: DIDX/DATA/HIRC) and packages (`.pck` AKPK) in `audio/wwise/bin/Switch/`; sound catalogue by group |
| `src/wem.rs` | `.wem` header and decoders: PTADPCM, Wwise Vorbis (rebuilt for lewton, codebooks in `assets/`), Switch Opus (libopus), PCM |
| `src/sound_names.rs` | names for sounds: event / state names from the game's audio tables matched by FNV-1 hash, HIRC walk (events → actions → music switches → tracks), menu titles and species from `msg.rs` |
| `src/msg.rs` | the game's text: `message/dat/<language>/<name>.tbl` labels and `.dat` lines (decrypted) |
| `src/audio.rs` | Sound tab: worker (catalogue, decode, waveform), rodio player with loop points, WAV export, user names |
| `src/gallery.rs` | Textures tab: worker thread with its own `Archive`, thumbnail grid, single view (zoom, channels, layers) |
| `src/effects.rs` | effect catalogue: `.ptcl` (Nintendo VFXB particles) and `.traef` (animation effect lists) from pack names, by group |
| `src/effects_tab.rs` | Effects tab: sortable, filterable table of the effect catalogue; opens an effect in the 3D view with playback controls |
| `src/vfxb.rs` | `.ptcl` (Nintendo VFXB 0x16): section tree, emitter sets, emitters (0xAC0-byte block, offsets in the doc comment), texture table |
| `src/particles.rs` | effect playback: CPU particle simulation, per emitter quads or its primitive mesh, back to front |
| `src/particle_gpu.rs` | effect drawing: `ParticleMaterial` (the emitter's combiner, shader `src/shaders/particle.frag`) and a geometry with three UV sets and both colours (quads, or instanced primitive meshes) |
| `src/bfres.rs` | Switch BFRES 0.9, as far as the effects' primitive meshes (`G3PR`) need it |
| `src/skeleton.rs` | `.trskl`, rest pose vs. bind pose |
| `src/anim.rs` | `.tranm` tracks, `_base.tracr` animation lists |
| `src/demo.rs` | play mode (F7): locomotion blend space and jump states |
| `src/motion.rs` | trainer and NPC body clips: motion folders, own lists, shared body types (`cc_base`), `model_vr` base skeleton |
| `src/eye.rs` | model materials: three-d physical, or the eye material (Pokémon eye highlight, shader `src/shaders/eye.frag`) |
| `src/player.rs` | player character: part catalogue by slot, assembly of parts on the base skeleton |
| `src/terrain.rs` | overworld terrain tiles: `.trtrr` and the height file in its pack |
| `src/instances.rs` | `.trins` instance buffers (`SrtInstanceBuffer`: 4x4 matrices) |
| `src/plants.rs` | World tab: placed models (trees, buildings, props) as instanced groups, draw distance |
| `src/scene.rs` | `.trscn` scene trees: transforms, model and instance references; the overworld's streamed scenes |
| `src/maps.rs` | the World tab's places: overworld maps (tile grids, scene folders, resident scenes) and room scenes found from pack names |
| `src/water.rs` | World tab: the sea, rivers, lakes and waterfalls from each map's resident scenes; `WaterMaterial` (shader `src/shaders/water.frag`) |
| `src/world.rs` | World tab: terrain meshes per tile, splat `TerrainMaterial` (shader `src/shaders/terrain.frag`), minimap panel, right-drag pan |
| `src/config.rs` | remembers the RomFS path (`%APPDATA%\trinity-model-viewer\romfs_path.txt`) and the user's sound names (`sound_names.txt`) |
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
- Opus decoding links libopus (`opus` crate). Without a system libopus it is built from source with CMake;
  `.cargo/config.toml` sets `CMAKE_POLICY_VERSION_MINIMUM` so CMake 4 accepts it. CI installs `libopus-dev` and
  `libasound2-dev` (rodio's audio output).
- The user's dump is a RomFS **image** (`romfs.bin` + `index.json`), not a folder; `archive.rs` handles both.
  Its path is in the config file above, or set `TRINITY_ROMFS`.
- `data.trpfs` is about 8 GB; reading the pack table is quick, but whole-dump probe runs take minutes, so run
  them in the background.
