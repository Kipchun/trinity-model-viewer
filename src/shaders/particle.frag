// Particle material (see `ParticleMaterial` in particles.rs): the vertex colour (colour and alpha from the
// emitter, may go past 1 for glows) times the emitter's first texture. Single-channel textures (BC4, R8) are
// masks: their red is the alpha and the colour is white. The game's colour combiners are not read yet.

uniform sampler2D tex;
uniform int useTex;
uniform int alphaOnly;
uniform float colorScale;
uniform int premultiplyForAdd;

in vec2 uvs;
in vec4 col;

layout (location = 0) out vec4 outColor;

void main()
{
    vec4 t = vec4(1.0);
    if (useTex == 1) {
        t = texture(tex, uvs);
        if (alphaOnly == 1) {
            t = vec4(1.0, 1.0, 1.0, t.r);
        }
    }
    vec4 c = col * t;
    c.rgb *= colorScale;
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
