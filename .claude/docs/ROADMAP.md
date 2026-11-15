# Roadmap: every model, animation and scene

Phases build on each other. Each phase ends with: a probe check over every file of the new type, `--check`
extended to cover it, README "What it understands" table updated, PROGRESS.md updated.

Counts below are **pack** counts from the user's dump (one pack can hold many files), from `find-assets`.

## Phase 0: asset index and browser (foundation)

- Parse every pack name into `{category, path, name, ext}`. Recover the path's slashes using the known
  directory prefixes (see [ARCHIVE_PATHS.md](ARCHIVE_PATHS.md)) and confirm with `Archive::contains`.
- Categories: Pokémon, Characters, Props, Field objects, Terrain, Interiors/Rooms, Scenes, Effects.
- Left panel: category tabs, search across all; remember the last selection.
- `--check --category <c>` to load every asset in a category headless.

## Phase 1: characters (`chara/`)

Pack groups: `charamodel_*` (pc 202, ob 133, uqtr 74, trtr 30, vrtr 29), `charasharecm_*` (697, shared
clothing/body textures), `charamotion_*` (274, `.trmdt`, `.tracn`, `.tracr`), `charademo_*` (188, `.tracp`).

1. **Props (`ob####`)**: phone, books, bags. Plain `.trmdl`, should mostly load with the current code. Check
   which reference `.trmmt` (material sets) instead of `.trmtr`.
2. **Trainers (`tr####`, `uqtr`, `vrtr`)**: likely a shared base skeleton (`cc_base`) plus per-character parts.
   Find how a trainer `.trmdl` points at its skeleton and at shared `cm_*` textures.
3. **Player (`pc`)**: modular body + clothing (`cm_drs####` dress-up sets). Needs a part picker: assemble
   head, body, tops, bottoms, shoes on one skeleton.
4. **Character animations**: `.trmdt` (motion data? 385 packs), `.tracn` (22, maybe face/material curves),
   `.tracp` (cutscene clips). Study with `study-format` before guessing.
5. `.trmmt` (23): material sets / variants. Likely also unlocks Pokémon alternate colours (shiny).

## Phase 2: static field models (`field_graphic/...`)

- `field_graphic/terrain_model/objects_*`, `plants_*`, `ground_*` (many hundreds): buildings, rocks, trees, fences, with LODs
  (`_lod2`). Same `.trmdl` family, so mostly a discovery problem.
- `common_model/a_*`: interior furniture and rooms (academy `a_sch_*`, towns `a_t01_i01` = town 01 interior 01,
  cities `a_c01_*`, shared `a_com_*` houses/buildings/hair salons, DLC `a_w23_*`, `area23`).
- Shared texture folders: `glb_share_tex`, `tr_share_tex`, `common_share_tex`. Material texture paths must
  resolve into these, not only next to the model. Extend `model::resolve`.
- Vegetation needs alpha cut-out and two-sided rendering; lots of instances.

## Phase 3: scenes (placement) and interiors

- `.trscn` (153; `world/scene/parts/field/room/a_*` for rooms, `.../battlebg/` for battle backgrounds,
  `resident_event/` for gym courses), `.trsog` (2), `.trsot` (959; scene object templates?), `.trins` (99),
  `.trspb` (127). These place models with transforms. Decode one interior (`a_com_hairsalon01` or
  `a_sch_room01`) end to end first: list its objects, load each model, apply transforms.
- Lighting: `.trlgt` (358; `env/fel/fel_<room>` per room, `terrain_env/envter_*` per terrain tile) and the
  specular probe `.bntx` next to them. Start with the directional light and ambient only.
- Collision `.trcol` (19) is optional; useful as a debug overlay.
- Scene viewer UI: object list as a tree, click to select and frame, toggle categories, free-fly camera (WASD +
  mouse) since scenes are large.

## Phase 4: the overworld, Area Zero

- Terrain is tiled: `field_graphic/terrain/terrain_XX_YY/ter_XX_YY.trtrr` (`.trtrr` 38 packs,
  `terrain_` 137 packs) with `clod/clod_terrain_XX_YY_*` (334 packs; coarse LOD meshes, a good first target as
  they are ordinary `.trmdl`) and `clod_town_tNN_*` for towns.
- Load the `clod` meshes for the whole grid first: it gives a full-map view cheaply. Then `.trtrr` heightfield
  and splat materials for close-up detail, streamed by camera distance.
- Then place field objects on top from the world's scene/placement files (not found yet; candidates: `.trsot`,
  `.trins`, `world/data/...`).
- **Area Zero**: its asset prefix is not identified yet (no "zero"/"crater" in pack names). Find it by elimination
  over terrain tiles and area codes (the `a_w##` / `area##` style codes, field object groups) once the map grid
  renders. Record the answer in ARCHIVE_PATHS.md.
- Kitakami / Blueberry Academy DLC content is in `*_dlc1_*`, `*_dlc2_*` paths.

## Phase 5: polish

- Effects (`.ptcl`, `.traef`), emission/roughness, transparency sorting, material and face animations.
- Export (glTF) of whatever is loaded, for the user's own use. Keep exports out of the repo.
- Performance: GPU skinning, instancing for vegetation, texture cache across models.

## Open questions

- Which file ties terrain tiles, towns and object placements into one world?
- What are `.trmdt`, `.tracn`, `.tracp`, `.tracm`, `.trsot`, `.trins`, `.trspb`, `.trslp`, `.trcma`, `.imap`?
- How do trainer models reference the shared `cc_base` skeleton?
- Area Zero's code name.
