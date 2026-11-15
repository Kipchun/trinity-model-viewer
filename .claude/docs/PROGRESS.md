# Progress

Update this file at the end of each work session: move items between sections, add the date, keep it short.

## Done

### Pokémon (commit 720bbe8)
- Archive: `data.trpfd` + `data.trpfs`, pack lookup by FNV-1a 64 hash, Oodle Kraken; dumped folder or
  `romfs.bin` + `index.json` image.
- Discovery: every Pokémon model, from pack names (`Archive::pokemon_models`).
- Models: all meshes, parts and LODs from `.trmdl` / `.trmsh` / `.trmbf`.
- Materials: `.trmtr` textures by role, layer-mask tinting (BaseColorLayer1..4), UV scale/offset, cut-outs.
- Textures: `.bntx` BC1–BC7 and ASTC, Tegra block-linear deswizzle.
- Skeletons: `.trskl`, rest pose vs. bind pose (18 of 976 Pokémon differ), bone tree with selection.
- Animations: `_base.tracr` list, `.tranm` tracks (fixed, dynamic, keyed u8/u16; packed quaternions),
  GPU-free CPU skinning, crossfade, two-clip blend with phase sync.
- Rendering: textured / per-material colour / plain; Unlit or Lit with shadows; floor; skeleton overlay.
- Probe tool: slot dumps and whole-dump checks (`--survey`, `--skeletons`, `--anims`, `--skin`, ...).

### Play mode (2026-11-14, not committed yet)
- `src/demo.rs`: F7 play mode. WASD/arrows, Shift run, Space jump. Idle/walk/run 1D blend space with speeds from
  root motion (`origin` bone stride), walk/run phase-locked, jump state machine (take-off, rise, fall start, fall,
  land) with crossfades.
- Changes in `main.rs`, `anim.rs`, `skeleton.rs`, README. Builds cleanly (`cargo check --release`).

## In flight

- Commit the play-mode work (waiting on the user).

## Next

See [ROADMAP.md](ROADMAP.md). Immediate next steps:

1. Generalise discovery: an asset index built from all pack names (category, name, path, extension), replacing
   the Pokémon-only list. Left panel gets a category picker.
2. Phase 1 of the roadmap: character models (`chara/model/...`), starting with `ob` props (no shared skeleton)
   then trainers.

## Known gaps (from README)

Emission, roughness, transparency effects; face/material animations (blinking, eye textures, `.tracn`?);
alternate colour sets (shiny); everything that is not a Pokémon.

## Facts learned (keep; they cost time to find)

- Pack names are full archive paths with the `/` removed plus `.trpak`, e.g.
  `arc/pokemondatapm0025pm0025_00_00pm0025_00_00.trmdl.trpak`. 23,586 packs in this dump. A pack holds many files
  but is named after one of them, so a pack name gives a directory and one file; siblings must be guessed and
  confirmed with `Archive::contains` (hash lookup).
- The `.trpfd` stores hashes only, no path strings. Every path the viewer opens must be derived or guessed.
- `.gitignore` lists `probe.rs`; `src/bin/probe.rs` is tracked anyway (force-added). A *new* file named
  `probe.rs` would be ignored.
- Confirmed real paths (2026-11-14): `field_graphic/common_model/a_t01_i01/a_t01_i01.trmdl` (interior),
  `field_graphic/terrain/terrain_00_00/ter_00_00.trtrr` (terrain tile), `chara/model_ob/ob0001_phone/ob0001_00.trmdl`
  (prop), `world/scene/parts/field/room/a_c01_hairsalon_/a_c01_hairsalon_0.trscn` (room scene). More in
  ARCHIVE_PATHS.md.
