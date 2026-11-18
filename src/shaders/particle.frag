// Particle material (see `ParticleMaterial` in particle_gpu.rs): the emitter's combiner (`vfxb::Combiner`) mixes
// up to three textures and the mesh's vertex colour into one texture value, then the colour and alpha processes
// mix that with colour 0 / colour 1 and alpha 0 / alpha 1 (vertex data, may pass 1 for glows). Textures are
// sampled through their BNTX channel selectors (applied on upload), so single-channel masks read as R in every
// channel. Meanings of the modes: see the doc comment of `vfxb::Combiner`; the dissolve and distance-field formulas
// are guesses.

uniform sampler2D tex0;
uniform sampler2D tex1;
uniform sampler2D tex2;
uniform int useTex0;
uniform int useTex1;
uniform int useTex2;
uniform int colorIn0;
uniform int colorIn1;
uniform int colorIn2;
uniform int alphaIn0;
uniform int alphaIn1;
uniform int alphaIn2;
uniform int colorBlend1;
uniform int colorBlend2;
uniform int colorBlendPrim;
uniform int alphaBlend1;
uniform int alphaBlend2;
uniform int alphaBlendPrim;
uniform int colorInPrim;
uniform int alphaInPrim;
uniform int usePrim;
uniform int colorProcess;
uniform int alphaProcess;
uniform int distort;
uniform float distortion;
uniform float colorScale;
uniform int premultiplyForAdd;

in vec2 uv0;
in vec2 uv1;
in vec2 uv2;
in vec4 c0;
in vec4 c1;
in vec4 vcol;

layout (location = 0) out vec4 outColor;

// input kinds: 0 as sampled, 1 red, 2 one, 3 alpha (colour) / not known (alpha, taken as sampled)
vec3 colorInput(vec4 t, int kind)
{
    if (kind == 1) return vec3(t.r);
    if (kind == 2) return vec3(1.0);
    if (kind == 3) return vec3(t.a);
    return t.rgb;
}

float alphaInput(vec4 t, int kind)
{
    if (kind == 1) return t.r;
    if (kind == 2) return 1.0;
    return t.a;
}

vec3 blendColor(vec3 a, vec3 b, int mode)
{
    if (mode == 1) return a + b;
    if (mode == 2) return a - b;
    return a * b;
}

float blendAlpha(float a, float b, int mode)
{
    if (mode == 1) return a + b;
    if (mode == 2) return a - b;
    return a * b;
}

void main()
{
    vec3 rgb = vec3(1.0);
    float a = 1.0;
    vec2 d = vec2(0.0);
    bool started = false;
    if (useTex0 == 1) {
        vec4 t = texture(tex0, uv0);
        if (distort == 1) {
            // texture 0 only pushes the UVs of textures 1-2
            d = (t.rg - 0.5) * 2.0 * distortion;
        } else {
            rgb = colorInput(t, colorIn0);
            a = alphaInput(t, alphaIn0);
            started = true;
        }
    }
    if (useTex1 == 1) {
        vec4 t = texture(tex1, uv1 + d);
        vec3 c = colorInput(t, colorIn1);
        float ta = alphaInput(t, alphaIn1);
        rgb = started ? blendColor(rgb, c, colorBlend1) : c;
        a = started ? blendAlpha(a, ta, alphaBlend1) : ta;
        started = true;
    }
    if (useTex2 == 1) {
        vec4 t = texture(tex2, uv2 + d);
        vec3 c = colorInput(t, colorIn2);
        float ta = alphaInput(t, alphaIn2);
        rgb = started ? blendColor(rgb, c, colorBlend2) : c;
        a = started ? blendAlpha(a, ta, alphaBlend2) : ta;
        started = true;
    }
    if (usePrim == 1) {
        rgb = blendColor(rgb, colorInput(vcol, colorInPrim), colorBlendPrim);
        a = blendAlpha(a, alphaInput(vcol, alphaInPrim), alphaBlendPrim);
    }
    rgb = clamp(rgb, 0.0, 1.0);
    a = clamp(a, 0.0, 1.0);

    vec3 color;
    if (colorProcess == 0) {
        color = c0.rgb;
    } else if (colorProcess == 2) {
        // colour 1 where the texture is dark, colour 0 where it is bright
        color = mix(c1.rgb, c0.rgb, rgb);
    } else if (colorProcess == 3) {
        color = rgb * c0.rgb + c1.rgb;
    } else {
        color = rgb * c0.rgb;
    }
    float alpha;
    float dissolve = clamp(a - (1.0 - c0.a), 0.0, 1.0);
    if (alphaProcess == 1) {
        alpha = dissolve;
    } else if (alphaProcess == 2) {
        alpha = a * c0.a * c1.a;
    } else if (alphaProcess == 3) {
        alpha = dissolve * c1.a;
    } else if (alphaProcess == 4) {
        float edge = 1.0 - c0.a;
        alpha = smoothstep(edge - 0.05, edge + 0.05, a) * c1.a;
    } else {
        alpha = a * c0.a;
    }
    vec4 c = vec4(color * colorScale, clamp(alpha, 0.0, 1.0));
    if (c.a <= 0.002) {
        discard;
    }
    if (premultiplyForAdd == 1) {
        // additive: the colour is weighted by alpha here, the blend adds it as is
        c.rgb *= c.a;
    }
    // colours and textures are taken as display values (no linear-to-sRGB step)
    outColor = c;
}
