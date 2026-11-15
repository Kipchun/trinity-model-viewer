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
| `charamodel_tr` / `_uqtr` / `_vrtr` / `_pc` | `chara/model_tr/tr0330_karate/tr0330_00.trmdl`? | trainers, player parts | inferred |
| `charasharecm_` | `chara/share_cm/...`? | shared clothing / body textures (`cm_drs####`) | inferred |
| `charamotion_` | `chara/motion_cc_base/...`? | character motions (`.trmdt`, `.tracn`, `.tracr`) | inferred |
| `charademo_` | `chara/demo_cc_base/...`? | cutscene clips (`.tracp`) | inferred |
| `field_graphicterrain_model` | `field_graphic/terrain_model/plants_me_tree_pine03/plants_me_tree_pine03.trmdl` | objects, plants, ground | confirmed |
| `field_graphicterrainterrain_` | `field_graphic/terrain/terrain_00_00/ter_00_00.trtrr` | overworld terrain tiles | confirmed |
| `field_graphiccommon_model` | `field_graphic/common_model/a_t01_i01/a_t01_i01.trmdl` | interiors and furniture | confirmed |
| `field_graphicterrain_clod` | `field_graphic/terrain_clod/...` | coarse LOD terrain, towns | inferred (first guess failed) |
| `field_graphicterrain_env` | `field_graphic/terrain_env/...` | per-tile lighting (`envter_*.trlgt`) | inferred |
| `field_graphic{glb,tr,common}_share_tex` | `field_graphic/<x>_share_tex/...` | shared field textures | inferred |
| `worldscenepartsfieldroom` | `world/scene/parts/field/room/a_c01_hairsalon_/a_c01_hairsalon_0.trscn` | room scenes | confirmed |
| `worldscenepartsevent` | `world/scene/parts/event/...` | event scenes (`.trsog`) | inferred |
| `envfel` | `env/fel/fel_a_com_hairsalon01/fel_a_com_hairsalon01.trlgt` | room lighting, specular probes | confirmed |

Gotchas found while confirming:

- The field root is `field_graphic/`, **not** `field/graphic/`.
- Folder names can end in `_` (`a_c01_hairsalon_/`), and file names need not repeat the folder name
  (`ob0001_phone/ob0001_00.trmdl`). A rebuilt path is only real once `contains` returns true; try several
  splits.
- Trainer skeletons come as `tr####_00_loc.trskl` (a separate "loc" skeleton per trainer).

## Area codes seen

- `a_sch_*`: the academy (classrooms, dorms, cafe, office, entrance, grounds, studio, clubroom).
- `a_tNN_iMM`: town NN, interior MM. `a_tNN_gMM`: town NN, gym MM.
- `a_cNN_*`: city NN (hair salon, sandwich shops, gym `g01`).
- `a_com_*`: shared interiors (houses, buildings, hair salon, sandwich shop, `nushi`).
- `a_raid_<type>`: Tera raid arenas, one per type (Japanese type names: `honoo` fire, `mizu` water, ...).
- `a_plNN`: unknown; `a_w23_*`, `area23`: DLC area interiors and caves; `pi_*`: picnic.
- `terrain_XX_YY`: overworld grid tiles; `clod_town_tNN`: town LOD meshes.
- **Area Zero**: not identified yet.

## Character codes seen

`pm####` Pokémon, `tr####` trainers (`tr0410_mania`), `cc####_<kind>` crowd-NPC bodies (`kid`, `adl_m`, `adl_f`,
...), `ob####_<name>` props, `cm_drs####` dress-up clothing, `pc` player.
