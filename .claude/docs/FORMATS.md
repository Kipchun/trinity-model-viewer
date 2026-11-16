# Trinity file formats

Status: **read** = parsed and checked over the dump; **partial** = some slots known; **unknown** = not studied.
Counts are packs in the user's dump whose name ends in that extension (a pack can hold more files of other types).
The field layouts of read formats live in the doc comment at the top of each source file; this table only points.

| Ext | Packs | Status | What it is (known or guessed) | Source |
| --- | ---: | --- | --- | --- |
| `.trpfd` / `.trpfs` | 1 / 1 | read | archive file table / packs | `archive.rs` |
| `.trpak` | 23,586 | read | one pack (FlatBuffers, Kraken per file) | `archive.rs` |
| `.trmdl` | 2,389 | read (Pokémon, trainers, props) | model: meshes, skeleton, materials, LODs. All 264 trainer/prop models load with no warnings; player-base models have no meshes | `model.rs` |
| `.trmsh` | — | read | mesh shapes, vertex layouts | `model.rs` |
| `.trmbf` | 281 | read | vertex and index buffers | `model.rs` |
| `.trmtr` | 19 | read | materials | `model.rs`, `materials.rs` |
| `.trmmt` | 23 | unknown | material sets / variants (guess: alt colours, outfit variants) | |
| `.bntx` | 4,984 | read (first mip, any array layer; RGBA32F for the splat params) | textures | `texture.rs` |
| `.trskl` | 159 | read | skeletons. Player parts: slot 4 = joint offset in the player's shared joint list, node slot 6 = base bone to hang from (all 208 parts attach cleanly) | `skeleton.rs` |
| `.tranm` | 162 | read | skeletal animation | `anim.rs` |
| `.tracr` | 95 | partial (`_base.tracr`) | animation list of a model; the player's is `chara/motion_pc/base/p0_base.tracr` (727 clips) | `anim.rs` |
| `.tracn` | 22 | unknown | guess: non-skeletal curves (face, material, visibility) | |
| `.trmdt` | 385 | partly | motion set per body type: `{0: [string]}` naming a `.trmdd` ("motion detector") file, e.g. `cc0004_adl_m_motion_detector.trmdd` | |
| `.tracp` | 359 | unknown | cutscene animation (`chara/demo/...`) | |
| `.tracm` | 43 | unknown | guess: camera animation | |
| `.trcma` | 64 | unknown | | |
| `.trtml` | 267 | unknown | guess: timeline | |
| `.trpokecfg` | 100 | unknown | per-Pokémon config | |
| `.trscn` | 153 | unknown | scene: object placement (rooms, battle backgrounds) | |
| `.trsog` | 2 | unknown | scene object group | |
| `.trsot` | 959 | unknown | guess: scene object template | |
| `.trins` | 99 | unknown | guess: instances | |
| `.trspb` | 127 | unknown | | |
| `.trslp` | 65 | unknown | | |
| `.trtrr` | 38 (100 files) | partial | terrain tile header (`terrain_XX_YY/ter_XX_YY`): names `ter_XX_YY.trmtr`, `_detailmap_N.trtbf`, grass models. Its pack also holds the **height file** (name unknown, found by shape): 513 x 513 f32 heights, origin, scale, splat and grass names. All 100 tiles parse (`probe --terrain`) | `terrain.rs` |
| terrain `.trmtr` | — | read | `ter_XX_YY.trmtr`: "Terrain" shader; splat control map (R/G two layer indices, B/A weights), shared 60-layer colour and normal arrays, per-layer tiling (5 m) in `global_splatST_params` | `terrain.rs`, `shaders/terrain.frag` |
| `.trtbf` | 2 | unknown | terrain detail map, 1024 x 1024 x 4 bytes (+72 header), FlatBuffer | |
| `.trlgt` | 358 | unknown | lighting per room / terrain tile | |
| `.trcol` | 19 | unknown | collision: Havok packfile (`TAG0`, `SDKV`), e.g. `ter_XX_YY.trcol` | |
| `.trbsm` | 10 | unknown | | |
| `.traef` | 518 | unknown | effect | |
| `.ptcl` | 428 | unknown | particles (NintendoWare-style?) | |
| `.imap` | 145 | unknown | | |
| `.bfbs` | 6 | unknown | binary FlatBuffers schemas: **check these first**, they may describe other formats | |
| `.tbl`, `.dat`, `.bin`, `.arc` | many | out of scope | game data, messages, UI | |

## Studying a format

Use the `study-format` skill. In short: find one file by pack name, `probe -- <path> 3` to dump its slots,
compare several files, write the layout into a module doc comment, add a probe mode that checks every file of
that type, then load it in the viewer.
