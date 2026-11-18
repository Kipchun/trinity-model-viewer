# Trinity file formats

Status: **read** = parsed and checked over the dump; **partial** = some slots known; **unknown** = not studied.
Counts are packs in the user's dump whose name ends in that extension (a pack can hold more files of other types).
The field layouts of read formats live in the doc comment at the top of each source file; this table only points.

| Ext | Packs | Status | What it is (known or guessed) | Source |
| --- | ---: | --- | --- | --- |
| `.trpfd` / `.trpfs` | 1 / 1 | read | archive file table / packs | `archive.rs` |
| `.trpak` | 23,586 | read | one pack (FlatBuffers, Kraken per file) | `archive.rs` |
| `.trmdl` | 2,389 | read (Pokémon, trainers, props, field models with LODs: slot 1 = mesh files per LOD, slot 4 = `{0: [LOD index], 1: "Custom"}`) | model: meshes, skeleton, materials, LODs. All 264 trainer/prop models load with no warnings; player-base models have no meshes | `model.rs` |
| `.trmsh` | — | read | mesh shapes, vertex layouts | `model.rs` |
| `.trmbf` | 281 | read | vertex and index buffers | `model.rs` |
| `.trmtr` | 19 | read | materials: textures, float/float4 params, shader name + switches (`EnableAlphaTest`, ...), render type (`Opaque`, `Blend`, `BlendPreMultiAlpha`, `Add`, `Mul`); `probe --transparency` surveys them | `model.rs`, `materials.rs` |
| `.trmmt` | 23 | unknown | material sets / variants (guess: alt colours, outfit variants) | |
| `.bnk` (Wwise) | 16 (RomFS `audio/wwise/bin/Switch/`, outside the archive) | read: DIDX/DATA media, HIRC Sound (2) and Music track (11) sources; bank version 140 | sound banks | `wwise.rs` |
| `.pck` (Wwise AKPK) | 5 (Stream, SDC0, SDC2 hold files; LooseMedia, Patch00 empty) | read: stream table | streamed sounds | `wwise.rs` |
| message `.tbl` / `.dat` | ~9000 packs (`message/dat/<language>/...`) | read: AHTB labels, XOR-rotated UTF-16 lines | game text | `msg.rs` |
| `.wem` | 17,512 (+ 74 rumble in VIB.bnk, left out) | decoded: PTADPCM 13,261, Vorbis 3,883, Opus NX 329, PCM 39; all decode (`probe --audio decode`) | music, sound effects | `wem.rs` |
| `.bntx` | 4,984 | read (first mip, any array layer; RGBA16F/RGBA32F clamped for display); name at BRTI 0x60. 36,478 in the packs named by `.bntx` and the model packs, all decode (`probe --textures decode`) | textures, UI sprites | `texture.rs`, `textures.rs` |
| `.trskl` | 159 | read | skeletons. Player parts: slot 4 = joint offset in the player's shared joint list, node slot 6 = base bone to hang from (all 208 parts attach cleanly) | `skeleton.rs` |
| `.tranm` | 162 | read | skeletal animation | `anim.rs` |
| `.tracr` | 95 | partial (`_base.tracr`) | animation list of a model; the player's is `chara/motion_pc/base/p0_base.tracr` (727 clips); trainers' in `chara/motion_cc_base/` and `motion_uq/` (see `motion.rs`); `motion_tr/.../<code>_00_head00.tracr` lists face clips | `anim.rs`, `motion.rs` |
| `.tracn` | 22 | unknown | guess: non-skeletal curves (face, material, visibility) | |
| `.trmdt` | 385 | partly | motion set per body type: `{0: [string]}` naming a `.trmdd` ("motion detector") file, e.g. `cc0004_adl_m_motion_detector.trmdd`. A trainer's own `.trmdt` names either itself or the body type it is built on (`tr0420_00.trmdt`: `cc0005_adl_f`) | `motion.rs` (the name only) |
| `.tracp` | 359 | unknown | cutscene animation (`chara/demo/...`) | |
| `.tracm` | 43 | unknown | guess: camera animation | |
| `.trcma` | 64 | unknown | | |
| `.trtml` | 267 | unknown | guess: timeline | |
| `.trpokecfg` | 100 | unknown | per-Pokémon config | |
| `.trmsh` vertex colour | - | read | semantic 5, RGBA8 (format 20); water uses a = opacity, r = foam. See `model.rs` for all semantics/formats | `model.rs` |
| `.trmtr` shader "Water" | 1 + 21 + 6 parts | approximated | BaseColor, NormalMap/NormalMap1 (BC5) with UVScaleOffset(1)/ScrollUVSpeed(1), FoamMaskMap (BC4), FoamColor, Foam*; FlowMap 1x1 on rivers. `probe --water` | `water.rs` |
| `.trscn` | 153 (thousands unnamed) | partial | scene: tree of typed nodes `{0: type, 1: [u8] data (FlatBuffer), 2: children}`. Read: SceneObject / ObjectTemplate transforms, ModelComponent model paths, ModelInstancerComponent `.trins` paths. `SubScene` {0: relative path; stored as `<name>_0.trscn`} composes scenes (rooms, resident scenes); `.trsog` references (events) are skipped. All 579 streamed scenes of the three maps and 156 resident/room scenes load (`--check`, `probe --rooms`) | `scene.rs` |
| `.trsog` | 2 | unknown | scene object group | |
| `.trsot` | 959 | unknown | guess: scene object template | |
| `.trins` | 99 (many more unnamed) | read | `SrtInstanceBuffer`: instance count + 4x4 column-major matrices, no model name (the lister names it). Terrain trees: 311 files, 3509 instances, all parse; town streaming pack: 2405 files, 25,000 instances, all parse | `instances.rs` |
| `.trspb` | 127 | unknown | | |
| `.trslp` | 65 | unknown | | |
| `.trtrr` | 38 (100 files) | partial | terrain tile header (`terrain_XX_YY/ter_XX_YY`): slot 4 tree models + their `.trins` (all 100 read), slot 6 grass models + parameters. Its pack also holds the **height file** (name unknown, found by shape): 513 x 513 f32 heights, origin, scale, grass names + per-cell grass bit masks, splat names. All 100 tiles parse (`probe --terrain`). `_heightmap.bntx` (R16) = the same heights | `terrain.rs` |
| terrain `.trmtr` | — | read | `ter_XX_YY.trmtr`: "Terrain" shader; splat control map (R/G two layer indices, B/A weights), shared 60-layer colour and normal arrays (both drawn), per-layer tiling (5 m) and flags in `global_splatST_params`; triplanar off in all 100 | `terrain.rs`, `shaders/terrain.frag` |
| `.trtbf` | 2 | unknown | terrain detail map, FlatBuffer {0: {1: 1024, 2: 1024, 5: 4194304 bytes}}: 1024² x 4 channels of 0/255 masks, mostly empty; not grass cover (weak link to the splat layers) | |
| `.trlgt` | 358 | unknown | lighting per room / terrain tile | |
| `.trcol` | 19 | unknown | collision: Havok packfile (`TAG0`, `SDKV`), e.g. `ter_XX_YY.trcol` | |
| `.trbsm` | 10 | unknown | | |
| `.ptcl` | 428 | partial: emitters read and played (`probe --vfxb`: 428 files, 2594 emitters, 0 failures; offsets in `vfxb.rs`); primitives read (`G3PR` BFRES + `G3NT`, `bfres.rs`; `probe --vfxb-prims`: 622 meshes, 1034 emitters use one, 0 failures); combiner (+0x8F8) and colour 1 / alpha 1 read, meanings from the dump (`vfxb::Combiner`); emitter animations (`EA**`), fields (`FSPN`, `FRND`, `FRN1`, `FCOV`, `FMAG`, `FPAD`, `FCOL`) and stripes (`EP02`/`EP03`) read (`vfxb::Extras`); `FCLN`, `FCSF`, `EAET`, `EP04`, `CADP`/`CSDP`/`CUDP` not | particle effects: Nintendo's **VFXB** binary (NintendoWare / Eft2, the Switch particle system also used by BotW, Splatoon 2 and others), version 0x16. All under `effect/` (battle 292, menus 63, cutscenes 40, field 33). Sections seen (`nushiflying_rocksmoke.ptcl`, 128 KB): `ESTA` emitter set table, `ESET` emitter set (named), `EMTR` emitters (`smoke_front`, ...), `GRTF` with an embedded **`BNTX`** (its textures, readable by `texture.rs`), `G3PR` primitive meshes, `GRSN` / `GRSC` compiled shaders. Public VFXB readers exist in the Switch modding community (field order from KillzXGaming's EffectLibrary) | `effects.rs`, `vfxb.rs`, `particles.rs` |
| `.traef` | 518 | listed (Effects tab); contents not decoded | small FlatBuffers named after an animation clip (`pokemon/data/pm0145/pm0145_00_00/pm0145_00_00_20500_damage01.traef`, `chara/motion_uq/tr0210_insect/..._cityloopaction01_loop.traef`): Pokémon 485, characters 33. Guess: effect events for the clip (which effect, frame, bone). The damage clip looked at is a near-empty 24-byte table | `effects.rs` |
| `.imap` | 145 | unknown | | |
| `effect/` other files | ~1100 packs | unknown | besides `.ptcl`: 957 `.trsot` (guess: scene object templates that place effects), 58 `.tracp`, 54 `.trtml`, 38 `.bntx`, 29 `.trskl`, 23 `.trmbf`, 19 `.trmtr`, 6 `.trcma`, 3 `.trmdl` (mesh effects?) | |
| `.bfbs` | 6 | unknown | binary FlatBuffers schemas: **check these first**, they may describe other formats | |
| `.tbl`, `.dat`, `.bin`, `.arc` | many | out of scope | game data, messages, UI | |

## Studying a format

Use the `study-format` skill. In short: find one file by pack name, `probe -- <path> 3` to dump its slots,
compare several files, write the layout into a module doc comment, add a probe mode that checks every file of
that type, then load it in the viewer.
