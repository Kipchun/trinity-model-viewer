# Plan: play mode inside scenes

Goal: in play mode (F7), pick a place (a room, a town, a stretch of the overworld) and run a Pokémon or a human
character around in it: walking on the floor, up slopes and steps, against walls, off ledges and back down.

Status: planned (2026-11-15). Nothing written yet.

## Where play mode is today

`src/demo.rs` (character controller) and `DemoFloor` in `main.rs`:
- The ground is a flat checker floor. "On the ground" means `pos.y == 0` (`Demo::update`, the jump state
  machine lands at `pos.y <= 0.0`).
- Nothing blocks movement; the character can walk anywhere.
- The camera follows only horizontally, on purpose, so jumps show against the view.
- Clips are picked by Pokémon labels (`defaultwait01_loop`, `walk01_loop`, ...) in `demo::pick`.
- Character size comes from the model's bounds (`height`, `radius`).

The locomotion itself (blend space, root-motion speeds, jump states, crossfades) is the part to keep. What
changes is what the character stands on and bumps into.

## Decisions

- **Collision from the visible triangles first.** `.trcol` exists in only 19 packs, so most places won't have
  one, and its format is unknown. The scene's own render meshes give a usable collision world everywhere:
  - triangles whose normal points up (slope under ~50°) are floor
  - steeper triangles are walls
  - very small objects and alpha cut-out meshes (grass, leaves) are skipped.
  
  If `.trcol` or a terrain heightfield is decoded later, it replaces this per place.
- **Character as a capsule:** radius and height from the model's bounds (already computed for the floor), with a
  step height (about 30% of the radius) so it walks up stairs and kerbs.
- **Movement:**
  - slide along walls instead of stopping dead
  - snap to the floor while walking down slopes
  - walking off a ledge with no floor within step height enters the falling states the jump already uses
    (Fall start, then Falling, then Landing)
  - landing happens on the floor's height, not at 0.
- **Same units, no guessing.** First check that characters and places share a unit by putting a Pokémon next to
  a door or a chair in an interior. If they don't, fix one scale factor here and write it down. Pokémon may also
  have an in-world scale in `.trpokecfg`; check that before scaling by hand.
- **A `World` the controller asks, not a flat number:** `ground(x, z, from_y) -> Option<f32>` and
  `move_capsule(pos, delta) -> pos`. The checker floor becomes one implementation, the scene collision another.
  `demo.rs` stays free of rendering code.

## Steps

### S0: separate the ground from the controller (can start now)
- Add a `World` trait (in `src/world.rs` or `demo.rs`). Implement `FlatWorld`, which behaves exactly as today.
- `Demo::update` takes `&dyn World`. Every `pos.y == 0` / `pos.y <= 0.0` goes through it. A transition from
  Ground to Fall start/Falling for walking off ledges is added (never triggered on a flat world).
- No visible change. Play mode must feel identical.

### S1: a single big model as the world (can start now, no scene format needed)
Many places are already plain `.trmdl` files that load with the current code. Examples from ARCHIVE_PATHS.md:
- an interior: `field_graphic/common_model/a_t01_i01/a_t01_i01.trmdl`
- the coarse overworld tiles: `clod_terrain_XX_YY_*`.

So the first playable "scene" is just one of those.
- Play mode gets a **World** picker: Checker floor / Model from the archive (search box over the static models
  the index knows).
- Material textures for these models live in shared folders (`glb_share_tex`, ...). This needs the
  `model::resolve` extension from roadmap Phase 2. Until then, untextured is fine for walking around.
- Render the world model with the character. The checker floor is hidden.

### S2: collision from triangles
- Build a uniform grid (or a simple BVH) over the world's triangles once, at load time, split into floor and
  wall triangles by normal.
- Ground query: a ray down from the capsule's step height; take the highest floor hit below.
- Walls: a capsule against nearby triangles, push out along the contact normal, up to 3 iterations, then slide
  along what remains.
- Fall below the world's bounding box: respawn at the spawn point (also on `R`).
- Probe check: load every interior model and report its triangle count and grid build time, so slow or huge
  worlds show up early.

### S3: camera for places
- Follow vertically as well, smoothed, so stairs and slopes don't jolt the view. During a jump it still holds
  height until landing, as today.
- Pull the camera in when a wall is between it and the character (the same ray test as the ground query).
- Near and far planes from the world's size, not the character's, so big places don't clip.
- Optional free-fly camera (from roadmap Phase 3) to look around and then drop the character where the camera
  points.

### S4: spawn and controls
- Spawn: the centre of the world's bounds, dropped onto the highest floor below. Or click a floor point in the
  view to place the character there.
- UI in play mode: world name, the character's position, a "Respawn" button, and a toggle to draw the collision
  triangles (floor green, walls red) to debug S2.

### S5: real scenes (after roadmap Phase 3, `.trscn`)
- The world is the scene's placed models with their transforms.
- Collision is built from all of them, in world space.
- Skip non-solid objects by name or size: small props, vegetation, effects. Use `.trcol` where one exists once
  it is understood.
- Lighting from `.trlgt` when Phase 3 has it. Until then, the viewer's lights.
- Start with one interior end to end (the hair salon or a classroom, as the roadmap says), then the academy and
  Pokémon Centers.

### S6: human characters (after roadmap Phase 1)
- Trainer and player locomotion clips probably sit under `chara/motion/` (`.tranm` and possibly `.trmdt`). Their
  labels differ from the Pokémon ones, so `ROLE_INFO` becomes one table per character family (Pokémon, player,
  trainer).
- Humans may have no jump clips. Play mode already copes with a missing role; check that a missing jump means
  Space does nothing rather than a pose glitch.
- The player character is the assembled outfit (all parts on one skeleton). Play mode moves that whole set.

### S7: the overworld (after roadmap Phase 4)
- Stream tiles around the character: full models within a radius, `clod` meshes further out, unload behind.
  Collision only for the loaded close tiles.
- Ground from the `.trtrr` heightfield once decoded (exact and cheap: no ray needed). Field objects from the
  placement files on top.
- Area Zero is a later target once its tiles are identified.

## Risks and open questions
- **Scale between characters and places** (see Decisions). Checked in S1, before anything else depends on it.
- **Big Pokémon** (Wailord-sized) won't fit indoors. Allow a size override in play mode rather than refusing.
- **Collision from render meshes isn't perfect.** Invisible walls, glass, railings and doorway thresholds won't
  behave like the game's own collision. Good enough for running around; `.trcol` and the terrain data are the
  real fix.
- **Performance:** large scenes with lit shadows and CPU skinning. A draw distance and the roadmap's instancing
  and GPU-skinning work help here.
- **Floors with holes:** interiors may have open doorways onto nothing. The respawn handles falling out.

## Rough cost
- S0: half a day.
- S1–S4: 3–5 days. Playable in one interior or overworld tile with the current loaders, plus the shared-texture
  path work from Phase 2.
- S5–S7: mostly the roadmap phases they wait on. The play-mode part of each is a day or two.
