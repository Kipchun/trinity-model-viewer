---
name: run-viewer
description: Build, check and launch the Trinity model viewer, headless or with a window, to verify a change. Use after editing loaders, rendering, UI or animation code, or when asked to run or test the viewer.
---

# Run and verify the viewer

From the repository root.

1. Compile and lint:

   ```
   cargo fmt --all
   cargo clippy --locked --all-targets --all-features -- -D warnings
   ```

   These match the CI workflow; any warning fails CI.

   ```
   cargo check --release
   ```

2. Headless load of every model (no window; prints failures and a summary). Run it after any loader change; it
   reads the whole archive, so start it in the background and wait for the notification:

   ```
   cargo run --release -- --check
   ```

   Compare the failure count with the previous run noted in `.claude/docs/PROGRESS.md`.

3. Whole-dump format checks with the probe (list in the header of `src/bin/probe.rs`), e.g.
   `cargo run --release --bin probe -- --skeletons`, `--anims`, `--skin`.

4. Open a window on a specific case:

   ```
   cargo run --release -- --model pm0006_00_00 --anim walk01 --skeleton --lit
   ```

   The window is for the user to look at; ask them what they see, or use the `game-development` skill to capture
   a frame. Never save screenshots of game content into the repository.

The RomFS comes from the config file (`%APPDATA%\trinity-model-viewer\romfs_path.txt`), a folder argument, or
`TRINITY_ROMFS` (probe only). If none is set, ask the user for the path; do not search their drives.
