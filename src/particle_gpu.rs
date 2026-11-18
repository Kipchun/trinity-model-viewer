//! Drawing for `particles.rs`: a three-d material that mixes up to three textures and two colours the way the
//! emitter's combiner says (`vfxb::Combiner`, shader `shaders/particle.frag`), and a geometry with the vertex data
//! it needs (three UV sets, colour 0 and colour 1), either quads built on the CPU or a primitive mesh drawn once per
//! particle (instanced: a matrix, the colours and three UV transforms per particle).

use crate::vfxb::Combiner;
use three_d::*;

/// How far texture 0 pushes the UVs of textures 1-2 when it distorts them (the strength in the file is not found;
/// a guess that looks like a waver).
const DISTORTION: f32 = 0.08;

#[derive(Clone)]
pub struct ParticleMaterial {
    pub textures: [Option<Texture2DRef>; 3],
    pub blend: u8,
    pub color_scale: f32,
    pub combiner: Combiner,
    /// The mesh has vertex colours (the combiner's "primitive" input); quads and meshes without them skip it.
    pub vertex_colors: bool,
    /// Texture 0 only pushes the UVs of textures 1-2 (shader type 1 or 2 with a two-channel texture 0).
    pub distort: bool,
}

impl Material for ParticleMaterial {
    fn fragment_shader_source(&self, _lights: &[&dyn Light]) -> String {
        include_str!("shaders/particle.frag").to_string()
    }

    fn id(&self) -> EffectMaterialId {
        // public-use range; terrain 0x0100, eye 0x0101
        EffectMaterialId(0x0110)
    }

    fn use_uniforms(&self, program: &Program, _viewer: &dyn Viewer, _lights: &[&dyn Light]) {
        let c = &self.combiner;
        for (i, t) in self.textures.iter().enumerate() {
            if let Some(t) = t {
                program.use_texture(&format!("tex{i}"), t);
            }
            program.use_uniform_if_required(&format!("useTex{i}"), t.is_some() as i32);
            program.use_uniform_if_required(&format!("colorIn{i}"), c.color_input[i] as i32);
            program.use_uniform_if_required(&format!("alphaIn{i}"), c.alpha_input[i] as i32);
        }
        for (i, name) in ["1", "2", "Prim"].iter().enumerate() {
            program.use_uniform_if_required(&format!("colorBlend{name}"), c.color_blend[i] as i32);
            program.use_uniform_if_required(&format!("alphaBlend{name}"), c.alpha_blend[i] as i32);
        }
        program.use_uniform_if_required("colorInPrim", c.color_input[3] as i32);
        program.use_uniform_if_required("alphaInPrim", c.alpha_input[3] as i32);
        program.use_uniform_if_required("usePrim", self.vertex_colors as i32);
        program.use_uniform_if_required("colorProcess", c.color_process as i32);
        program.use_uniform_if_required("alphaProcess", c.alpha_process as i32);
        program.use_uniform_if_required("distort", self.distort as i32);
        program.use_uniform_if_required("distortion", DISTORTION);
        program.use_uniform_if_required("colorScale", self.color_scale);
        program.use_uniform_if_required("premultiplyForAdd", (self.blend == 1) as i32);
    }

    fn render_states(&self) -> RenderStates {
        use BlendMultiplierType as M;
        let blend = match self.blend {
            // add: rgb already weighted by alpha in the shader
            1 => Blend::Enabled {
                source_rgb_multiplier: M::One,
                source_alpha_multiplier: M::Zero,
                destination_rgb_multiplier: M::One,
                destination_alpha_multiplier: M::One,
                rgb_equation: BlendEquationType::Add,
                alpha_equation: BlendEquationType::Add,
            },
            // subtract
            2 => Blend::Enabled {
                source_rgb_multiplier: M::SrcAlpha,
                source_alpha_multiplier: M::Zero,
                destination_rgb_multiplier: M::One,
                destination_alpha_multiplier: M::One,
                rgb_equation: BlendEquationType::ReverseSubtract,
                alpha_equation: BlendEquationType::Add,
            },
            // screen
            3 => Blend::Enabled {
                source_rgb_multiplier: M::OneMinusDstColor,
                source_alpha_multiplier: M::Zero,
                destination_rgb_multiplier: M::One,
                destination_alpha_multiplier: M::One,
                rgb_equation: BlendEquationType::Add,
                alpha_equation: BlendEquationType::Add,
            },
            // multiply
            4 => Blend::Enabled {
                source_rgb_multiplier: M::DstColor,
                source_alpha_multiplier: M::Zero,
                destination_rgb_multiplier: M::Zero,
                destination_alpha_multiplier: M::One,
                rgb_equation: BlendEquationType::Add,
                alpha_equation: BlendEquationType::Add,
            },
            _ => Blend::TRANSPARENCY,
        };
        RenderStates {
            write_mask: WriteMask::COLOR,
            blend,
            cull: Cull::None,
            ..Default::default()
        }
    }

    fn material_type(&self) -> MaterialType {
        MaterialType::Transparent
    }
}

/// One quad corner: position, colour 0, colour 1 (RGBA, may pass 1), UVs of the three samplers.
#[derive(Clone, Copy)]
pub struct QuadVertex {
    pub pos: Vec3,
    pub c0: Vec4,
    pub c1: Vec4,
    pub uv: [Vec2; 3],
}

/// One particle drawn as the primitive mesh: placement, colours, and per sampler a UV transform
/// (scale xy, offset zw).
#[derive(Clone, Copy)]
pub struct MeshInstance {
    pub transform: Mat4,
    pub c0: Vec4,
    pub c1: Vec4,
    pub uv: [Vec4; 3],
}

#[allow(clippy::large_enum_variant)] // one per emitter
enum Kind {
    Quads {
        position: VertexBuffer<Vec3>,
        c0: VertexBuffer<Vec4>,
        c1: VertexBuffer<Vec4>,
        uv01: VertexBuffer<Vec4>,
        uv2: VertexBuffer<Vec2>,
        count: u32,
    },
    Mesh {
        position: VertexBuffer<Vec3>,
        uv: VertexBuffer<Vec2>,
        color: VertexBuffer<Vec4>,
        indices: ElementBuffer<u32>,
        rows: [InstanceBuffer<Vec4>; 3],
        c0: InstanceBuffer<Vec4>,
        c1: InstanceBuffer<Vec4>,
        st: [InstanceBuffer<Vec4>; 3],
        count: u32,
    },
}

pub struct ParticleGeometry {
    context: Context,
    kind: Kind,
    aabb: AxisAlignedBoundingBox,
}

impl ParticleGeometry {
    pub fn quads(context: &Context) -> Self {
        ParticleGeometry {
            context: context.clone(),
            kind: Kind::Quads {
                position: VertexBuffer::new(context),
                c0: VertexBuffer::new(context),
                c1: VertexBuffer::new(context),
                uv01: VertexBuffer::new(context),
                uv2: VertexBuffer::new(context),
                count: 0,
            },
            aabb: AxisAlignedBoundingBox::EMPTY,
        }
    }

    /// A primitive mesh; `colors` empty when it has none.
    pub fn mesh(
        context: &Context,
        positions: &[Vec3],
        uvs: &[Vec2],
        colors: &[Vec4],
        indices: &[u32],
    ) -> Self {
        let colors = if colors.len() == positions.len() {
            colors.to_vec()
        } else {
            vec![vec4(1.0, 1.0, 1.0, 1.0); positions.len()]
        };
        ParticleGeometry {
            context: context.clone(),
            kind: Kind::Mesh {
                position: VertexBuffer::new_with_data(context, positions),
                uv: VertexBuffer::new_with_data(context, uvs),
                color: VertexBuffer::new_with_data(context, &colors),
                indices: ElementBuffer::new_with_data(context, indices),
                rows: std::array::from_fn(|_| InstanceBuffer::new(context)),
                c0: InstanceBuffer::new(context),
                c1: InstanceBuffer::new(context),
                st: std::array::from_fn(|_| InstanceBuffer::new(context)),
                count: 0,
            },
            aabb: AxisAlignedBoundingBox::EMPTY,
        }
    }

    /// Sets the quads (6 vertices each, two triangles) and the bounds they fill.
    pub fn set_quads(&mut self, v: &[QuadVertex], aabb: AxisAlignedBoundingBox) {
        if let Kind::Quads {
            position,
            c0,
            c1,
            uv01,
            uv2,
            count,
        } = &mut self.kind
        {
            position.fill(&v.iter().map(|x| x.pos).collect::<Vec<_>>());
            c0.fill(&v.iter().map(|x| x.c0).collect::<Vec<_>>());
            c1.fill(&v.iter().map(|x| x.c1).collect::<Vec<_>>());
            uv01.fill(
                &v.iter()
                    .map(|x| vec4(x.uv[0].x, x.uv[0].y, x.uv[1].x, x.uv[1].y))
                    .collect::<Vec<_>>(),
            );
            uv2.fill(&v.iter().map(|x| x.uv[2]).collect::<Vec<_>>());
            *count = v.len() as u32;
            self.aabb = aabb;
        }
    }

    /// Sets the particles drawn with the mesh (in draw order) and the bounds they fill.
    pub fn set_instances(&mut self, v: &[MeshInstance], aabb: AxisAlignedBoundingBox) {
        if let Kind::Mesh {
            rows,
            c0,
            c1,
            st,
            count,
            ..
        } = &mut self.kind
        {
            // rows of the 3x4 part of each matrix (as three-d's instanced meshes send them)
            for (r, buf) in rows.iter_mut().enumerate() {
                buf.fill(
                    &v.iter()
                        .map(|x| {
                            let m = x.transform;
                            vec4(m.x[r], m.y[r], m.z[r], m.w[r])
                        })
                        .collect::<Vec<_>>(),
                );
            }
            c0.fill(&v.iter().map(|x| x.c0).collect::<Vec<_>>());
            c1.fill(&v.iter().map(|x| x.c1).collect::<Vec<_>>());
            for (n, buf) in st.iter_mut().enumerate() {
                buf.fill(&v.iter().map(|x| x.uv[n]).collect::<Vec<_>>());
            }
            *count = v.len() as u32;
            self.aabb = aabb;
        }
    }

    pub fn is_empty(&self) -> bool {
        match &self.kind {
            Kind::Quads { count, .. } | Kind::Mesh { count, .. } => *count == 0,
        }
    }
}

fn vertex<T: BufferDataType>(p: &Program, name: &str, b: &VertexBuffer<T>) {
    if p.requires_attribute(name) {
        p.use_vertex_attribute(name, b);
    }
}

fn instance<T: BufferDataType>(p: &Program, name: &str, b: &InstanceBuffer<T>) {
    if p.requires_attribute(name) {
        p.use_instance_attribute(name, b);
    }
}

impl Geometry for ParticleGeometry {
    fn draw(&self, viewer: &dyn Viewer, program: &Program, render_states: RenderStates) {
        program.use_uniform("viewProjection", viewer.projection() * viewer.view());
        match &self.kind {
            Kind::Quads {
                position,
                c0,
                c1,
                uv01,
                uv2,
                count,
            } => {
                if *count == 0 {
                    return;
                }
                vertex(program, "position", position);
                vertex(program, "color0", c0);
                vertex(program, "color1", c1);
                vertex(program, "uv01", uv01);
                vertex(program, "uv2in", uv2);
                program.draw_arrays(render_states, viewer.viewport(), *count);
            }
            Kind::Mesh {
                position,
                uv,
                color,
                indices,
                rows,
                c0,
                c1,
                st,
                count,
            } => {
                if *count == 0 {
                    return;
                }
                vertex(program, "position", position);
                vertex(program, "uv", uv);
                vertex(program, "vertexColor", color);
                for (i, r) in rows.iter().enumerate() {
                    instance(program, &format!("row{}", i + 1), r);
                }
                instance(program, "color0", c0);
                instance(program, "color1", c1);
                for (i, s) in st.iter().enumerate() {
                    instance(program, &format!("st{i}"), s);
                }
                program.draw_elements_instanced(render_states, viewer.viewport(), indices, *count);
            }
        }
    }

    fn vertex_shader_source(&self) -> String {
        match self.kind {
            Kind::Quads { .. } => QUAD_VERT.to_string(),
            Kind::Mesh { .. } => MESH_VERT.to_string(),
        }
    }

    fn id(&self) -> GeometryId {
        // public-use range
        match self.kind {
            Kind::Quads { .. } => GeometryId(0x0110),
            Kind::Mesh { .. } => GeometryId(0x0111),
        }
    }

    fn render_with_material(
        &self,
        material: &dyn Material,
        viewer: &dyn Viewer,
        lights: &[&dyn Light],
    ) {
        if let Err(e) = render_with_material(&self.context, viewer, self, material, lights) {
            panic!("{e}");
        }
    }

    fn render_with_effect(
        &self,
        material: &dyn Effect,
        viewer: &dyn Viewer,
        lights: &[&dyn Light],
        color_texture: Option<ColorTexture>,
        depth_texture: Option<DepthTexture>,
    ) {
        if let Err(e) = render_with_effect(
            &self.context,
            viewer,
            self,
            material,
            lights,
            color_texture,
            depth_texture,
        ) {
            panic!("{e}");
        }
    }

    fn aabb(&self) -> AxisAlignedBoundingBox {
        self.aabb
    }
}

const QUAD_VERT: &str = "
uniform mat4 viewProjection;
in vec3 position;
in vec4 color0;
in vec4 color1;
in vec4 uv01;
in vec2 uv2in;
out vec2 uv0;
out vec2 uv1;
out vec2 uv2;
out vec4 c0;
out vec4 c1;
out vec4 vcol;
void main()
{
    // three-d uploads textures with their rows flipped, so v is flipped too
    uv0 = vec2(uv01.x, 1.0 - uv01.y);
    uv1 = vec2(uv01.z, 1.0 - uv01.w);
    uv2 = vec2(uv2in.x, 1.0 - uv2in.y);
    c0 = color0;
    c1 = color1;
    vcol = vec4(1.0);
    gl_Position = viewProjection * vec4(position, 1.0);
}
";

const MESH_VERT: &str = "
uniform mat4 viewProjection;
in vec3 position;
in vec2 uv;
in vec4 vertexColor;
in vec4 row1;
in vec4 row2;
in vec4 row3;
in vec4 color0;
in vec4 color1;
in vec4 st0;
in vec4 st1;
in vec4 st2;
out vec2 uv0;
out vec2 uv1;
out vec2 uv2;
out vec4 c0;
out vec4 c1;
out vec4 vcol;
void main()
{
    mat4 m;
    m[0] = vec4(row1.x, row2.x, row3.x, 0.0);
    m[1] = vec4(row1.y, row2.y, row3.y, 0.0);
    m[2] = vec4(row1.z, row2.z, row3.z, 0.0);
    m[3] = vec4(row1.w, row2.w, row3.w, 1.0);
    // three-d uploads textures with their rows flipped, so v is flipped too
    uv0 = uv * st0.xy + st0.zw;
    uv1 = uv * st1.xy + st1.zw;
    uv2 = uv * st2.xy + st2.zw;
    uv0.y = 1.0 - uv0.y;
    uv1.y = 1.0 - uv1.y;
    uv2.y = 1.0 - uv2.y;
    c0 = color0;
    c1 = color1;
    vcol = vertexColor;
    gl_Position = viewProjection * (m * vec4(position, 1.0));
}
";
