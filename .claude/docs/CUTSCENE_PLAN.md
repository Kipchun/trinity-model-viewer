# Plan: playing cutscenes

Status: planned 2026-11-18. C1 done 2026-11-18 (scenes in their own sub-scenes; cutscenes set in the field need
the overworld, see C1b). C2 done 2026-11-18 (cast, clips per cut, visibility, position, variants).

The Cutscenes tab lists every timeline (`.trtml`, see `timeline.rs`) and draws it. This plan plays one in the 3D
view, story events first (`demo/ev/dNNN`); battle timelines need the battle's actors and come last.

## What a story cutscene is made of (checked on d010)

- **Root scene** `world/scene/parts/demo/ev/<name>_/<name>_0.trscn`: the camera (`trinity_CameraEntity` with one
  `trinity_CameraAnimationComponent` per shot, data `{0: type name, 1: .trcma path}`), the cast
  (`<code>_Mod` objects with a `CharacterCreationMasterComponent` or `CharacterCreationComponent`, and
  `<code>_Anm` objects with a `pe_AnimationContainerComponent`), lights, UI layouts, effects, and `SubScene`s
  that are there from the start (`d010_bg_c01`, `d010_a_sch_room03`).
- **Sub-scenes** `world/scene/parts/demo/ev/<name>/<sub>_/<sub>_0.trscn`: rooms and buildings as plain
  `ModelComponent`s (interiors at the origin, outdoor parts at their place on the map, e.g. the academy near
  (2390, 166, -1890)), and more cast (`ti_PokemonModelComponent` for Pokémon). The timeline loads and ends them
  (`Scene/LoadSubScene` Path, `Scene/TerminateSubScene` SubSceneName).
- **Cameras** `.trcma` (60 fps): `{0: {1: frames, 2: fps}, 1: {0: name, 1: lens {near, far, fov (radians)} as
  (tag, value) pairs, 2: transform: pair 0 rotation (packed quaternions, `anim.rs` codec), pair 1 translation,
  pairs 2-4 zero in the files seen}}`. Same union tags as `.tranm` (1 fixed, 2 per frame, 3/4 keyed). Played by
  `CameraAnimation/Play` (`fileName` = the `.trcma` stem); a shot holds its last frame until the next.
- **Characters** `CharacterCreationMasterComponent` slot 15 `[{0: "default", 1: .ccdatam, 2: variant}]`; the
  `.ccdatam` maps variants to `.ccdata`; a `.ccdata` lists the base model (`model_cc_base/model/cc_base.trmdl`)
  and parts (`model_vr/...` `.trmdl` + `.trmmt`), body scalers and variants (`race00`..). Props:
  `CharacterCreationComponent` slot 0 = `.ccdata` with one model.
- **Character animation**: `pe_AnimationContainerComponent` slot 0 = `.tracn` files (body, then `_secondary` for
  the head). A `.tracn` names a `.tracp` (parameters / state machine?), a `.tracr` (clip list, one `.tranm` per
  cut: `d010_cc_anim_A_c003`) and a `.tracl` (layers: bone lists). The timeline picks the clip with
  `Animation/SetInt int_cutnum = k` (k-th clip of the `.tracr`, matching the camera shot that starts on the same
  frame), shows and hides with `Model/SetVisibility`, places with `GameObject/SetTranslation` (vec3).

## Steps

- **C1 scenes and camera** (done 2026-11-18): Play in the Cutscenes tab. Build every sub-scene of the root scene and
  of the timeline's `LoadSubScene`s as its own placed-model group (`plants::scene_items` / `build`), shown while
  loaded. Read the `.trcma` cameras (`camera_anim.rs`) and drive the view from the shot playing; free camera on a
  toggle. Play, pause, scrub, speed. Current message line shown over the view. Scenes the timeline ends or
  drives objects of but never loads (`d010_bg_c01`) are there from the start. Scenes build one per frame;
  playback waits for the one on screen. Seen right in d010: the academy front (frame 200), a classroom (500),
  the office (2406), the battle court (4700).
- **C1b field cutscenes**: some story events have no scenes of their own and play in the overworld (d200: camera
  at (2406, 139, -1680) in Mesagoza, only `tml_msg_demo`). Load the World tab's terrain and streamed scenes
  around the shots (`world::World`, `show_only` tiles); which map (Paldea, Kitakami, Terarium) is not in the
  timeline: guess from the number range (`d6xx` Teal Mask?) or the sub-scene names (`dlc1`).
- **C2 cast** (done 2026-11-18, `cast.rs`): `.ccdata` characters assembled on their default part's skeleton
  (`player::assemble_on`), Pokémon from the object's name, props; the variant's `vis_*` mesh groups hidden
  (`.trmmt`). Clips: the timeline's `ChangeCharaCreateAnimation` / `AddConfig` names the container and
  `.tracn`; per cut the clip named after the shot starting then (`c020`), else the k-th; `_loop` after it;
  Pokémon without clips play their own idle. Visibility and position from the timeline. Skinning moved to
  `skin.rs` so the model view and cutscenes share it. `probe --cast`: 220 actors in the 29 story events (165
  characters and props, 55 Pokémon), 2 fail (the player: needs the save's outfit), 5 without clips; 627 clips,
  all parse; 414 cuts picked by shot, 79 by number. Seen in d010: students at their desks facing the teacher
  (frame 500), the director holding a Poké Ball in his palm (2406).
- **C2b cast, next**: the player (`playables.Player` tracks; a default outfit from `player.rs`), heads' own clips
  (`_secondary.tracn`, `int_cutnum_head`), `GameObject/SetRotation` / `SetScale`, `AttachTransform` (props
  in hands), the version's colours (`.trmmt` version `.trmtr`), `discard_*` hair masks, Pokémon forms.
- **C3 more commands**: `GameObject/SetRotation`, `SetScale`, `AttachTransform`, fades (`Title/Fade/*`), the
  camera's `Camera/SetFovy`, lights (`Light/*`), particles (`Particle/Emit` with `effects`), version A / B.
- **C4 sound**: `Audio/PostEventGlobal` names Wwise events; play them through the Sound tab's player.
- **C5 battle timelines**: conditions, the battle's trainer and Pokémon, `CameraAnimation/Load` by path.
