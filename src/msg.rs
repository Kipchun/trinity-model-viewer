//! The game's text: `message/dat/<language>/<folder>/<name>.tbl` (labels) and `.dat` (lines), e.g.
//! `message/dat/English/common/monsname` (species names by National Dex number: `MONSNAME_025` "Pikachu").
//!   .tbl  "AHTB", u32 count, then per label: u64 hash, u16 length, the label's bytes (with a final 0).
//!   .dat  u16 sections, u16 line count, u32 total length, u32 key, u32 section offset; at the section: u32
//!         length, then per line u32 offset (from the section), u16 length in UTF-16 units, u16 unused. Each
//!         line is UTF-16 LE, XORed with a key starting at 0x7C89 + 0x2983 x line index and rotated left by 3
//!         bits per character. 0x0010 starts a variable (u16 count of parameters follows), 0xE07F is a space,
//!         0xBE00 / 0xBE01 are line / page breaks.

use crate::archive::Archive;
use crate::fb::{u16_at, u32_at};
use anyhow::{Result, bail};

/// Every (label, text) of a message file. `name` is `<folder>/<file>`, e.g. `common/monsname`.
pub fn read(ar: &mut Archive, language: &str, name: &str) -> Result<Vec<(String, String)>> {
    let base = format!("message/dat/{language}/{name}");
    let tbl = ar.read(&format!("{base}.tbl"))?;
    let dat = ar.read(&format!("{base}.dat"))?;
    let labels = labels(&tbl)?;
    let lines = lines(&dat)?;
    Ok(labels.into_iter().zip(lines).collect())
}

fn labels(b: &[u8]) -> Result<Vec<String>> {
    if !b.starts_with(b"AHTB") {
        bail!("not a label table");
    }
    let n = u32_at(b, 4)? as usize;
    let mut p = 8;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let len = u16_at(b, p + 8)? as usize;
        let s = b.get(p + 10..p + 10 + len).unwrap_or(&[]);
        out.push(
            String::from_utf8_lossy(s)
                .trim_end_matches('\0')
                .to_string(),
        );
        p += 10 + len;
    }
    Ok(out)
}

fn lines(b: &[u8]) -> Result<Vec<String>> {
    let count = u16_at(b, 2)? as usize;
    let section = u32_at(b, 0x0C)? as usize;
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let e = section + 4 + 8 * i;
        let off = u32_at(b, e)? as usize;
        let len = u16_at(b, e + 4)? as usize;
        let mut key = (0x7C89u32 + 0x2983 * i as u32) as u16;
        let mut chars = Vec::with_capacity(len);
        for k in 0..len {
            chars.push(u16_at(b, section + off + 2 * k)? ^ key);
            key = key.rotate_left(3);
        }
        let mut s = String::new();
        let mut j = 0;
        while j < chars.len() {
            match chars[j] {
                0 => break,
                0x10 => {
                    // a variable: skip it and its parameters
                    let n = chars.get(j + 1).copied().unwrap_or(0) as usize;
                    j += 2 + n;
                    continue;
                }
                0xE07F => s.push(' '),
                0xBE00 | 0xBE01 => s.push(' '),
                c => s.extend(char::decode_utf16([c]).map(|r| r.unwrap_or('?'))),
            }
            j += 1;
        }
        out.push(s.trim().to_string());
    }
    Ok(out)
}
