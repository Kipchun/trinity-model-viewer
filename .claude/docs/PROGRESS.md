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

### Characters, step 1 (2026-11-15, not committed yet)
- Paths confirmed for `model_tr`, `model_uq`, `model_vr`, `model_ob`, `model_pc`, `model_pc_base`, `model_cc_base`,
  `motion_cc_base` (ARCHIVE_PATHS.md).
- `Archive::character_models()`: 264 models (30 tr, 74 uq, 29 vr, 131 ob) from pack names.
- Viewer: Pokémon / Trainers / Props switch above the model list. `--check` covers all 1240 models, 0 failures.
- All 264 load with the existing loaders: meshes, skeletons, textures, no warnings. Units are metres (trainers
  ~1.7 tall).
- Probe: `--load <trmdl>...` (loads like the viewer, prints counts and warnings), `--characters`; the slot dump now
  prints string vectors.

### Player character viewer (2026-11-15, not committed yet)
- `src/player.rs`: 208 parts from `Archive::player_parts()`, by slot (outfit p1/p2 19 each; legwear, gloves,
  footwear, face, hair, headwear, eyewear, bag, accessory). `assemble()` puts them on `p0_base.trskl`.
- `.trskl` slot 4 (joint offset) and node slot 6 (base bone to hang from) decoded; `Skeleton::attach` merges a part's
  bones. `probe --player`: all 208 parts attach with every skinned vertex on a bone.
- Animations: `chara/motion_pc/base/p0_base.tracr`, 727 clips (`anim::list_at`); idle/walk/run drive 57 of 58 tracks.
  The list also has `p0_01_*` and `p0_02_*` copies of the locomotion clips: probably the two body types' gaits
  (outfit set 1 / 2); play mode currently uses the shared `p0_00` ones.
- Viewer: **Player** tab with a drop-down and arrows per slot, outfit set 1/2, Random, Default; `--player` flag.
  Outfit changes keep the playing clip. Play mode works with the player's walk/run/jump clips.
- Not looked at in the window yet by Claude; to check: textures on clothing (many come from `chara/share_cm`),
  hair/face bones in animation, which outfit set is which body.

### World tool, step 1: terrain heightmap (2026-11-15, not committed yet)
- `src/terrain.rs`: each `terrain_XX_YY` pack holds an unnamed FlatBuffer with the heights (513 x 513 f32, times
  1000 m), found by shape. Tiles are 500 m; origin from the file. The `_00` row lies at z -5500, past row 09.
  Height 0 = hole (sea, towns, Area Zero's crater). 100 tiles parse, 74 have ground; seams agree within 0.13 m.
- `src/world.rs`: **World** tab (`--world`): loads tiles one per frame, height-coloured lit meshes, minimap to
  show/hide tiles, detail Full/1/2/1/4/1/8 (1/4 = 1.85M triangles for the island), Frame button. Right drag pans
  now (the header always said it did; OrbitControl has no pan).
- Archive: `read_hash`, `pack_files` (list a pack's files by hash). Probe: `--pack`, `--save`, `#<hash>` dumps,
  `--terrain`. `--check` also loads every tile (100, 0 failures).
- Seen in the window: the whole island; snow peak at -z.
- Known: near-vertical "skirts" at coasts and hole edges (samples falling to near 0 next to holes). Not checked
  whether they are real data or should be cut.

### World tool, step 2: landscape material (2026-11-15, not committed yet)
- Tile material `ter_XX_YY.trmtr` ("Terrain" shader): `splatmap` control texture 512² RGBA8, R/G = two layer
  indices into `terrain_splat_tex/global_color_layer_array` (BC7 sRGB 256², 60 layers), B/A = weights. Layer
  tiling from `global_splatST_params` (RGBA32F, row 0: 5 m; layer 59: 6.25 m).
- Indices are global (probe `--splat`); control maps agree across tile edges 98.8% / 98.4% (`--splat-seams`).
- `TerrainMaterial` (custom three-d material, `src/shaders/terrain.frag`): per texel blends its two layers, then
  bilinear over four texels; layers tiled in world x/z; lit with three-d's lights. Ground: Textures / Height colours.
- Gotcha: three-d flips texture rows on upload (`flip_y`), so `texelFetch` row = height - 1 - row.
- `texture.rs`: array layers (`decode_bntx_layer`, `layer_count`), `decode_rgba32f`. `model::parse_materials`
  public. Probe: `--texture`, `--splat`, `--splat-seams`. Viewer: `--tiles`. `--check`: 100 tiles, 60 layers, 0 failures.
- Seen in the window: whole island and a 2 x 2 close-up; matches a top-down reference drawn from the control maps.
- Not done: layer normal maps (`SplatNormal`), roughness per layer, row 1 of the params (flags: x = 2 on layers
  16–18, 30–31: triplanar?), `SplatSTnFar`/thresholds, distance colour. Greens look more saturated than in game.

## In flight

- Commit the play-mode work (waiting on the user).

## Next

See [ROADMAP.md](ROADMAP.md). Immediate next steps:

1. Generalise discovery: an asset index built from all pack names (category, name, path, extension), replacing
   the Pokémon-only list. Left panel gets a category picker.
2. World tool, next steps: layer normal maps and the params' flags (triplanar cliffs?), the `.trtbf`
   detail maps, name the height file, coarse `clod_terrain_*` meshes, towns
   (`clod_town_*`) in the holes, `.trins` grass/tree instances, then the play-in-scenes ground query
   ([PLAY_IN_SCENES_PLAN.md](PLAY_IN_SCENES_PLAN.md)) from the heightfield.
3. Phase 1 of the roadmap, continued:
   - Look at a few trainers and props in the window; check materials that use `DetailMaskMap`, `NormalMap1/2`,
     `HairFlowMap`, `SSSMaskMap` (ignored by the bake for now).
   - Trainer animations: `.trmdt` names a `.trmdd` ("motion detector") file; study that next. Clips are probably
     `.tranm` per body type under `chara/motion_cc_base/`. The bone tracks are by name, so trainers whose bones
     match `cc_base` can share clips.
   - Player: clothing colour variants (`.trmmt`, 7 packs), face animation (`p0_face0000.tracn`).
   - `model_vr` body + head parts: assemble the same way.
4. glTF export (planned 2026-11-15, [EXPORT_PLAN.md](EXPORT_PLAN.md)): single model and bulk per category into an
   ignored `Exports/` folder. Steps E0–E6 work on Pokémon now; later categories follow the roadmap phases.
5. Play mode inside scenes (planned 2026-11-15, [PLAY_IN_SCENES_PLAN.md](PLAY_IN_SCENES_PLAN.md)): a `World` trait
   replacing the flat floor, collision from the place's triangles, a camera that follows height. S0–S4 work with a
   single interior/terrain model now; real scenes, humans and the overworld wait on roadmap Phases 3, 1 and 4.

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
- Pack files can be listed by hash (`probe --pack <path>`), and names tested against them. Terrain tile pack:
  `ter_XX_YY.trtrr`, `.trmtr`, `.trcol`, `_heightmap.bntx`, `_detailmap_N.trtbf`, the height file (unnamed), more
  `.bntx`. (2026-11-15)
- Confirmed real paths (2026-11-14): `field_graphic/common_model/a_t01_i01/a_t01_i01.trmdl` (interior),
  `field_graphic/terrain/terrain_00_00/ter_00_00.trtrr` (terrain tile), `chara/model_ob/ob0001_phone/ob0001_00.trmdl`
  (prop), `world/scene/parts/field/room/a_c01_hairsalon_/a_c01_hairsalon_0.trscn` (room scene). More in
  ARCHIVE_PATHS.md.
