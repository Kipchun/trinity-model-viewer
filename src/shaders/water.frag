// Water material (see `WaterMaterial` in water.rs). Appended after three-d's lighting, tone mapping and colour
// mapping sources, which provide calculate_lighting, tone_mapping and color_mapping.
//
// Approximation of the game's "Water" shader, from its material parameters: two normal maps scroll over the
// surface (each with its own UV scale/offset and speed) and tilt the normal; the base colour is lit by the
// viewer's lights (the sun's glint comes from the low roughness) and mixed with a fixed sky colour by Fresnel;
// foam is the foam mask times the vertex colour's red; opacity is the base colour's alpha, rising to opaque at
// grazing angles, times the vertex alpha (river edges fade out).

uniform vec3 cameraPosition;
uniform sampler2D normal0;
uniform sampler2D normal1;
uniform sampler2D foamMask;
uniform vec4 baseColor;
uniform vec4 foamColor;
uniform vec4 uvScaleOffset0;
uniform vec4 uvScaleOffset1;
uniform vec2 scroll0;
uniform vec2 scroll1;
uniform float foamScale;
uniform float foamIntensity;
uniform float foamSharpness;
uniform float roughness;
uniform int vertexAlpha;
uniform int vertexFoam;
uniform float time;

in vec3 pos;
in vec3 nor;
in vec2 uvs;
in vec4 col;

layout (location = 0) out vec4 outColor;

const vec3 skyColor = vec3(0.45, 0.62, 0.82);

// BC5 normal map: x/y in red/green, -1..1.
vec2 bump(sampler2D tex, vec2 uv) {
    return texture(tex, uv).rg * 2.0 - 1.0;
}

void main()
{
    vec3 n = normalize(gl_FrontFacing ? nor : -nor);

    // tangent frame from screen-space derivatives of position and UV (the meshes carry no tangents)
    vec3 dp1 = dFdx(pos);
    vec3 dp2 = dFdy(pos);
    vec2 duv1 = dFdx(uvs);
    vec2 duv2 = dFdy(uvs);
    vec3 dp2perp = cross(dp2, n);
    vec3 dp1perp = cross(n, dp1);
    vec3 t = dp2perp * duv1.x + dp1perp * duv2.x;
    vec3 b = dp2perp * duv1.y + dp1perp * duv2.y;
    float m = max(dot(t, t), dot(b, b));
    float scale = m > 1e-20 ? inversesqrt(m) : 0.0;
    t *= scale;
    b *= scale;

    vec2 uvA = uvs * uvScaleOffset0.xy + uvScaleOffset0.zw + scroll0 * time;
    vec2 uvB = uvs * uvScaleOffset1.xy + uvScaleOffset1.zw + scroll1 * time;
    vec2 d = bump(normal0, uvA) + bump(normal1, uvB);
    vec3 N = normalize(n + 1.5 * (t * d.x + b * d.y));

    vec3 V = normalize(cameraPosition - pos);
    float fresnel = 0.02 + 0.98 * pow(1.0 - clamp(dot(N, V), 0.0, 1.0), 5.0);

    // body colour lit softly (rough), the glint added on its own (black albedo: only the specular term remains),
    // so several lights do not wash the surface out; the body is darkened for the depth the game fogs
    vec3 body = calculate_lighting(cameraPosition, baseColor.rgb * 0.6, pos, N, 0.0, 1.0, 1.0);
    vec3 glint = calculate_lighting(cameraPosition, vec3(0.0), pos, N, 0.0, roughness, 1.0);
    vec3 color = mix(body, skyColor, fresnel * 0.8) + glint;

    float foam = 0.0;
    if (vertexFoam != 0) {
        float mask = texture(foamMask, uvs * foamScale + scroll0 * time).r;
        foam = pow(clamp(mask * col.r * foamIntensity, 0.0, 1.0), foamSharpness);
        vec3 foamLit = calculate_lighting(cameraPosition, min(foamColor.rgb, vec3(1.0)), pos, n, 0.0, 0.9, 1.0);
        color = mix(color, foamLit, foam);
    }

    float alpha = mix(baseColor.a, 1.0, fresnel);
    alpha = max(alpha, foam);
    if (vertexAlpha != 0) {
        alpha *= col.a;
    }

    outColor.rgb = tone_mapping(color);
    outColor.rgb = color_mapping(outColor.rgb);
    outColor.a = clamp(alpha, 0.0, 1.0);
}
