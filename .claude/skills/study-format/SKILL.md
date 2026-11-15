---
name: study-format
description: Reverse engineer a Trinity file format (.trscn, .trtrr, .trmdt, .trlgt, .trmmt, ...) from the user's own dump with the probe tool, then document and load it. Use when a needed file type is unknown or only partly understood.
---

# Study a Trinity file format

Trinity files are FlatBuffers without schemas; fields are read by slot index with `src/fb.rs`.

1. **Check for schemas first.** The archive has a few `.bfbs` files (binary FlatBuffers schemas). Find them with
   the `find-assets` skill (`"\.bfbs\.trpak$"`); if one covers the format, its field names and types settle it.
   Also check `.claude/docs/FORMATS.md` and the module doc comments for what is already known.

2. **Find samples.** Use `find-assets` to pick 3–5 files of the type from different categories (small and large,
   simple and complex). Rebuild and confirm their paths.

3. **Dump slots.** For each sample:

   ```
   cargo run --release --bin probe -- <archive path> 3
   ```

   Each slot prints as u32 / f32 / string / vector length, with nested tables. Compare samples side by side:
   constant slots are versions or flags, slots whose vector length matches another file's count are references,
   floats in groups of 3/4 are vectors, quaternions or colours, strings ending in a known extension are links.

4. **Write the layout down** as the doc comment at the top of the module that will read it, in the existing
   style (`name { slot: meaning, ... }`), marking guesses with `?`. Update `.claude/docs/FORMATS.md`.

5. **Prove it over the whole dump.** Add a probe mode (`--<format>`) in `src/bin/probe.rs` that parses every
   file of the type it can find and prints failures and counts; list it in the probe's header comment. Run it in
   the background (it takes minutes). Put the result into the doc comment ("all 153 scenes parse").

6. **Load it** in the viewer, keeping parsing bounds-checked (`anyhow`, no panics, no unchecked indexing).
   Extend `--check` to cover the new type. Run `cargo clippy --release`.

7. Update `.claude/docs/PROGRESS.md` and, when user-visible, the README table "What it understands".

For native code questions (how the game itself reads a format), the `reverse-engineer-anything` skill can
analyse the user's own game executable, but only if the user provides it and asks.
