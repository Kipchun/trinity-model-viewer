//! Materials of the model view: three-d's physical material, or the eye material for Pokémon eyes.
//!
//! Pokémon eyes ("Eye", "EyeClearCoat" shaders; see `materials::Eye`) get their shine from a highlight on the clear
//! coat; the eye material draws it over the layered colour (shader `shaders/eye.frag`) as a white spot reflected
//! from a light fixed to the camera. The game's formula is not known; this is an approximation.

use three_d::*;

#[derive(Clone)]
pub struct EyeMaterial {
    pub albedo: Texture2DRef,
    pub normal: Option<Texture2DRef>,
    pub coat: Option<Texture2DRef>,
    pub roughness: f32,
    pub highlight: f32,
    pub highlight_roughness: f32,
}

impl Material for EyeMaterial {
    fn fragment_shader_source(&self, lights: &[&dyn Light]) -> String {
        let mut s = lights_shader_source(lights);
        s.push_str(ToneMapping::fragment_shader_source());
        s.push_str(ColorMapping::fragment_shader_source());
        s.push_str(include_str!("shaders/eye.frag"));
        s
    }

    fn id(&self) -> EffectMaterialId {
        // public-use range (0x0000..0x4FFF); the terrain material is 0x0100
        EffectMaterialId(0x0101)
    }

    fn use_uniforms(&self, program: &Program, viewer: &dyn Viewer, lights: &[&dyn Light]) {
        viewer.tone_mapping().use_uniforms(program);
        viewer.color_mapping().use_uniforms(program);
        program.use_uniform_if_required("cameraPosition", viewer.position());
        for (i, light) in lights.iter().enumerate() {
            light.use_uniforms(program, i as u32);
        }
        program.use_texture("albedoTexture", &self.albedo);
        // unused samplers still need a texture bound; the albedo stands in
        program.use_texture(
            "normalTexture",
            self.normal.as_ref().unwrap_or(&self.albedo),
        );
        program.use_texture("coatTexture", self.coat.as_ref().unwrap_or(&self.albedo));
        program.use_uniform_if_required("useNormal", self.normal.is_some() as i32);
        program.use_uniform_if_required("useCoat", self.coat.is_some() as i32);
        program.use_uniform_if_required("roughness", self.roughness);
        // the highlight's light, fixed to the camera: up-left and towards the viewer
        let to_world = viewer.view().invert().unwrap_or(Mat4::identity());
        let dir = (to_world * vec3(-0.35, 0.5, 1.0).normalize().extend(0.0)).truncate();
        program.use_uniform_if_required("highlightDir", dir.normalize());
        program.use_uniform_if_required("highlight", self.highlight);
        program.use_uniform_if_required("highlightRoughness", self.highlight_roughness);
    }

    fn render_states(&self) -> RenderStates {
        RenderStates::default()
    }

    fn material_type(&self) -> MaterialType {
        MaterialType::Opaque
    }
}

/// The material of one part of the model.
#[derive(Clone)]
pub enum ModelMaterial {
    Physical(Box<PhysicalMaterial>),
    Eye(EyeMaterial),
}

impl ModelMaterial {
    fn inner(&self) -> &dyn Material {
        match self {
            ModelMaterial::Physical(m) => m.as_ref(),
            ModelMaterial::Eye(m) => m,
        }
    }
}

impl Material for ModelMaterial {
    fn fragment_shader_source(&self, lights: &[&dyn Light]) -> String {
        self.inner().fragment_shader_source(lights)
    }
    fn id(&self) -> EffectMaterialId {
        self.inner().id()
    }
    fn use_uniforms(&self, program: &Program, viewer: &dyn Viewer, lights: &[&dyn Light]) {
        self.inner().use_uniforms(program, viewer, lights)
    }
    fn render_states(&self) -> RenderStates {
        self.inner().render_states()
    }
    fn material_type(&self) -> MaterialType {
        self.inner().material_type()
    }
}
