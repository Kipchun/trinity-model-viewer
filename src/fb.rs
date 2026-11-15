//! Minimal, bounds-checked FlatBuffers reader. Trinity files (trpfd, trpfs, packs, trmdl, trmsh, trmbf) are all
//! FlatBuffers without schemas shipped next to them, so fields are read by slot index.

#![allow(dead_code)] // a small general reader; not every accessor is used yet

use anyhow::{Result, anyhow, bail};

pub fn u16_at(buf: &[u8], at: usize) -> Result<u16> {
    Ok(u16::from_le_bytes(get(buf, at, 2)?.try_into()?))
}
pub fn u32_at(buf: &[u8], at: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(get(buf, at, 4)?.try_into()?))
}
pub fn u64_at(buf: &[u8], at: usize) -> Result<u64> {
    Ok(u64::from_le_bytes(get(buf, at, 8)?.try_into()?))
}
pub fn get(buf: &[u8], at: usize, n: usize) -> Result<&[u8]> {
    buf.get(at..at.checked_add(n).ok_or_else(|| anyhow!("overflow"))?)
        .ok_or_else(|| {
            anyhow!(
                "read of {n} bytes at {at:#x} is past the end ({:#x})",
                buf.len()
            )
        })
}

#[derive(Clone, Copy)]
pub struct Table<'a> {
    pub buf: &'a [u8],
    pub pos: usize,
    vt: usize,
    slots: usize,
}

impl<'a> Table<'a> {
    /// The root table of a buffer that starts at `at`.
    pub fn root(buf: &'a [u8], at: usize) -> Result<Self> {
        Self::at(buf, at + u32_at(buf, at)? as usize)
    }

    pub fn at(buf: &'a [u8], pos: usize) -> Result<Self> {
        let soff = u32_at(buf, pos)? as i32 as i64;
        let vt = pos as i64 - soff;
        if vt < 0 {
            bail!("vtable before buffer start");
        }
        let vt = vt as usize;
        let vt_size = u16_at(buf, vt)? as usize;
        if vt_size < 4 {
            bail!("bad vtable size {vt_size}");
        }
        Ok(Table {
            buf,
            pos,
            vt,
            slots: (vt_size - 4) / 2,
        })
    }

    pub fn slot_count(&self) -> usize {
        self.slots
    }

    /// Absolute position of field `i`, if present.
    pub fn field(&self, i: usize) -> Option<usize> {
        if i >= self.slots {
            return None;
        }
        let o = u16_at(self.buf, self.vt + 4 + 2 * i).ok()? as usize;
        (o != 0).then_some(self.pos + o)
    }

    pub fn u8(&self, i: usize, def: u8) -> u8 {
        self.field(i)
            .and_then(|a| self.buf.get(a).copied())
            .unwrap_or(def)
    }
    pub fn i8(&self, i: usize, def: i8) -> i8 {
        self.u8(i, def as u8) as i8
    }
    pub fn u32(&self, i: usize, def: u32) -> u32 {
        self.field(i)
            .and_then(|a| u32_at(self.buf, a).ok())
            .unwrap_or(def)
    }
    pub fn u64(&self, i: usize, def: u64) -> u64 {
        self.field(i)
            .and_then(|a| u64_at(self.buf, a).ok())
            .unwrap_or(def)
    }
    pub fn f32(&self, i: usize, def: f32) -> f32 {
        self.field(i)
            .and_then(|a| u32_at(self.buf, a).ok())
            .map(f32::from_bits)
            .unwrap_or(def)
    }

    fn indirect(&self, i: usize) -> Result<Option<usize>> {
        match self.field(i) {
            None => Ok(None),
            Some(a) => Ok(Some(a + u32_at(self.buf, a)? as usize)),
        }
    }

    pub fn table(&self, i: usize) -> Result<Option<Table<'a>>> {
        self.indirect(i)?
            .map(|p| Table::at(self.buf, p))
            .transpose()
    }

    pub fn string(&self, i: usize) -> Result<Option<String>> {
        match self.indirect(i)? {
            None => Ok(None),
            Some(s) => Ok(Some(read_str(self.buf, s)?)),
        }
    }

    /// (start of elements, element count) of vector field `i`.
    pub fn vector(&self, i: usize) -> Result<(usize, usize)> {
        match self.indirect(i)? {
            None => Ok((0, 0)),
            Some(v) => Ok((v + 4, u32_at(self.buf, v)? as usize)),
        }
    }

    pub fn tables(&self, i: usize) -> Result<Vec<Table<'a>>> {
        let (start, n) = self.vector(i)?;
        (0..n)
            .map(|k| {
                let e = start + 4 * k;
                Table::at(self.buf, e + u32_at(self.buf, e)? as usize)
            })
            .collect()
    }

    pub fn strings(&self, i: usize) -> Result<Vec<String>> {
        let (start, n) = self.vector(i)?;
        (0..n)
            .map(|k| {
                let e = start + 4 * k;
                read_str(self.buf, e + u32_at(self.buf, e)? as usize)
            })
            .collect()
    }

    pub fn u64s(&self, i: usize) -> Result<Vec<u64>> {
        let (start, n) = self.vector(i)?;
        (0..n).map(|k| u64_at(self.buf, start + 8 * k)).collect()
    }

    pub fn bytes(&self, i: usize) -> Result<&'a [u8]> {
        let (start, n) = self.vector(i)?;
        get(self.buf, start, n)
    }
}

fn read_str(buf: &[u8], s: usize) -> Result<String> {
    let n = u32_at(buf, s)? as usize;
    Ok(String::from_utf8_lossy(get(buf, s + 4, n)?).into_owned())
}
