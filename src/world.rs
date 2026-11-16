//! The World view: the overworld's terrain tiles (see `terrain.rs` for the data), drawn with the game's splat
//! ground textures (`TerrainMaterial`, shader in `shaders/terrain.frag`) or coloured by height.
//! Tiles load one per frame, so the window stays responsive while the map fills in.

use crate::archive::Archive;
use crate::terrain::{self, HeightTile};
use std::collections::VecDeque;
use std::rc::Rc;
use three_d::*;

/// How the ground is coloured.
#[derive(Clone, Copy, PartialEq)]
pub enum Shading {
    /// The game's ground textures, blended by each tile's control map.
    Splat,
    /// A colour ramp by height.
    Height,
}

/// The ground textures shared by every tile, on the GPU.
pub struct GpuLayers {
    colors: Texture2DArray,
    count: usize,
    /// Tiling size in metres per layer, padded to the shader's 64.
    tiling: Vec<f32>,
}

impl GpuLayers {
    fn new(context: &Context, layers: &terrain::SplatLayers) -> Result<GpuLayers, String> {
        let first = layers
            .colors
            .first()
            .ok_or("the colour array has no layers")?;
        if layers.colors.len() > 64 {
            return Err(format!(
                "{} layers; the shader takes 64",
                layers.colors.len()
            ));
        }
        if layers
            .colors
            .iter()
            .any(|l| (l.width, l.height) != (first.width, first.height))
        {
            return Err("the colour layers differ in size".into());
        }
        let cpu: Vec<CpuTexture> = layers
            .colors
            .iter()
            .map(|img| CpuTexture {
                data: TextureData::RgbaU8(img.rgba.clone()),
                width: img.width,
                height: img.height,
                wrap_s: Wrapping::Repeat,
                wrap_t: Wrapping::Repeat,
                ..Default::default()
            })
            .collect();
        let refs: Vec<&CpuTexture> = cpu.iter().collect();
        let mut tiling = layers.tiling.clone();
        tiling.resize(64, 5.0);
        Ok(GpuLayers {
            colors: Texture2DArray::new(context, &refs),
            count: layers.colors.len(),
            tiling,
        })
    }
}

/// One tile's splat material: the shared layers, the tile's control map, and where the tile lies.
pub struct TerrainMaterial {
    layers: Rc<GpuLayers>,
    control: Texture2D,
    origin: Vec2,
    cell: Vec2,
}

impl TerrainMaterial {
    fn new(
        context: &Context,
        layers: Rc<GpuLayers>,
        control: &crate::texture::Image,
        h: &HeightTile,
    ) -> TerrainMaterial {
        let cpu = CpuTexture {
            data: TextureData::RgbaU8(control.rgba.clone()),
            width: control.width,
            height: control.height,
            min_filter: Interpolation::Nearest,
            mag_filter: Interpolation::Nearest,
            mipmap: None,
            wrap_s: Wrapping::ClampToEdge,
            wrap_t: Wrapping::ClampToEdge,
            ..Default::default()
        };
        // the control map spans the tile's cells (512 texels for 512 cells)
        let size = vec2(
            (h.width - 1) as f32 * h.scale[0],
            (h.depth - 1) as f32 * h.scale[2],
        );
        TerrainMaterial {
            layers,
            control: Texture2D::new(context, &cpu),
            origin: vec2(h.origin[0], h.origin[2]),
            cell: vec2(
                size.x / control.width as f32,
                size.y / control.height as f32,
            ),
        }
    }
}

impl Material for TerrainMaterial {
    fn fragment_shader_source(&self, lights: &[&dyn Light]) -> String {
        let mut s = lights_shader_source(lights);
        s.push_str(ToneMapping::fragment_shader_source());
        s.push_str(ColorMapping::fragment_shader_source());
        s.push_str(include_str!("shaders/terrain.frag"));
        s
    }

    fn id(&self) -> EffectMaterialId {
        // public-use range (0x0000..0x4FFF)
        EffectMaterialId(0x0100)
    }

    fn use_uniforms(&self, program: &Program, viewer: &dyn Viewer, lights: &[&dyn Light]) {
        viewer.tone_mapping().use_uniforms(program);
        viewer.color_mapping().use_uniforms(program);
        program.use_uniform_if_required("cameraPosition", viewer.position());
        for (i, light) in lights.iter().enumerate() {
            light.use_uniforms(program, i as u32);
        }
        program.use_texture_array("colorLayers", &self.layers.colors);
        program.use_texture("control", &self.control);
        program.use_uniform_if_required("tileOrigin", self.origin);
        program.use_uniform_if_required("cellSize", self.cell);
        program.use_uniform_if_required("layerCount", self.layers.count as i32);
        if program.requires_uniform("layerTiling") {
            program.use_uniform_array("layerTiling", &self.layers.tiling);
        }
    }

    fn render_states(&self) -> RenderStates {
        RenderStates::default()
    }

    fn material_type(&self) -> MaterialType {
        MaterialType::Opaque
    }
}

/// Mesh detail: every Nth height sample. 512 cells per tile divide by each.
pub const STEPS: [(usize, &str); 4] = [(1, "Full"), (2, "1/2"), (4, "1/4"), (8, "1/8")];

pub enum TileState {
    NotLoaded,
    /// Loaded; `None` mesh when the tile is all holes (sea or outside the map).
    Loaded {
        heights: Option<HeightTile>,
        mesh: Option<Box<Gm<Mesh, PhysicalMaterial>>>,
        /// The ground-texture material, or why it could not be made.
        splat: Result<Box<TerrainMaterial>, String>,
        triangles: usize,
        range: (f32, f32),
    },
    Failed(String),
}

pub struct Tile {
    pub x: i32,
    pub y: i32,
    pub path: String,
    pub shown: bool,
    pub state: TileState,
}

pub struct World {
    pub tiles: Vec<Tile>,
    queue: VecDeque<usize>,
    pub step: usize,
    pub shading: Shading,
    /// Set when the first load finished, so the camera moves to the map once.
    pub reframe: bool,
    framed: bool,
    /// The shared ground textures, loaded with the first tile that has ground.
    layers: Option<Result<Rc<GpuLayers>, String>>,
}

/// Where a tile sits in the minimap, top row first. Rows run along -z; the YY = 00 row lies past 09 (origin
/// z -5500 in all ten of its tiles), so it is drawn at the top.
pub fn grid_row(y: i32) -> i32 {
    let order = if y == 0 { terrain::TILES } else { y };
    terrain::TILES - order
}

/// A colour ramp by height in metres: shore, grass, forest, earth, rock, snow.
fn height_color(h: f32) -> Srgba {
    const STOPS: [(f32, [f32; 3]); 6] = [
        (0.0, [196.0, 184.0, 140.0]),
        (25.0, [118.0, 156.0, 84.0]),
        (200.0, [78.0, 122.0, 62.0]),
        (420.0, [132.0, 112.0, 86.0]),
        (650.0, [138.0, 136.0, 132.0]),
        (900.0, [240.0, 242.0, 246.0]),
    ];
    let mut c = STOPS[STOPS.len() - 1].1;
    for w in STOPS.windows(2) {
        let ((h0, c0), (h1, c1)) = (w[0], w[1]);
        if h <= h1 {
            let t = ((h - h0) / (h1 - h0)).clamp(0.0, 1.0);
            c = std::array::from_fn(|i| c0[i] + (c1[i] - c0[i]) * t);
            break;
        }
    }
    Srgba::new(c[0] as u8, c[1] as u8, c[2] as u8, 255)
}

/// A mesh of every `step`th sample; cells with a hole (height 0) at any corner are left out.
fn build_mesh(
    context: &Context,
    t: &HeightTile,
    step: usize,
) -> Option<(Gm<Mesh, PhysicalMaterial>, usize)> {
    let step = step.max(1);
    let (cols, rows) = ((t.width - 1) / step + 1, (t.depth - 1) / step + 1);
    let sample = |c: usize, r: usize| (c * step, r * step);
    let mut positions = Vec::with_capacity(cols * rows);
    let mut normals = Vec::with_capacity(cols * rows);
    let mut colors = Vec::with_capacity(cols * rows);
    for r in 0..rows {
        for c in 0..cols {
            let (sc, sr) = sample(c, r);
            let p = t.position(sc, sr);
            positions.push(vec3(p[0], p[1], p[2]));
            // central differences on the full-resolution grid, so shading stays smooth at any step
            let h = |dc: isize, dr: isize| {
                let cc = (sc as isize + dc).clamp(0, t.width as isize - 1) as usize;
                let rr = (sr as isize + dr).clamp(0, t.depth as isize - 1) as usize;
                let v = t.height(cc, rr);
                if v == 0.0 { t.height(sc, sr) } else { v }
            };
            let dx = (h(1, 0) - h(-1, 0)) / (2.0 * t.scale[0]);
            let dz = (h(0, 1) - h(0, -1)) / (2.0 * t.scale[2]);
            normals.push(vec3(-dx, 1.0, -dz).normalize());
            colors.push(height_color(p[1]));
        }
    }
    let mut indices: Vec<u32> = Vec::new();
    for r in 0..rows - 1 {
        for c in 0..cols - 1 {
            let corners = [(c, r), (c + 1, r), (c, r + 1), (c + 1, r + 1)];
            if corners.iter().any(|&(cc, rr)| {
                let (sc, sr) = sample(cc, rr);
                t.height(sc, sr) == 0.0
            }) {
                continue;
            }
            let i = |cc: usize, rr: usize| (rr * cols + cc) as u32;
            // counter-clockwise seen from above (+y): x right, z towards the viewer
            indices.extend([i(c, r), i(c, r + 1), i(c + 1, r)]);
            indices.extend([i(c + 1, r), i(c, r + 1), i(c + 1, r + 1)]);
        }
    }
    if indices.is_empty() {
        return None;
    }
    let triangles = indices.len() / 3;
    let mesh = CpuMesh {
        positions: Positions::F32(positions),
        indices: Indices::U32(indices),
        normals: Some(normals),
        colors: Some(colors),
        ..Default::default()
    };
    let material = CpuMaterial {
        albedo: Srgba::WHITE,
        roughness: 0.95,
        metallic: 0.0,
        ..Default::default()
    };
    Some((
        Gm::new(
            Mesh::new(context, &mesh),
            PhysicalMaterial::new_opaque(context, &material),
        ),
        triangles,
    ))
}

impl World {
    pub fn new(ar: &Archive) -> World {
        let tiles = terrain::tiles(ar)
            .into_iter()
            .map(|(x, y, path)| Tile {
                x,
                y,
                path,
                shown: true,
                state: TileState::NotLoaded,
            })
            .collect();
        World {
            tiles,
            queue: VecDeque::new(),
            step: 4,
            shading: Shading::Splat,
            reframe: false,
            framed: false,
            layers: None,
        }
    }

    /// Shows only the listed tiles ("05_03,06_03"); unknown names are ignored. Returns how many matched.
    pub fn show_only(&mut self, list: &str) -> usize {
        let wanted: Vec<&str> = list.split(',').map(str::trim).collect();
        let mut n = 0;
        for t in &mut self.tiles {
            t.shown = wanted.contains(&format!("{:02}_{:02}", t.x, t.y).as_str());
            n += t.shown as usize;
        }
        n
    }

    /// Queues every shown tile that is not loaded yet.
    pub fn request_shown(&mut self) {
        for (i, t) in self.tiles.iter().enumerate() {
            if t.shown && matches!(t.state, TileState::NotLoaded) && !self.queue.contains(&i) {
                self.queue.push_back(i);
            }
        }
    }

    pub fn loading(&self) -> usize {
        self.queue.len()
    }

    /// Loads one queued tile. Returns true when something changed.
    pub fn update(&mut self, context: &Context, ar: &mut Archive) -> bool {
        let Some(i) = self.queue.pop_front() else {
            return false;
        };
        let path = self.tiles[i].path.clone();
        let state = match terrain::load_heights(ar, &path) {
            Ok(h) => {
                let built = build_mesh(context, &h, self.step);
                let solid = h.heights.iter().filter(|v| **v != 0.0);
                let range = solid.fold((f32::MAX, f32::MIN), |(a, b), v| (a.min(*v), b.max(*v)));
                let (mesh, triangles) = built.map_or((None, 0), |(m, n)| (Some(Box::new(m)), n));
                let splat = if mesh.is_some() {
                    self.splat_material(context, ar, &path, &h).map(Box::new)
                } else {
                    Err("no ground".into())
                };
                TileState::Loaded {
                    heights: mesh.is_some().then_some(h),
                    mesh,
                    splat,
                    triangles,
                    range,
                }
            }
            Err(e) => TileState::Failed(format!("{e:#}")),
        };
        self.tiles[i].state = state;
        if self.queue.is_empty() && !self.framed {
            self.framed = true;
            self.reframe = true;
        }
        true
    }

    /// A tile's splat material; the shared layers are loaded the first time.
    fn splat_material(
        &mut self,
        context: &Context,
        ar: &mut Archive,
        path: &str,
        h: &HeightTile,
    ) -> Result<TerrainMaterial, String> {
        let m = terrain::tile_material(ar, path).map_err(|e| format!("{e:#}"))?;
        let layers = self
            .layers
            .get_or_insert_with(|| {
                terrain::load_layers(ar, &m)
                    .map_err(|e| format!("{e:#}"))
                    .and_then(|l| GpuLayers::new(context, &l))
                    .map(Rc::new)
            })
            .clone()?;
        let control = ar
            .read(&m.splat_alpha)
            .and_then(|b| crate::texture::decode_bntx(&b))
            .map_err(|e| format!("{}: {e:#}", m.splat_alpha))?;
        Ok(TerrainMaterial::new(context, layers, &control, h))
    }

    /// Draws the shown tiles with the chosen shading. A tile whose splat material failed is coloured by height.
    pub fn render(&self, target: &RenderTarget, camera: &Camera, lights: &[&dyn Light]) {
        for t in self.tiles.iter().filter(|t| t.shown) {
            let TileState::Loaded {
                mesh: Some(gm),
                splat,
                ..
            } = &t.state
            else {
                continue;
            };
            match (self.shading, splat) {
                (Shading::Splat, Ok(m)) => {
                    target.render_with_material(m.as_ref(), camera, [&gm.geometry], lights);
                }
                _ => {
                    target.render(camera, [gm.as_ref()], lights);
                }
            }
        }
    }

    /// Rebuilds every loaded mesh at the current detail step.
    pub fn rebuild(&mut self, context: &Context) {
        for t in &mut self.tiles {
            if let TileState::Loaded {
                heights: Some(h),
                mesh,
                triangles,
                ..
            } = &mut t.state
                && let Some((m, n)) = build_mesh(context, h, self.step)
            {
                *mesh = Some(Box::new(m));
                *triangles = n;
            }
        }
    }

    pub fn triangles(&self) -> usize {
        self.tiles
            .iter()
            .filter(|t| t.shown)
            .map(|t| match &t.state {
                TileState::Loaded { triangles, .. } => *triangles,
                _ => 0,
            })
            .sum()
    }

    /// Bounds (min, max) of the shown tiles that have ground.
    pub fn bounds(&self) -> Option<(Vec3, Vec3)> {
        let mut out: Option<(Vec3, Vec3)> = None;
        for t in self.tiles.iter().filter(|t| t.shown) {
            let TileState::Loaded {
                heights: Some(h),
                range,
                ..
            } = &t.state
            else {
                continue;
            };
            let lo = vec3(h.origin[0], range.0, h.origin[2]);
            let hi = vec3(
                h.origin[0] + (h.width - 1) as f32 * h.scale[0],
                range.1,
                h.origin[2] + (h.depth - 1) as f32 * h.scale[2],
            );
            out = Some(match out {
                None => (lo, hi),
                Some((a, b)) => (
                    vec3(a.x.min(lo.x), a.y.min(lo.y), a.z.min(lo.z)),
                    vec3(b.x.max(hi.x), b.y.max(hi.y), b.z.max(hi.z)),
                ),
            });
        }
        out
    }

    /// Moves the camera to look at the shown ground from the +z side, a little above.
    pub fn frame(&self, camera: &mut Camera, control: &mut OrbitControl) {
        let Some((min, max)) = self.bounds() else {
            return;
        };
        let center = (min + max) * 0.5;
        let radius = ((max - min).magnitude() * 0.5).max(10.0);
        let eye = center + vec3(0.0, 0.8, 1.0).normalize() * radius * 1.6;
        camera.set_view(eye, center, vec3(0.0, 1.0, 0.0));
        camera.set_perspective_projection(degrees(45.0), radius * 0.0005, radius * 20.0);
        *control = OrbitControl::new(center, 5.0, radius * 8.0);
    }

    /// The left panel: a minimap of the grid (click a tile to show or hide it), detail and framing.
    /// Returns true when the meshes need rebuilding (detail changed).
    pub fn panel(
        &mut self,
        ui: &mut egui::Ui,
        camera: &mut Camera,
        control: &mut OrbitControl,
    ) -> bool {
        let with_ground = self
            .tiles
            .iter()
            .filter(|t| matches!(t.state, TileState::Loaded { mesh: Some(_), .. }))
            .count();
        let loaded = self
            .tiles
            .iter()
            .filter(|t| !matches!(t.state, TileState::NotLoaded))
            .count();
        ui.small(format!(
            "Overworld terrain: {} tiles of 500 m, {loaded} loaded, {with_ground} with ground.",
            self.tiles.len()
        ));
        if self.loading() > 0 {
            ui.label(format!("Loading… {} tiles to go", self.loading()));
        }
        ui.add_space(4.0);

        // the minimap
        let cell = 22.0;
        let n = terrain::TILES as f32;
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(cell * n, cell * n), egui::Sense::click());
        let painter = ui.painter_at(rect);
        let cell_rect = |t: &Tile| {
            let min = rect.min + egui::vec2(t.x as f32 * cell, grid_row(t.y) as f32 * cell);
            egui::Rect::from_min_size(min, egui::vec2(cell, cell)).shrink(1.0)
        };
        let hover = response.hover_pos();
        let mut hovered: Option<usize> = None;
        for (i, t) in self.tiles.iter().enumerate() {
            let r = cell_rect(t);
            let base = match &t.state {
                TileState::NotLoaded => egui::Color32::from_gray(70),
                TileState::Loaded { mesh: None, .. } => egui::Color32::from_rgb(40, 52, 72),
                TileState::Loaded { range, .. } => {
                    let c = height_color((range.0 + range.1) * 0.5);
                    egui::Color32::from_rgb(c.r, c.g, c.b)
                }
                TileState::Failed(_) => egui::Color32::from_rgb(170, 60, 60),
            };
            let fill = if t.shown {
                base
            } else {
                base.gamma_multiply(0.3)
            };
            painter.rect_filled(r, 2.0, fill);
            if hover.is_some_and(|p| r.contains(p)) {
                hovered = Some(i);
                painter.rect_stroke(
                    r,
                    2.0,
                    egui::Stroke::new(1.5f32, egui::Color32::WHITE),
                    egui::StrokeKind::Inside,
                );
            }
        }
        if let Some(i) = hovered {
            let t = &self.tiles[i];
            let text = match &t.state {
                TileState::NotLoaded => "not loaded".to_string(),
                TileState::Loaded { mesh: None, .. } => "no ground (all holes)".to_string(),
                TileState::Loaded {
                    range, triangles, ..
                } => {
                    format!(
                        "heights {:.0}–{:.0} m, {triangles} triangles",
                        range.0, range.1
                    )
                }
                TileState::Failed(e) => format!("failed: {e}"),
            };
            response.clone().on_hover_text(format!(
                "terrain_{:02}_{:02}: {text}\nClick to {}",
                t.x,
                t.y,
                if t.shown { "hide" } else { "show" }
            ));
            if response.clicked() {
                let t = &mut self.tiles[i];
                t.shown = !t.shown;
                self.request_shown();
            }
        }
        ui.small("Click a tile to show or hide it. Top of the map is -z.");

        ui.horizontal(|ui| {
            if ui.button("Show all").clicked() {
                self.tiles.iter_mut().for_each(|t| t.shown = true);
                self.request_shown();
            }
            if ui.button("Hide all").clicked() {
                self.tiles.iter_mut().for_each(|t| t.shown = false);
            }
            if ui
                .button("Frame")
                .on_hover_text("Move the camera to the shown tiles")
                .clicked()
            {
                self.frame(camera, control);
            }
        });
        ui.horizontal(|ui| {
            ui.label("Ground");
            ui.radio_value(&mut self.shading, Shading::Splat, "Textures")
                .on_hover_text("The game's ground textures, blended by each tile's splat map");
            ui.radio_value(&mut self.shading, Shading::Height, "Height colours")
                .on_hover_text("A colour ramp by height");
        });
        let before = self.step;
        ui.horizontal(|ui| {
            ui.label("Detail");
            for (s, label) in STEPS {
                ui.radio_value(&mut self.step, s, label)
                    .on_hover_text(format!(
                        "Every {s}. height sample (cell {:.1} m)",
                        s as f32 * 500.0 / 512.0
                    ));
            }
        });
        self.step != before
    }

    /// The right panel's summary.
    pub fn details(&self, ui: &mut egui::Ui) {
        ui.heading("World");
        ui.label(format!("{} triangles shown", self.triangles()));
        if let Some((min, max)) = self.bounds() {
            ui.label(format!(
                "x {:.0}..{:.0}, z {:.0}..{:.0} m",
                min.x, max.x, min.z, max.z
            ));
            ui.label(format!("heights {:.0}..{:.0} m", min.y, max.y));
        }
        if let Some(Err(e)) = &self.layers {
            ui.colored_label(
                egui::Color32::from_rgb(255, 120, 120),
                format!("Ground textures: {e}"),
            );
        } else if let Some(Ok(l)) = &self.layers {
            ui.small(format!("{} ground texture layers", l.count));
        }
        let no_splat: Vec<String> = self
            .tiles
            .iter()
            .filter_map(|t| match &t.state {
                TileState::Loaded {
                    mesh: Some(_),
                    splat: Err(e),
                    ..
                } => Some(format!("{:02}_{:02}: {e}", t.x, t.y)),
                _ => None,
            })
            .collect();
        if !no_splat.is_empty() {
            ui.collapsing(
                format!("Tiles without ground textures ({})", no_splat.len()),
                |ui| {
                    for e in &no_splat {
                        ui.small(e);
                    }
                },
            );
        }
        let failed: Vec<&Tile> = self
            .tiles
            .iter()
            .filter(|t| matches!(t.state, TileState::Failed(_)))
            .collect();
        if !failed.is_empty() {
            ui.collapsing(format!("Failed tiles ({})", failed.len()), |ui| {
                for t in failed {
                    if let TileState::Failed(e) = &t.state {
                        ui.small(format!("{:02}_{:02}: {e}", t.x, t.y));
                    }
                }
            });
        }
        ui.add_space(6.0);
        ui.small(
            "Heights come from each tile's height file; holes (height 0) are towns and other places that \
             have their own models, and the sea. Ground textures: two of the 60 shared layers per splat-map texel, \
             blended by its weights; layer normal maps and roughness are not used yet.",
        );
    }
}

/// Right drag pans: the camera and its target move together across the view plane, scaled by the distance so
/// the ground follows the mouse.
pub fn pan(camera: &mut Camera, control: &mut OrbitControl, events: &mut [Event]) {
    for e in events.iter_mut() {
        if let Event::MouseMotion {
            delta,
            button: Some(MouseButton::Right),
            handled,
            ..
        } = e
            && !*handled
        {
            let distance = control.target.distance(camera.position());
            let height = camera.viewport().height.max(1) as f32;
            // about the vertical field of view of the viewer's cameras (40-45°)
            let per_pixel = 2.0 * distance * 0.4 / height;
            let right = camera.right_direction();
            let up = right.cross(camera.view_direction()).normalize();
            let step = (-right * delta.0 + up * delta.1) * per_pixel;
            camera.translate(step);
            control.target += step;
            *handled = true;
        }
    }
}
