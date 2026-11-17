//! Wwise `.wem` sounds (RIFF) to 16-bit PCM. The codecs Scarlet/Violet uses (bank version 140, by fmt tag):
//!   0x8311 PTADPCM   Platinum's ADPCM; frames of `block_align / channels` bytes per channel, interleaved:
//!                    s16 hist2, s16 hist1, u8 table index, then nibbles (low first). Most music and voices.
//!   0xFFFF Vorbis    Wwise Vorbis (fmt extra 0x30): stripped setup with external aoTuV 6.03 codebooks, 2-byte
//!                    packet headers, "modified" packets without the mode/window bits. Rebuilt into standard
//!                    Vorbis packets (ww2ogg's method) and decoded with lewton. Most sound effects.
//!   0x3039 Opus NX   Switch Opus (fmt size 0x28): seek table, then frames of u32 BE size, 4 unused bytes, the
//!                    Opus packet. Always 48 kHz out (libopus). Ambience loops.
//!   0xFFFE PCM       16-bit little-endian.
//! fmt: 0x00 tag, 0x02 channels, 0x04 rate, 0x0C block align, 0x0E bits, 0x10 extra size; Vorbis extra at 0x18:
//! +0x00 total samples, +0x10 seek table size, +0x14 audio offset (both within data), +0x28 / +0x29 the small and
//! large block size exponents. Opus NX: 0x18 samples, 0x24 seek table size.
//! Loop points: the `smpl` chunk (one loop of type 0: start at +0x2C, end at +0x30, inclusive).
//! The PTADPCM table and the Vorbis rebuild follow vgmstream and ww2ogg (BSD licence, Adam Gashlin / Xiph.org;
//! the codebook file `assets/packed_codebooks_aoTuV_603.bin` comes from ww2ogg).

use crate::fb::{u16_at, u32_at};
use anyhow::{Context, Result, anyhow, bail};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Codec {
    Pcm,
    PtAdpcm,
    Vorbis,
    OpusNx,
    Other(u16),
}

impl Codec {
    pub fn label(self) -> String {
        match self {
            Codec::Pcm => "PCM".into(),
            Codec::PtAdpcm => "PTADPCM".into(),
            Codec::Vorbis => "Vorbis".into(),
            Codec::OpusNx => "Opus".into(),
            Codec::Other(t) => format!("codec {t:#06x}"),
        }
    }
}

/// What a `.wem`'s header says (no decoding).
#[derive(Clone, Debug)]
pub struct Header {
    pub codec: Codec,
    pub channels: u16,
    pub rate: u32,
    /// Samples per channel, as the header gives or implies them.
    pub samples: u32,
    /// Loop start and end (end exclusive), in samples, from `smpl`.
    pub looping: Option<(u32, u32)>,
    fmt_at: usize,
    fmt_size: usize,
    block_align: u16,
    bits: u16,
    data_at: usize,
    data_size: usize,
}

impl Header {
    pub fn seconds(&self) -> f32 {
        self.samples as f32 / self.rate.max(1) as f32
    }
}

/// Reads the header from the start of a `.wem` (`b` may be cut short after the data chunk's header).
pub fn header(b: &[u8]) -> Result<Header> {
    if !b.starts_with(b"RIFF") || b.get(8..12) != Some(b"WAVE") {
        bail!("not a RIFF WAVE file");
    }
    let (mut fmt, mut smpl, mut data) = (None, None, None);
    let mut p = 12;
    while p + 8 <= b.len() {
        let id = &b[p..p + 4];
        let size = u32_at(b, p + 4)? as usize;
        match id {
            b"fmt " => fmt = Some((p + 8, size)),
            b"smpl" => smpl = Some((p + 8, size)),
            b"data" => {
                data = Some((p + 8, size));
                break;
            }
            _ => {}
        }
        p += 8 + size + (size & 1);
    }
    let (fmt_at, fmt_size) = fmt.ok_or_else(|| anyhow!("no fmt chunk"))?;
    let (data_at, data_size) = data.ok_or_else(|| anyhow!("no data chunk"))?;
    let tag = u16_at(b, fmt_at)?;
    let channels = u16_at(b, fmt_at + 2)?.max(1);
    let rate = u32_at(b, fmt_at + 4)?;
    let block_align = u16_at(b, fmt_at + 0x0C)?;
    let bits = u16_at(b, fmt_at + 0x0E)?;
    let codec = match tag {
        0xFFFE | 0x0001 => Codec::Pcm,
        0x8311 => Codec::PtAdpcm,
        0xFFFF => Codec::Vorbis,
        0x3039 => Codec::OpusNx,
        t => Codec::Other(t),
    };
    let ch = channels as usize;
    let samples = match codec {
        Codec::Pcm => (data_size / (ch * (bits.max(8) as usize / 8))) as u32,
        Codec::PtAdpcm => {
            let frame = block_align as usize / ch;
            if frame < 6 {
                0
            } else {
                (data_size / (ch * frame) * (2 + (frame - 5) * 2)) as u32
            }
        }
        Codec::Vorbis | Codec::OpusNx => u32_at(b, fmt_at + 0x18)?,
        Codec::Other(_) => 0,
    };
    let looping = smpl.and_then(|(at, size)| {
        if size < 0x34 || u32_at(b, at + 0x1C).ok()? != 1 || u32_at(b, at + 0x28).ok()? != 0 {
            return None;
        }
        let start = u32_at(b, at + 0x2C).ok()?;
        let end = u32_at(b, at + 0x30).ok()?.saturating_add(1);
        (end > start).then_some((start, end))
    });
    Ok(Header {
        codec,
        channels,
        rate,
        samples,
        looping,
        fmt_at,
        fmt_size,
        block_align,
        bits,
        data_at,
        data_size,
    })
}

/// Decoded sound: interleaved 16-bit samples.
pub struct Pcm {
    pub channels: u16,
    pub rate: u32,
    pub samples: Vec<i16>,
    pub looping: Option<(u32, u32)>,
    pub codec: Codec,
}

pub fn decode(b: &[u8]) -> Result<Pcm> {
    let h = header(b)?;
    let end = (h.data_at + h.data_size).min(b.len());
    let data = b.get(h.data_at..end).unwrap_or(&[]);
    let fmt = b
        .get(h.fmt_at..h.fmt_at + h.fmt_size)
        .ok_or_else(|| anyhow!("fmt chunk cut short"))?;
    let (rate, samples) = match h.codec {
        Codec::Pcm => {
            if h.bits != 16 {
                bail!("{}-bit PCM not handled", h.bits);
            }
            let s = data
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| i16::from_le_bytes(*c))
                .collect();
            (h.rate, s)
        }
        Codec::PtAdpcm => (
            h.rate,
            ptadpcm(data, h.channels as usize, h.block_align as usize)?,
        ),
        Codec::Vorbis => (
            h.rate,
            vorbis(fmt, data, h.channels, h.rate).context("Vorbis")?,
        ),
        Codec::OpusNx => (48000, opus_nx(fmt, data, h.channels).context("Opus")?),
        Codec::Other(t) => bail!("codec {t:#06x} not handled"),
    };
    // loop points are in the file's own rate; Opus is decoded at 48 kHz
    let scale = rate as f64 / h.rate.max(1) as f64;
    let looping = h
        .looping
        .map(|(s, e)| ((s as f64 * scale) as u32, (e as f64 * scale) as u32));
    Ok(Pcm {
        channels: h.channels,
        rate,
        samples,
        looping,
        codec: h.codec,
    })
}

// ---------------------------------------------------------------- PTADPCM

/// [index][nibble] = (step, next index); rows 12..15 are zero.
#[rustfmt::skip]
const PTADPCM: [[(i32, u8); 16]; 12] = [
    [(-14, 2), (-10, 2), (-7, 1), (-5, 1), (-3, 0), (-2, 0), (-1, 0), (0, 0), (0, 0), (1, 0), (2, 0), (3, 0), (5, 1), (7, 1), (10, 2), (14, 2)],
    [(-28, 3), (-20, 3), (-14, 2), (-10, 2), (-7, 1), (-5, 1), (-3, 1), (-1, 0), (1, 0), (3, 1), (5, 1), (7, 1), (10, 2), (14, 2), (20, 3), (28, 3)],
    [(-56, 4), (-40, 4), (-28, 3), (-20, 3), (-14, 2), (-10, 2), (-6, 2), (-2, 1), (2, 1), (6, 2), (10, 2), (14, 2), (20, 3), (28, 3), (40, 4), (56, 4)],
    [(-112, 5), (-80, 5), (-56, 4), (-40, 4), (-28, 3), (-20, 3), (-12, 3), (-4, 2), (4, 2), (12, 3), (20, 3), (28, 3), (40, 4), (56, 4), (80, 5), (112, 5)],
    [(-224, 6), (-160, 6), (-112, 5), (-80, 5), (-56, 4), (-40, 4), (-24, 4), (-8, 3), (8, 3), (24, 4), (40, 4), (56, 4), (80, 5), (112, 5), (160, 6), (224, 6)],
    [(-448, 7), (-320, 7), (-224, 6), (-160, 6), (-112, 5), (-80, 5), (-48, 5), (-16, 4), (16, 4), (48, 5), (80, 5), (112, 5), (160, 6), (224, 6), (320, 7), (448, 7)],
    [(-896, 8), (-640, 8), (-448, 7), (-320, 7), (-224, 6), (-160, 6), (-96, 6), (-32, 5), (32, 5), (96, 6), (160, 6), (224, 6), (320, 7), (448, 7), (640, 8), (896, 8)],
    [(-1792, 9), (-1280, 9), (-896, 8), (-640, 8), (-448, 7), (-320, 7), (-192, 7), (-64, 6), (64, 6), (192, 7), (320, 7), (448, 7), (640, 8), (896, 8), (1280, 9), (1792, 9)],
    [(-3584, 10), (-2560, 10), (-1792, 9), (-1280, 9), (-896, 8), (-640, 8), (-384, 8), (-128, 7), (128, 7), (384, 8), (640, 8), (896, 8), (1280, 9), (1792, 9), (2560, 10), (3584, 10)],
    [(-7168, 11), (-5120, 11), (-3584, 10), (-2560, 10), (-1792, 9), (-1280, 9), (-768, 9), (-256, 8), (256, 8), (768, 9), (1280, 9), (1792, 9), (2560, 10), (3584, 10), (5120, 11), (7168, 11)],
    [(-14336, 11), (-10240, 11), (-7168, 11), (-5120, 11), (-3584, 10), (-2560, 10), (-1536, 10), (-512, 9), (512, 9), (1536, 10), (2560, 10), (3584, 10), (5120, 11), (7168, 11), (10240, 11), (14336, 11)],
    [(-28672, 11), (-20480, 11), (-14336, 11), (-10240, 11), (-7168, 11), (-5120, 11), (-3072, 11), (-1024, 10), (1024, 10), (3072, 11), (5120, 11), (7168, 11), (10240, 11), (14336, 11), (20480, 11), (28672, 11)],
];

fn ptadpcm(data: &[u8], ch: usize, block_align: usize) -> Result<Vec<i16>> {
    let frame = block_align / ch.max(1);
    if frame < 6 {
        bail!("PTADPCM frame size {frame}");
    }
    let spf = 2 + (frame - 5) * 2;
    let frames = data.len() / (frame * ch);
    let mut out = vec![0i16; frames * spf * ch];
    for f in 0..frames {
        for c in 0..ch {
            let fr = &data[(f * ch + c) * frame..][..frame];
            let mut hist2 = i16::from_le_bytes([fr[0], fr[1]]) as i32;
            let mut hist1 = i16::from_le_bytes([fr[2], fr[3]]) as i32;
            let mut index = (fr[4] as usize).min(12);
            let base = f * spf;
            out[base * ch + c] = hist2 as i16;
            out[(base + 1) * ch + c] = hist1 as i16;
            for i in 0..spf - 2 {
                let byte = fr[5 + i / 2];
                let nibble = (if i & 1 == 0 { byte & 0xF } else { byte >> 4 }) as usize;
                let (step, next) = PTADPCM.get(index).map_or((0, 12), |row| row[nibble]);
                index = next as usize;
                let s = (step + 2 * hist1 - hist2).clamp(-32768, 32767);
                out[(base + 2 + i) * ch + c] = s as i16;
                hist2 = hist1;
                hist1 = s;
            }
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------- Opus NX

fn opus_nx(fmt: &[u8], data: &[u8], channels: u16) -> Result<Vec<i16>> {
    let seek = u32_at(fmt, 0x24)? as usize;
    let ch = channels as usize;
    if ch > 2 {
        bail!("{ch}-channel Opus not handled");
    }
    let channels = if ch == 1 {
        opus::Channels::Mono
    } else {
        opus::Channels::Stereo
    };
    let mut dec = opus::Decoder::new(48000, channels).map_err(|e| anyhow!("{e}"))?;
    let mut buf = vec![0i16; 5760 * ch];
    let mut out = Vec::new();
    let mut skip = None;
    let mut p = seek;
    while p + 8 <= data.len() {
        let size = u32::from_be_bytes([data[p], data[p + 1], data[p + 2], data[p + 3]]) as usize;
        let Some(pkt) = data.get(p + 8..p + 8 + size) else {
            break;
        };
        let n = dec
            .decode(pkt, &mut buf, false)
            .map_err(|e| anyhow!("frame at {p:#x}: {e}"))?;
        // encoder delay: 1/8 of the first frame (vgmstream)
        let s = *skip.get_or_insert(n / 8);
        out.extend_from_slice(&buf[s.min(n) * ch..n * ch]);
        skip = Some(0);
        p += 8 + size;
    }
    Ok(out)
}

// ---------------------------------------------------------------- Wwise Vorbis

/// Vorbis bit stream reader: least significant bit first.
struct BitReader<'a> {
    b: &'a [u8],
    pos: usize,
}

impl BitReader<'_> {
    fn get(&mut self, n: u32) -> Result<u32> {
        let mut v = 0u32;
        for i in 0..n {
            let byte = *self
                .b
                .get(self.pos / 8)
                .ok_or_else(|| anyhow!("setup read past its end"))?;
            v |= (((byte >> (self.pos % 8)) & 1) as u32) << i;
            self.pos += 1;
        }
        Ok(v)
    }
}

#[derive(Default)]
struct BitWriter {
    buf: Vec<u8>,
    pos: usize,
}

impl BitWriter {
    fn put(&mut self, n: u32, v: u32) {
        for i in 0..n {
            if self.pos.is_multiple_of(8) {
                self.buf.push(0);
            }
            if (v >> i) & 1 == 1 {
                *self.buf.last_mut().unwrap() |= 1 << (self.pos % 8);
            }
            self.pos += 1;
        }
    }

    /// Get `n` bits from `r` and write them unchanged.
    fn copy(&mut self, r: &mut BitReader, n: u32) -> Result<u32> {
        let v = r.get(n)?;
        self.put(n, v);
        Ok(v)
    }
}

fn ilog(mut v: u32) -> u32 {
    let mut r = 0;
    while v != 0 {
        r += 1;
        v >>= 1;
    }
    r
}

fn quantvals(entries: u32, dims: u32) -> u32 {
    let bits = ilog(entries);
    let mut vals = entries >> ((bits.saturating_sub(1)) * (dims - 1) / dims);
    loop {
        let (mut acc, mut acc1) = (1u64, 1u64);
        for _ in 0..dims {
            acc *= vals as u64;
            acc1 *= vals as u64 + 1;
        }
        if acc <= entries as u64 && acc1 > entries as u64 {
            return vals;
        }
        if acc > entries as u64 {
            vals -= 1;
        } else {
            vals += 1;
        }
    }
}

const CODEBOOKS: &[u8] = include_bytes!("../assets/packed_codebooks_aoTuV_603.bin");

/// Codebook `id` of the packed aoTuV 6.03 library: data, then a table of u32 offsets whose own offset is the
/// file's last u32.
fn codebook(id: u32) -> Result<&'static [u8]> {
    let table = u32_at(CODEBOOKS, CODEBOOKS.len() - 4)? as usize;
    let count = (CODEBOOKS.len() - table) / 4;
    let id = id as usize;
    if id + 1 >= count {
        bail!("codebook {id} not in the library ({count})");
    }
    let (a, b) = (
        u32_at(CODEBOOKS, table + 4 * id)? as usize,
        u32_at(CODEBOOKS, table + 4 * id + 4)? as usize,
    );
    CODEBOOKS
        .get(a..b)
        .ok_or_else(|| anyhow!("codebook {id} out of range"))
}

/// A packed Wwise codebook to a Vorbis one.
fn rebuild_codebook(w: &mut BitWriter, cb: &[u8]) -> Result<()> {
    let mut r = BitReader { b: cb, pos: 0 };
    w.put(24, 0x564342);
    let dims = r.get(4)?;
    w.put(16, dims);
    let entries = r.get(14)?;
    w.put(24, entries);
    let ordered = w.copy(&mut r, 1)?;
    if ordered == 1 {
        w.copy(&mut r, 5)?;
        let mut current = 0;
        while current < entries {
            current += w.copy(&mut r, ilog(entries - current))?;
        }
        if current > entries {
            bail!("codebook entries out of range");
        }
    } else {
        let len_bits = r.get(3)?;
        let sparse = w.copy(&mut r, 1)?;
        if len_bits == 0 || len_bits > 5 {
            bail!("codeword length size {len_bits}");
        }
        for _ in 0..entries {
            let present = if sparse == 1 { w.copy(&mut r, 1)? } else { 1 };
            if present == 1 {
                let len = r.get(len_bits)?;
                w.put(5, len);
            }
        }
    }
    let lookup = r.get(1)?;
    w.put(4, lookup);
    if lookup == 1 {
        w.copy(&mut r, 32)?;
        w.copy(&mut r, 32)?;
        let value_len = w.copy(&mut r, 4)?;
        w.copy(&mut r, 1)?;
        if dims == 0 {
            bail!("codebook without dimensions");
        }
        for _ in 0..quantvals(entries, dims) {
            w.copy(&mut r, value_len + 1)?;
        }
    }
    if r.pos / 8 + 1 != cb.len() {
        bail!("codebook size mismatch");
    }
    Ok(())
}

/// Wwise's stripped setup packet to a standard one. Returns it and each mode's block flag.
fn rebuild_setup(packet: &[u8], channels: u32) -> Result<(Vec<u8>, Vec<bool>)> {
    let mut r = BitReader { b: packet, pos: 0 };
    let mut w = BitWriter::default();
    w.put(8, 5);
    for c in b"vorbis" {
        w.put(8, *c as u32);
    }
    let books = w.copy(&mut r, 8)? + 1;
    for _ in 0..books {
        rebuild_codebook(&mut w, codebook(r.get(10)?)?)?;
    }
    // time domain transforms: one, a placeholder
    w.put(6, 0);
    w.put(16, 0);

    let floors = w.copy(&mut r, 6)? + 1;
    for _ in 0..floors {
        w.put(16, 1); // floor type 1
        let partitions = w.copy(&mut r, 5)?;
        let mut classes = Vec::new();
        for _ in 0..partitions {
            classes.push(w.copy(&mut r, 4)?);
        }
        let max_class = classes.iter().copied().max().unwrap_or(0);
        let mut dims = vec![0u32; max_class as usize + 1];
        for d in dims.iter_mut() {
            *d = w.copy(&mut r, 3)? + 1;
            let subclasses = w.copy(&mut r, 2)?;
            if subclasses != 0 && w.copy(&mut r, 8)? >= books {
                bail!("floor masterbook out of range");
            }
            for _ in 0..(1u32 << subclasses) {
                let book = w.copy(&mut r, 8)? as i64 - 1;
                if book >= books as i64 {
                    bail!("floor subclass book out of range");
                }
            }
        }
        w.copy(&mut r, 2)?;
        let rangebits = w.copy(&mut r, 4)?;
        for c in classes {
            for _ in 0..dims[c as usize] {
                w.copy(&mut r, rangebits)?;
            }
        }
    }

    let residues = w.copy(&mut r, 6)? + 1;
    for _ in 0..residues {
        let kind = r.get(2)?;
        w.put(16, kind);
        if kind > 2 {
            bail!("residue type {kind}");
        }
        w.copy(&mut r, 24)?;
        w.copy(&mut r, 24)?;
        w.copy(&mut r, 24)?;
        let classifications = w.copy(&mut r, 6)? + 1;
        if w.copy(&mut r, 8)? >= books {
            bail!("residue classbook out of range");
        }
        let mut cascade = Vec::new();
        for _ in 0..classifications {
            let low = w.copy(&mut r, 3)?;
            let high = if w.copy(&mut r, 1)? == 1 {
                w.copy(&mut r, 5)?
            } else {
                0
            };
            cascade.push(high * 8 + low);
        }
        for c in cascade {
            for k in 0..8 {
                if c & (1 << k) != 0 && w.copy(&mut r, 8)? >= books {
                    bail!("residue book out of range");
                }
            }
        }
    }

    let mappings = w.copy(&mut r, 6)? + 1;
    for _ in 0..mappings {
        w.put(16, 0); // mapping type 0
        let submaps = if w.copy(&mut r, 1)? == 1 {
            w.copy(&mut r, 4)? + 1
        } else {
            1
        };
        if w.copy(&mut r, 1)? == 1 {
            let steps = w.copy(&mut r, 8)? + 1;
            let bits = ilog(channels - 1);
            for _ in 0..steps {
                let mag = w.copy(&mut r, bits)?;
                let ang = w.copy(&mut r, bits)?;
                if mag == ang || mag >= channels || ang >= channels {
                    bail!("bad channel coupling");
                }
            }
        }
        if w.copy(&mut r, 2)? != 0 {
            bail!("mapping reserved field set");
        }
        if submaps > 1 {
            for _ in 0..channels {
                if w.copy(&mut r, 4)? >= submaps {
                    bail!("mapping mux out of range");
                }
            }
        }
        for _ in 0..submaps {
            w.copy(&mut r, 8)?;
            if w.copy(&mut r, 8)? >= floors {
                bail!("mapping floor out of range");
            }
            if w.copy(&mut r, 8)? >= residues {
                bail!("mapping residue out of range");
            }
        }
    }

    let modes = w.copy(&mut r, 6)? + 1;
    let mut flags = Vec::new();
    for _ in 0..modes {
        flags.push(w.copy(&mut r, 1)? == 1);
        w.put(16, 0);
        w.put(16, 0);
        if w.copy(&mut r, 8)? >= mappings {
            bail!("mode mapping out of range");
        }
    }
    w.put(1, 1); // framing
    Ok((w.buf, flags))
}

fn vorbis(fmt: &[u8], data: &[u8], channels: u16, rate: u32) -> Result<Vec<i16>> {
    use lewton::audio::{PreviousWindowRight, read_audio_packet_generic};
    use lewton::header::{read_header_ident, read_header_setup};
    use lewton::samples::InterleavedSamples;

    if u16_at(fmt, 0x10)? != 0x30 {
        bail!(
            "fmt extra size {:#x} (only the 0x30 layout is handled)",
            u16_at(fmt, 0x10)?
        );
    }
    let x = 0x18;
    let total = u32_at(fmt, x)? as usize;
    let seek = u32_at(fmt, x + 0x10)? as usize;
    let audio = u32_at(fmt, x + 0x14)? as usize;
    let (small, big) = (
        *fmt.get(x + 0x28).ok_or_else(|| anyhow!("fmt cut short"))?,
        *fmt.get(x + 0x29).ok_or_else(|| anyhow!("fmt cut short"))?,
    );
    let modified = small != big;

    let mut ident = vec![1];
    ident.extend_from_slice(b"vorbis");
    ident.extend_from_slice(&0u32.to_le_bytes());
    ident.push(channels as u8);
    ident.extend_from_slice(&rate.to_le_bytes());
    ident.extend_from_slice(&[0; 12]);
    ident.push(small | (big << 4));
    ident.push(1);
    let ident = read_header_ident(&ident).map_err(|e| anyhow!("identification: {e:?}"))?;

    let setup_size = u16_at(data, seek)? as usize;
    let packet = data
        .get(seek + 2..seek + 2 + setup_size)
        .ok_or_else(|| anyhow!("setup packet cut short"))?;
    let (setup, flags) = rebuild_setup(packet, channels as u32)?;
    let setup = read_header_setup(&setup, channels as u8, (small, big))
        .map_err(|e| anyhow!("setup: {e:?}"))?;
    let mode_bits = ilog(flags.len() as u32 - 1);

    let mut pwr = PreviousWindowRight::new();
    let mut out: Vec<i16> = Vec::with_capacity(total * channels as usize);
    let mut prev_long = false;
    let mut p = audio;
    while p + 2 <= data.len() {
        let size = u16_at(data, p)? as usize;
        let Some(pkt) = data.get(p + 2..p + 2 + size).filter(|_| size > 0) else {
            break;
        };
        let rebuilt;
        let pkt = if modified {
            // put back the packet type bit and, for long blocks, the previous / next window flags
            let mut r = BitReader { b: pkt, pos: 0 };
            let mut w = BitWriter::default();
            w.put(1, 0);
            let mode = w.copy(&mut r, mode_bits)? as usize;
            let rest = r.get(8 - mode_bits)?;
            let long = *flags.get(mode).ok_or_else(|| anyhow!("mode {mode}"))?;
            if long {
                let next = data.get(p + 2 + size + 2).is_some_and(|b| {
                    let m = (*b as u32 & ((1 << mode_bits) - 1)) as usize;
                    flags.get(m).copied().unwrap_or(false)
                });
                w.put(1, prev_long as u32);
                w.put(1, next as u32);
            }
            prev_long = long;
            w.put(8 - mode_bits, rest);
            for _ in 1..size {
                w.copy(&mut r, 8)?;
            }
            rebuilt = w.buf;
            &rebuilt[..]
        } else {
            pkt
        };
        let s: InterleavedSamples<i16> = read_audio_packet_generic(&ident, &setup, pkt, &mut pwr)
            .map_err(|e| anyhow!("packet at {p:#x}: {e:?}"))?;
        out.extend_from_slice(&s.samples);
        p += 2 + size;
    }
    out.truncate(total * channels as usize);
    Ok(out)
}
