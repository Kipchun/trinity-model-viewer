# Plan: glTF export

Goal: export what the viewer loads to `.glb` (binary glTF), either **one model** (the one on screen) or **a whole
category** in bulk (Pokémon, Characters, Props, Scenes, ...). Everything goes into an `Exports/` folder that git
never sees.

Status: planned (2026-11-15). Nothing written yet.

## Decisions

- **Format: `.glb` only.** One self-contained file per asset: JSON, binary buffer and PNG textures embedded. No
  loose `.gltf` + `.bin` + `.png` sets, which are harder to keep track of and to keep out of the repo.
- **Output folder: `Exports/`** in the working directory (the repo root when run with `cargo run`). The UI shows
  the full path and has an "Open folder" button. `--out <dir>` overrides it; the UI choice is remembered in the
  config folder (`%APPDATA%\trinity-model-viewer\export_path.txt`), as `config.rs` does for the RomFS path.
- **Layout:** `Exports/<Category>/<asset name>/<asset name>.glb`, plus `Exports/export_log.txt` for bulk runs.
  Example: `Exports/Pokemon/pm0025_00_00/pm0025_00_00.glb`. One folder per asset leaves room for options that
  write several files later (for example, one file per clip).
- **Animations go in the model's file**, one glTF animation per `.tranm`, named after its clip. The single-model
  export lets you pick which clips to include (default: all). Bulk export has a switch: all clips / none.
- **LOD 0 only** by default (most detailed). Other LODs are not worth the size.
- **Materials are baked:** the albedo and normal map that `materials.rs` already produces, tinting included.
  The raw `.trmtr` parameters (layer colours, texture roles, render type) go in the material's `extras` so the
  original shader can be rebuilt later.
- **Hand-written glTF** with `serde_json` (already a dependency) and `png` (already in `Cargo.lock` through
  three-d; make it a direct dependency). No glTF crate is needed. Revisit if the writer grows past ~500 lines.

## Keeping exports out of the repo

Added to `.gitignore` in step E0, before anything is exported:

```
# glTF exports: decoded game assets, never commit
/Exports/
/exports/
*.glb
*.gltf
```

Both spellings, because Windows ignores case but git may not. The extension rules also catch exports written
anywhere else in the tree with `--out`. CI could add a check that fails if any `*.glb` or `*.gltf` is tracked.

This prevents publishing exports by accident. It does not change what the files are: decoded game assets for
the user's own use, the same as what the viewer shows on screen. The README should say so next to the export
instructions.

## Steps

Each step ends with `cargo fmt --all -- --check`, `cargo clippy --locked --all-targets --all-features -- -D warnings`,
and an update to PROGRESS.md.

### E0: groundwork
- `.gitignore` rules above.
- New module `src/export.rs` (split into `src/export/` if it grows): a GLB writer that builds a buffer with
  4-byte-aligned buffer views and accessors with min/max on positions, plus a function that picks the output path.
- Export path in `config.rs`.

### E1: static model
- For each `SubMesh`: one primitive with `POSITION`, `NORMAL`, `TEXCOORD_0` and u32 indices. Group primitives
  by `.trmsh` shape into glTF meshes.
- UVs: apply `UVScaleOffset` but **do not** flip V (glTF uses the same top-left origin as the game; only the
  OpenGL path in `main.rs` flips it).
- Materials: PBR metallic-roughness with the baked albedo (PNG embedded), the normal map, and `alphaMode: MASK`
  when `Baked::cutout` is set. Metallic is 0 and roughness is about 0.8 until those maps are decoded.
- Done when a Pokémon without a skeleton looks the same in Blender as in the viewer.

### E2: skeleton and skin
- One node per `Bone` with local TRS. Convert the Euler rotation to a quaternion in the documented order
  (`Rz * Ry * Rx`). Children follow the bone tree.
- `skin.joints` in joint-index order, `inverseBindMatrices` from `Skeleton::inverse_bind`. glTF keeps the rest
  pose (node TRS) separate from the inverse bind matrices, so the 18 Pokémon whose two poses differ need no
  special handling.
- Joint indices that point to a missing bone, or gaps in the joint list: map them to the root and log a
  warning. Normalise weights so they sum to 1 (validators require it), and set the weight to 0 on unused slots.
- Done when a Pokémon in its bind pose looks right in Blender, including at least two of the 18 Pokémon whose
  poses differ.

### E3: animations
- Make `anim::Track` readable from outside `anim.rs` (an accessor that returns key times and values, so the
  enum stays private).
- Per bone track, write up to three channels (translation, rotation, scale):
  - Fixed: a single key.
  - Dynamic: one key per frame.
  - Keyed: its own key times, `LINEAR` interpolation.
  - Key times are `frame / fps`.
- Bones without a track get no channel. glTF then keeps the node's rest TRS, which matches the viewer.
- Root motion on `origin` is exported as is.
- Done when idle, walk and a non-looping clip play in Blender as they do in the viewer, with no drift or
  flipped rotations.

### E4: single-model export (UI + CLI)
- UI: an "Export…" button in the model panel. Its dialog has a clip checklist and an output path, then a
  result line ("Saved to …", with any warnings).
- CLI: `--export-model <archive path> [--out dir] [--no-anims]`, headless.

### E5: bulk export by category
- CLI: `--export <category|all> [--out dir] [--no-anims] [--overwrite]`, headless, like `--check`.
- UI: an "Export category" action. It runs on a background thread with **its own `Archive`** (opened
  read-only, so the viewer's archive stays free), shows progress (n / total and the current asset), and can
  be cancelled.
- Resumable: skip assets whose `.glb` already exists unless `--overwrite`.
- A failure skips that asset, never stops the run. `export_log.txt` lists each failure and warning, plus a
  summary ("958 of 976 exported, 18 with warnings").
- Categories come from the Phase 0 asset index (ROADMAP.md). Until that exists, bulk export offers **Pokémon**
  only (`Archive::pokemon_models`). Each new category appears once its loader passes `--check`.

### E6: checking it
- Run [glTF-Validator](https://github.com/KhronosGroup/glTF-Validator) (a local binary, offline) over a bulk
  Pokémon export. Count files with errors and fix until 0. Record the result here.
- Spot-check in Blender: a few ordinary Pokémon, two whose poses differ, one with cut-outs, one with layer
  tinting.
- Check units and orientation. glTF is metres and +Y up. If models come out 100× too big, add a scale on a
  root node and note the game's unit here.

### E7: Characters and props (after roadmap Phase 1)
- Props (`ob####`) should need nothing new.
- Trainers and the player: export the assembled character (all chosen parts on one skeleton) as one `.glb`.
  Bulk export uses each character's default outfit.
- Character animation formats (`.trmdt`, `.tracn`) are exported once they are decoded.

### E8: Scenes and maps (after roadmap Phases 3–4)
- A scene becomes one `.glb`: one node per placed object with its transform. Each distinct model is written
  once and reused by every node that places it, so instanced trees and rocks don't make the file huge.
- Terrain tiles: one `.glb` per tile under `Exports/Terrain/`, plus an optional single merged file for the
  coarse `clod` map.
- Lights: optional `KHR_lights_punctual` for the directional light once `.trlgt` is understood.

## Not covered yet (needs format work first)
Face and material animations (blinking, eye textures), shiny / alternate colour sets (`.trmmt`), emission,
roughness, and transparency effects. glTF can carry all of these (material variants via
`KHR_materials_variants`, emission natively). They can be exported once they are decoded.

## Rough cost
E0–E4: 3–5 days. E5–E6: 1–2 days. E7 and E8 mostly reuse the same code; their cost is in the roadmap phases
they depend on.
