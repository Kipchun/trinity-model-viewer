# Archive paths and naming

The `.trpfd` holds only FNV-1a 64 hashes of paths. Pack names are the one source of path text:
`arc/` + the path with every `/` removed + `.trpak`. To open a file, rebuild its path (insert the slashes) and
confirm with `Archive::contains(path)`; then guess siblings (same folder, other extensions or suffixes) the same
way. `Archive::pokemon_models` is the worked example.

Collapsed prefix → real folder. **Confirmed** rows were checked against the hash table (2026-11-14) with the
example path shown; the others are inferred and need a check.

| Pack prefix | Path (example) | Content | |
| --- | --- | --- | --- |
| `pokemondata` | `pokemon/data/pm0025/pm0025_00_00/pm0025_00_00.trmdl` | Pokémon | confirmed |
| `charamodel_ob` | `chara/model_ob/ob0001_phone/ob0001_00.trmdl` | props | confirmed |
| `charamodel_trtr` | `chara/model_tr/tr0330_karate/tr0330_00.trmdl` | trainer classes (30) | confirmed |
| `charamodel_uqtr` | `chara/model_uq/tr0080_principal/tr0080_00.trmdl` | named characters (74): teachers, rivals, ... | confirmed |
| `charamodel_vrtr` | `chara/model_vr/tr0620_schildm/tr0620_00_drs02.trmdl` | split into body (`_drs##`) and head (`_head##`) parts (29) | confirmed |
| `charamodel_pc_base` | `chara/model_pc_base/model/p0_base.trmdl` | player base: skeleton only, no meshes | confirmed |
| `charamodel_pc` | `chara/model_pc/p0_bag0000_schoolbackpack/p0_bag0000_00_bag00.trmdl` | player parts (`p0`/`p1`; acc, bag, drs, eyw, face, ftw, glv, hdw, hrs, lgw) | confirmed |
| `charamodel_cc_base` | `chara/model_cc_base/model/cc_base.trmdl` | crowd-NPC base: skeleton only | confirmed |
| `charasharecm_` | `chara/share_cm/...`? | shared clothing / body textures (`cm_drs####`, `uni`, `fes`, ...) | inferred |
| `charamotion_cc_base` | `chara/motion_cc_base/cc0004_adl_m/cc0004_adl_m.tracr` | 25 shared body types (`cc0001_kid` .. `cc2011_cla_f`): clip list `<type>.tracr`, clips `<type>_<NNNNN>_<name>.tranm`; per-trainer folders (`tr0420_mountain/tr0420_00.tracr`) with full lists or per-clip files of special actions | confirmed |
| `charamotion_uq` | `chara/motion_uq/tr0080_principal/tr0080_00.tracr` | named characters' own clip lists (`tr0080_00_<NNNNN>_<name>.tranm`) | confirmed |
| `charamotion_tr`, `_vr` | `chara/motion_tr/tr0360_tanpan/tr0360_00_head00.tracr` | face clips (`.tracr`/`.tracp`/`.tracl`/`.tracn` of the head), spring bones (`.trspb`) | confirmed |
| `charademo_` | `chara/demo_cc_base/...`? | cutscene clips (`.tracp`) | inferred |
| `field_graphicterrain_model` | `field_graphic/terrain_model/plants_me_tree_pine03/plants_me_tree_pine03.trmdl` | objects, plants, ground | confirmed |
| `field_graphicterrainterrain_` | `field_graphic/terrain/terrain_00_00/ter_00_00.trtrr` | overworld terrain tiles | confirmed |
| `field_graphiccommon_model` | `field_graphic/common_model/a_t01_i01/a_t01_i01.trmdl` | interiors and furniture | confirmed |
| `field_graphicterrain_env` | `field_graphic/terrain_env/...` | per-tile lighting (`envter_*.trlgt`) | inferred |
| `field_graphic{glb,tr,common}_share_tex` | `field_graphic/<x>_share_tex/...` | shared field textures | inferred |
| `field_graphicterrain_clod` (confirmed) | `field_graphic/terrain_clod/clod_town_c02_clod000/clod_town_c02_clod000.trmdl` | coarse LOD meshes: `clod_town_<tNN/cNN>_clod###` / `_clod_far###`, `clod_terrain_XX_YY_clod###_<what>` (XX up to 23: grid codes beyond the 10 x 10 tiles?), `clod_c01_both_01_*`. Centred on their origin: need placements | confirmed |
| `worldscenepartsfieldunity_output` | `world/scene/parts/field/unity_output/town_t07_field_streaming_s_/ins_root_plants_un_garden_tulip01_yellow_007_X00_Z00.trins` | overworld streaming instances: one pack of 2405 `.trins`; the scenes beside them name 1560 (`<scene>_/ins_root_<model>_<NNN>_X00_Z00.trins`, also `ins_clod000_...`) | confirmed |
| (no pack name) | `world/scene/parts/field/world_resident/outdoor_resident_/outdoor_resident_0.trscn`, `world/scene/parts/field/world_resident_/world_resident_0.trscn`, `unity_output_1/dlc1_resident_/dlc1_resident_0.trscn`, `unity_output_2/dlc2_resident_/dlc2_resident_0.trscn` | resident scenes (water, outer scenery, the Terarium dome); found from `SubScene` references, confirmed by hash. `unity_output/town_c01_field_resident_`, `unity_output/c01_both_01_field_resident_` too. ~100 `ter_XX_YY_resident` scenes place nothing | confirmed |
| (no pack name) | `world/scene/parts/field/unity_output_1/ter_12_02_streaming_m_/ter_12_02_streaming_m_0.trscn` | DLC maps' streamed scenes: `unity_output_1` Kitakami (tiles 10-13), `unity_output_2` Terarium (20-23); sizes s/m/l/xl | confirmed |
| `worldscenepartsfieldroom` | `world/scene/parts/field/room/a_c01_hairsalon_/a_c01_hairsalon_0.trscn`, `room/battlebg_normal_c01_/...`, `room/clubroom/clubroom_02_obj_/...`, `room/interior_common/...` | 151 room scenes (`maps::rooms` recovers them from pack names); made of `SubScene`s (`<code>_obj`, `_camera`, `_event`, `_scenario`, `_area_col`) | confirmed |
| (no pack name) | `world/scene/parts/field/unity_output/town_t07_field_streaming_s_/town_t07_field_streaming_s_0.trscn` | the streamed scenes: `<name>_/<name>_0.trscn` (+ an identical `_1`). Families in `scene.rs` (`area_scenes`, `tile_scenes`); all names readable from the two scene packs (`probe --scene-packs`) | confirmed |
| `field_graphicterrainterrain_` (`.trins`) | `field_graphic/terrain/terrain_02_03/ter_02_03_plants_me_tree_oak03_X0Z0_d0.trins` | a tile's trees, listed in its `.trtrr` | confirmed |
| `worldscenepartsfieldroom` | `world/scene/parts/field/room/a_c01_hairsalon_/a_c01_hairsalon_0.trscn` | room scenes | confirmed |
| `worldscenepartsevent` | `world/scene/parts/event/...` | event scenes (`.trsog`) | inferred |
| `applitex` | `appli/tex/icon_item/item_0183/item_0183.bntx`, `appli/tex/icon_type/tga_jpn/...` | UI textures (icons, map pictures, menus); one pack holds a whole set (562 item icons), named after one | confirmed |
| `applidata` | `appli/data/bag/state_bag_00.truis`, `appli/data/btl_dan/view_btl_dan_rush_ko_00.truiv` | UI layouts ("UikitBody" FlatBuffers, no textures) | confirmed |
| `demoapplipokedex` | `demo/appli/pokedex/zk0000_00/pokedex_capture_thum_001.bntx` | Pokédex pictures and the Pokédex scene model | confirmed |
| `pokemondata` (icons) | `pokemon/data/pm0001/pm0001_00_00/icon/pm0001_00_00_00.bntx` | Pokémon menu icons | confirmed |
| (all `.bntx`) | | `textures::catalogue` rebuilds all 4984 `.bntx` pack names (2026-11-16) | confirmed |
| `envfel` | `env/fel/fel_a_com_hairsalon01/fel_a_com_hairsalon01.trlgt` | room lighting, specular probes | confirmed |

Gotchas found while confirming:

- The field root is `field_graphic/`, **not** `field/graphic/`.
- Folder names can end in `_` (`a_c01_hairsalon_/`), and file names need not repeat the folder name
  (`ob0001_phone/ob0001_00.trmdl`). A rebuilt path is only real once `contains` returns true; try several
  splits.
- Trainer skeletons come as `tr####_00_loc.trskl` (a separate "loc" skeleton per trainer, locators such as
  `loc_eff_head`). Trainer packs are named after that file, not the `.trmdl`, which is `tr####_00.trmdl` in the
  same folder. `Archive::character_models` turns the pack names into model paths (264 found, 2026-11-15).
- `uqtr` packs live in `model_uq/`, `vrtr` in `model_vr/`: the group's last two letters are the folder code's start.
- Player parts index the **base** skeleton's joints (a bag's own `.trskl` has 4 joints, its meshes use joint 107),
  so they can only be shown on `p0_base.trskl`. The same holds for one `model_vr` head part.

## Effects

- Particle effects: `effect/<group>/.../<name>.ptcl`. Groups: `battle_cmn/`, `battle_eb/`, `battle_ee/eeNNN/`
  (moves?), `battle_ej/` (raids: `ej_raid_bg00/`, ...), `applilastbattle/`, `applicooking/`, `applinet_battlespot/`,
  `demo/evdNNNN/`, `field/nushi/flying/`, `field/gym/`, ... (`effect/` + up to 3 slashes; all 428 rebuild).
- Animation effect lists: next to the clips, `pokemon/data/pmNNNN/pmNNNN_FF_VV/pmNNNN_FF_VV_<id>_<clip>.traef`
  and `chara/motion_<group>/<code>_<name>/<code>_00_00<id>_<clip>.traef`.

## Area codes seen

- `a_sch_*`: the academy (classrooms, dorms, cafe, office, entrance, grounds, studio, clubroom).
- `a_tNN_iMM`: town NN, interior MM. `a_tNN_gMM`: town NN, gym MM.
- `a_cNN_*`: city NN (hair salon, sandwich shops, gym `g01`).
- `a_com_*`: shared interiors (houses, buildings, hair salon, sandwich shop, `nushi`).
- `a_raid_<type>`: Tera raid arenas, one per type (Japanese type names: `honoo` fire, `mizu` water, ...).
- `a_plNN`: unknown; `a_w23_*`, `area23`: DLC area interiors and caves; `pi_*`: picnic.
- `terrain_XX_YY`: overworld grid tiles; `clod_town_tNN` / `cNN`: town LOD meshes (t01-t10, c02, c03_01-03 seen).
- Pack names of the town streaming scenes: only one exists. Rebuild others with `probe --split` / `--guess-ins`.
- **Area Zero**: not identified yet.

## Character codes seen

`pm####` Pokémon, `tr####` trainers (`tr0410_mania`), `cc####_<kind>` crowd-NPC bodies (`kid`, `adl_m`, `adl_f`,
...), `ob####_<name>` props, `cm_drs####` dress-up clothing, `pc` player.
