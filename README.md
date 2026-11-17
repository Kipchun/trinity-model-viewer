# Trinity Model Viewer

A desktop viewer for the 3D models of games built on Game Freak's Trinity engine (developed against
*Pokémon Scarlet & Violet*). It reads the game's packed archive directly and shows:

- **Models**: every Pokémon model in the archive, plus trainers, named characters and props (pick the list at the
  top of the panel), with all their parts and detail levels (LODs)
- **Textures**: colour and normal maps, including the layered tint colours the game uses for eyes, flames and bodies;
  see-through materials (eye lenses, glasses, visors) and cut-out hair and lashes
- **Skeletons**: the bone hierarchy, with bind and rest pose, drawn over or through the mesh
- **Animations**: all of a model's animations (idle, walk, run, battle moves and more), with the mesh skinned to the bones;
  crossfades between animations and blends two at once
- **Player character**: build the player from its parts (outfit, legwear, gloves, shoes, face, hair, headwear,
  eyewear, bag) and see them animated together with the player's own animations
- **Trainers and NPCs**: animated too, with their own animations or any of the 25 shared sets of body motions
- **Play mode** (F7): walk, run and jump the model around to see its animations hand over to each other
- **World**: the overworld's terrain with its ground textures, the whole island at once or tile by tile

**No game files are included.** You need your own dump of the game's RomFS, made from a copy you own.
The viewer reads it in place and never changes it.

## Getting started

1. Dump the game's RomFS. For example, in an emulator use "Dump RomFS" or "Extract RomFS" on your copy of the game
   (with the update applied, if you have one). The result is a folder that contains an `arc` folder with
   `data.trpfd` and `data.trpfs` (about 8 GB) in it.
2. Install Rust (<https://rustup.rs>) and build the viewer:

   ```
   cargo run --release
   ```

3. The first time, the viewer asks for your RomFS folder. Click **Choose RomFS folder…** and select the folder from
   step 1. Selecting the `arc` folder or `data.trpfs` itself also works. The viewer remembers the folder; use
   **Change RomFS folder…** at the bottom of the left panel to pick another.

## Using it

- **Left panel**: search and pick a model (e.g. `pm0025` or `0006`). Choose how it is drawn: textures, a colour per
  material, or plain, and lit softly from all sides (**Unlit**) or by one sun that casts shadows on the floor
  (**Lit**). Turn the mesh, floor and skeleton on and off, and pick the level of detail. The **Animation**
  section plays any of the model's animations, with play/pause, loop, a frame slider and speed. Switching animation
  crossfades from the current pose (the time is adjustable; 0 switches at once). **Blend with** mixes a second
  animation over the first with a weight slider; with **Sync** on, both stay at the same point of their cycle
  (useful for idle/walk/run mixes), otherwise the second plays at its own rate.
- **Player** (the last tab above the list): pick one part per slot from the drop-downs, or step through them with
  the arrow buttons; the character updates at once and keeps playing the current animation. **Outfit set** switches
  between the two sets of outfits (`p1` and `p2`). **Random** dresses the player at random, **Default** goes back.
  The outfit, legwear and gloves are also the body (bare arms, legs and hands are parts too), so leaving them out
  leaves gaps.
- **Trainers**: trainers and NPCs play their animations like Pokémon. Many have their own set; the others use
  one of 25 shared sets of body motions (child, adult man or woman, and several styles), and **Body motions** in
  the Animation section switches between them. Any set plays on any trainer. Trainers with special actions
  (karate, painting, ...) list those too. NPCs from `model_vr` come in parts (body, head, ...), each shown alone
  on the shared NPC skeleton.
- **Play mode** (F7, or the button in the left panel): control the model like a game character. WASD or the arrow
  keys move it relative to the camera, Shift runs, Space jumps, F7 or Esc leaves. Idle, walk and run are blended
  by speed, with walk and run kept in step; the speeds come from the clips' own root motion, so the feet do not
  slide. A jump plays take-off, rising, fall and landing in turn, with the crossfade between each. The panel shows
  the state, speed and blend weights, and which clips the model provides.
- **World** (the tab after Player): **Map** switches between Paldea, Kitakami (The Teal Mask) and the Terarium
  (The Indigo Disk). **Rooms and interiors** lists the game's 151 room scenes (houses, shops, gyms, academy
  rooms, battle backgrounds); search and click one to show it.
  The overworld view: the map's terrain, 10 x 10 tiles of 500 m, coloured by height. Tiles load
  one by one when the tab opens. **Ground** switches between the game's ground textures (grass, rock, sand,
  snow, paths, blended as the game's splat maps say) and colours by height. Click a tile in the minimap to hide or
  show it; **Detail** sets how many height
  samples are used (Full is about 0.98 m between samples and heavy for the whole map). **Sea** and **Rivers and
  lakes** show the game's water: the sea at 27 m with its coast waves, and every river, lake, pond and waterfall,
  with scrolling ripples and foam (an approximation of the game's water shader). Area Zero is a hole in the
  terrain: it has its own models, which are not placed yet.
- **Textures** (the tab after World): every texture and sprite in the game, in groups: **UI & icons** (item, type,
  ribbon and move icons, map pictures, picnic menus, ...), **Pokédex** (capture pictures), **Pokémon** (menu icons
  and every model's textures), **Characters**, **Field**, **Effects**, **Environment** and **Cutscenes**. Pick a
  folder on the left (the search finds folders and file names, e.g. `pm0025` or `item_0001`); its textures show as
  a grid. A folder's count ends in `+` until it is opened, because many textures are only found by reading the
  folder's packs. Click a texture to see its size and format on the right, double-click to view it large: Fit,
  1:1 or any zoom (also Ctrl + wheel), the colour channels alone (R, G, B, A), a checker, dark or light
  background, sharp or smooth enlargement, and the layers of array textures. The arrow keys step through the
  folder, Esc goes back to the grid.
- **Sound** (the tab after Textures): the game's music and sound effects, 17,512 sounds in groups: **Music**, **Jingles**,
  **Pokémon cries**, **Pokémon sounds**, **Moves**, **Battle**, **Menus**, **Player**, **People**, **Objects**,
  **Ambience**, **Cutscenes** and **Common**. Sounds are listed by id (the number Wwise tools use for `.wem` file
  names) with length, channels, rate, codec and a name where one is known: the song titles from the game's own
  music menus (the club room speaker and the battle music selector: "Mesagoza", "Gym Leader", ...), the species
  for Pokémon cries ("Pikachu (0025)"), and otherwise the developers' names from the game's sound tables
  (`ONIBALLOON_CLEAR_GAME`, `WEST_MOUNTAIN`, ...). Type your own name for any sound in **Your name** on the
  right; it is kept in `sound_names.txt` in the viewer's settings folder. **Find** searches ids and names,
  **Named only** hides the rest; **Sort** by id or longest first. Click a sound to play it (or turn off **Play on click**); Up/Down step through the list, Space plays
  and pauses, and a click on the waveform jumps there. **Loop** repeats the sound, between its loop points where it
  has them (most music loops through the game's music playlists, which are not read, so it repeats whole).
  **Save as WAV…** writes the decoded sound wherever you choose. Sound keeps playing when you switch tabs.
- **Effects** (the last tab): a list of the game's particle effects (428, the battle, raid, field, menu and
  cutscene effects) and of the effect lists attached to Pokémon and character animations (518). Click a column
  title to sort by name, kind, group, folder or size (click again to reverse); filter by kind and group, or type
  in **find**. Double-click a particle effect (or pick it and press **View**) to play it in the 3D view: Play /
  Pause, Restart, Loop and speed on the left, and a checkbox per emitter to see what each part does; **Back to
  list** returns to the table. Effects look close to the game, not exact: parts drawn with meshes show as flat
  squares, and the game's colour mixing is approximated.
- **Right panel**: the model's files, parts and materials, and its skeleton as a tree; click a bone to highlight it
  and see its transform.
- **Mouse**: left drag orbits, right drag pans, the wheel zooms.

Command-line options:

| Option | Effect |
| --- | --- |
| `<folder>` | use this RomFS folder (and remember it) |
| `--model pm0006_00_00` | start with this model |
| `--player` | start with the player character |
| `--world` | start in the World tab (overworld terrain) |
| `--tiles 05_03,06_03` | start in the World tab showing only these terrain tiles |
| `--map Kitakami` | start in the World tab on a map: `Paldea`, `Kitakami`, `Terarium`, or a room scene's path |
| `--sound [group]` | start in the Sound tab, optionally on a group (e.g. `music`, `cries`) |
| `--effects [name]` | start in the Effects tab, optionally playing the first particle effect whose name contains this text (e.g. `ee003_star`) |
| `--textures [folder]` | start in the Textures tab, optionally on the first folder whose path contains this text (e.g. `icon_item`) |
| `--anim walk01` | start the first animation whose name contains this text |
| `--look head` | put the camera close in front of this bone (first bone whose name contains the text) |
| `--lit` | start in Lit mode (one sun with shadows) |
| `--skeleton` | start with the skeleton shown |
| `--check` | load every model and terrain tile (with its ground textures) once and report failures, without opening a window |

For example: `cargo run --release -- --model pm0006_00_00 --anim walk01 --skeleton`.

## What it understands

| File | Content |
| --- | --- |
| `arc/data.trpfd`, `arc/data.trpfs` | the archive: file table, packs, Oodle Kraken compression |
| `.trmdl`, `.trmsh`, `.trmbf` | model, mesh layout, vertex and index buffers |
| `.trmtr` | materials: textures by role, layer colours, UV transforms |
| `.bntx` | textures (BC1–BC7, ASTC, 8-bit and float formats; Tegra block-linear layout) |
| `.trskl` | skeletons and inverse bind matrices; player parts' bones attached to the base skeleton |
| `_base.tracr`, `.tranm` | a model's animation list and its skeletal animations |
| `audio/wwise/.../*.bnk`, `*.pck`, `.wem` | Wwise sound banks and packages; sounds in PTADPCM, Wwise Vorbis, Switch Opus and PCM |
| `.trtrr` and its height file | overworld terrain tiles: heights and splat ground textures (grass and placed objects not yet) |

The source files start with the field layouts they read, as checked against the game's files.

Not yet supported: emission, roughness and special effect shaders (Pokémon eye highlights are an approximation of the
game's); face and material animations (blinking, eye textures); alternate colour sets (shiny) and the player's
clothing colour variants; everything placed on the map; scenes and interiors.

## Development

`cargo run --release --bin probe -- …` is a small tool for studying the formats: it prints the field layout of any
file in the archive and runs checks over the whole dump (skeleton maths, skin weights, animation parsing). It uses
the folder the viewer remembers, or the `TRINITY_ROMFS` environment variable. Its options are listed at the top of
`src/bin/probe.rs`.

Never commit anything from a game dump. `.gitignore` excludes the usual dump folders and file types.

## Licence

MIT; see [LICENSE](LICENSE). Third-party libraries and their licences are listed in
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

This project is not affiliated with or endorsed by Nintendo, Game Freak, Creatures or The Pokémon Company. All game
content belongs to its owners and is not part of this repository.
