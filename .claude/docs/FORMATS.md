# Trinity file formats

Status: **read** = parsed and checked over the dump; **partial** = some slots known; **unknown** = not studied.
Counts are packs in the user's dump whose name ends in that extension (a pack can hold more files of other types).
The field layouts of read formats live in the doc comment at the top of each source file; this table only points.

| Ext | Packs | Status | What it is (known or guessed) | Source |
| --- | ---: | --- | --- | --- |
| `.trpfd` / `.trpfs` | 1 / 1 | read | archive file table / packs | `archive.rs` |
| `.trpak` | 23,586 | read | one pack (FlatBuffers, Kraken per file) | `archive.rs` |
| `.trmdl` | 2,389 | read (Pokémon) | model: meshes, skeleton, materials, LODs | `model.rs` |
| `.trmsh` | — | read | mesh shapes, vertex layouts | `model.rs` |
| `.trmbf` | 281 | read | vertex and index buffers | `model.rs` |
| `.trmtr` | 19 | read | materials | `model.rs`, `materials.rs` |
| `.trmmt` | 23 | unknown | material sets / variants (guess: alt colours, outfit variants) | |
| `.bntx` | 4,984 | read (first mip/layer) | textures | `texture.rs` |
| `.trskl` | 159 | read | skeletons | `skeleton.rs` |
| `.tranm` | 162 | read | skeletal animation | `anim.rs` |
| `.tracr` | 95 | partial (`_base.tracr`) | animation list of a model | `anim.rs` |
| `.tracn` | 22 | unknown | guess: non-skeletal curves (face, material, visibility) | |
| `.trmdt` | 385 | unknown | character motion data (`chara/motion/...`) | |
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
| `.trtrr` | 38 | unknown | terrain tile (`terrain_XX_YY/ter_XX_YY`) | |
| `.trlgt` | 358 | unknown | lighting per room / terrain tile | |
| `.trcol` | 19 | unknown | collision mesh | |
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
