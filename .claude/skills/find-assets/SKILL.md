---
name: find-assets
description: List, count and search the Trinity archive's pack names to find where models, animations, scenes, terrain tiles or interiors live and to rebuild their real paths. Use before adding support for a new asset category or when looking for a specific character, area or file type.
---

# Find assets in the archive

The archive stores only path hashes. Pack names (`arc/<path without slashes>.trpak`) are the only path text, so
every new asset category starts here.

1. Overview (counts by prefix and extension; builds the cache `target/packnames.txt` on first run, ~10 s):

   ```
   python -I .claude/skills/find-assets/packnames.py
   ```

2. Search with a regex, optional limit:

   ```
   python -I .claude/skills/find-assets/packnames.py "charamodel_trtr" 20
   python -I .claude/skills/find-assets/packnames.py "terrainterrain_0[0-3]_" 
   python -I .claude/skills/find-assets/packnames.py "\.trscn\.trpak$" 200
   ```

   Delete `target/packnames.txt` if the user switches to a different dump.

3. Rebuild the real path: put the slashes back using the prefix table in
   `.claude/docs/ARCHIVE_PATHS.md`. The file name usually repeats inside the pack name
   (`...pm0025_00_00pm0025_00_00.trmdl` → folder `pm0025_00_00/`, file `pm0025_00_00.trmdl`).

4. Confirm the guess against the hash table. Use the probe, which prints the slots if the path exists and an
   error if not:

   ```
   cargo run --release --bin probe -- field_graphic/common_model/a_t01_i01/a_t01_i01.trmdl 1
   ```

   Or in code: `Archive::contains(path)`.

5. Record new prefixes, area codes and conventions in `.claude/docs/ARCHIVE_PATHS.md`.

Rules: never commit the name list or paste large parts of it into tracked files; summarise with counts and a
few examples instead.
