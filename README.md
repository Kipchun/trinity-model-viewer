# Trinity Model Viewer

A desktop viewer for the 3D models of games built on Game Freak's Trinity engine (developed against
*Pokémon Scarlet & Violet*). It reads the game's packed archive directly and shows:

- **Models**: every Pokémon model in the archive, with all its parts and detail levels (LODs)
- **Textures**: colour and normal maps, including the layered tint colours the game uses for eyes, flames and bodies
- **Skeletons**: the bone hierarchy, with bind and rest pose, drawn over or through the mesh
- **Animations**: all of a model's animations (idle, walk, run, battle moves and more), with the mesh skinned to the bones;
  crossfades between animations and blends two at once
- **Play mode** (F7): walk, run and jump the model around to see its animations hand over to each other

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
- **Play mode** (F7, or the button in the left panel): control the model like a game character. WASD or the arrow
  keys move it relative to the camera, Shift runs, Space jumps, F7 or Esc leaves. Idle, walk and run are blended
  by speed, with walk and run kept in step; the speeds come from the clips' own root motion, so the feet do not
  slide. A jump plays take-off, rising, fall and landing in turn, with the crossfade between each. The panel shows
  the state, speed and blend weights, and which clips the model provides.
- **Right panel**: the model's files, parts and materials, and its skeleton as a tree; click a bone to highlight it
  and see its transform.
- **Mouse**: left drag orbits, right drag pans, the wheel zooms.

Command-line options:

| Option | Effect |
| --- | --- |
| `<folder>` | use this RomFS folder (and remember it) |
| `--model pm0006_00_00` | start with this model |
| `--anim walk01` | start the first animation whose name contains this text |
| `--lit` | start in Lit mode (one sun with shadows) |
| `--skeleton` | start with the skeleton shown |
| `--check` | load every model once and report failures, without opening a window |

For example: `cargo run --release -- --model pm0006_00_00 --anim walk01 --skeleton`.

## What it understands

| File | Content |
| --- | --- |
| `arc/data.trpfd`, `arc/data.trpfs` | the archive: file table, packs, Oodle Kraken compression |
| `.trmdl`, `.trmsh`, `.trmbf` | model, mesh layout, vertex and index buffers |
| `.trmtr` | materials: textures by role, layer colours, UV transforms |
| `.bntx` | textures (BC1–BC7, ASTC; Tegra block-linear layout) |
| `.trskl` | skeletons and inverse bind matrices |
| `_base.tracr`, `.tranm` | a model's animation list and its skeletal animations |

The source files start with the field layouts they read, as checked against the game's files.

Not yet supported: emission, roughness and transparency effects; face and material animations (blinking, eye
textures); alternate colour sets (shiny); models other than Pokémon.

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
