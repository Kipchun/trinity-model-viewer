//! The World view: the overworld's terrain tiles (see `terrain.rs` for the data), drawn with the game's splat
//! ground textures (`TerrainMaterial`, shader in `shaders/terrain.frag`) or coloured by height.
//! Tiles load one per frame, so the window stays responsive while the map fills in.

use crate::archive::Archive;
use crate::plants::Group;
use crate::terrain::{self, HeightTile};
use std::collections::{HashMap, VecDeque};
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
    /// The layers' normal maps; `None` if they did not load or differ in count or size from the colours.
    normals: Option<Texture2DArray>,
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
        let upload = |images: &[crate::texture::Image]| {
            let cpu: Vec<CpuTexture> = images
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
            Texture2DArray::new(context, &refs)
        };
        let normals_fit = layers.normals.len() == layers.colors.len()
            && layers
                .normals
                .iter()
                .all(|l| (l.width, l.height) == (first.width, first.height));
        let mut tiling = layers.tiling.clone();
        tiling.resize(64, 5.0);
        Ok(GpuLayers {
            colors: upload(&layers.colors),
            normals: normals_fit.then(|| upload(&layers.normals)),
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
    /// Use the layers' normal maps (copied from `World::layer_normals` before drawing).
    pub normals: bool,
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
            normals: true,
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
        // without normal maps the colour array is bound in their place and the shader ignores it
        let normals = self.layers.normals.as_ref().filter(|_| self.normals);
        program.use_texture_array("normalLayers", normals.unwrap_or(&self.layers.colors));
        program.use_uniform_if_required("useNormals", normals.is_some() as i32);
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
/// Step 0 is "Auto": each tile's step by its distance from the camera (`auto_step`).
pub const STEPS: [(usize, &str); 5] =
    [(0, "Auto"), (1, "Full"), (2, "1/2"), (4, "1/4"), (8, "1/8")];

/// The terrain step for a tile at `distance` metres from the camera: full detail close by, coarser further out.
/// Neighbouring tiles of different steps meet with small cracks (T-junctions), as the coarser edge skips
/// samples.
fn auto_step(distance: f32) -> usize {
    match distance {
        d if d < 400.0 => 1,
        d if d < 1000.0 => 2,
        d if d < 2500.0 => 4,
        _ => 8,
    }
}

/// Distance from `eye` to the nearest point of a tile (its x/z rectangle at its middle height).
fn tile_distance(h: &HeightTile, range: (f32, f32), eye: Vec3) -> f32 {
    let (x0, z0) = (h.origin[0], h.origin[2]);
    let (x1, z1) = (
        x0 + (h.width - 1) as f32 * h.scale[0],
        z0 + (h.depth - 1) as f32 * h.scale[2],
    );
    let p = vec3(
        eye.x.clamp(x0, x1),
        (range.0 + range.1) * 0.5,
        eye.z.clamp(z0, z1),
    );
    p.distance(eye)
}

pub enum TileState {
    NotLoaded,
    /// Loaded; `None` mesh when the tile is all holes (sea or outside the map).
    Loaded {
        heights: Option<HeightTile>,
        mesh: Option<Box<Gm<Mesh, PhysicalMaterial>>>,
        /// The ground-texture material, or why it could not be made.
        splat: Result<Box<TerrainMaterial>, String>,
        triangles: usize,
        /// The step the mesh was built with (1, 2, 4 or 8).
        built_step: usize,
        range: (f32, f32),
        /// The tile's trees (instanced), when its ground loaded.
        plants: Option<Box<Group>>,
        /// What the tile's streamed scenes place outside the towns (rocks, ruins, signs, ...).
        objects: Option<Box<Group>>,
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

/// An area (`scene::area_scenes`: a town, the academy, a cave, a Team Star base): its streamed scenes, loaded once
/// the tile it stands on is shown.
pub struct Town {
    pub code: String,
    paths: Vec<String>,
    state: TownState,
}

enum TownState {
    NotLoaded,
    /// Scenes read, models not loaded: the town lies on a hidden tile.
    Parsed {
        items: crate::plants::Items,
        warnings: Vec<String>,
        center: Vec2,
    },
    Loaded(Group),
    /// The scenes place nothing.
    Empty,
}

/// What the loader does next.
#[derive(Clone, Copy, PartialEq)]
enum Job {
    Tile(usize),
    Town(usize),
}

pub struct World {
    /// What is shown: an overworld map or a room (`maps.rs`).
    pub map: crate::maps::Map,
    /// Set by the panel's map picker; the app builds a new World for it.
    pub switch_to: Option<crate::maps::Map>,
    /// The rooms for the picker, found on first use.
    rooms: Option<Vec<(String, String)>>,
    room_filter: String,
    pub tiles: Vec<Tile>,
    pub towns: Vec<Town>,
    queue: VecDeque<Job>,
    /// Terrain mesh step: 1, 2, 4, 8, or 0 for Auto (by distance).
    pub step: usize,
    /// How placed models (trees, buildings, props) pick their LOD.
    pub lod: crate::plants::LodSettings,
    /// The camera position at the last `update`.
    eye: Vec3,
    pub shading: Shading,
    /// Set when the first load finished, so the camera moves to the map once.
    pub reframe: bool,
    framed: bool,
    /// The shared ground textures, loaded with the first tile that has ground.
    layers: Option<Result<Rc<GpuLayers>, String>>,
    /// Draw the game's sea (27 m) and its rivers, lakes and waterfalls (`water.rs`).
    pub sea: bool,
    pub rivers: bool,
    /// Bump the ground with the layers' normal maps.
    pub layer_normals: bool,
    /// Draw the tiles' trees, and the buildings and objects of the towns and tiles, within `draw_distance`
    /// metres of the camera.
    pub trees: bool,
    pub objects: bool,
    pub draw_distance: f32,
    /// Placed models (trees, buildings, props), loaded once and shared by the groups.
    plant_models: HashMap<String, Result<Rc<crate::plants::CpuModel>, String>>,
    /// Loaded with the first update.
    water: Option<crate::water::Water>,
    /// The other models of the map's resident scenes (outer scenery, the dome, a room), loaded after the water.
    resident: Option<Group>,
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
    pub fn new(ar: &Archive, map: crate::maps::Map) -> World {
        let tiles = map
            .grid
            .map(|g| g.tiles(ar))
            .unwrap_or_default()
            .into_iter()
            .map(|(x, y, path)| Tile {
                x,
                y,
                path,
                shown: true,
                state: TileState::NotLoaded,
            })
            .collect();
        // towns, the academy, caves, Team Star bases: one group each, all its size classes together
        let towns = if map.areas {
            crate::scene::area_scenes(ar)
        } else {
            Vec::new()
        };
        let towns = towns
            .into_iter()
            .map(|(code, paths)| Town {
                code,
                paths,
                state: TownState::NotLoaded,
            })
            .collect();
        World {
            map,
            switch_to: None,
            rooms: None,
            room_filter: String::new(),
            tiles,
            towns,
            queue: VecDeque::new(),
            step: 0,
            lod: crate::plants::LodSettings::default(),
            eye: vec3(0.0, 0.0, 0.0),
            shading: Shading::Splat,
            reframe: false,
            framed: false,
            layers: None,
            sea: true,
            rivers: true,
            layer_normals: true,
            trees: true,
            objects: true,
            draw_distance: 1500.0,
            plant_models: HashMap::new(),
            water: None,
            resident: None,
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

    /// Queues every shown tile that is not loaded yet, then the towns: unread ones (to find where they lie)
    /// and read ones that now stand on a shown tile.
    pub fn request_shown(&mut self) {
        for (i, t) in self.tiles.iter().enumerate() {
            let job = Job::Tile(i);
            if t.shown && matches!(t.state, TileState::NotLoaded) && !self.queue.contains(&job) {
                self.queue.push_back(job);
            }
        }
        for (i, t) in self.towns.iter().enumerate() {
            let job = Job::Town(i);
            let wanted = match &t.state {
                TownState::NotLoaded => true,
                TownState::Parsed { center, .. } => self.shown_at(*center),
                _ => false,
            };
            if wanted && !self.queue.contains(&job) {
                self.queue.push_back(job);
            }
        }
    }

    /// Whether the tile under world x/z (`at.x`, `at.y` = z) is shown.
    /// Off the grid (or in a room): true.
    fn shown_at(&self, at: Vec2) -> bool {
        let Some((x, y)) = self.map.grid.and_then(|g| g.tile_at(at.x, at.y)) else {
            return true;
        };
        self.tiles.iter().any(|t| t.x == x && t.y == y && t.shown)
    }

    pub fn loading(&self) -> usize {
        self.queue.len()
    }

    /// The step a tile is built with now: the fixed one, or by distance in Auto.
    fn wanted_step(&self, h: &HeightTile, range: (f32, f32)) -> usize {
        if self.step == 0 {
            auto_step(tile_distance(h, range, self.eye))
        } else {
            self.step
        }
    }

    /// Per frame, for a camera at `eye`: picks the LODs of the placed models in range, rebuilds one tile whose
    /// Auto step changed, then loads one queued tile or town. Returns true when something loaded.
    pub fn update(&mut self, context: &Context, ar: &mut Archive, eye: Vec3) -> bool {
        if self.water.is_none() {
            self.water = Some(crate::water::Water::load(context, ar, &self.map.resident));
            return true;
        }
        if self.resident.is_none() {
            let (items, warnings) = crate::plants::scene_items(ar, &self.map.resident);
            self.resident = Some(crate::plants::build(
                context,
                ar,
                items,
                warnings,
                &mut self.plant_models,
            ));
            if self.tiles.is_empty() && !self.framed {
                // a room: nothing else to load, frame it now
                self.framed = true;
                self.reframe = true;
            }
            return true;
        }
        self.eye = eye;
        self.update_lods();
        if self.step == 0 {
            self.auto_rebuild_one(context);
        }
        let Some(job) = self.queue.pop_front() else {
            return false;
        };
        let i = match job {
            Job::Tile(i) => i,
            Job::Town(i) => {
                self.update_town(context, ar, i);
                if self.queue.is_empty() && !self.framed {
                    self.framed = true;
                    self.reframe = true;
                }
                return true;
            }
        };
        let path = self.tiles[i].path.clone();
        let state = match terrain::load_heights(ar, &path) {
            Ok(h) => {
                let solid = h.heights.iter().filter(|v| **v != 0.0);
                let range = solid.fold((f32::MAX, f32::MIN), |(a, b), v| (a.min(*v), b.max(*v)));
                let built_step = self.wanted_step(&h, range);
                let built = build_mesh(context, &h, built_step);
                let (mesh, triangles) = built.map_or((None, 0), |(m, n)| (Some(Box::new(m)), n));
                let splat = if mesh.is_some() {
                    self.splat_material(context, ar, &path, &h).map(|mut m| {
                        m.normals = self.layer_normals;
                        Box::new(m)
                    })
                } else {
                    Err("no ground".into())
                };
                let plants = mesh.is_some().then(|| {
                    let (items, warnings) = crate::plants::tile_trees(ar, &path);
                    Box::new(crate::plants::build(
                        context,
                        ar,
                        items,
                        warnings,
                        &mut self.plant_models,
                    ))
                });
                let (x, y) = (self.tiles[i].x, self.tiles[i].y);
                let scenes = crate::scene::tile_scenes(ar, &self.map.folder, x, y);
                let objects = (!scenes.is_empty()).then(|| {
                    let (items, warnings) = crate::plants::scene_items(ar, &scenes);
                    Box::new(crate::plants::build(
                        context,
                        ar,
                        items,
                        warnings,
                        &mut self.plant_models,
                    ))
                });
                TileState::Loaded {
                    heights: mesh.is_some().then_some(h),
                    mesh,
                    splat,
                    triangles,
                    built_step,
                    range,
                    plants,
                    objects,
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

    /// Reads a town's scenes; builds its models when it stands on a shown tile.
    fn update_town(&mut self, context: &Context, ar: &mut Archive, i: usize) {
        let state = std::mem::replace(&mut self.towns[i].state, TownState::Empty);
        let (items, warnings, center) = match state {
            TownState::NotLoaded => {
                let (items, warnings) = crate::plants::scene_items(ar, &self.towns[i].paths);
                match crate::plants::extent(&items) {
                    Some((center, _)) => (items, warnings, center),
                    // places nothing: stays Empty
                    None => return,
                }
            }
            TownState::Parsed {
                items,
                warnings,
                center,
            } => (items, warnings, center),
            done => {
                self.towns[i].state = done;
                return;
            }
        };
        self.towns[i].state = if self.shown_at(center) {
            TownState::Loaded(crate::plants::build(
                context,
                ar,
                items,
                warnings,
                &mut self.plant_models,
            ))
        } else {
            TownState::Parsed {
                items,
                warnings,
                center,
            }
        };
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
        let eye = camera.position();
        let d = self.draw_distance;
        for t in self.tiles.iter().filter(|t| t.shown) {
            if let TileState::Loaded {
                plants, objects, ..
            } = &t.state
            {
                let groups = [
                    plants.as_deref().filter(|_| self.trees),
                    objects.as_deref().filter(|_| self.objects),
                ];
                for g in groups.into_iter().flatten().filter(|g| g.near(eye, d)) {
                    target.render(camera, g.objects(), lights);
                }
            }
        }
        if self.objects {
            for t in &self.towns {
                if let TownState::Loaded(g) = &t.state
                    && g.near(eye, d)
                    && self.shown_at(g.center)
                {
                    target.render(camera, g.objects(), lights);
                }
            }
        }
        // the resident scenes' models: outer scenery, or the room; not limited by the draw distance
        if let (true, Some(g)) = (self.objects, &self.resident) {
            target.render(camera, g.objects(), lights);
        }
        // last: water blends over the ground and models below it
        if let Some(w) = &self.water {
            w.render(target, camera, lights, self.sea, self.rivers);
        }
    }

    /// Rebuilds every loaded mesh at the current fixed detail step (Auto rebuilds tile by tile in `update`).
    pub fn rebuild(&mut self, context: &Context) {
        if self.step == 0 {
            return;
        }
        for t in &mut self.tiles {
            if let TileState::Loaded {
                heights: Some(h),
                mesh,
                triangles,
                built_step,
                ..
            } = &mut t.state
                && let Some((m, n)) = build_mesh(context, h, self.step)
            {
                *mesh = Some(Box::new(m));
                *triangles = n;
                *built_step = self.step;
            }
        }
    }

    /// Auto detail: rebuilds the nearest shown tile whose step for the current camera differs from its mesh's.
    /// One per frame, so moving the camera never stalls on many rebuilds at once.
    fn auto_rebuild_one(&mut self, context: &Context) {
        let mut best: Option<(f32, usize, usize)> = None;
        for (i, t) in self.tiles.iter().enumerate().filter(|(_, t)| t.shown) {
            if let TileState::Loaded {
                heights: Some(h),
                range,
                built_step,
                ..
            } = &t.state
            {
                let want = self.wanted_step(h, *range);
                let d = tile_distance(h, *range, self.eye);
                if want != *built_step && best.is_none_or(|b| d < b.0) {
                    best = Some((d, i, want));
                }
            }
        }
        let Some((_, i, want)) = best else { return };
        if let TileState::Loaded {
            heights: Some(h),
            mesh,
            triangles,
            built_step,
            ..
        } = &mut self.tiles[i].state
            && let Some((m, n)) = build_mesh(context, h, want)
        {
            *mesh = Some(Box::new(m));
            *triangles = n;
            *built_step = want;
        }
    }

    /// Sorts the instances of every placed group within the draw distance into LODs for the current camera.
    fn update_lods(&mut self) {
        let (eye, d, s) = (self.eye, self.draw_distance, self.lod);
        for t in self.tiles.iter_mut().filter(|t| t.shown) {
            if let TileState::Loaded {
                plants, objects, ..
            } = &mut t.state
            {
                for g in [plants.as_deref_mut(), objects.as_deref_mut()]
                    .into_iter()
                    .flatten()
                {
                    if g.near(eye, d) {
                        g.update_lods(eye, s);
                    }
                }
            }
        }
        for t in &mut self.towns {
            if let TownState::Loaded(g) = &mut t.state
                && g.near(eye, d)
            {
                g.update_lods(eye, s);
            }
        }
        if let Some(g) = &mut self.resident {
            g.update_lods(eye, s);
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
        // a room: its models
        if self.tiles.is_empty() {
            return self.resident.as_ref().and_then(|g| g.aabb);
        }
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
    /// Map picker: the overworld maps, and the rooms (searchable). Picking one sets `switch_to`.
    fn map_picker(&mut self, ui: &mut egui::Ui, ar: &Archive) {
        ui.horizontal(|ui| {
            ui.label("Map");
            for m in crate::maps::overworlds() {
                if ui
                    .selectable_label(self.map.name == m.name, &m.name)
                    .on_hover_text(format!(
                        "Terrain tiles, streamed scenes and {}",
                        m.resident.join(", ")
                    ))
                    .clicked()
                    && self.map.name != m.name
                {
                    self.switch_to = Some(m);
                }
            }
        });
        egui::CollapsingHeader::new("Rooms and interiors")
            .default_open(self.map.grid.is_none())
            .show(ui, |ui| {
                let rooms = self.rooms.get_or_insert_with(|| crate::maps::rooms(ar));
                ui.add(
                    egui::TextEdit::singleline(&mut self.room_filter)
                        .hint_text("search, e.g. a_t01 or sch or hairsalon"),
                );
                ui.small(format!(
                    "{} room scenes. Codes: a_tNN town, a_cNN city, a_sch academy, a_com shared, a_w23 DLC, \
                     _gNN gym, _iNN interior.",
                    rooms.len()
                ));
                let q = self.room_filter.to_lowercase();
                egui::ScrollArea::vertical()
                    .max_height(160.0)
                    .show(ui, |ui| {
                        for (code, path) in rooms.iter().filter(|(c, _)| c.contains(&q)) {
                            let current = self.map.resident.first() == Some(path) && self.map.grid.is_none();
                            if ui.selectable_label(current, code).on_hover_text(path).clicked() && !current {
                                self.switch_to = Some(crate::maps::room(code, path));
                            }
                        }
                    });
            });
    }

    pub fn panel(
        &mut self,
        ui: &mut egui::Ui,
        ar: &Archive,
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
        self.map_picker(ui, ar);
        if self.map.grid.is_some() {
            ui.small(format!(
                "{}: {} tiles of 500 m, {loaded} loaded, {with_ground} with ground.",
                self.map.name,
                self.tiles.len()
            ));
        } else {
            ui.small(format!("Room scene: {}", self.map.resident.join(", ")));
        }
        if self.loading() > 0 {
            ui.label(format!("Loading… {} tiles to go", self.loading()));
        }
        ui.add_space(4.0);

        // the minimap
        let grid = self.map.grid.unwrap_or(crate::maps::Grid {
            first: (0, 0),
            size: (0, 0),
            wrap_first_row: false,
        });
        let cell = (220.0 / grid.size.0.max(grid.size.1).max(1) as f32).min(40.0);
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(cell * grid.size.0 as f32, cell * grid.size.1 as f32),
            egui::Sense::click(),
        );
        let painter = ui.painter_at(rect);
        let cell_rect = |t: &Tile| {
            let (col, row) = grid.cell(t.x, t.y);
            let min = rect.min + egui::vec2(col as f32 * cell, row as f32 * cell);
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
        if ui
            .checkbox(&mut self.layer_normals, "Layer normal maps")
            .on_hover_text("Bump the ground with each layer's normal map (SplatNormal)")
            .changed()
        {
            let on = self.layer_normals;
            for t in &mut self.tiles {
                if let TileState::Loaded { splat: Ok(m), .. } = &mut t.state {
                    m.normals = on;
                }
            }
        }
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.trees, "Trees")
                .on_hover_text("Each tile's trees, from the instance files its header lists");
            ui.checkbox(&mut self.objects, "Towns and objects")
                .on_hover_text(
                    "Buildings, props and plants placed by the towns' and tiles' streamed scenes",
                );
        });
        ui.add(
            egui::Slider::new(&mut self.draw_distance, 200.0..=6000.0)
                .text("draw distance")
                .suffix(" m")
                .logarithmic(true),
        )
        .on_hover_text("Trees, towns and objects are drawn within this distance of the camera");
        ui.horizontal(|ui| {
            let (sea, inland) = self
                .water
                .as_ref()
                .map_or((0, 0), |w| (w.sea_triangles, w.inland_triangles));
            ui.checkbox(&mut self.sea, "Sea").on_hover_text(format!(
                "The game's sea surface at 27 m and its coast wave strips ({sea} triangles). \
                 The ground next to the sea holes is sea bed."
            ));
            ui.checkbox(&mut self.rivers, "Rivers and lakes")
                .on_hover_text(format!(
                    "Rivers, lakes, ponds, waterfalls and the academy's pools ({inland} triangles)"
                ));
        });
        if let Some(w) = &self.water {
            for e in &w.warnings {
                ui.colored_label(egui::Color32::YELLOW, format!("water: {e}"));
            }
        }
        let before = self.step;
        ui.horizontal(|ui| {
            ui.label("Terrain");
            for (s, label) in STEPS {
                let hint = if s == 0 {
                    "Each tile by its distance from the camera: full detail within 400 m, 1/2 to 1 km, \
                     1/4 to 2.5 km, 1/8 beyond"
                        .to_string()
                } else {
                    format!(
                        "Every {s}. height sample (cell {:.1} m)",
                        s as f32 * 500.0 / 512.0
                    )
                };
                ui.radio_value(&mut self.step, s, label).on_hover_text(hint);
            }
        });
        use crate::plants::LodMode;
        ui.horizontal(|ui| {
            ui.label("Model LOD");
            ui.radio_value(&mut self.lod.mode, LodMode::Auto, "Auto").on_hover_text(
                "Each tree, building and prop by its distance from the camera, at the distances the game \
                 stores per placement (most 20 m / 40 m, buildings 200-650 m)",
            );
            for k in 0..3 {
                ui.radio_value(&mut self.lod.mode, LodMode::Fixed(k), k.to_string())
                    .on_hover_text(format!("Every model at LOD {k} (or its last, if it has fewer)"));
            }
        });
        ui.add_enabled(
            self.lod.mode == LodMode::Auto,
            egui::Slider::new(&mut self.lod.scale, 0.25..=8.0)
                .logarithmic(true)
                .text("LOD distance ×"),
        )
        .on_hover_text("Scales the game's switch distances: 1 = as in the game, higher keeps detail further out");
        self.step != before
    }

    /// The right panel's summary.
    /// `target`: the point the camera orbits, whose ground height is shown (the query play mode will use).
    pub fn details(&self, ui: &mut egui::Ui, target: Vec3) {
        use crate::demo::Ground;
        ui.heading("World");
        let ground = TerrainGround(self).height(target.x, target.z);
        ui.label(format!(
            "Looking at x {:.0}, z {:.0}: {}",
            target.x,
            target.z,
            ground.map_or("no ground (hole or not loaded)".to_string(), |h| format!(
                "ground {h:.1} m"
            ))
        ))
        .on_hover_text("The ground height under the point the camera orbits, from the heightfield");
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
            ui.small(format!(
                "{} ground texture layers{}",
                l.count,
                if l.normals.is_some() {
                    ", with normal maps"
                } else {
                    " (normal maps did not load)"
                }
            ));
        }
        // counts of what is placed: trees, tile objects, towns (instances, triangles if all were drawn)
        let mut counts = [(0usize, 0usize); 3];
        let mut warnings = Vec::new();
        // what is drawn now: triangles and instances per LOD, of the groups within the draw distance
        let (mut drawn_tris, mut per_lod) = (0usize, [0usize; 3]);
        let (eye, reach) = (self.eye, self.draw_distance);
        let mut add = |k: usize, g: &Group, label: String| {
            counts[k].0 += g.instances;
            counts[k].1 += g.triangles;
            if g.near(eye, reach) && (if k == 0 { self.trees } else { self.objects }) {
                let (t, l) = g.drawn();
                drawn_tris += t;
                per_lod = std::array::from_fn(|i| per_lod[i] + l[i]);
            }
            warnings.extend(g.warnings.iter().map(|w| format!("{label}: {w}")));
        };
        for t in self.tiles.iter().filter(|t| t.shown) {
            if let TileState::Loaded {
                plants, objects, ..
            } = &t.state
            {
                let label = format!("{:02}_{:02}", t.x, t.y);
                if let Some(g) = plants {
                    add(0, g, label.clone());
                }
                if let Some(g) = objects {
                    add(1, g, label);
                }
            }
        }
        let mut towns = 0;
        for t in &self.towns {
            if let TownState::Loaded(g) = &t.state
                && self.shown_at(g.center)
            {
                towns += 1;
                add(2, g, format!("town {}", t.code));
            }
        }
        let tris = |k: usize| counts[k].1 as f64 / 1e6;
        ui.small(format!(
            "Shown tiles: {} trees ({:.1}M triangles), {} objects ({:.1}M); {towns} towns: {} objects \
             ({:.1}M). {} models loaded.",
            counts[0].0,
            tris(0),
            counts[1].0,
            tris(1),
            counts[2].0,
            tris(2),
            self.plant_models.len()
        ))
        .on_hover_text("Triangle counts are for everything placed at LOD 0; only what is within the draw distance is drawn");
        ui.small(format!(
            "Drawn now: {:.2}M model triangles; instances at LOD 0 / 1 / 2: {} / {} / {}",
            drawn_tris as f64 / 1e6,
            per_lod[0],
            per_lod[1],
            per_lod[2]
        ));
        if !warnings.is_empty() {
            ui.collapsing(format!("Placement warnings ({})", warnings.len()), |ui| {
                for w in &warnings {
                    ui.small(w);
                }
            });
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
        let holes = match self.map.name.as_str() {
            "Paldea" => {
                "holes (height 0) are the sea and Area Zero's crater, which has its own models"
            }
            "Terarium" => "holes (height 0) are the sea biome",
            _ => "holes (height 0) have no ground",
        };
        if self.map.grid.is_some() {
            ui.small(format!(
                "Heights come from each tile's height file; {holes}. Ground textures: two of the map's shared \
                 layers per splat-map texel, blended by its weights, with each layer's normal map; roughness \
                 per layer is not used yet."
            ));
        } else {
            ui.small("A room: the models its scene and sub-scenes place, and their water.");
        }
    }
}

/// The loaded tiles as ground for play mode (`demo::Ground`): the heightfield, bilinear, `None` over holes (sea,
/// the crater) and tiles not loaded. Step S1+ of PLAY_IN_SCENES_PLAN.md; not hooked to play mode yet.
pub struct TerrainGround<'a>(pub &'a World);

impl crate::demo::Ground for TerrainGround<'_> {
    fn height(&self, x: f32, z: f32) -> Option<f32> {
        self.0.tiles.iter().find_map(|t| match &t.state {
            TileState::Loaded {
                heights: Some(h), ..
            } => h.height_at(x, z),
            _ => None,
        })
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
