// Terrain splat material (see `TerrainMaterial` in world.rs). Appended after three-d's lighting, tone mapping and
// colour mapping sources, which provide calculate_lighting, tone_mapping and color_mapping.
//
// The control map has one texel per height cell: R and G pick two layers of the colour array, B and A weigh them.
// Each of the four nearest texels is blended on its own (indices cannot be interpolated), then the four results
// are mixed bilinearly. Layers repeat every `layerTiling[layer]` metres in world x/z, so tiles join seamlessly.

precision highp sampler2DArray;

uniform vec3 cameraPosition;
uniform sampler2DArray colorLayers;
uniform sampler2D control;
uniform vec2 tileOrigin;
uniform vec2 cellSize;
uniform float layerTiling[64];
uniform int layerCount;

in vec3 pos;
in vec3 nor;

layout (location = 0) out vec4 outColor;

vec3 layerColor(int layer, vec2 dx, vec2 dy) {
    int l = clamp(layer, 0, layerCount - 1);
    // v runs against z because three-d stores the layers' rows flipped; this keeps them the way round they are drawn
    vec2 s = vec2(1.0, -1.0) / layerTiling[l];
    // explicit gradients: the layer (and so the scale) can change between neighbouring pixels
    vec3 c = textureGrad(colorLayers, vec3(pos.xz * s, float(l)), dx * s, dy * s).rgb;
    return pow(c, vec3(2.2)); // the layers are sRGB
}

vec3 texelColor(ivec2 t, vec2 dx, vec2 dy) {
    ivec2 size = textureSize(control, 0);
    t = clamp(t, ivec2(0), size - 1);
    // three-d stores the image's first row as the texture's last (GL origin bottom-left)
    vec4 s = texelFetch(control, ivec2(t.x, size.y - 1 - t.y), 0);
    int a = int(s.r * 255.0 + 0.5);
    int b = int(s.g * 255.0 + 0.5);
    float sum = max(s.b + s.a, 1e-4);
    return (layerColor(a, dx, dy) * s.b + layerColor(b, dx, dy) * s.a) / sum;
}

void main()
{
    vec2 dx = dFdx(pos.xz);
    vec2 dy = dFdy(pos.xz);
    vec2 t = (pos.xz - tileOrigin) / cellSize - 0.5;
    ivec2 i = ivec2(floor(t));
    vec2 f = t - floor(t);
    vec3 c00 = texelColor(i, dx, dy);
    vec3 c10 = texelColor(i + ivec2(1, 0), dx, dy);
    vec3 c01 = texelColor(i + ivec2(0, 1), dx, dy);
    vec3 c11 = texelColor(i + ivec2(1, 1), dx, dy);
    vec3 surface = mix(mix(c00, c10, f.x), mix(c01, c11, f.x), f.y);

    vec3 normal = normalize(gl_FrontFacing ? nor : -nor);
    outColor.rgb = calculate_lighting(cameraPosition, surface, pos, normal, 0.0, 0.95, 1.0);
    outColor.rgb = tone_mapping(outColor.rgb);
    outColor.rgb = color_mapping(outColor.rgb);
    outColor.a = 1.0;
}
