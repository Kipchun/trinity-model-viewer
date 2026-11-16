// Pokémon eye material (see `EyeMaterial` in eye.rs). Appended after three-d's lighting, tone mapping and colour
// mapping sources, which provide calculate_lighting, tone_mapping and color_mapping.
//
// The layered eye colour is lit as the rest of the model is. The highlight is a white spot reflected off the clear
// coat (NormalMap1: the bulge of the cornea) from a light fixed to the camera, up-left and in front
// (`highlightDir`), so the shine shows from every side as in the game's close-ups. The game's textures hold no
// highlight. Approximation: its exact formula is not known; RoughnessHighlight sets the spot size.

uniform vec3 cameraPosition;
uniform sampler2D albedoTexture;
uniform sampler2D normalTexture;
uniform sampler2D coatTexture;
uniform int useNormal;
uniform int useCoat;
uniform float roughness;
uniform float highlight;
uniform float highlightRoughness;
uniform vec3 highlightDir;

in vec3 pos;
in vec3 nor;
in vec3 tang;
in vec3 bitang;
in vec2 uvs;

layout (location = 0) out vec4 outColor;

// The same tangent-space convention as three-d's physical material, so these normals match the rest of the model.
vec3 mapped(sampler2D tex, mat3 tbn) {
    return normalize(tbn * (2.0 * texture(tex, uvs).xyz - 1.0));
}

void main()
{
    vec3 normal = normalize(gl_FrontFacing ? nor : -nor);
    vec3 tangent = normalize(gl_FrontFacing ? tang : -tang);
    vec3 bitangent = normalize(gl_FrontFacing ? bitang : -bitang);
    mat3 tbn = mat3(tangent, bitangent, normal);

    vec3 baseNormal = useNormal == 1 ? mapped(normalTexture, tbn) : normal;
    vec3 coatNormal = useCoat == 1 ? mapped(coatTexture, tbn) : normal;

    vec3 surface = texture(albedoTexture, uvs).rgb;
    vec3 color = calculate_lighting(cameraPosition, surface, pos, baseNormal, 0.0, roughness, 1.0);
    if (highlight > 0.0) {
        // Blinn-Phong lobe matching GGX roughness r (alpha = r^2): exponent 2 / alpha^2 - 2, kept to a visible spot
        vec3 V = normalize(cameraPosition - pos);
        vec3 H = normalize(V + highlightDir);
        float a = highlightRoughness * highlightRoughness;
        float n = clamp(2.0 / (a * a) - 2.0, 40.0, 4000.0);
        float spot = pow(max(dot(coatNormal, H), 0.0), n);
        // a crisp edge, as the game's highlights are drawn
        spot = smoothstep(0.25, 0.6, spot);
        color = mix(color, vec3(1.0), clamp(highlight * spot, 0.0, 1.0));
    }

    outColor.rgb = tone_mapping(color);
    outColor.rgb = color_mapping(outColor.rgb);
    outColor.a = 1.0;
}
