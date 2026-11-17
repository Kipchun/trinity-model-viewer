// Terrain splat material (see `TerrainMaterial` in world.rs). Appended after three-d's lighting, tone mapping and
// colour mapping sources, which provide calculate_lighting, tone_mapping and color_mapping.
//
// The control map has one texel per height cell: R and G pick two layers of the colour array, B and A weigh them.
// Each of the four nearest texels is blended on its own (indices cannot be interpolated), then the four results
// are mixed bilinearly. Layers repeat every `layerTiling[layer]` metres in world x/z, so tiles join seamlessly.
// The layers' normal maps are blended the same way and tilt the mesh normal (OpenGL convention, as for models:
// x along the layer's u = world +x, y along its v = world -z).

precision highp sampler2DArray;

uniform vec3 cameraPosition;
uniform sampler2DArray colorLayers;
uniform sampler2DArray normalLayers;
uniform int useNormals;
uniform sampler2D control;
uniform vec2 tileOrigin;
uniform vec2 cellSize;
uniform float layerTiling[64];
uniform int layerCount;

in vec3 pos;
in vec3 nor;

layout (location = 0) out vec4 outColor;

// One layer at this pixel: linear colour, and the normal map's x/y in -1..1.
void layerSample(int layer, vec2 dx, vec2 dy, out vec3 color, out vec2 bump) {
    int l = clamp(layer, 0, layerCount - 1);
    // v runs against z because three-d stores the layers' rows flipped; this keeps them the way round they are drawn
    vec2 s = vec2(1.0, -1.0) / layerTiling[l];
    vec3 uv = vec3(pos.xz * s, float(l));
    // explicit gradients: the layer (and so the scale) can change between neighbouring pixels
    color = pow(textureGrad(colorLayers, uv, dx * s, dy * s).rgb, vec3(2.2)); // the layers are sRGB
    bump = useNormals != 0 ? textureGrad(normalLayers, uv, dx * s, dy * s).rg * 2.0 - 1.0 : vec2(0.0);
}

void texelSample(ivec2 t, vec2 dx, vec2 dy, out vec3 color, out vec2 bump) {
    ivec2 size = textureSize(control, 0);
    t = clamp(t, ivec2(0), size - 1);
    // three-d stores the image's first row as the texture's last (GL origin bottom-left)
    vec4 s = texelFetch(control, ivec2(t.x, size.y - 1 - t.y), 0);
    int a = int(s.r * 255.0 + 0.5);
    int b = int(s.g * 255.0 + 0.5);
    float sum = max(s.b + s.a, 1e-4);
    vec3 ca, cb;
    vec2 na, nb;
    layerSample(a, dx, dy, ca, na);
    layerSample(b, dx, dy, cb, nb);
    color = (ca * s.b + cb * s.a) / sum;
    bump = (na * s.b + nb * s.a) / sum;
}

void main()
{
    vec2 dx = dFdx(pos.xz);
    vec2 dy = dFdy(pos.xz);
    vec2 t = (pos.xz - tileOrigin) / cellSize - 0.5;
    ivec2 i = ivec2(floor(t));
    vec2 f = t - floor(t);
    vec3 c00, c10, c01, c11;
    vec2 n00, n10, n01, n11;
    texelSample(i, dx, dy, c00, n00);
    texelSample(i + ivec2(1, 0), dx, dy, c10, n10);
    texelSample(i + ivec2(0, 1), dx, dy, c01, n01);
    texelSample(i + ivec2(1, 1), dx, dy, c11, n11);
    vec3 surface = mix(mix(c00, c10, f.x), mix(c01, c11, f.x), f.y);
    vec2 bump = mix(mix(n00, n10, f.x), mix(n01, n11, f.x), f.y);

    vec3 normal = normalize(gl_FrontFacing ? nor : -nor);
    // tangent frame of the world-aligned layer mapping, kept perpendicular to the mesh normal
    vec3 tangent = normalize(vec3(1.0, 0.0, 0.0) - normal * normal.x);
    vec3 bitangent = normalize(cross(normal, tangent)); // = world -z on flat ground
    float z = sqrt(max(1.0 - dot(bump, bump), 0.0));
    normal = normalize(tangent * bump.x + bitangent * bump.y + normal * z);

    outColor.rgb = calculate_lighting(cameraPosition, surface, pos, normal, 0.0, 0.95, 1.0);
    outColor.rgb = tone_mapping(outColor.rgb);
    outColor.rgb = color_mapping(outColor.rgb);
    outColor.a = 1.0;
}
