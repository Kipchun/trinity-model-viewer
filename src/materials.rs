//! Turns Trinity materials into what a simple PBR renderer takes: one colour texture and one normal map.
//!
//! Colour: BaseColorMap, tinted by the layer system when the material has a LayerMaskMap. The mask's R, G, B, A
//! channels blend the linear colours BaseColorLayer1..4 over BaseColor (white when absent), and the result multiplies
//! the base texture. That is how eyes, flames and many bodies get their colours (the base texture is often white).
//! Normal: NormalMap, two channels (BC5); Z is rebuilt.

use crate::archive::Archive;
use crate::model::{Model, resolve};
use crate::texture::{Image, decode_bntx};
use std::collections::HashMap;

pub struct Baked {
    pub albedo: Option<Image>,
    pub normal: Option<Image>,
    /// The colour texture has cut-out transparency.
    pub cutout: bool,
}

pub fn bake(ar: &mut Archive, model: &Model) -> (HashMap<String, Baked>, Vec<String>) {
    let mut cache: HashMap<String, Option<Image>> = HashMap::new();
    let mut warnings = Vec::new();
    let mut out = HashMap::new();
    let mut get = |ar: &mut Archive, file: &str, warnings: &mut Vec<String>| -> Option<Image> {
        let path = resolve(&model.dir, file);
        cache
            .entry(path.clone())
            .or_insert_with(|| match ar.read(&path).and_then(|b| decode_bntx(&b)) {
                Ok(img) => Some(img),
                Err(e) => {
                    warnings.push(format!("{file}: {e:#}"));
                    None
                }
            })
            .as_ref()
            .map(|i| Image {
                width: i.width,
                height: i.height,
                rgba: i.rgba.clone(),
                format: i.format.clone(),
            })
    };

    let mut names: Vec<&String> = model.submeshes.iter().map(|s| &s.material).collect();
    names.sort();
    names.dedup();
    for name in names {
        let Some(mat) = model.materials.get(name) else {
            continue;
        };
        let base = mat
            .texture("BaseColorMap")
            .and_then(|f| get(ar, f, &mut warnings));
        let mask = mat
            .texture("LayerMaskMap")
            .and_then(|f| get(ar, f, &mut warnings));
        let normal = mat
            .texture("NormalMap")
            .and_then(|f| get(ar, f, &mut warnings))
            .map(rebuild_normal);

        let albedo = match (base, mask) {
            (base, Some(mask)) => {
                // Each layer: its colour and how strongly its mask channel counts (LayerMaskScaleN; absent = 1,
                // stored without a value = 0, which switches the layer off).
                let layers: Vec<([f32; 3], f32)> = (1..=4)
                    .map(|i| {
                        let v = mat
                            .params
                            .get(&format!("BaseColorLayer{i}"))
                            .copied()
                            .unwrap_or([1.0; 4]);
                        let scale = mat
                            .floats
                            .get(&format!("LayerMaskScale{i}"))
                            .copied()
                            .unwrap_or(1.0);
                        ([v[0], v[1], v[2]], scale)
                    })
                    .collect();
                let start = mat
                    .params
                    .get("BaseColor")
                    .map(|v| [v[0], v[1], v[2]])
                    .unwrap_or([1.0; 3]);
                Some(apply_layers(base, &mask, start, &layers))
            }
            (base, None) => base,
        };
        let cutout = albedo.as_ref().is_some_and(|a| a.has_alpha());
        out.insert(
            name.clone(),
            Baked {
                albedo,
                normal,
                cutout,
            },
        );
    }
    (out, warnings)
}

fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}
fn linear_to_srgb(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.0031308 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

fn apply_layers(
    base: Option<Image>,
    mask: &Image,
    start: [f32; 3],
    layers: &[([f32; 3], f32)],
) -> Image {
    // Work at the larger of the two resolutions; the other is sampled (nearest).
    let (w, h) = match &base {
        Some(b) if b.width * b.height >= mask.width * mask.height => (b.width, b.height),
        _ => (mask.width, mask.height),
    };
    let sample = |img: &Image, x: u32, y: u32| -> [u8; 4] {
        let sx = (x as u64 * img.width as u64 / w as u64) as usize;
        let sy = (y as u64 * img.height as u64 / h as u64) as usize;
        img.rgba[sy * img.width as usize + sx]
    };
    let mut rgba = Vec::with_capacity((w * h) as usize);
    for y in 0..h {
        for x in 0..w {
            let m = sample(mask, x, y);
            let mut c = start;
            for (i, (layer, scale)) in layers.iter().enumerate() {
                let t = (m[i] as f32 / 255.0 * scale).clamp(0.0, 1.0);
                for k in 0..3 {
                    c[k] += (layer[k] - c[k]) * t;
                }
            }
            let b = base.as_ref().map(|b| sample(b, x, y)).unwrap_or([255; 4]);
            let px = |k: usize| {
                (linear_to_srgb(srgb_to_linear(b[k] as f32 / 255.0) * c[k]) * 255.0).round() as u8
            };
            rgba.push([px(0), px(1), px(2), b[3]]);
        }
    }
    Image {
        width: w,
        height: h,
        rgba,
        format: "layered".into(),
    }
}

/// Two-channel normal maps keep X and Y; Z = sqrt(1 - x² - y²).
fn rebuild_normal(mut img: Image) -> Image {
    for p in &mut img.rgba {
        let x = p[0] as f32 / 127.5 - 1.0;
        let y = p[1] as f32 / 127.5 - 1.0;
        let z = (1.0 - x * x - y * y).max(0.0).sqrt();
        p[2] = ((z * 0.5 + 0.5) * 255.0).round() as u8;
        p[3] = 255;
    }
    img
}
