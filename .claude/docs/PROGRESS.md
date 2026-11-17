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
