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
- Known: near-vertical "skirts" at coasts and hole edges. **Settled 2026-11-16 (step 3 below): real data, kept.**

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
- Not done then: layer normal maps, row 1 of the params, saturation. **Done or explained 2026-11-16 (step 3 below).**

### World tool, step 3: skirts, normals, colours, trees, ground query (2026-11-16, not committed yet)
- **Skirts are real.** `probe --heightmap`: the R16 `_heightmap.bntx` the game renders equals the height file
  (x1000, within 0.011 m), holes 0 in both. `probe --hole-regions` (all tiles stitched, flood fill from the border):
  holes are only the sea (edges run down to ~0 m: beaches and sea cliffs), **Area Zero's crater** (one 1 km² region
  around 04_04; rim cut at ~200 m in places, walls falling to ~0 elsewhere) and 26 small pits. **Towns are not
  holes.** Viewer keeps the strips and adds a sea surface at 0 m (toggle).
- **Layer normal maps**: `SplatNormal` (RGBA8 256² x 60, RGB = tangent normal) blended per texel like the colours,
  OpenGL convention assumed (as for models; not checked against the game). Toggle "Layer normal maps".
- **Row 1 of the splat params is not triplanar**: triplanar is off in all 100 tile materials
  (`probe --terrain-materials`). x = 2 marks the sand and snow layers (desert 1-3, snow02, beach), y = 2/1 the grass
  layers, w = 0.7 the cliff layers. Meaning unknown; not needed for colour.
- **Colours are ours to keep**: the layers are saturated as stored (grass01 mean sRGB 45,123,59) and our path does
  one sRGB decode. Layers 2 and 4 (`grass_deep01/03`) are orange gravel despite their names: the orange east
  (05_03, 06_03) is right. The game's softer look comes from grass models, fog and grading we don't draw.
- **Trees**: `.trtrr` slot 4 lists `{model, [.trins]}`; `.trins` = `SrtInstanceBuffer` (`src/instances.rs`, 4x4
  column-major matrices). `probe --tile-instances`: 311 files, 3509 trees, 49 models, 0 failures, 3478 within
  0.5 m of the ground. `src/plants.rs` draws them instanced per tile (cut-out leaves two-sided), "Trees" toggle and
  draw distance (default 1500 m). Seen in the window: olives, palms, dry bushes, orchard rows on 02_03, grounded.
- **Grass**: height file slot 13 is a per-cell bit mask over slot 12's grass models; `.trtrr` slot 6 has the
  grass parameters. Not drawn yet.
- **Towns / streaming objects** (step 3 of the plan, partly): `clod_town_*` / `clod_terrain_*` meshes are centred on
  their origin, so they need placements, not found yet. The pack of
  `world/scene/parts/field/unity_output/town_t07_field_streaming_s_/...` holds 2405 `SrtInstanceBuffer`s (25,000
  instances over the whole island). `probe --guess-ins` names 396 in 12 town folders (decor: pots, lamps, cars,
  garden plants; no buildings). Not drawn: the other 2009 names and the buildings' placement file are unknown.
- **Ground query** (play plan S0 + terrain part of S1): `demo::Ground` trait, `FlatGround` for the checker floor,
  `world::TerrainGround` (bilinear `HeightTile::height_at`, `None` over holes). `Demo::update` follows slopes and
  steps (step = 0.35 x radius, min 0.3 m), stops at higher rises, falls off ledges and lands on the ground height.
  World panel shows the ground under the orbit target. Play mode is not yet available in the World tab.
- Viewer: `--camera ex,ey,ez,tx,ty,tz` (World view). Probe: `--heightmap`, `--holes`, `--hole-regions`,
  `--terrain-materials`, `--grass-cells`, `--tile-instances`, `--instances`, `--pack-survey`, `--split`,
  `--guess-ins`; `--material` takes a `.trmtr`; `--splat` prints the layer → name table.

### World tool, step 4: towns and buildings (2026-11-16, not committed yet)
- **Placement found**: the overworld's streamed scenes, `.trscn` under
  `world/scene/parts/field/unity_output/<name>_/<name>_0.trscn` (`src/scene.rs`). A scene is a tree of typed
  nodes; `trinity_SceneObject` / `trinity_ObjectTemplate` carry a transform (scale, Euler radians, translation;
  top-level ones in world metres), `trinity_ModelComponent` names a `.trmdl`, a `trinity_ModelInstancerComponent`
  next to it names the `.trins` that model is drawn from (instance matrices already in world space).
- **Families** (names from `probe --scene-packs`, which lists the ~3000 scenes in the two scene packs): each in
  size classes `_streaming_s` (props, plants), `_m` (buildings), `_l` (rare). Areas: `town_<t01..t10, c01_01,
  c01_02, c02, c03_01..03, city, sl>_field`, `c01_both_01_field` (the academy and the central city's core),
  `battle_c01[_01]_field`, `cave_0N_field`, `dan_<type>` (Team Star bases). Tiles: `ter_XX_YY` (objects outside
  the towns) and `plan_ter_XX_YY` (ruins). `ter_10..13_*`, `ter_20..23_*` are the DLC maps (not drawn).
  `*_clod` / `*_clod_far` scenes place nothing through these components (distant LODs; not read).
- `probe --town-scenes`: 368 scenes, 2044 placed objects, 1560 instance files (17,630 instances), 1497 models,
  0 failures. 1094 of the models carry a "Shadow" part (a shadow-casting hull): left out, it showed as grey facets.
- Viewer: "Towns and objects" toggle; areas load when the tile under them is shown, tile objects with the tile;
  one draw distance for trees and objects. `--check` covers every scene, instance file and model.
- Seen in the window: town t07 (Pokémon Center, shops, battle court, props), the central city c01 with the academy
  on its hill, the plaza, towers and bridge gates, old-town houses; rock and roof textures clean after the Shadow fix.
- Not done: "BGLayer"/"BGLayerDepthFade" (623 models) mix two colour layers by vertex colour; only the first is
  drawn (grass tops on cliffs show rock). Decals z-fight in places. `clod` LOD scenes; the 845 instance files of
  the DLC tiles; NPC/trainer/item scenes (`world_npc`, `world_trainer`, `c01_npc`, ...).

### World tool, step 5: LODs (2026-11-16, not committed yet)
- **Game data**: a placed model's mesh files are its LODs (1393 of 1497 have three). Each `ModelComponent`
  stores switch distances in slots 5/6/7 (`probe --lod-survey`): 3401 of 3604 use 20 / 40 / 60 m, buildings and
  landmarks 220-500 / 300-650 / 400-1000. Slot 7 looks like the game's cull distance (not used: the viewer keeps
  the last LOD to its own draw distance). Slots 9-15 are the same everywhere.
- **Placed models** (`plants.rs`): every LOD loaded; "Model LOD" Auto (per instance, by distance from the camera,
  the placement's own distances times "LOD distance ×") or fixed 0 / 1 / 2. Instances re-sort when the camera
  moves a metre; only changed LOD buffers re-upload. Panel shows drawn triangles and instances per LOD.
- **Trees' last LOD is a billboard**: 2 triangles, shader "Grass", `BillboardType AxisY`. Turned about y towards
  the camera per instance; cut out by the texture's alpha. A lower LOD can use a material LOD 0 does not; those
  are baked too (they were the flat green cards).
- **Cut-outs never worked before**: three-d's forward `PhysicalMaterial` ignores `alpha_cutout`.
  `eye::ModelMaterial::Cutout` defines the shader's `ALPHACUT` with a uniform threshold; used by the model view
  too (hair, lashes, beards and leaves are cut now) and by the World's models (leaves, window grilles, cards).
- **Terrain**: "Terrain" detail gains Auto (default): per tile by distance (full within 400 m, 1/2 to 1 km, 1/4 to
  2.5 km, 1/8 beyond), rebuilding one tile per frame, nearest first. Tiles of different steps meet with small
  cracks (T-junctions).
- **Model view**: "Auto" next to the LOD slider: LOD 0 within 6 model radii of the camera, LOD 1 within 14, then
  LOD 2; 15% hysteresis (each switch reloads the model). Not yet watched switching in the window.
- Seen in the window: the central city with Auto LOD: near buildings at LOD 0, far trees as camera-facing cards;
  tr0080's beard cut into strands.

### World tool, step 7: maps and rooms (2026-11-16, not committed yet)
- **Maps** (`src/maps.rs`): Paldea (00-09, odd 00 row), **Kitakami** (tiles 10-13 x 00-03, scenes in
  `unity_output_1/`), **Terarium** (20-23 x 00-03, `unity_output_2/`); origins ((XX-first)*500, -(YY+1)*500).
  `probe --map-tiles`: 100 + 16 + 16 tiles fit their grid, 0 do not. `99_9x` are test tiles, not offered.
- **SubScene** nodes (`{0: relative path}`, stored as `<name>_0.trscn`) are followed (`scene::load_content`).
  The world composes this way: `world_resident_0` -> `outdoor_resident`; `resident_field` scenes ->
  `dlc1_resident`, `dlc2_resident`, `town_c01_field_resident`, `c01_both_01_field_resident`. All paths now real.
- **`xl` size class** added: 28 scenes the viewer never loaded (landmarks; `town_city_field_streaming_xl`, ...).
- **Resident scenes** per map: water to `water.rs`, the rest as one object group (Kitakami's outer ground and
  forest, the Terarium's outer walls and dome). `plants.rs` skips "Water" and "BGDistant" (the academy's 4 km sky
  dome) parts.
- **Rooms**: 151 from pack names (`maps::rooms`, `probe --rooms`: all place models, 0 failures). A room is only
  sub-scenes; `<code>_obj` places the room model. 1638 sub-scene references to `.trsog` (event objects) skipped.
- Viewer: World panel **Map** buttons and a searchable **Rooms and interiors** list; picking one builds a new
  World. `--map <name|room path>`. Minimap from the map's grid. `--check`: 132 tiles, 579 streamed scenes
  (2803 objects, 22,587 instances), 156 resident/room scenes, 180 water parts, 0 failures.
- Seen in the window: Kitakami (Mt. Kitakami, rivers, outer forest ring), the Terarium (four biomes, dome frame,
  sea), the `a_c01_hairsalon` room (framed small inside a dark shell; camera framing for rooms could be better).
- Not done: rooms' lighting (`.trlgt`), NPC/event content, Area Zero, the DLC `a_w23_*` field
  (`room/a_w23_field...` instance files), `clod` scenes on all maps.

### World tool, step 6: water (2026-11-16, not committed yet)
- **Source**: scene `world_resident/outdoor_resident_/outdoor_resident_0.trscn` (path found in step 7) places
  `objects_un_sea01` (sea) and `objects_un_water01` (all of Paldea's rivers, lakes, ponds, waterfalls: 21 parts,
  6822 triangles) at identity; `town_c01_field_resident` places `objects_un_c01water01` (academy pools/canals + a
  duplicate sea sheet, skipped). `src/water.rs`, `scene::water_scenes`.
- **Sea level is 27 m, not 0**: `probe --water` (each water part vs. the ground under it): the sea's vertices over
  ground sit at the shoreline (median +0.3 m); the ground beside sea holes is sea bed. The old flat 0 m quad is gone.
  Rivers/lakes meet banks ~1 m under the ground (edges tuck under), waterfalls stand above it.
- **Vertex colours** now read (`SubMesh::colors`, semantic 5, RGBA8): water a = opacity, r = foam.
- `WaterMaterial` + `shaders/water.frag` (**approximation**): two scrolling BC5 normal maps, soft diffuse + glint,
  Fresnel to a fixed sky colour, foam = (mask x vertex r x intensity)^sharpness, blended, no depth write. Not used:
  flow map (1x1 on rivers), wave animation, depth fade, refraction, water fog.
- Viewer: "Sea" and "Rivers and lakes" toggles. `--check`: water 33 parts, 0 failures (and 1240 models, 100 tiles,
  368 scenes, 0 failures). Seen in the window: the central city's river, lake, falls and outer ponds, the sea.
- Not done: `town_c01_field_resident`'s cliffs (BGLayer) and skybox; DLC water (`dlc1/dlc2_resident`,
  `objects_un_dlc*_water*`); cave water (`ground_un_*_cave01_water01`); interior water (`a_sch_2_entrance01_sea`).

### Trainer and NPC animations (2026-11-16, not committed yet)
- `src/motion.rs`: clips of `model_tr`, `model_uq`, `model_vr` from `chara/motion_cc_base/` and `motion_uq/`.
  80 models have their own full list (`<code>_00.tracr`), 52 have per-clip special actions (karate02_loop, ...),
  the rest use one of 25 shared body types (`cc0001_kid` .. `cc2011_cla_f`). `motion_tr`/`motion_vr` hold only face
  clips and spring bones.
- Body types are motion styles, not proportions: each type's idle drives 57 of 58 tracks of every `model_tr`
  skeleton and matches its rest offsets within 3 mm. Default without own list: the type the `.trmdt` names, else a
  guess from the folder name (child/infant: kid, ends in f: adl_f, else adl_m). **Not known yet how the game picks
  it**; probably a trainer/NPC data table.
- `model_vr` parts name no skeleton: they go on `chara/model_cc_base/model/cc_base.trskl` (74 bones) via
  `player::assemble_on`; all 28 parts, 0 vertices off a bone. `tr0601`/`tr0611` (`vat`) have no skeleton at all
  (vertex animation textures?).
- Viewer: **Body motions** drop-down in the Animation section (own clips / any body type), keeps the playing clip by
  label; play mode uses the list. `--check`: 133 trainers and NPCs, 2 without a skeleton (the vat ones), 0 without
  clips. Probe: `--trainer-anims` (every list, clip load and body-type fit): 0 clips fail.
- Launched on tr0330 (walk) and a vr part without errors; not yet looked at in the window.

### Eyes and transparency (2026-11-16, not committed yet)
- `.trmtr` shader name and switches now parsed (slot 1: `{0: name, 1: [{0: switch, 1: value}]}`).
- Alpha from the render type and `EnableAlphaTest`/`DiscardValue` (`materials::AlphaMode`): eye lenses, glasses,
  visors blend (three-d's transparent pass, back to front); hair/lashes/brows cut out at their own threshold; `Add`
  and `Mul` effects. Blend only when the colour has alpha < 1 (Pokémon `BlendPreMultiAlpha` eyes are opaque).
- `BaseColor` without a layer mask: flat colour when there is no colour map (eyelashes were grey, now black);
  hair colour for the `Hair` shader (208 materials).
- Seen in the window (`--look head`, new option): tr0040 eyes now show iris and highlight (the opaque lens hid
  them), tr0080's glasses are see-through, Pikachu unchanged.
- Pokémon eye highlights (same day): `src/eye.rs` + `shaders/eye.frag`, used for "Eye"/"EyeClearCoat" materials
  (`probe --eyes`: 1981, 1460 with `EnableHighlight`). The textures hold no highlight: NormalMap is the iris dish,
  NormalMap1 the cornea bulge. The shader draws the old lit colour plus a white spot reflected off NormalMap1 from
  a light fixed to the camera (up-left, front); size from RoughnessHighlight, strength MetallicHighlight.
  **Approximation**: the game's formula is unknown. Seen: Pikachu now has its highlight; Charizard's eyes unchanged.
  Probe: `--material <trmdl> <name>` prints a material's every field.

### Textures tab: texture and sprite viewer (2026-11-16, not committed yet)
- `src/textures.rs`: catalogue of every `.bntx`. The 4984 pack names ending in `.bntx` get their slashes back by
  brute force from learned folders (all 4984, 0.4 s); e.g. `appli/tex/icon_item/item_0183/item_0183.bntx`,
  `demo/appli/pokedex/zk0000_00/pokedex_capture_thum_001.bntx`, `pokemon/data/pm0001/pm0001_00_00/icon/...`.
  Model folders (Pokémon, trainers/props, player parts) are added too: their `.trmdl` pack holds their textures.
- A pack is named after one file but can hold many (item icons: 562 per pack, 2558 in all). Opening a folder reads
  its packs and lists the other BNTX by the name inside (BRTI 0x60); `<dir>/<name>.bntx` or
  `<dir>/<name>/<name>.bntx` confirms a path. Over the dump (`probe --textures expand`, 47 s): UI & icons 5008,
  Pokédex 1695, Pokémon 17,390, characters 6295, field 5715, environment 332, effects 38, cutscenes 5.
- Decoder: RGBA16F (0x15, light probes `_prb`) and RGBA32F (0x19) now decode (clamped 0..1). `probe --textures decode`: all 36,478 decode, 0 failures (313 s).
- `src/gallery.rs`: **Textures** tab (`--textures [folder]`). Worker thread with its own `Archive` (catalogue,
  expansion, decoding; thumbnails only for visible cells, newest first). Groups and folder search on the left,
  thumbnail grid, single view (Fit, 1:1, zoom, Ctrl+wheel), channels RGBA/RGB/R/G/B/A, checker/dark/light,
  nearest/linear, array layers, arrow keys. Seen in the window: item icons grid with alpha.
- Not done: PNG export (would go in an ignored `Exports/` folder, see EXPORT_PLAN.md); mip levels; BNTX files with
  more than one texture show the first; UI layouts (`.truis`/`.truiv`, "UikitBody" FlatBuffers, no textures inside)
  are not read, so sprites are shown as whole images, not cut from atlases.

### Sound tab: music and sound effects (2026-11-16, not committed yet)
- The sound is Wwise (bank version 140) in the RomFS at `audio/wwise/bin/Switch/`, outside `data.trpfs`: 16 `.bnk`
  and 5 `.pck`. `archive::RomFile` opens any RomFS file (folder or image). No names: sounds go by 32-bit id.
- `src/wwise.rs`: banks (DIDX/DATA media, HIRC Sound/Music track source ids) and AKPK packages (stream table).
  Every streamed file is named by exactly one bank, which gives its group: Music 439 (336 min), Jingles 24,
  Pokémon cries 10,107, Pokémon sounds 2032, Moves 1882, Battle 105, Menus 277, Player 632, People 861,
  Objects 341, Ambience 279, Cutscenes 476, Common 57. VIB.bnk (rumble) left out. Catalogue 2.4 s.
- `src/wem.rs`: PTADPCM (vgmstream's table), Wwise Vorbis (setup and packets rebuilt as ww2ogg does, aoTuV 6.03
  codebooks in `assets/` with their BSD licence, decoded by lewton), Switch Opus (libopus via `opus`; the pure-Rust
  `opus-decoder` ran at 3x real time, libopus 177x), PCM. `probe --audio decode`: all 17,512 decode, 0 failures
  (87 s for 12.5 h of sound).
- `src/audio.rs`: **Sound** tab (`--sound [group]`): groups and files on the left, list (id, length, ch, Hz, codec,
  loop), waveform with play position and click-to-jump, play/pause/stop, volume, loop (smpl loop points where
  present: only 2 music tracks have them), Up/Down/Space, Save as WAV. Seen in the window: list and groups. Playback
  not heard by Claude (the user should check).
- Build: `opus`/audiopus_sys builds libopus with CMake; `.cargo/config.toml` sets `CMAKE_POLICY_VERSION_MINIMUM=3.5`.
  CI apt installs `libasound2-dev libopus-dev`. `.gitignore`: `*.bnk *.pck *.wem *.wav`.
- Next: names (HIRC events → the game's event names, if a table of them turns up), music playlists/segments
  (HIRC 10/12/13) to loop music as the game does, which sounds belong to which Pokémon.

### Sound names (2026-11-16, not committed yet)
- Wwise stores FNV-1 32 hashes of lower-case names only. Names found and confirmed by hash (`src/sound_names.rs`):
  - **Event names** are strings in the game's audio tables (`audio/fb/env_poke_voice/.../..._data.bin` pack,
    `audio/exl/fb/EnvPokeVoiceChainData.bin`): 1506 events (723 BGM, 669 ENV, 59 ME, ...). The executable alone
    names only ~40 (UI). Cries: `PLAY_PV_%04hu` (format string in the executable) gives 733 species.
  - **State values**: set-state event names without `SET_STATE_` (`CLUBROOM_12`), checked against music switch
    tree keys.
  - **Titles** from the game's text (`src/msg.rs`, message .tbl/.dat): club room speaker (Indigo Disk academy)
    `common/clubroom_bgm`, 28 songs = state `CLUBROOM_NN`; battle music selector `common/btl_bgm_select`, ~55
    themes = state `VS_SELECT_TYPE_NN`; species `common/monsname` (`MONSNAME_025` Pikachu).
- HIRC walk: event, actions (Play 0x04xx target at +6; Set State 0x1204 ends with group, value), music switch
  decision trees (up to 11 groups deep; found by validation, not position), playlists / segments / containers
  (children found as object ids in their bytes), track sources. Paths into nested switches carry the parent
  leaf's values, skipping combinations that set a group twice.
- Coverage (`probe --audio`): music 160 titles + 326 developer names of 439; jingles 20 of 24; cries 10,106 of
  10,107 by species; ambience 64; objects 19. Moves, Pokémon sounds, NPC, player, battle effects: none yet (their
  event names are not in the tables found).
- Sound tab: name column, titles / developer names in the details, **Your name** field (saved to
  `sound_names.txt` in the config folder, wins over found names), find by name, **Named only**.
- Probe: `--grep <text> [pack filter]` (search unpacked file contents), `--audio names`.
- Next for names: moves (`PM_SKILLS`), Pokémon sounds, the remaining music.

### Effects tab: list of particle effects (2026-11-16, not committed yet)
- **Effects** tab (`--effects`): every `.ptcl` (428) and `.traef` (518) pack as a table (name, kind, group,
  folder, pack size). Sort by clicking a column title (again to reverse), filter by kind and group (Battle 292,
  Field 33, Menus 63, Cutscenes 40, Pokémon 485, Characters 33), find by name or folder. The right panel shows the
  path (with **Copy path**) and what the file is. Nothing is decoded or played yet.
- `src/effects.rs` (catalogue, no UI; shares `textures::recover` for the slashes), `src/effects_tab.rs` (UI).
  All 946 paths rebuilt from the pack names in 0.02 s (`probe --effects`).
- `.ptcl` is Nintendo's VFXB (version 0x16), not a Game Freak format: emitter sets, emitters, embedded BNTX
  textures, primitives, compiled shaders. See FORMATS.md.

### Effects: particle playback, step 1 (2026-11-16, not committed yet)
- Double-click a particle effect (or **View**) in the Effects tab: it plays in the 3D view. Left panel: **Back to
  list**, Play/Pause, Restart, Loop, speed, effect picker (files with several sets), one checkbox per emitter
  (child emitters indented, blend shown). `--effects <name>` opens the first match.
- `src/vfxb.rs`: section tree (ESTA > ESET > EMTR with child emitters, GRTF + GTNT, ...) and the 0xAC0-byte
  emitter block; offsets in its doc comment. Field order from KillzXGaming's EffectLibrary (MIT), offsets checked
  on the dump. `probe --vfxb`: **428 files, 0 failures, 2594 emitters (436 children), all 4957 sampler textures
  found in the file's BNTX** (GTNT name = BNTX texture name); all key tables valid (counts <= 8, times sorted
  in 0..1).
- `src/particles.rs` + `shaders/particle.frag`: CPU simulation at 60 frames/s (start, duration, rate, interval,
  16 volume shapes, outward + directional speed with spread, gravity, air resistance, life, scale + random +
  scale keys, colour 0 / alpha 0 fixed or keyed, emitter colour, colour scale, Z rotation, child emitters on each
  particle), quads rebuilt each frame per emitter, blend alpha / add / subtract / screen / multiply, particle
  types billboard / Y billboard / XY / XZ polygon / along velocity. Texture 0 only, with sprite sheets
  (columns x rows), pattern animation (fit to life, clamp, loop, random), UV scale and scroll. Camera framed on
  where the particles go in the first 2 s.
- Learned while looking: sampler wrap 0 = **mirror**, and those textures are a quarter of the image shown over
  UV 0..2 (glows, rings, shockwaves); 1 repeat, 2 clamp. Textures with no alpha on alpha-blended emitters are
  masks (shape in red, other shapes in green): drawn as red = alpha, white colour. BC4 / R8 textures likewise.
- `texture.rs`: `texture_names` and `decode_bntx_texture(b, index, layer)` (a BNTX can hold many textures).
- Looks right in shape and timing (checked: `cmn_land_smoke_m` landing puff, `ee003_star` glow + shards +
  sparkles), not exact: the game's colour combiners (how colour 1, alpha 1 and textures 1-2 mix) are not read.
- Next for effects: (1) **mesh particles** (particle type 6/7 and volume 15 use `G3PR` primitives, a BFRES;
  `star_A_01` shows as squares now); (2) the combiner: colour 1 / alpha 1 and textures 1-2 (`CUDP`/`CSDP`
  attribute blocks and the shader index may say how); (3) the remaining fields: rotation on X/Y, emitter
  follow types, emission over distance, `FCOL` / `FRND` / `CADP` blocks (fields: colour, random, ...);
  (4) `.traef` events to play effects on Pokémon / character animations; (5) a floor or a model to show scale.

### Effects: mesh particles (2026-11-18, not committed yet)
- **Primitives found**: emitter +0x8C8 is a u64 primitive id. `G3PR`'s data is a Switch **BFRES 0.9** (its size from
  the BFRES header; the section size is smaller); child `G3NT` lists the primitives in the BFRES's model order
  (same entry layout as `GTNT`; 8 bytes = attribute index for position, normal, tangent, colour, UV 0, UV 1).
  `PRMA` is empty in all but 14 files.
- `src/bfres.rs`: minimal reader (models, shapes' LOD 0, vertex attributes `_p0 _n0 _c0 _u0`, u8/u16/u32 indices);
  formats seen: 0x1805 / 0x1505 positions, 0x0E02 normals, 0x0B01 / 0x1505 colours, 0x0901 / 0x0902 / 0x1201 /
  0x1202 / 0x1205 UVs. `probe --vfxb-prims`: **428 files, 622 meshes, 0 failures; 1034 of 2594 emitters use one,
  all ids found; every G3NT map matches the attribute names.** Only 149 meshes are flat: rings, cylinders, beams.
  `probe --vfxb-tree [path]` dumps the section tree.
- `particles.rs`: an emitter with a primitive draws it once per particle (three-d `InstancedMesh`: matrix, colour,
  UV transform per particle; colours pre-encoded for three-d's sRGB-to-linear step; glows past 1 go through the
  material's colour scale). Mesh vertex colours used.
- **Rotation on three axes** (0x700 / 0x710 / 0x720 / 0x730 are x, y, z) and **spin decay** at 0x72C (per frame:
  903 at 1.0, 0.8-0.99 elsewhere, 0 taken as none). Billboard quads still spin on the screen only.
- Particle type for meshes (from which flat axis goes with which type): billboards map the mesh's XY to the screen,
  3 keeps the emitter frame, 4 lays XY on XZ, 5/6 point Y along the velocity. Mirror-wrap UV doubling applied to
  meshes as to quads (not checked).
- Effects tab: mesh emitters marked "· mesh". Seen in the window: `ee003_star` (tumbling 3D stars, were squares),
  `ee008_end_impact` (light rings as tilted ring meshes).
- Next for effects: combiners (colour 1 / alpha 1, textures 1-2), `CADP`/`FCOL`/`FRND` blocks, `.traef` events.

### Effects: colour combiners (2026-11-18, not committed yet)
- **Combiner found** at emitter +0x8F8: 20 bytes, field order from EffectLibrary's `EmitterCombiner` (versions
  < 36; the library names the fields, not the values): colour / alpha process, texture 1 / 2 / primitive colour
  and alpha blends, texture 0-2 / primitive colour and alpha inputs, shader type, apply alpha. Also read: colour 1
  / alpha 1 (types 0x99D / 0x99F, values 0x9B0 / 0x9BC, keys 0x4C0 / 0x540), emitter colour 1 (0x7BC).
- **Value meanings worked out from the dump** (`probe --vfxb` counts them; `--vfxb-dump <out>` writes every
  emitter block plus each sampler's BNTX format and channel selectors to study outside the repo):
  colour process 0 colour 0, 1 tex x c0, 2 c1 where the texture is dark to c0 where bright (`glow`: white core,
  orange edge), 3 tex x c0 + c1; alpha process 0 tex x a0, 1 dissolve, 2 x a1, 3 dissolve x a1, 4 distance field
  (last three formulas guessed); blends 0 multiply, 1 add, 2 subtract; inputs 0 as sampled, 1 red (`glow_back`:
  1 = 1 - alpha drew a dark square), 2 one (opaque BC1 51 of 60), colour 3 alpha.
- **BNTX channel selectors** (BRTI +0x58): BC4 / R8 `R,R,R,R`, BC5 `R,G,G,G` or `R,R,R,G`, colour formats as
  stored. `texture::channel_select` / `apply_channels`; the particle textures use them (Textures tab unchanged).
- **Blend type 5 = screen distortion** (19 emitters, the `dist` ones): switched off, labelled. Shader type 1 / 2
  with a two-channel texture 0: texture 0 pushes the UVs of textures 1-2 (strength not found, fixed 0.08).
- **Spawners**: 53 emitters with no texture and no mesh (48 with children: `gp1`, `EmitOya`, `*_parent`) are
  simulated, not drawn (they were red / white squares).
- `src/particle_gpu.rs`: own three-d `Geometry` (quads, or the primitive mesh instanced) with three UV sets and
  both colours; `ParticleMaterial` with the combiner uniforms; `shaders/particle.frag` rewritten. Particles of an
  emitter are sorted back to front. V is flipped in the vertex shaders (three-d flips texture rows).
- `texture.rs`: R5G6B5 (0x0701, 32 effect textures; red taken as the low bits, not checked). Viewer:
  `--emitters a,b` (with `--effects`: only these emitters on).
- Seen in the window: `ee003_star` (round orange glow, stars with their second/third textures, violet colour-1
  sparkles), `ee008_end_impact` (rings now thin and sharp: colour 1 keyed to about -9), the raid shield dome
  (`ej_raid_powercharge_success_shield`: layered blue crystal, no more red spawner squares).
- Not checked against the game: the direction of colour process 2, dissolve / distance-field formulas, alpha
  input 3 and colour input 4 (rare), UV distortion strength. Comparing with a capture of the game would settle it.
- Next for effects: `CADP`/`FCOL`/`FRND`/`FSPN` blocks (fields: colour, random, spin), emitter follow types and
  emission by distance, `.traef` events on Pokémon / character animations.

### Effects: emitter animations, fields, stripes (2026-11-18, not committed yet)
- An emitter's attribute chain (EMTR section +0x10) is read (`vfxb::Extras`). `probe --vfxb-blocks <magic> [n]`
  prints every block of a type as words with its emitter; `probe --vfxb` counts what is read.
- **Emitter animations** (`EA**`, 427; layout from EffectLibrary: u8 enable, loop, random start; u32 key count;
  u32 loop count; keys {x, y, z, frame of the emitter's life}). Targets by name and values: EATR translation,
  EASL scale, EAER rate, EAC0/EAC1 colours, EAA0 alpha, EAOV/EADV speeds, EAGV gravity, EAPL life, EASS particle
  scale, EAES volume scale; all keys in order. Not used: EAET (14; values look like positions).
- **Fields** (layouts from the dump, formulas guessed): FSPN spin (142; radians per frame, axis 0/1/2, outward
  speed), FRN1 jitter (11; amplitude, every n frames), FRND smooth random offset (240; amplitude + 4 noise
  frequencies/weights), FCOV pull to the emitter (60; rate or keys over life), FMAG magnet (9), FPAD position add
  (1), FCOL floor (5). Not read: FCLN curl noise (114), FCSF (8).
- **Stripes**: EP02 (97, history length +0x14) / EP03 (14, +0x18) draw the particle as a camera-facing ribbon
  through its last positions, texture U along it. EP04 (19, raid background dust; looks like an area loop) not used.
- Viewer: emitter list marks "· stripe"; `--frame n` (with `--effects`) jumps to a frame and pauses.
- Seen in the window: `eb004_ballout` trail at frame 12: teal ribbons spiralling out (stripe + spin + outward).
  `merge_trail` stripes curve; `ej_raid_powercharge_start` uses EATR/EAC0 (not looked at closely).
- Next for effects: FCLN curl noise, EP04, emitter follow (child emitters inherit parent motion), `.traef` events
  on Pokémon / character animations.

### Cutscenes tab: list and timelines (2026-11-18, not committed yet)
- How cutscenes work: a `.trtml` **timeline** is the script (`demo/<kind>/<name>/<name>.trtml`). It holds no
  animation itself: tracks per object of the cutscene's scenes, each with clips of commands on a 60 fps frame line
  (`Scene/LoadSubScene` the rooms and characters, `CameraAnimation/Load` + `/Play` `.trcma` cameras,
  `Animation/*` on models (`.tracp`/`.tracm` clips next to the timeline, trainers' in `chara/demo_cc_base/`),
  `GameObject/Set*` transforms, `Model/SetVisibility`, `Light/*`, `Particle/Emit`, `Audio/PostEventGlobal` Wwise
  events, `Title/Message/ShowMessageWindow` with a message file + label, fades, post effects). Battle timelines
  repeat tracks per condition (`PlayerRide`, `BallFall`, `SelfOther`, ...); story events have a track per game
  version for text (`PackageType` A / B).
- `timeline.rs` (format in its doc comment), `cutscenes.rs` (catalogue: each `demo/` pack name rebuilt, each
  folder above it tried as `<folder>/<folder>.trtml`), `cutscenes_tab.rs`, `probe --timelines [name]`.
- Found 285 timelines: story events 29 (`d010`..`d810`), battle 69, trainers in battle 97, battle idle cameras 51,
  Terastal and raids 20, system 15, test 4. All parse; no clip ends after the length (root slot 1). Brute force
  over `demo/ev/dNNN`, `demo/ee/eeNNN`, `demo/ej/ejNNN` (0-999) found none the pack names miss.
- Frame rate checked: d010's cameras (60 fps `.trcma`) follow each other with no gap at 60 timeline frames/s.
- Seen in the window: d010 (academy introduction, 1:37, 106 tracks, 999 clips) with its English lines; ee101
  (capture, 44 tracks with `BallFall` / `PlayerRide` variants).
- Next for cutscenes: play one. Load the sub-scenes (`world/scene/parts/demo/...`, `scene.rs` already reads
  them), find objects by the track names (`d010.tr0690_G_Mod` = scene `d010`, object `tr0690_G_Mod`), read
  `.trcma` cameras and `.tracp`/`.tracm` clips (unknown formats), then run the clips: visibility, transforms,
  camera, animation, text. Battle timelines also need the battle's actors. `effect/battle_*` move timelines use
  the same format.

### Cutscenes: playing, step C2 cast (2026-11-18, not committed yet)
- `cast.rs`: actors from every scene (model component: `CharacterCreationMasterComponent` -> `.ccdatam` ->
  `.ccdata`, `CharacterCreationComponent` -> `.ccdata`, `ti_PokemonModelComponent` -> species from the name);
  clips through the timeline's container link; cut clips by shot name; variants' `vis_*` groups hidden.
- `skin.rs`: `Loaded`, `build`, `apply_skin`, `Clip` moved out of `main.rs` (no behaviour change; `--check` as
  before: 579 streamed scenes, 156 room scenes, 0 failures).
- `probe --cast [name]` (220 actors, 2 fail: the player), `probe --ccdata <path>` (parts, variants, mesh groups).
- Facts: `int_cutnum` values are not dense per actor (tr0680_M has one clip, asked for cut 4), so the clip is the
  one named after the shot; k-th and by-shot differ for 29 cuts. A variant's tags are `{0: tag}` tables. The
  white domes on students were the hat and the head's hat-hair cap, which variants hide.

### Cutscenes: playing, step C1 scenes and camera (2026-11-18, not committed yet)
- Plan: [CUTSCENE_PLAN.md](CUTSCENE_PLAN.md). **Play** in the Cutscenes tab (or `--play d010 [--frame n]`).
- `camera_anim.rs`: `.trcma` read (lens + transform channels, `anim.rs` union tracks made `pub(crate)`);
  `probe --cameras`: all 495 cameras the timelines play parse, all 60 fps.
- `cutscene_play.rs`: root scene `world/scene/parts/demo/<kind>/<name>_/<name>_0.trscn`, its sub-scenes, the
  timeline's `LoadSubScene`s and scenes it only ends or addresses, each a `plants` group shown while loaded;
  the camera from the shot playing; the current line as a subtitle (`msg.rs`).
- Timeline values: tag 5 is a vec3 (inline struct), not a float; fixed in `timeline.rs`.
- Found on the way: characters are `.ccdata` (base + parts), their clips `.tracn` -> `.tracr` -> `.tranm`, one
  clip per cut picked by `int_cutnum`; see the plan for C2.
- All 29 story events have a root scene and camera shots. d200 (and probably others) play in the overworld and
  show nothing yet (C1b).

## In flight

- Commit the play-mode work (waiting on the user).

## Next

See [ROADMAP.md](ROADMAP.md). Immediate next steps:

1. Generalise discovery: an asset index built from all pack names (category, name, path, extension), replacing
   the Pokémon-only list. Left panel gets a category picker.
2. World tool, next steps:
   - Play mode in the World tab: a character placed on `TerrainGround`, camera following height (plan S3/S4).
   - Grass: height file slot 13 masks + `.trtrr` slot 6 parameters, scattered per cell, instanced, short range.
   - Towns (done 2026-11-16, step 4): next, the two-layer "BGLayer" material (vertex-colour mix), distant
     `clod` scenes, and the DLC maps (`ter_1x`/`ter_2x`, `su1`/`su2`) as their own worlds.
   - Loading speed (measured 2026-11-18, release, CPU side only, all 100 Paldea tiles without towns: 17.2 s on the
     main thread). Model bake 12.4 s (layered models 6.5 s for 195, about 33 ms each in `apply_layers`' powf;
     plain 5.9 s for 733), all LODs' mesh loads 1.6 s, heights 0.9 s (1026 files decompressed to find 100 height
     files), shared layers 0.37 s, scenes 0.2 s. Decoding each unique texture once costs only about 2 s: 2160 decodes
     repeat across models (no shared cache). Fix order: texture cache shared across models; no normal maps
     for placed models (they draw albedo only); sRGB LUTs in `apply_layers`; pick the height file by size before
     decompressing; worker threads with their own `Archive` (as `gallery.rs`); nearest tiles first. GPU side not
     measured: every group uploads its models' vertex buffers again (`InstancedMesh` per group), water and
     resident load the same models twice, the pack cache clears all 64 entries at once.
   - Area Zero's crater models; check the layer normal map's green sign against the game; white trunk bases on
     palms (a material without colour texture?); `.trtbf` detail maps (0/255 masks, meaning unknown).
3. Phase 1 of the roadmap, continued:
   - Look at a few trainers and props in the window; check materials that use `DetailMaskMap`, `NormalMap1/2`,
     `HairFlowMap`, `SSSMaskMap` (ignored by the bake for now).
   - Trainer animations (done 2026-11-16): next, find the game's body type per trainer/NPC (data tables), face
     clips (`motion_tr/.../_head00.tracr`, `.tracp`), the `vat` trainers, and `model_vr` assembly (body + head +
     parts on `cc_base`, like the player).
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
- Path recovery by brute force: `probe --split <prefix/> <collapsed rest>` tries up to 3 slashes (one found
  `unity_output/town_t07_field_streaming_s_/ins_root_...`); `probe --guess-ins` names a pack's files from a
  pattern. Folder names can end in `_` and file names can be long joined words. (2026-11-16)
- Confirmed real paths (2026-11-14): `field_graphic/common_model/a_t01_i01/a_t01_i01.trmdl` (interior),
  `field_graphic/terrain/terrain_00_00/ter_00_00.trtrr` (terrain tile), `chara/model_ob/ob0001_phone/ob0001_00.trmdl`
  (prop), `world/scene/parts/field/room/a_c01_hairsalon_/a_c01_hairsalon_0.trscn` (room scene). More in
  ARCHIVE_PATHS.md.
