# Third-party notices

The viewer is built from these crates (direct dependencies; versions as locked in `Cargo.lock`). Their own
dependencies come with their licences in the usual places; `cargo tree` lists them.

| Crate | Version | Licence | Source | Used for |
| --- | --- | --- | --- | --- |
| three-d | 0.19.0 | MIT | <https://github.com/asny/three-d> | window, OpenGL rendering, egui integration |
| oozextract | 0.5.7 | MIT | <https://github.com/lvlvllvlvllvlvl/oozextract> | Oodle Kraken decompression of the archive |
| texture2ddecoder | 0.1.2 | MIT OR Apache-2.0 | <https://github.com/UniversalGameExtraction/texture2ddecoder> | BC1–BC7 and ASTC texture decoding |
| rfd | 0.18.0 | MIT | <https://github.com/PolyMeilex/rfd> | native folder dialog |
| serde_json | 1.0.154 | MIT OR Apache-2.0 | <https://github.com/serde-rs/json> | reading an optional RomFS index file |
| anyhow | 1.0.104 | MIT OR Apache-2.0 | <https://github.com/dtolnay/anyhow> | error handling |

No code from these projects is copied into this repository; they are used as crates.
