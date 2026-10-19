//! WebGL2 renderer: flat-shaded boxes and lines, one shader.

use crate::gunmodel::{ease, Rounds};
use crate::sim::{Bot, Game, V3};
use crate::terrain::{Terrain, STEP as HILL_STEP};
use js_sys::Float32Array;
use std::cell::{Cell, RefCell};
use web_sys::{
    WebGl2RenderingContext as GL, WebGlBuffer, WebGlFramebuffer, WebGlProgram, WebGlShader, WebGlTexture, WebGlUniformLocation,
    WebGlVertexArrayObject,
};

type M4 = [f32; 16];

const VERT: &str = r#"#version 300 es
layout(location=0) in vec3 a_pos;
layout(location=1) in vec3 a_nrm;
layout(location=2) in vec3 a_col;
layout(location=3) in float a_mat;
// per-instance data for the zombie parts that are drawn many at a time: a matrix, colour and material, finish, shape
layout(location=4) in vec4 i_m0;
layout(location=5) in vec4 i_m1;
layout(location=6) in vec4 i_m2;
layout(location=7) in vec4 i_m3;
layout(location=8) in vec4 i_col;
layout(location=9) in vec4 i_misc;
// skinned bodies: the four bones that move a vertex and by how much
layout(location=11) in vec4 a_bi;
layout(location=12) in vec4 a_bw;
uniform mat4 u_vp;
uniform mat4 u_model;
uniform mat4 u_lvp;
uniform float u_inst;
uniform float u_skin;
uniform mat4 u_bones[17];
uniform float u_view;
uniform mat3 u_envrot;
uniform vec3 u_eyew;
uniform float u_batch;
uniform vec3 u_color;
uniform float u_mat;
uniform float u_seed;
uniform float u_unlit;
uniform float u_shiny;
out vec3 v_nrm;
out vec3 v_pos;
out vec3 v_col;
out float v_mat;
out vec3 v_obj;
out vec4 v_ls;
out vec3 v_rest;
out float v_aux;
flat out vec3 v_ucol;
flat out float v_umat;
flat out float v_ushiny;
flat out float v_unlit;
flat out float v_seed;
void main() {
    bool inst = u_inst > 0.5;
    mat4 M = inst ? mat4(i_m0, i_m1, i_m2, i_m3) : u_model;
    vec3 lp = a_pos;
    vec3 ln = a_nrm;
    vec4 w;
    if (u_skin > 0.5) {
        mat4 S = a_bw.x * u_bones[int(a_bi.x)] + a_bw.y * u_bones[int(a_bi.y)] + a_bw.z * u_bones[int(a_bi.z)] + a_bw.w * u_bones[int(a_bi.w)];
        w = M * (S * vec4(a_pos, 1.0));
        ln = normalize(mat3(S) * a_nrm);
        v_obj = a_pos;
        v_rest = a_pos;
        v_aux = a_mat;
    } else {
        w = M * vec4(lp, 1.0);
        // the part's own position in metres, so skin patterns move with the limb
        v_obj = lp * vec3(length(M[0].xyz), length(M[1].xyz), length(M[2].xyz));
        v_rest = v_obj;
        v_aux = 9.0;
    }
    v_pos = w.xyz;
    // stretched ellipsoids need the inverse transpose; the baked world is already in world space
    v_nrm = u_batch > 0.5 ? ln : transpose(inverse(mat3(M))) * ln;
    v_col = a_col;
    v_mat = a_mat;
    v_ucol = inst ? i_col.rgb : u_color;
    v_umat = inst ? i_col.w : u_mat;
    v_ushiny = inst ? i_misc.x : u_shiny;
    v_unlit = inst ? i_misc.y : u_unlit;
    v_seed = inst ? i_misc.z : u_seed;
    if (u_view > 0.5) {
        // the gun is drawn in the camera's own space, but it should be lit and shadowed where it is in the world
        vec3 nw = normalize(u_envrot * v_nrm);
        v_ls = u_lvp * vec4(u_eyew + u_envrot * w.xyz + nw * 0.01, 1.0);
    } else {
        v_ls = u_lvp * vec4(w.xyz + normalize(v_nrm) * 0.05, 1.0);
    }
    gl_Position = u_vp * w;
}"#;

// Lighting: sky and ground ambient, a warm sun (keep SUN in step with SKY_FRAG), and walls darkening towards
// the ground. Surfaces from the batched world carry a colour and material per vertex and get a procedural
// pattern that fades out with distance; everything else (zombies, gun, blood) uses u_color.
const FRAG: &str = r#"#version 300 es
precision highp float;
in vec3 v_nrm;
in vec3 v_pos;
in vec3 v_col;
in float v_mat;
in vec3 v_obj;
in vec4 v_ls;
in vec3 v_rest;
in float v_aux;
flat in vec3 v_ucol;
flat in float v_umat;
flat in float v_ushiny;
flat in float v_unlit;
flat in float v_seed;
uniform float u_batch;
uniform float u_pass;
uniform float u_shon;
// 1 for soft shadow edges, 0 for a single hard lookup
uniform float u_soft;
uniform float u_skin;
uniform float u_view;
uniform mat3 u_envrot;
// cuts to the shirt and trousers: hem height, sleeve length, which arm is bare, which leg is torn
uniform vec4 u_cut;
// which of hair and beard this one has
uniform vec2 u_hairk;
uniform vec3 u_shoe;
// wounds as a point on the body in the rest pose and a radius
uniform vec4 u_w0;
uniform vec4 u_w1;
uniform vec4 u_w2;
uniform highp sampler2DShadow u_shadow;
uniform vec3 u_eye;
uniform vec3 u_fogc;
uniform vec2 u_fogr;
uniform float u_dark;
uniform float u_flash;
uniform vec3 u_fdir;
out vec4 o;

const vec3 SUN = vec3(0.55, 0.8, 0.35);

float hash(vec2 p) {
    p = fract(p * vec2(123.34, 456.21));
    p += dot(p, p + 45.32);
    return fract(p.x * p.y);
}
// GRADE
float noise(vec2 p) {
    vec2 i = floor(p), f = fract(p);
    f = f * f * (3.0 - 2.0 * f);
    return mix(mix(hash(i), hash(i + vec2(1.0, 0.0)), f.x), mix(hash(i + vec2(0.0, 1.0)), hash(i + vec2(1.0, 1.0)), f.x), f.y);
}
float hash3(vec3 p) {
    p = fract(p * 0.3183099 + 0.1);
    p *= 17.0;
    return fract(p.x * p.y * p.z * (p.x + p.y + p.z));
}
float noise3(vec3 x) {
    vec3 i = floor(x), f = fract(x);
    f = f * f * (3.0 - 2.0 * f);
    return mix(mix(mix(hash3(i), hash3(i + vec3(1, 0, 0)), f.x), mix(hash3(i + vec3(0, 1, 0)), hash3(i + vec3(1, 1, 0)), f.x), f.y),
               mix(mix(hash3(i + vec3(0, 0, 1)), hash3(i + vec3(1, 0, 1)), f.x), mix(hash3(i + vec3(0, 1, 1)), hash3(i + vec3(1, 1, 1)), f.x), f.y), f.z);
}
// flesh, cloth and blood for the zombies: 9 skin, 10 cloth, 11 blood; p is in metres on the part
vec3 organic(vec3 base, float m, vec3 p) {
    if (m < 9.5) {
        float n = noise3(p * 9.0), n2 = noise3(p * 31.0);
        vec3 c = base * (0.88 + 0.22 * n) * (0.95 + 0.1 * n2);
        float v = smoothstep(0.86, 0.99, 1.0 - abs(noise3(p * 15.0 + 3.1) * 2.0 - 1.0));
        c = mix(c, base * vec3(0.8, 0.6, 0.65), v * 0.4);
        float b = smoothstep(0.6, 0.8, noise3(p * 4.5 + 7.7));
        c = mix(c, vec3(0.42, 0.26, 0.28), b * 0.3);
        float r = smoothstep(0.80, 0.9, noise3(p * 11.0 + 21.0));
        return mix(c, vec3(0.30, 0.03, 0.04), r * 0.35);
    }
    if (m < 10.5) return base * (0.7 + 0.4 * noise3(p * 14.0)) * (0.65 + 0.5 * noise3(p * 5.0 + 11.0));
    return base * (0.75 + 0.4 * noise3(p * 20.0));
}
// a wet bloodshot eye seen along its own axis: dark pupil, iris, veined white
vec3 eye_color(vec3 iris, vec3 d) {
    float ang = acos(clamp(-d.z, -1.0, 1.0));
    float veins = smoothstep(0.55, 0.9, noise3(d * 11.0 + 3.0)) * smoothstep(0.5, 0.9, ang);
    vec3 white = mix(vec3(0.86, 0.80, 0.74), vec3(0.62, 0.14, 0.12), 0.1 + 0.5 * veins);
    vec3 c = mix(white, iris * (0.7 + 0.6 * noise3(d * 30.0)), smoothstep(0.66, 0.58, ang));
    return mix(c, vec3(0.015), smoothstep(0.3, 0.24, ang));
}
// a point on the body to where it is on the face as modelled (body::to_head, with copies of its numbers)
const float HEAD_K = 0.87, HEAD_PIVOT = 1.50, HEAD_LIFT = 0.025;
vec3 head_space(vec3 p) {
    return vec3(p.x / HEAD_K, HEAD_PIVOT + (p.y - HEAD_PIVOT - HEAD_LIFT) / HEAD_K, p.z / HEAD_K);
}
// the rest of a face and body that paint can do: sunken eyes, dark lips, blood on the mouth and hands, wounds
vec3 skin_detail(vec3 c, vec3 p, float aux, float wet) {
    vec3 h = head_space(p);
    vec3 q = vec3(abs(h.x), h.y, h.z);
    float eyes = exp(-pow(length((q - vec3(0.04, 1.664, -0.085)) * vec3(1.0, 1.3, 1.0)) / 0.034, 2.0));
    c = mix(c, vec3(0.20, 0.10, 0.15), eyes * 0.45);
    float lips = exp(-pow(length((h - vec3(0.0, 1.575, -0.09)) * vec3(1.0, 1.6, 1.4)) / 0.036, 2.0));
    c = mix(c, vec3(0.30, 0.10, 0.12), lips * 0.65);
    // blood round the mouth, running down the chin and neck
    float mouth = exp(-pow(length((h - vec3(0.0, 1.55, -0.09)) * vec3(0.9, 0.55, 1.0)) / 0.06, 2.0));
    float run = smoothstep(0.55, 0.78, noise3(vec3(p.x * 42.0, p.y * 7.0, p.z * 42.0))) * step(p.y, 1.56) * step(1.36, p.y) * (1.0 - smoothstep(0.0, 0.08, abs(p.x))) * step(p.z, 0.02);
    c = mix(c, vec3(0.30, 0.025, 0.035), clamp(mouth * (0.5 + 0.7 * noise3(p * 55.0)) + run * 0.8, 0.0, 0.9));
    // hands and forearms
    float hands = (1.0 - smoothstep(0.84, 0.93, aux)) * smoothstep(0.35, 0.6, noise3(p * 30.0 + 2.0)) + (1.0 - smoothstep(0.7, 0.9, aux)) * 0.5;
    c = mix(c, vec3(0.26, 0.025, 0.03), clamp(hands, 0.0, 0.85));
    vec4 w[3] = vec4[3](u_w0, u_w1, u_w2);
    for (int i = 0; i < 3; i++) {
        if (w[i].w > 0.0) {
            float r = length(p - w[i].xyz) / w[i].w + 0.4 * (noise3(p * 55.0 + float(i)) - 0.5);
            c = mix(c, c * vec3(0.55, 0.35, 0.4), smoothstep(1.7, 1.0, r) * 0.7);
            c = mix(c, vec3(0.22, 0.02, 0.03), smoothstep(1.0, 0.7, r));
            c = mix(c, vec3(0.50, 0.12, 0.14) * (0.8 + 0.4 * noise3(p * 90.0)), smoothstep(0.65, 0.3, r) * 0.9);
        }
    }
    return c;
}
// the sky as a mirror sees it, in world directions
vec3 sky_env(vec3 d) {
    vec3 zen = vec3(0.34, 0.47, 0.70);
    float h = d.y;
    vec3 c = mix(u_fogc, zen, pow(clamp(h * 1.4, 0.0, 1.0), 0.6));
    if (h < 0.0) c = mix(u_fogc * 0.9, vec3(0.20, 0.18, 0.16), clamp(-h * 2.5, 0.0, 1.0));
    float sd = max(dot(d, normalize(SUN)), 0.0);
    return c + vec3(1.0, 0.85, 0.6) * (pow(sd, 400.0) * 3.0 + pow(sd, 12.0) * 0.25);
}
// how much of the sun reaches this point: a 3x3 blur of hardware depth comparisons, fading out at the map's edge
float shadow_term(vec4 ls) {
    vec3 p = ls.xyz / ls.w * 0.5 + 0.5;
    if (p.z >= 1.0) return 1.0;
    float e = max(abs(p.x - 0.5), abs(p.y - 0.5)) * 2.0;
    // past the edge of the map it comes out as full sun whatever the lookups say
    if (e >= 1.0) return 1.0;
    if (u_soft < 0.5) return mix(texture(u_shadow, vec3(p.xy, p.z - 0.0004)), 1.0, smoothstep(0.8, 1.0, e));
    float t = 1.0 / 2048.0;
    float s = 0.0;
    for (int y = -1; y <= 1; y++) {
        for (int x = -1; x <= 1; x++) {
            s += texture(u_shadow, vec3(p.xy + vec2(float(x), float(y)) * t, p.z - 0.0004));
        }
    }
    return mix(s / 9.0, 1.0, smoothstep(0.8, 1.0, e));
}
// coordinates on the surface, from the world position and the plane it lies in (v runs up walls)
vec2 surf(vec3 p, vec3 n) {
    vec3 a = abs(n);
    if (a.y > 0.5) return p.xz;
    if (a.x > a.z) return p.zy;
    return p.xy;
}
// k is how much pattern to show, 1 close up and 0 far away
vec3 material(vec3 base, float m, vec2 uv, float k) {
    if (m < 0.5) return base;
    // far away the pattern is gone, and only the broad tone of the last three is left
    if (k <= 0.0) {
        if (m > 7.5) return base * (0.88 + 0.2 * noise(uv * 0.2));
        if (m > 6.5) return base * (0.9 + 0.2 * noise(uv * 0.2));
        if (m > 5.5) return base * (0.9 + 0.2 * noise(uv * 0.15));
        return base;
    }
    float n1 = noise(uv * 3.0);
    float n2 = noise(uv * 11.0);
    if (m < 1.5) return base * mix(1.0, 0.84 + 0.2 * n1 + 0.1 * (n2 - 0.5), k);
    if (m < 2.5) {
        vec2 b = vec2(uv.x / 0.5, uv.y / 0.22);
        float row = floor(b.y);
        b.x += 0.5 * mod(row, 2.0);
        vec2 f = fract(b);
        float tone = 0.8 + 0.35 * hash(vec2(floor(b.x), row));
        float mortar = clamp(step(f.x, 0.05) + step(f.y, 0.1), 0.0, 1.0);
        vec3 c = mix(base * tone * (0.9 + 0.1 * n2), vec3(0.58, 0.56, 0.52), mortar);
        return mix(base, c, k);
    }
    if (m < 3.5) return base * mix(1.0, 0.82 + 0.25 * noise(uv * vec2(1.6, 0.6)) + 0.08 * (n2 - 0.5), k);
    if (m < 4.5) {
        float ridge = 0.86 + 0.14 * sin(uv.x * 14.0);
        float streak = 0.8 + 0.35 * noise(uv * vec2(1.2, 0.35));
        return base * mix(1.0, ridge * streak, k);
    }
    if (m < 5.5) {
        float rows = uv.y * 5.0;
        float plank = floor(rows);
        float tone = 0.78 + 0.4 * hash(vec2(plank, floor(uv.x / 2.0 + plank * 0.37)));
        float gap = step(fract(rows), 0.07);
        return base * mix(1.0, tone * (1.0 - 0.45 * gap) * (0.9 + 0.1 * n2), k);
    }
    if (m < 6.5) return base * (0.9 + 0.2 * noise(uv * 0.15)) * mix(1.0, 0.82 + 0.28 * n1 + 0.12 * (n2 - 0.5), k);
    if (m < 7.5) {
        vec2 f = fract(uv / 2.0);
        float line = clamp(step(f.x, 0.012) + step(f.y, 0.012), 0.0, 1.0);
        return base * (0.9 + 0.2 * noise(uv * 0.2)) * mix(1.0, (0.86 + 0.18 * n1) * (1.0 - 0.35 * line), k);
    }
    return base * (0.88 + 0.2 * noise(uv * 0.2)) * mix(1.0, 0.8 + 0.4 * n2, k);
}

void main() {
    if (u_pass > 0.5) {
        o = vec4(0.0);
        return;
    }
    if (u_skin > 0.5) {
        float nz = noise3(v_rest * 17.0 + v_seed);
        if (v_umat > 9.5 && v_umat < 10.5) {
            if (v_rest.y < u_cut.x + 0.035 * nz) discard;
            if (v_aux < 9.0) {
                if (v_aux < u_cut.y + 0.05 * nz) discard;
                if (u_cut.z != 0.0 && sign(v_rest.x) == u_cut.z) discard;
            }
            if (u_cut.w != 0.0 && sign(v_rest.x) == u_cut.w && v_rest.y < 0.52 && v_rest.y > 0.1 + 0.04 * nz) discard;
            // moth-eaten, except on the living (a seed below zero: see `draw_mate`)
            if (v_seed >= 0.0 && noise3(v_rest * 21.0 + 7.0 + v_seed) > 0.83) discard;
        } else if (v_umat > 11.5 && v_umat < 12.5) {
            vec3 h = head_space(v_rest);
            vec3 r = h - vec3(0.0, 1.68, 0.01);
            float thr = mix(0.05, -0.075, smoothstep(0.0, 0.1, r.z)) + 0.02 * smoothstep(0.06, 0.1, abs(r.x)) + 0.012 * pow(abs(r.x) / 0.1, 2.0) + 0.04 * (noise3(v_rest * 48.0) - 0.5) + 0.016 * (nz - 0.5);
            bool hair = u_hairk.x > 0.5 && r.y > thr && h.y > 1.61;
            bool beard = u_hairk.y > 0.5 && r.y < -0.025 && r.z < 0.02 && h.y > 1.5 && !(abs(h.x) < 0.034 && abs(h.y - 1.575) < 0.018);
            if (!(hair || beard)) discard;
        }
    }
    float dist = length(v_pos - u_eye);
    vec3 n = normalize(v_nrm);
    vec3 V = normalize(u_eye - v_pos);
    if (u_view > 0.5) {
        n = normalize(u_envrot * n);
        V = normalize(u_envrot * V);
    }
    vec3 l = normalize(SUN);
    bool batch = u_batch > 0.5;
    vec3 base = batch ? v_col : v_ucol;
    float k = 1.0 - smoothstep(18.0, 60.0, dist);
    bool organic_ = !batch && v_umat > 8.5 && v_umat < 19.5;
    bool hard = !batch && v_umat > 19.5;
    vec3 albedo = batch ? material(base, v_mat, surf(v_pos, n), k) : (organic_ ? organic(base, v_umat, v_obj + v_seed) : base);
    if (organic_) {
        if (v_umat > 13.5) {
            albedo = eye_color(base, normalize(v_obj));
        } else if (v_umat > 11.5 && v_umat < 12.5) {
            albedo = base * (0.5 + 0.9 * noise3(v_rest * vec3(240.0, 50.0, 240.0))) * (0.8 + 0.3 * noise3(v_rest * 60.0));
        } else if (v_umat < 9.5 && u_skin > 0.5) {
            albedo = skin_detail(albedo, v_rest, v_aux, 0.0);
        } else if (v_umat > 9.5 && v_umat < 10.5 && u_skin > 0.5 && v_rest.y < 0.12 + 0.01 * noise3(v_rest * 50.0)) {
            albedo = u_shoe * (0.6 + 0.6 * noise3(v_rest * 45.0));
        }
    }
    float metal = 0.0;
    if (hard) {
        vec3 q = v_obj;
        float scratch = smoothstep(0.78, 0.9, noise3(q * vec3(700.0, 700.0, 50.0) + 3.0));
        float grain = noise3(q * 420.0);
        if (v_umat < 20.5) {
            albedo = mix(base * (0.93 + 0.14 * grain), vec3(0.55, 0.56, 0.58), scratch * 0.25);
            metal = 0.85;
        } else if (v_umat < 21.5) {
            albedo = base * (0.7 + 0.6 * grain);
        } else if (v_umat < 22.5) {
            // wood: grain running along the gun, with darker streaks between the growth rings
            float rings = noise3(q * vec3(90.0, 90.0, 6.0));
            float streak = smoothstep(0.45, 0.75, noise3(q * vec3(260.0, 260.0, 14.0) + rings * 2.0));
            albedo = base * (0.8 + 0.35 * rings) * (1.0 - 0.3 * streak);
        } else if (v_umat < 23.5) {
            albedo = mix(base * (0.8 + 0.4 * noise3(q * vec3(40.0, 40.0, 400.0))), vec3(0.62), scratch * 0.3);
            metal = 1.0;
        } else {
            albedo = base * (0.9 + 0.2 * grain);
        }
    }
    float ao = 1.0;
    if (batch) {
        float side = 1.0 - smoothstep(0.3, 0.7, n.y);
        ao = mix(1.0, mix(0.55, 1.0, smoothstep(0.0, 2.4, v_pos.y)), side);
    }
    vec3 amb = mix(vec3(0.26, 0.23, 0.20), vec3(0.50, 0.56, 0.68), n.y * 0.5 + 0.5) * (1.0 - 0.4 * u_dark);
    vec3 suncol = mix(vec3(1.0, 0.92, 0.78) * 0.85, vec3(0.40, 0.52, 0.75) * 0.5, u_dark);
    // flesh takes light softly, wrapping a little past the terminator
    if (organic_ && dist < 22.0) {
        // pores and weave: a bump from noise, strongest up close
        vec3 q = (v_obj + v_seed) * (v_umat < 9.5 ? 70.0 : (v_umat > 11.5 && v_umat < 12.5 ? 180.0 : 34.0));
        float b0 = noise3(q);
        vec3 g = vec3(noise3(q + vec3(0.4, 0.0, 0.0)) - b0, noise3(q + vec3(0.0, 0.4, 0.0)) - b0, noise3(q + vec3(0.0, 0.0, 0.4)) - b0);
        n = normalize(n - (g - n * dot(g, n)) * (v_umat < 9.5 ? 0.3 : 0.7) * (1.0 - dist / 22.0));
    }
    float d = organic_ ? max(dot(n, l) * 0.6 + 0.4, 0.0) : max(dot(n, l), 0.0);
    float sh = u_shon > 0.5 ? shadow_term(v_ls) : 1.0;
    vec3 lit = albedo * (amb * ao + suncol * d * sh);
    if (organic_) lit += albedo * pow(1.0 - max(dot(n, V), 0.0), 3.0) * 0.14;
    if (organic_ && v_umat < 9.5) {
        // light scattering under the skin: a red glow where the sun just grazes it
        float nl = dot(n, l);
        lit += albedo * vec3(0.95, 0.2, 0.1) * smoothstep(-0.3, 0.15, nl) * (1.0 - smoothstep(0.15, 0.6, nl)) * 0.14 * sh;
    }
    if (u_flash > 0.0) {
        // a torch held at the eye: a cone that fades with distance
        vec3 to = v_pos - u_eye;
        float dd = length(to);
        vec3 dir = to / max(dd, 0.001);
        float cone = smoothstep(0.88, 0.97, dot(dir, u_fdir));
        float fall = clamp(1.0 - dd / 70.0, 0.0, 1.0) / (1.0 + dd * dd * 0.006);
        lit += albedo * vec3(1.0, 0.93, 0.78) * cone * fall * max(dot(n, -dir), 0.0) * 1.8 * u_flash;
    }
    if (hard) {
        // a mirror-ish finish: the sky reflected, strongest at grazing angles, plus the sun's glint
        vec3 R = reflect(-V, n);
        vec3 env = sky_env(R);
        float fres = pow(1.0 - max(dot(n, V), 0.0), 4.0);
        vec3 f0 = mix(vec3(0.04), albedo * 0.9 + 0.1, metal);
        vec3 F = f0 + (vec3(1.0) - f0) * fres;
        float rough = metal > 0.5 ? 0.3 : 0.55;
        vec3 diffuse = albedo * (amb * ao + suncol * d * sh) * (1.0 - 0.85 * metal);
        float gl = pow(max(dot(n, normalize(l + V)), 0.0), mix(180.0, 30.0, rough)) * 1.4;
        lit = diffuse + env * F * (0.55 + 0.45 * sh) * (1.0 - 0.5 * rough) + suncol * F * gl * sh;
    }
    float shiny = v_ushiny;
    float expo = 36.0;
    if (hard) shiny = 0.0;
    if (organic_) {
        // blood is wet and bright, skin has a faint oily sheen, cloth none
        if (v_umat > 13.5) { shiny = 1.4; expo = 140.0; } else if (v_umat > 11.5) { shiny = 0.18; expo = 24.0; } else if (v_umat > 10.5) { shiny = 1.1; expo = 80.0; } else if (v_umat < 9.5) { shiny = 0.22; expo = 22.0; } else { shiny = 0.0; }
    }
    float spec = pow(max(dot(n, normalize(l + V)), 0.0), expo) * shiny * sh;
    vec3 c = mix(lit + spec, v_ucol, v_unlit);
    float f = clamp((dist - u_fogr.x) / (u_fogr.y - u_fogr.x), 0.0, 1.0);
    o = vec4(grade(mix(c, u_fogc, f)), 1.0);
}"#;

const SKY_VERT: &str = r#"#version 300 es
out vec2 v_ndc;
void main() {
    vec2 p = vec2(float((gl_VertexID << 1) & 2), float(gl_VertexID & 2));
    v_ndc = p * 2.0 - 1.0;
    gl_Position = vec4(v_ndc, 1.0, 1.0);
}"#;

// A full-screen triangle: haze at the horizon rising to blue, a sun with a glow, and slow clouds.
const SKY_FRAG: &str = r#"#version 300 es
precision highp float;
in vec2 v_ndc;
uniform vec3 u_right;
uniform vec3 u_up;
uniform vec3 u_fwd;
uniform vec2 u_tan;
uniform vec3 u_haze;
uniform float u_t;
uniform float u_dark;
out vec4 o;

const vec3 SUN = vec3(0.55, 0.8, 0.35);

float hash(vec2 p) {
    p = fract(p * vec2(123.34, 456.21));
    p += dot(p, p + 45.32);
    return fract(p.x * p.y);
}
float noise(vec2 p) {
    vec2 i = floor(p), f = fract(p);
    f = f * f * (3.0 - 2.0 * f);
    return mix(mix(hash(i), hash(i + vec2(1.0, 0.0)), f.x), mix(hash(i + vec2(0.0, 1.0)), hash(i + vec2(1.0, 1.0)), f.x), f.y);
}
float fbm(vec2 p) {
    float s = 0.0, a = 0.5;
    for (int i = 0; i < 4; i++) {
        s += a * noise(p);
        p = p * 2.03 + 17.0;
        a *= 0.5;
    }
    return s;
}
// GRADE

void main() {
    vec3 d = normalize(u_fwd + u_right * (v_ndc.x * u_tan.x) + u_up * (v_ndc.y * u_tan.y));
    float h = d.y;
    vec3 zen = mix(vec3(0.34, 0.47, 0.70), vec3(0.03, 0.05, 0.09), u_dark);
    vec3 c = mix(u_haze, zen, pow(clamp(h * 1.4, 0.0, 1.0), 0.6));
    float sd = max(dot(d, normalize(SUN)), 0.0);
    c += vec3(1.0, 0.85, 0.6) * (pow(sd, 800.0) * 4.0 + pow(sd, 14.0) * 0.22) * (1.0 - u_dark);
    c += vec3(0.75, 0.85, 1.0) * (pow(sd, 1500.0) * 3.0 + pow(sd, 30.0) * 0.05) * u_dark;
    if (h > 0.0) {
        float cl = smoothstep(0.5, 0.8, fbm(d.xz / (h + 0.3) * 2.2 + vec2(u_t * 0.02, 0.0)));
        c = mix(c, vec3(0.92, 0.93, 0.95) * (1.0 - 0.85 * u_dark), cl * 0.7 * smoothstep(0.0, 0.3, h));
        float star = step(0.996, hash(floor(d.xz / (h + 0.2) * 90.0))) * smoothstep(0.05, 0.3, h) * (1.0 - cl);
        c += vec3(star) * u_dark;
    }
    o = vec4(grade(c), 1.0);
}"#;

// The finish every pixel gets on its way out, put into FRAG and SKY_FRAG where they say GRADE: colour grading,
// a vignette and a little film grain (none of the last two with u_plain). Done as each surface is drawn rather
// than in a pass over the finished picture, which saves copying the picture out and drawing it again; with
// nothing see-through it comes to the same. `grade_rgb` is the grading alone, in Rust, and must keep in step.
const GRADE: &str = r#"
uniform vec2 u_screen;
uniform float u_grain;
uniform float u_plain;
vec3 grade(vec3 c) {
    c = clamp(c, 0.0, 1.0);
    float lum = dot(c, vec3(0.299, 0.587, 0.114));
    c = mix(vec3(lum), c, 1.04);
    c = mix(c, c * c * (3.0 - 2.0 * c), 0.2);
    c *= mix(vec3(1.0), vec3(1.04, 1.0, 0.94), smoothstep(0.3, 0.9, lum));
    if (u_plain < 0.5) {
        vec2 uv = gl_FragCoord.xy / u_screen;
        float v = length((uv - 0.5) * vec2(u_screen.x / u_screen.y, 1.0));
        c *= 1.0 - 0.38 * smoothstep(0.35, 1.05, v);
        c += (hash(gl_FragCoord.xy + u_grain * 61.0) - 0.5) * 0.03;
    }
    return clamp(c, 0.0, 1.0);
}
"#;

/// The grading part of GRADE, for a colour that no surface is drawn over (the showcase's background).
fn grade_rgb(c: [f32; 3]) -> [f32; 3] {
    let c = c.map(|v| v.clamp(0.0, 1.0));
    let lum = c[0] * 0.299 + c[1] * 0.587 + c[2] * 0.114;
    let t = ((lum - 0.3) / 0.6).clamp(0.0, 1.0);
    let warm = t * t * (3.0 - 2.0 * t);
    let tint = [1.04, 1.0, 0.94];
    [0, 1, 2].map(|i| {
        let v = lum + (c[i] - lum) * 1.04;
        let v = v + (v * v * (3.0 - 2.0 * v) - v) * 0.2;
        (v * (1.0 + (tint[i] - 1.0) * warm)).clamp(0.0, 1.0)
    })
}

/// The fog starts here and is fully opaque at FOG_END; past that there is only haze, so nothing is drawn.
const FOG_START: f32 = 45.0;
const FOG_END: f32 = 150.0;
/// Overcast sky and the colour the fog fades to, so the horizon melts into it.
const HAZE: [f32; 3] = [0.52, 0.56, 0.62];
/// Shadow map: its side in texels, and half the width of the area it covers around the player, in metres.
const SHADOW_SIZE: i32 = 2048;
const NEAR: f32 = 0.05;
const FAR: f32 = 460.0;
/// Bytes of per-instance data: a matrix, a colour and material, and a finish.
const INSTANCE_BYTES: i32 = 96;
const SHADOW_HALF: f32 = 48.0;
/// Direction to the sun; keep in step with the SUN constants in the shaders.
const SUN: V3 = [0.55, 0.8, 0.35];
/// Size of one cell of the batched world, for culling.
const CHUNK: f32 = 25.0;

/// A run of the batched world's vertices that is drawn, or skipped, together.
struct Chunk {
    first: i32,
    count: i32,
    min: V3,
    max: V3,
}

/// Every wall, road and patch of ground baked into one vertex buffer.
struct WorldMesh {
    vao: WebGlVertexArrayObject,
    ground: (i32, i32),
    chunks: Vec<Chunk>,
}

/// One piece of the body on the GPU.
struct GpuMesh {
    vao: WebGlVertexArrayObject,
    count: i32,
}

/// All the pieces of the body at one level of detail.
struct GpuFigure {
    body: GpuMesh,
    head: GpuMesh,
    hands: [GpuMesh; 2],
    shirt: GpuMesh,
    pants: GpuMesh,
    hair: GpuMesh,
}

/// One part of a gun on the GPU, with the look it is drawn in.
struct GpuPiece {
    vao: WebGlVertexArrayObject,
    count: i32,
    mat: f32,
    color: [f32; 3],
    shiny: f32,
    glow: f32,
}

/// A gun, grouped by what moves together (see `gunmodel::Gun`).
struct GpuGun {
    body: Vec<GpuPiece>,
    mag: Vec<GpuPiece>,
    trigger: Vec<GpuPiece>,
    action: Vec<GpuPiece>,
    spec: crate::gunmodel::Spec,
    /// the box round every part, at rest
    bounds: (V3, V3),
}

/// The hands every gun shares.
struct GpuHands {
    right: Vec<GpuPiece>,
    left: Vec<GpuPiece>,
    pistol_left: Vec<GpuPiece>,
}

/// The grenade, in the hand and flying (see `gunmodel::HandGrenade`).
struct GpuGrenade {
    body: Vec<GpuPiece>,
    spoon: Vec<GpuPiece>,
    pin: Vec<GpuPiece>,
    hand: Vec<GpuPiece>,
}

/// A zombie to draw this frame: its bones, how detailed it is, and what it looks like.
struct SkinJob {
    bones: Vec<f32>,
    lod: usize,
    skin: [f32; 3],
    shirt: [f32; 3],
    pants: [f32; 3],
    shoe: [f32; 3],
    hair: [f32; 3],
    /// hem height, sleeve length, the bare arm's side, the torn leg's side
    cut: [f32; 4],
    hair_kind: [f32; 2],
    wounds: [[f32; 4]; 3],
    seed: f32,
}

/// The multisampled image the scene is drawn into, and its depth; `present` averages it onto the canvas.
struct Targets {
    w: u32,
    h: u32,
    samples: i32,
    msaa: WebGlFramebuffer,
    rb: [web_sys::WebGlRenderbuffer; 2],
}

/// Where a program takes what GRADE needs.
struct Finish {
    screen: Option<WebGlUniformLocation>,
    grain: Option<WebGlUniformLocation>,
    plain: Option<WebGlUniformLocation>,
}

impl Finish {
    fn new(gl: &GL, p: &WebGlProgram) -> Finish {
        Finish { screen: gl.get_uniform_location(p, "u_screen"), grain: gl.get_uniform_location(p, "u_grain"), plain: gl.get_uniform_location(p, "u_plain") }
    }

    /// For the program in use: the picture's size, the time that moves the grain, and `plain` for no vignette or grain.
    fn set(&self, gl: &GL, w: u32, h: u32, time: f32, plain: bool) {
        gl.uniform2f(self.screen.as_ref(), w as f32, h as f32);
        gl.uniform1f(self.grain.as_ref(), time % 100.0);
        gl.uniform1f(self.plain.as_ref(), if plain { 1.0 } else { 0.0 });
    }
}

pub struct Renderer {
    /// where the gun sits in the view, for inspecting it; None for the normal place
    pub gun_offset: Cell<Option<[f32; 3]>>,
    pub gun_yaw: Cell<f32>,
    /// Draws only this gun on a flat grey, for the loadout's pictures (see `draw_showcase`).
    pub showcase: Cell<Option<(usize, f32)>>,
    guns: Vec<GpuGun>,
    hands: GpuHands,
    grenade: GpuGrenade,
    u_view: Option<WebGlUniformLocation>,
    u_envrot: Option<WebGlUniformLocation>,
    u_eyew: Option<WebGlUniformLocation>,
    figures: Vec<GpuFigure>,
    jobs: RefCell<Vec<SkinJob>>,
    u_skin: Option<WebGlUniformLocation>,
    u_seed_f: Option<WebGlUniformLocation>,
    shadow_pass: Cell<bool>,
    u_bones: Option<WebGlUniformLocation>,
    u_cut: Option<WebGlUniformLocation>,
    u_hairk: Option<WebGlUniformLocation>,
    u_shoe: Option<WebGlUniformLocation>,
    u_wound: [Option<WebGlUniformLocation>; 3],
    targets: RefCell<Option<Targets>>,
    finish: Finish,
    s_finish: Finish,
    gl: GL,
    prog: WebGlProgram,
    cube_vao: WebGlVertexArrayObject,
    sphere_vao: WebGlVertexArrayObject,
    sphere_count: i32,
    u_mat: Option<WebGlUniformLocation>,
    line_vao: WebGlVertexArrayObject,
    line_buf: web_sys::WebGlBuffer,
    u_vp: Option<WebGlUniformLocation>,
    u_model: Option<WebGlUniformLocation>,
    u_color: Option<WebGlUniformLocation>,
    u_unlit: Option<WebGlUniformLocation>,
    u_shiny: Option<WebGlUniformLocation>,
    u_eye: Option<WebGlUniformLocation>,
    u_fogc: Option<WebGlUniformLocation>,
    u_fogr: Option<WebGlUniformLocation>,
    u_batch: Option<WebGlUniformLocation>,
    u_dark: Option<WebGlUniformLocation>,
    u_flash: Option<WebGlUniformLocation>,
    u_fdir: Option<WebGlUniformLocation>,
    s_dark: Option<WebGlUniformLocation>,
    /// the look of the day: fog colour and range, and how dark it is (0 day, 1 night)
    haze: [f32; 3],
    fog_start: f32,
    fog_end: f32,
    dark: f32,
    sky: WebGlProgram,
    s_right: Option<WebGlUniformLocation>,
    s_up: Option<WebGlUniformLocation>,
    s_fwd: Option<WebGlUniformLocation>,
    s_tan: Option<WebGlUniformLocation>,
    s_haze: Option<WebGlUniformLocation>,
    s_t: Option<WebGlUniformLocation>,
    world: Option<WorldMesh>,
    inst_e: (WebGlVertexArrayObject, WebGlBuffer),
    list_e: RefCell<Vec<f32>>,
    seed: Cell<f32>,
    u_inst: Option<WebGlUniformLocation>,
    u_lvp: Option<WebGlUniformLocation>,
    u_pass: Option<WebGlUniformLocation>,
    u_shon: Option<WebGlUniformLocation>,
    u_shadow: Option<WebGlUniformLocation>,
    u_soft: Option<WebGlUniformLocation>,
    shadow_fbo: WebGlFramebuffer,
    shadow_tex: WebGlTexture,
    /// the most samples the multisampled image can have here
    max_samples: i32,
    /// graphics quality: 0 low, 1 medium, 2 high (see `Quality`)
    pub quality: u8,
    /// spent shells and cases thrown out of the gun by a reload, and the game time they were last moved at
    brass: RefCell<Vec<crate::brass::Brass>>,
    brass_t: Cell<f32>,
    /// the gun in hand and how far through its reload it was last drawn, to throw the rounds out once as it passes
    /// the moment for it
    last_reload: Cell<(usize, f32)>,
    pub width: u32,
    pub height: u32,
}

/// What each graphics quality turns on. The picture's resolution is picked in JS, which sizes the canvas.
struct Quality {
    samples: i32,
    soft_shadows: bool,
}

const QUALITY: [Quality; 3] = [
    Quality { samples: 0, soft_shadows: false },
    Quality { samples: 2, soft_shadows: true },
    Quality { samples: 4, soft_shadows: true },
];

fn compile(gl: &GL, kind: u32, src: &str) -> Result<WebGlShader, String> {
    let s = gl.create_shader(kind).ok_or("create_shader failed")?;
    gl.shader_source(&s, src);
    gl.compile_shader(&s);
    if gl.get_shader_parameter(&s, GL::COMPILE_STATUS).as_bool().unwrap_or(false) {
        Ok(s)
    } else {
        Err(gl.get_shader_info_log(&s).unwrap_or_default())
    }
}

fn link(gl: &GL, vs: &WebGlShader, fs: &WebGlShader) -> Result<WebGlProgram, String> {
    let prog = gl.create_program().ok_or("create_program failed")?;
    gl.attach_shader(&prog, vs);
    gl.attach_shader(&prog, fs);
    gl.link_program(&prog);
    if gl.get_program_parameter(&prog, GL::LINK_STATUS).as_bool().unwrap_or(false) {
        Ok(prog)
    } else {
        Err(gl.get_program_info_log(&prog).unwrap_or_default())
    }
}

/// A unit sphere, smooth-shaded: position doubles as the normal. Stretched by a model matrix it makes limbs,
/// heads and lumps of flesh.
fn sphere_verts(seg: usize, rings: usize) -> Vec<f32> {
    let at = |i: usize, j: usize| {
        let (th, ph) = (i as f32 / seg as f32 * std::f32::consts::TAU, j as f32 / rings as f32 * std::f32::consts::PI);
        [ph.sin() * th.cos(), ph.cos(), ph.sin() * th.sin()]
    };
    let mut v = Vec::new();
    for j in 0..rings {
        for i in 0..seg {
            let (a, b, c, d) = (at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1));
            for q in [a, b, c, a, c, d] {
                v.extend_from_slice(&q);
                v.extend_from_slice(&q);
            }
        }
    }
    v
}

/// Point (i, j) of the hills: where it is, its normal smoothed over the points round it, and its colour: grass
/// going dry higher up, bare rock where it is steep, and snow on the peaks.
fn hill_point(t: &Terrain, i: usize, j: usize) -> (V3, V3, [f32; 3]) {
    let h = t.at(i, j);
    let (il, ir, jl, jr) = (i.saturating_sub(1), (i + 1).min(t.n - 1), j.saturating_sub(1), (j + 1).min(t.n - 1));
    let dx = (t.at(ir, j) - t.at(il, j)) / ((ir - il) as f32 * HILL_STEP);
    let dz = (t.at(i, jr) - t.at(i, jl)) / ((jr - jl) as f32 * HILL_STEP);
    let l = (dx * dx + 1.0 + dz * dz).sqrt();
    let n = [-dx / l, 1.0 / l, -dz / l];
    let s = |a: f32, b: f32, x: f32| {
        let u = ((x - a) / (b - a)).clamp(0.0, 1.0);
        u * u * (3.0 - 2.0 * u)
    };
    let mix = |a: [f32; 3], b: [f32; 3], k: f32| [a[0] + (b[0] - a[0]) * k, a[1] + (b[1] - a[1]) * k, a[2] + (b[2] - a[2]) * k];
    let c = mix([0.20, 0.27, 0.15], [0.33, 0.31, 0.19], s(4.0, 30.0, h) * 0.7);
    let c = mix(c, [0.40, 0.38, 0.35], 1.0 - s(0.62, 0.8, n[1]));
    let c = mix(c, [0.86, 0.87, 0.90], s(58.0, 68.0, h) * s(0.45, 0.65, n[1]));
    ([-t.half + i as f32 * HILL_STEP, h, -t.half + j as f32 * HILL_STEP], n, c)
}

fn cube_verts() -> Vec<f32> {
    // unit cube spanning 0..1; face normal followed by its four corners
    let faces: [([f32; 3], [[f32; 3]; 4]); 6] = [
        ([0.0, 0.0, 1.0], [[0., 0., 1.], [1., 0., 1.], [1., 1., 1.], [0., 1., 1.]]),
        ([0.0, 0.0, -1.0], [[1., 0., 0.], [0., 0., 0.], [0., 1., 0.], [1., 1., 0.]]),
        ([1.0, 0.0, 0.0], [[1., 0., 1.], [1., 0., 0.], [1., 1., 0.], [1., 1., 1.]]),
        ([-1.0, 0.0, 0.0], [[0., 0., 0.], [0., 0., 1.], [0., 1., 1.], [0., 1., 0.]]),
        ([0.0, 1.0, 0.0], [[0., 1., 1.], [1., 1., 1.], [1., 1., 0.], [0., 1., 0.]]),
        ([0.0, -1.0, 0.0], [[0., 0., 0.], [1., 0., 0.], [1., 0., 1.], [0., 0., 1.]]),
    ];
    let mut v = Vec::with_capacity(36 * 6);
    for (n, c) in faces {
        for i in [0, 1, 2, 0, 2, 3] {
            v.extend_from_slice(&c[i]);
            v.extend_from_slice(&n);
        }
    }
    v
}

fn mul(a: &M4, b: &M4) -> M4 {
    let mut r = [0.0; 16];
    for c in 0..4 {
        for row in 0..4 {
            r[c * 4 + row] = (0..4).map(|k| a[k * 4 + row] * b[c * 4 + k]).sum();
        }
    }
    r
}

fn perspective(fovy: f32, aspect: f32, n: f32, f: f32) -> M4 {
    let t = 1.0 / (fovy / 2.0).tan();
    let mut m = [0.0; 16];
    m[0] = t / aspect;
    m[5] = t;
    m[10] = (f + n) / (n - f);
    m[11] = -1.0;
    m[14] = 2.0 * f * n / (n - f);
    m
}

fn ortho(half: f32, far: f32) -> M4 {
    let mut m = IDENTITY;
    m[0] = 1.0 / half;
    m[5] = 1.0 / half;
    m[10] = -2.0 / far;
    m[14] = -1.0;
    m
}

fn norm(v: V3) -> V3 {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    [v[0] / l, v[1] / l, v[2] / l]
}
fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn look_dir(eye: V3, fwd: V3) -> M4 {
    look_rolled(eye, fwd, 0.0)
}

/// The camera's right and up, tipped `roll` radians around the way it looks.
fn view_axes(f: V3, roll: f32) -> (V3, V3) {
    let s = norm(cross(f, [0.0, 1.0, 0.0]));
    let u = cross(s, f);
    let (sr, cr) = roll.sin_cos();
    let s = [s[0] * cr + u[0] * sr, s[1] * cr + u[1] * sr, s[2] * cr + u[2] * sr];
    (s, cross(s, f))
}

fn look_rolled(eye: V3, fwd: V3, roll: f32) -> M4 {
    let f = norm(fwd);
    let (s, u) = view_axes(f, roll);
    [
        s[0], u[0], -f[0], 0.0, s[1], u[1], -f[1], 0.0, s[2], u[2], -f[2], 0.0, -dot(s, eye), -dot(u, eye), dot(f, eye),
        1.0,
    ]
}

/// Where the camera is and which way it faces, to take what is drawn in its own space, as the gun is, out into the
/// world and back.
struct Camera {
    eye: V3,
    right: V3,
    up: V3,
    fwd: V3,
    roll: f32,
}

impl Camera {
    /// A direction in the camera's space, in the world.
    fn dir(&self, v: V3) -> V3 {
        let (r, u, f) = (self.right, self.up, self.fwd);
        [r[0] * v[0] + u[0] * v[1] - f[0] * v[2], r[1] * v[0] + u[1] * v[1] - f[1] * v[2], r[2] * v[0] + u[2] * v[1] - f[2] * v[2]]
    }

    /// A point in the camera's space, in the world.
    fn point(&self, p: V3) -> V3 {
        let d = self.dir(p);
        [self.eye[0] + d[0], self.eye[1] + d[1], self.eye[2] + d[2]]
    }

    /// From the world into the camera's space.
    fn view(&self) -> M4 {
        look_rolled(self.eye, self.fwd, self.roll)
    }
}

/// How long thrown brass is drawn with the gun, over everything, before it is out in the world: as long as it is
/// still falling past the gun, which would otherwise hide it.
const BRASS_WITH_GUN: f32 = 0.4;

/// How the head moves with the feet, as right, up and roll: a dip onto each footstep, a sway over to the
/// planted foot and a lean into it. Sprinting makes it several times bigger, aiming steadies it.
fn head_bob(p: &crate::sim::Player) -> (f32, f32, f32) {
    let speed = p.body.vel[0].hypot(p.body.vel[2]);
    let amp = if p.body.on_ground { (speed / 6.0).min(1.5) } else { 0.0 } * (1.0 - 0.85 * p.ads);
    let big = 1.0 + 2.2 * p.sprint_t;
    // one turn per left-right pair, so each footstep sits at the bottom of a dip
    let th = p.gait / crate::sim::STRIDE * std::f32::consts::PI;
    let dip = (1.0 - (2.0 * th).cos()) * 0.5; // 0 on a footstep, 1 between them
    let up = (dip - 0.5) * 0.022 * amp * big - 0.09 * p.land_t * p.land_t;
    let side = th.sin() * 0.012 * amp * big;
    let roll = th.sin() * 0.006 * amp * big + (2.0 * th).sin() * 0.002 * amp * p.sprint_t;
    (side, up, roll)
}

fn box_model(min: V3, max: V3) -> M4 {
    let mut m = [0.0; 16];
    m[0] = max[0] - min[0];
    m[5] = max[1] - min[1];
    m[10] = max[2] - min[2];
    m[12] = min[0];
    m[13] = min[1];
    m[14] = min[2];
    m[15] = 1.0;
    m
}

const IDENTITY: M4 = [1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.];

fn translate(x: f32, y: f32, z: f32) -> M4 {
    let mut m = IDENTITY;
    m[12] = x;
    m[13] = y;
    m[14] = z;
    m
}

fn scale(x: f32, y: f32, z: f32) -> M4 {
    let mut m = IDENTITY;
    m[0] = x;
    m[5] = y;
    m[10] = z;
    m
}

fn rot_x(a: f32) -> M4 {
    let (s, c) = a.sin_cos();
    let mut m = IDENTITY;
    m[5] = c;
    m[6] = s;
    m[9] = -s;
    m[10] = c;
    m
}

fn rot_y(a: f32) -> M4 {
    let (s, c) = a.sin_cos();
    let mut m = IDENTITY;
    m[0] = c;
    m[2] = -s;
    m[8] = s;
    m[10] = c;
    m
}

fn rot_z(a: f32) -> M4 {
    let (s, c) = a.sin_cos();
    let mut m = IDENTITY;
    m[0] = c;
    m[1] = s;
    m[4] = -s;
    m[5] = c;
    m
}

/// Applies `r` around `pivot` instead of the origin.
fn around(pivot: V3, r: &M4) -> M4 {
    mul(&translate(pivot[0], pivot[1], pivot[2]), &mul(r, &translate(-pivot[0], -pivot[1], -pivot[2])))
}

/// Where a gun's moving part is, in gun space: a pump slides back, a bolt lifts before it pulls back, a slide kicks
/// back as it fires, barrels tip down on their hinge, a cylinder turns and swings out. A break action or a cylinder
/// opens at the start of a reload and closes at the end.
fn action_pose(s: &crate::gunmodel::Spec, recoil: f32, reload: f32, cycle: f32, turn: f32) -> M4 {
    use crate::gunmodel::Action;
    match s.action {
        Action::Fixed => IDENTITY,
        Action::Pump => translate(0.0, 0.0, 0.09 * cycle),
        Action::Bolt => mul(&translate(0.0, 0.0, 0.075 * ((cycle - 0.35) / 0.65).max(0.0)), &rot_z((cycle * 2.5).min(1.0) * 1.1)),
        Action::Slide => translate(0.0, 0.0, 0.032 * ((recoil - 0.45) / 0.55).clamp(0.0, 1.0).sqrt()),
        Action::Break | Action::Cylinder => mul(&opened(s, reload), &turned(s, turn)),
    }
}

/// How far a break action or a cylinder is swung open, without a cylinder's turn: fresh rounds are brought up to it
/// the same way however far it has turned.
fn opened(s: &crate::gunmodel::Spec, reload: f32) -> M4 {
    use crate::gunmodel::Action;
    let open = ease(0.04, 0.2, reload) - ease(0.8, 0.94, reload);
    match s.action {
        Action::Break => around(s.pivot, &rot_x(-0.6 * open)),
        Action::Cylinder => around(s.swing, &rot_z(1.3 * open)),
        _ => IDENTITY,
    }
}

/// How far a cylinder has turned on its axis, `turn` chambers.
fn turned(s: &crate::gunmodel::Spec, turn: f32) -> M4 {
    if s.action == crate::gunmodel::Action::Cylinder {
        around(s.pivot, &rot_z(-turn * std::f32::consts::TAU / 6.0))
    } else {
        IDENTITY
    }
}

/// A direction `v` turned by `m`, without its move.
fn turn_dir(m: &M4, v: V3) -> V3 {
    std::array::from_fn(|i| m[i] * v[0] + m[4 + i] * v[1] + m[8 + i] * v[2])
}

/// Moves what is at `pivot`, a round or the speedloader with all of them, `u` of the way in (see `Rounds::loading`).
fn bring(r: &Rounds, u: f32, pivot: V3) -> M4 {
    let (o, tilt) = r.loading(u);
    mul(&translate(pivot[0] + o[0], pivot[1] + o[1], pivot[2] + o[2]), &mul(&rot_x(tilt), &translate(-pivot[0], -pivot[1], -pivot[2])))
}

/// Where round `k` of a gun with `Rounds` is, and whether it is only a spent case; None while its chamber is empty.
/// As how it is being brought in, in the frame of the part that opens (see `opened`), and where it is in the turned
/// cylinder or barrels. Out of a reload they are all home. In one, a spent round (or any, from a cylinder) is drawn
/// part way out and then thrown clear (see `Renderer::eject`), and a fresh one is brought in.
fn round_at(r: &Rounds, k: usize, fired: usize, reload: f32) -> Option<(M4, M4, bool)> {
    let c = r.chambers[k];
    let spent = k < fired;
    if reload <= 0.0 || !(spent || r.all) {
        return Some((IDENTITY, translate(c[0], c[1], c[2]), spent));
    }
    if reload < r.eject {
        return Some((IDENTITY, translate(c[0], c[1], c[2] + r.out * ease(r.pull, r.eject, reload)), spent));
    }
    // on a speedloader they all come about its middle, otherwise each about its own base
    let u = r.progress(k, reload)?;
    Some((bring(r, u, r.loader.unwrap_or(c)), translate(c[0], c[1], c[2]), false))
}

/// Where the speedloader is in the part that opens, while it is in the hand.
fn loader_at(r: &Rounds, reload: f32) -> Option<M4> {
    let (l, u) = (r.loader?, r.progress(0, reload)?);
    let a = r.loader_away(reload);
    r.loader_held(reload).then(|| mul(&translate(a[0], a[1], a[2]), &mul(&bring(r, u, l), &translate(l[0], l[1], l[2]))))
}

/// One piece of the viewmodel, in gun space: -z is the muzzle, +z the stock, +y up.
struct Part {
    min: V3,
    max: V3,
    color: [f32; 3],
    shiny: f32,
    local: M4,
}

fn part(min: V3, max: V3, color: [f32; 3], shiny: f32) -> Part {
    Part { min, max, color, shiny, local: IDENTITY }
}

impl Part {
    fn rot(mut self, m: M4) -> Part {
        self.local = m;
        self
    }
}


impl Renderer {
    pub fn new(gl: GL, width: u32, height: u32) -> Result<Renderer, String> {
        let graded = |src: &str| src.replace("// GRADE\n", GRADE);
        let vs = compile(&gl, GL::VERTEX_SHADER, VERT)?;
        let fs = compile(&gl, GL::FRAGMENT_SHADER, &graded(FRAG))?;
        let prog = link(&gl, &vs, &fs)?;
        let sky = link(&gl, &compile(&gl, GL::VERTEX_SHADER, SKY_VERT)?, &compile(&gl, GL::FRAGMENT_SHADER, &graded(SKY_FRAG))?)?;
        let (finish, s_finish) = (Finish::new(&gl, &prog), Finish::new(&gl, &sky));

        let guns = crate::gunmodel::guns()
            .iter()
            .map(|g| {
                let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
                for v in g.body.iter().chain(&g.mag).chain(&g.trigger).chain(&g.action).flat_map(|p| p.mesh.verts.chunks(6)) {
                    for a in 0..3 {
                        lo[a] = lo[a].min(v[a]);
                        hi[a] = hi[a].max(v[a]);
                    }
                }
                Ok(GpuGun {
                    bounds: (lo, hi),
                    body: Self::upload_pieces(&gl, &g.body)?,
                    mag: Self::upload_pieces(&gl, &g.mag)?,
                    trigger: Self::upload_pieces(&gl, &g.trigger)?,
                    action: Self::upload_pieces(&gl, &g.action)?,
                    spec: g.spec,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let h = crate::gunmodel::Hands::new();
        let hands = GpuHands {
            right: Self::upload_pieces(&gl, &h.right)?,
            left: Self::upload_pieces(&gl, &h.left)?,
            pistol_left: Self::upload_pieces(&gl, &h.pistol_left)?,
        };
        let n = crate::gunmodel::grenade();
        let grenade = GpuGrenade {
            body: Self::upload_pieces(&gl, &n.body)?,
            spoon: Self::upload_pieces(&gl, &n.spoon)?,
            pin: Self::upload_pieces(&gl, &n.pin)?,
            hand: Self::upload_pieces(&gl, &n.hand)?,
        };
        // the zombie body at two levels of detail, near and far. A finer one for close up (half the grid) looked
        // no different even a metre away, and took most of a second to build at every load
        let figures = [2.0, 4.0].iter().map(|s| Self::upload_figure(&gl, &crate::body::Figure::new(*s))).collect::<Result<Vec<_>, _>>()?;

        let cube_vao = gl.create_vertex_array().ok_or("vao")?;
        gl.bind_vertex_array(Some(&cube_vao));
        let cube_buf = gl.create_buffer().ok_or("buffer")?;
        gl.bind_buffer(GL::ARRAY_BUFFER, Some(&cube_buf));
        let verts = cube_verts();
        unsafe {
            gl.buffer_data_with_array_buffer_view(GL::ARRAY_BUFFER, &Float32Array::view(&verts), GL::STATIC_DRAW);
        }
        Self::layout(&gl);

        let sphere_vao = gl.create_vertex_array().ok_or("vao")?;
        gl.bind_vertex_array(Some(&sphere_vao));
        let sphere_buf = gl.create_buffer().ok_or("buffer")?;
        gl.bind_buffer(GL::ARRAY_BUFFER, Some(&sphere_buf));
        let sphere = sphere_verts(14, 9);
        unsafe {
            gl.buffer_data_with_array_buffer_view(GL::ARRAY_BUFFER, &Float32Array::view(&sphere), GL::STATIC_DRAW);
        }
        Self::layout(&gl);
        let sphere_count = (sphere.len() / 6) as i32;

        let inst_e = Self::instanced(&gl, &sphere_buf)?;

        // the sun's depth map: a depth texture that compares as it samples, so a lookup returns how lit a point is
        let shadow_tex = gl.create_texture().ok_or("texture")?;
        gl.bind_texture(GL::TEXTURE_2D, Some(&shadow_tex));
        gl.tex_storage_2d(GL::TEXTURE_2D, 1, GL::DEPTH_COMPONENT24, SHADOW_SIZE, SHADOW_SIZE);
        for (p, v) in [
            (GL::TEXTURE_MIN_FILTER, GL::LINEAR),
            (GL::TEXTURE_MAG_FILTER, GL::LINEAR),
            (GL::TEXTURE_WRAP_S, GL::CLAMP_TO_EDGE),
            (GL::TEXTURE_WRAP_T, GL::CLAMP_TO_EDGE),
            (GL::TEXTURE_COMPARE_MODE, GL::COMPARE_REF_TO_TEXTURE),
            (GL::TEXTURE_COMPARE_FUNC, GL::LEQUAL),
        ] {
            gl.tex_parameteri(GL::TEXTURE_2D, p, v as i32);
        }
        let shadow_fbo = gl.create_framebuffer().ok_or("framebuffer")?;
        gl.bind_framebuffer(GL::FRAMEBUFFER, Some(&shadow_fbo));
        gl.framebuffer_texture_2d(GL::FRAMEBUFFER, GL::DEPTH_ATTACHMENT, GL::TEXTURE_2D, Some(&shadow_tex), 0);
        gl.bind_framebuffer(GL::FRAMEBUFFER, None);

        let line_vao = gl.create_vertex_array().ok_or("vao")?;
        gl.bind_vertex_array(Some(&line_vao));
        let line_buf = gl.create_buffer().ok_or("buffer")?;
        gl.bind_buffer(GL::ARRAY_BUFFER, Some(&line_buf));
        Self::layout(&gl);

        gl.enable(GL::DEPTH_TEST);
        let max_samples = gl.get_parameter(GL::MAX_SAMPLES).ok().and_then(|v| v.as_f64()).unwrap_or(4.0) as i32;
        let u = |n: &str| gl.get_uniform_location(&prog, n);
        let u_mat = u("u_mat");
        Ok(Renderer {
            gun_offset: Cell::new(None),
            gun_yaw: Cell::new(0.05),
            showcase: Cell::new(None),
            guns,
            hands,
            grenade,
            u_view: u("u_view"),
            u_envrot: u("u_envrot"),
            u_eyew: u("u_eyew"),
            figures,
            jobs: RefCell::new(Vec::new()),
            u_skin: u("u_skin"),
            u_seed_f: u("u_seed"),
            shadow_pass: Cell::new(false),
            u_bones: u("u_bones"),
            u_cut: u("u_cut"),
            u_hairk: u("u_hairk"),
            u_shoe: u("u_shoe"),
            u_wound: [u("u_w0"), u("u_w1"), u("u_w2")],
            targets: RefCell::new(None),
            finish,
            s_finish,
            u_vp: u("u_vp"),
            u_model: u("u_model"),
            u_color: u("u_color"),
            u_unlit: u("u_unlit"),
            u_shiny: u("u_shiny"),
            u_eye: u("u_eye"),
            u_fogc: u("u_fogc"),
            u_fogr: u("u_fogr"),
            u_batch: u("u_batch"),
            u_dark: u("u_dark"),
            u_flash: u("u_flash"),
            u_fdir: u("u_fdir"),
            u_inst: u("u_inst"),
            u_lvp: u("u_lvp"),
            u_pass: u("u_pass"),
            u_shon: u("u_shon"),
            u_shadow: u("u_shadow"),
            u_soft: u("u_soft"),
            max_samples,
            quality: 0,
            inst_e,
            list_e: RefCell::new(Vec::new()),
            seed: Cell::new(0.0),
            shadow_fbo,
            shadow_tex,
            s_dark: gl.get_uniform_location(&sky, "u_dark"),
            haze: HAZE,
            fog_start: FOG_START,
            fog_end: FOG_END,
            dark: 0.0,
            s_right: gl.get_uniform_location(&sky, "u_right"),
            s_up: gl.get_uniform_location(&sky, "u_up"),
            s_fwd: gl.get_uniform_location(&sky, "u_fwd"),
            s_tan: gl.get_uniform_location(&sky, "u_tan"),
            s_haze: gl.get_uniform_location(&sky, "u_haze"),
            s_t: gl.get_uniform_location(&sky, "u_t"),
            sky,
            world: None,
            gl,
            prog,
            cube_vao,
            sphere_vao,
            sphere_count,
            u_mat,
            line_vao,
            line_buf,
            brass: RefCell::new(Vec::new()),
            brass_t: Cell::new(0.0),
            last_reload: Cell::new((0, 0.0)),
            width,
            height,
        })
    }

    fn upload_pieces(gl: &GL, pieces: &[crate::gunmodel::Piece]) -> Result<Vec<GpuPiece>, String> {
        pieces
            .iter()
            .map(|p| {
                let vao = gl.create_vertex_array().ok_or("vao")?;
                gl.bind_vertex_array(Some(&vao));
                let vb = gl.create_buffer().ok_or("buffer")?;
                gl.bind_buffer(GL::ARRAY_BUFFER, Some(&vb));
                unsafe {
                    gl.buffer_data_with_array_buffer_view(GL::ARRAY_BUFFER, &Float32Array::view(&p.mesh.verts), GL::STATIC_DRAW);
                }
                Self::layout(gl);
                let ib = gl.create_buffer().ok_or("buffer")?;
                gl.bind_buffer(GL::ELEMENT_ARRAY_BUFFER, Some(&ib));
                unsafe {
                    gl.buffer_data_with_array_buffer_view(GL::ELEMENT_ARRAY_BUFFER, &js_sys::Uint32Array::view(&p.mesh.idx), GL::STATIC_DRAW);
                }
                Ok(GpuPiece { vao, count: p.mesh.idx.len() as i32, mat: p.mat, color: p.color, shiny: p.shiny, glow: p.glow })
            })
            .collect()
    }

    /// Puts a skinned mesh on the GPU: position, normal, four bone indices and weights, and one number for the shader.
    fn upload_skinned(gl: &GL, m: &crate::body::Mesh) -> Result<GpuMesh, String> {
        let vao = gl.create_vertex_array().ok_or("vao")?;
        gl.bind_vertex_array(Some(&vao));
        let vb = gl.create_buffer().ok_or("buffer")?;
        gl.bind_buffer(GL::ARRAY_BUFFER, Some(&vb));
        unsafe {
            gl.buffer_data_with_array_buffer_view(GL::ARRAY_BUFFER, &Float32Array::view(&m.verts), GL::STATIC_DRAW);
        }
        let stride = (crate::body::STRIDE * 4) as i32;
        for (loc, size, off) in [(0, 3, 0), (1, 3, 12), (11, 4, 24), (12, 4, 40), (3, 1, 56)] {
            gl.enable_vertex_attrib_array(loc);
            gl.vertex_attrib_pointer_with_i32(loc, size, GL::FLOAT, false, stride, off);
        }
        let ib = gl.create_buffer().ok_or("buffer")?;
        gl.bind_buffer(GL::ELEMENT_ARRAY_BUFFER, Some(&ib));
        unsafe {
            gl.buffer_data_with_array_buffer_view(GL::ELEMENT_ARRAY_BUFFER, &js_sys::Uint32Array::view(&m.idx), GL::STATIC_DRAW);
        }
        Ok(GpuMesh { vao, count: m.idx.len() as i32 })
    }

    fn upload_figure(gl: &GL, f: &crate::body::Figure) -> Result<GpuFigure, String> {
        Ok(GpuFigure {
            body: Self::upload_skinned(gl, &f.body)?,
            head: Self::upload_skinned(gl, &f.head)?,
            hands: [Self::upload_skinned(gl, &f.hands[0])?, Self::upload_skinned(gl, &f.hands[1])?],
            shirt: Self::upload_skinned(gl, &f.shirt)?,
            pants: Self::upload_skinned(gl, &f.pants)?,
            hair: Self::upload_skinned(gl, &f.hair)?,
        })
    }

    /// A vertex array over `mesh` that also reads one row of per-instance data (see the vertex shader) per copy.
    fn instanced(gl: &GL, mesh: &WebGlBuffer) -> Result<(WebGlVertexArrayObject, WebGlBuffer), String> {
        let vao = gl.create_vertex_array().ok_or("vao")?;
        gl.bind_vertex_array(Some(&vao));
        gl.bind_buffer(GL::ARRAY_BUFFER, Some(mesh));
        Self::layout(gl);
        let buf = gl.create_buffer().ok_or("buffer")?;
        gl.bind_buffer(GL::ARRAY_BUFFER, Some(&buf));
        for (i, loc) in (4..=9u32).enumerate() {
            gl.enable_vertex_attrib_array(loc);
            gl.vertex_attrib_pointer_with_i32(loc, 4, GL::FLOAT, false, INSTANCE_BYTES, i as i32 * 16);
            gl.vertex_attrib_divisor(loc, 1);
        }
        Ok((vao, buf))
    }

    fn layout(gl: &GL) {
        gl.enable_vertex_attrib_array(0);
        gl.vertex_attrib_pointer_with_i32(0, 3, GL::FLOAT, false, 24, 0);
        gl.enable_vertex_attrib_array(1);
        gl.vertex_attrib_pointer_with_i32(1, 3, GL::FLOAT, false, 24, 12);
    }

    fn draw_model(&self, model: &M4, color: [f32; 3], unlit: f32, shiny: f32) {
        self.draw_mesh(&self.cube_vao, 36, model, color, unlit, shiny, 0.0);
    }

    fn draw_mesh(&self, vao: &WebGlVertexArrayObject, count: i32, model: &M4, color: [f32; 3], unlit: f32, shiny: f32, mat: f32) {
        let gl = &self.gl;
        gl.bind_vertex_array(Some(vao));
        gl.uniform_matrix4fv_with_f32_array(self.u_model.as_ref(), false, model);
        gl.uniform3f(self.u_color.as_ref(), color[0], color[1], color[2]);
        gl.uniform1f(self.u_unlit.as_ref(), unlit);
        gl.uniform1f(self.u_shiny.as_ref(), shiny);
        gl.uniform1f(self.u_mat.as_ref(), mat);
        gl.draw_arrays(GL::TRIANGLES, 0, count);
    }

    fn queue(&self, m: &M4, col: [f32; 3], mat: f32, shiny: f32, unlit: f32) {
        let mut l = self.list_e.borrow_mut();
        l.extend_from_slice(m);
        l.extend_from_slice(&[col[0], col[1], col[2], mat, shiny, unlit, self.seed.get(), 0.0]);
    }

    /// An ellipsoid of half-sizes `h` centred on `c`, in the frame of `parent`, optionally rotated about its centre.
    /// It is queued and drawn with all the others in one go by `flush`.
    fn ell(&self, parent: &M4, c: V3, h: V3, rot: &M4, col: [f32; 3], mat: f32, shiny: f32, unlit: f32) {
        let model = mul(parent, &mul(&translate(c[0], c[1], c[2]), &mul(rot, &scale(h[0], h[1], h[2]))));
        self.queue(&model, col, mat, shiny, unlit);
    }

    /// Draws everything queued by `ell`, and empties nothing: the same lists are drawn once for the
    /// sun's shadow and once for the picture.
    fn flush(&self) {
        let gl = &self.gl;
        gl.uniform1f(self.u_inst.as_ref(), 1.0);
        {
            let l = self.list_e.borrow();
            if !l.is_empty() {
                let (vao, buf) = &self.inst_e;
                gl.bind_vertex_array(Some(vao));
                gl.bind_buffer(GL::ARRAY_BUFFER, Some(buf));
                unsafe {
                    gl.buffer_data_with_array_buffer_view(GL::ARRAY_BUFFER, &Float32Array::view(&l), GL::DYNAMIC_DRAW);
                }
                gl.draw_arrays_instanced(GL::TRIANGLES, 0, self.sphere_count, (l.len() / 24) as i32);
            }
        }
        gl.uniform1f(self.u_inst.as_ref(), 0.0);
        self.flush_bodies();
    }

    /// Draws the queued zombie bodies: skin, hands, head, then clothes and hair, each posed by the zombie's bones.
    fn flush_bodies(&self) {
        let gl = &self.gl;
        let shadow = self.shadow_pass.get();
        gl.uniform1f(self.u_skin.as_ref(), 1.0);
        gl.uniform_matrix4fv_with_f32_array(self.u_model.as_ref(), false, &IDENTITY);
        for job in self.jobs.borrow().iter() {
            // shadows need less detail than the picture: the far mesh's grid is about one texel of the shadow
            // map, which blurs its edges over three
            let f = &self.figures[if shadow { 1 } else { job.lod }];
            gl.uniform_matrix4fv_with_f32_array(self.u_bones.as_ref(), false, &job.bones);
            gl.uniform1f(self.u_seed_f.as_ref(), job.seed);
            gl.uniform4f(self.u_cut.as_ref(), job.cut[0], job.cut[1], job.cut[2], job.cut[3]);
            gl.uniform2f(self.u_hairk.as_ref(), job.hair_kind[0], job.hair_kind[1]);
            gl.uniform3f(self.u_shoe.as_ref(), job.shoe[0], job.shoe[1], job.shoe[2]);
            for (loc, w) in self.u_wound.iter().zip(&job.wounds) {
                gl.uniform4f(loc.as_ref(), w[0], w[1], w[2], w[3]);
            }
            let draw = |m: &GpuMesh, col: [f32; 3], mat: f32, shiny: f32| {
                gl.bind_vertex_array(Some(&m.vao));
                gl.uniform3f(self.u_color.as_ref(), col[0], col[1], col[2]);
                gl.uniform1f(self.u_mat.as_ref(), mat);
                gl.uniform1f(self.u_shiny.as_ref(), shiny);
                gl.uniform1f(self.u_unlit.as_ref(), 0.0);
                gl.draw_elements_with_i32(GL::TRIANGLES, m.count, GL::UNSIGNED_INT, 0);
            };
            // outside in: what the clothes and hair cover then fails the depth test instead of being worked out
            // and painted over, and skin is the dearest thing to draw
            draw(&f.shirt, job.shirt, 10.0, 0.0);
            if job.hair_kind[0] > 0.5 || job.hair_kind[1] > 0.5 {
                draw(&f.hair, job.hair, 12.0, 0.0);
            }
            // the trousers have no hem to cut, only a torn leg
            gl.uniform4f(self.u_cut.as_ref(), -1.0, 0.0, 0.0, job.cut[3]);
            draw(&f.pants, job.pants, 10.0, 0.0);
            draw(&f.body, job.skin, 9.0, 0.0);
            draw(&f.head, job.skin, 9.0, 0.0);
            draw(&f.hands[0], job.skin, 9.0, 0.0);
            draw(&f.hands[1], job.skin, 9.0, 0.0);
        }
        gl.uniform1f(self.u_skin.as_ref(), 0.0);
    }

    fn draw_part(&self, gun: &M4, p: &Part, unlit: f32) {
        let model = mul(gun, &mul(&p.local, &box_model(p.min, p.max)));
        self.draw_model(&model, p.color, unlit, p.shiny);
    }

    /// `pts` is pairs of endpoints, each as x y z.
    fn draw_lines(&self, pts: &[f32], color: [f32; 3], unlit: f32) {
        if pts.is_empty() {
            return;
        }
        let mut verts = Vec::with_capacity(pts.len() * 2);
        for p in pts.chunks(3) {
            verts.extend_from_slice(p);
            verts.extend_from_slice(&[0.0, 1.0, 0.0]);
        }
        let gl = &self.gl;
        gl.bind_vertex_array(Some(&self.line_vao));
        gl.bind_buffer(GL::ARRAY_BUFFER, Some(&self.line_buf));
        unsafe {
            gl.buffer_data_with_array_buffer_view(GL::ARRAY_BUFFER, &Float32Array::view(&verts), GL::DYNAMIC_DRAW);
        }
        gl.uniform_matrix4fv_with_f32_array(self.u_model.as_ref(), false, &IDENTITY);
        gl.uniform3f(self.u_color.as_ref(), color[0], color[1], color[2]);
        gl.uniform1f(self.u_unlit.as_ref(), unlit);
        gl.draw_arrays(GL::LINES, 0, (pts.len() / 3) as i32);
    }

    /// Bakes the ground and every wall into one buffer. Call once the game exists; the map never changes.
    pub fn upload_world(&mut self, g: &Game) -> Result<(), String> {
        // with hills round it, the haze starts further off so the mountains stand out across the map
        (self.fog_start, self.fog_end) = if g.world.ground.is_some() { (110.0, 440.0) } else { (FOG_START, FOG_END) };
        let gl = &self.gl;
        let cube = cube_verts();
        let mut verts: Vec<f32> = Vec::new();
        let vertex = |v: &mut Vec<f32>, p: V3, n: V3, c: [f32; 3], m: f32| {
            v.extend_from_slice(&[p[0], p[1], p[2], n[0], n[1], n[2], c[0], c[1], c[2], m]);
        };

        // the ground and its markings, flat tops only, drawn in order without writing depth
        let half = g.half;
        let mut flat: Vec<(V3, V3, [f32; 3])> = vec![([-half, 0.0, -half], [half, 0.0, half], crate::map::GROUND)];
        flat.extend(g.decals.iter().map(|(d, c)| (d.min, d.max, *c)));
        for (min, max, c) in &flat {
            let m = crate::map::material(*c) as f32;
            let y = max[1];
            for p in [[min[0], min[2]], [max[0], min[2]], [max[0], max[2]], [min[0], min[2]], [max[0], max[2]], [min[0], max[2]]] {
                vertex(&mut verts, [p[0], y, p[1]], [0.0, 1.0, 0.0], *c, m);
            }
        }
        let ground = (0, (verts.len() / 10) as i32);

        // walls, grouped by which cell of the map they are in
        let cells = (2.0 * half / CHUNK).ceil() as usize;
        let cell = |lo: f32, hi: f32| ((((lo + hi) / 2.0 + half) / CHUNK).floor().max(0.0) as usize).min(cells - 1);
        let mut buckets: Vec<Vec<usize>> = vec![Vec::new(); cells * cells];
        for (i, w) in g.world.walls.iter().enumerate() {
            buckets[cell(w.min[2], w.max[2]) * cells + cell(w.min[0], w.max[0])].push(i);
        }
        // the squares of the hills, by the corner of each on the terrain's grid, grouped the same way; where the
        // ground is flat the plane under everything covers it
        let mut hills: Vec<Vec<(usize, usize)>> = vec![Vec::new(); cells * cells];
        if let Some(t) = &g.world.ground {
            for j in 0..t.n - 1 {
                for i in 0..t.n - 1 {
                    if [t.at(i, j), t.at(i + 1, j), t.at(i, j + 1), t.at(i + 1, j + 1)].iter().all(|h| h.abs() < 0.001) {
                        continue;
                    }
                    let (x, z) = (-t.half + (i as f32 + 0.5) * HILL_STEP, -t.half + (j as f32 + 0.5) * HILL_STEP);
                    hills[cell(z, z) * cells + cell(x, x)].push((i, j));
                }
            }
        }
        let mut chunks = Vec::new();
        for (bucket, hill) in buckets.iter().zip(&hills).filter(|(b, h)| !b.is_empty() || !h.is_empty()) {
            let first = (verts.len() / 10) as i32;
            let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
            if let Some(t) = &g.world.ground {
                for &(i, j) in hill {
                    let c = [(i, j), (i + 1, j), (i, j + 1), (i + 1, j + 1)].map(|(i, j)| hill_point(t, i, j));
                    // split the way `Terrain::height` splits it, so what is drawn is what is stood on
                    for tri in [[0, 3, 1], [0, 2, 3]] {
                        let up = tri.iter().map(|&k| c[k].1[1]).sum::<f32>() / 3.0;
                        let m = if up < 0.72 { 1.0 } else { 8.0 };
                        for k in tri {
                            let (p, n, col) = c[k];
                            vertex(&mut verts, p, n, col, m);
                            for a in 0..3 {
                                lo[a] = lo[a].min(p[a]);
                                hi[a] = hi[a].max(p[a]);
                            }
                        }
                    }
                }
            }
            for &i in bucket {
                let w = &g.world.walls[i];
                for a in 0..3 {
                    lo[a] = lo[a].min(w.min[a]);
                    hi[a] = hi[a].max(w.max[a]);
                }
                // the underside of anything standing on the ground, all the way round, can never be seen
                let ground = [(w.min[0], w.min[2]), (w.max[0], w.min[2]), (w.min[0], w.max[2]), (w.max[0], w.max[2])].map(|(x, z)| g.world.ground_at(x, z));
                let grounded = w.min[1] <= ground.iter().fold(f32::INFINITY, |a, &b| a.min(b)) + 0.001;
                for v in cube.chunks(6) {
                    if grounded && v[4] < -0.5 {
                        continue;
                    }
                    let p = [w.min[0] + v[0] * (w.max[0] - w.min[0]), w.min[1] + v[1] * (w.max[1] - w.min[1]), w.min[2] + v[2] * (w.max[2] - w.min[2])];
                    vertex(&mut verts, p, [v[3], v[4], v[5]], g.colors[i], g.mats[i] as f32);
                }
            }
            chunks.push(Chunk { first, count: (verts.len() / 10) as i32 - first, min: lo, max: hi });
        }

        let vao = gl.create_vertex_array().ok_or("vao")?;
        gl.bind_vertex_array(Some(&vao));
        let buf = gl.create_buffer().ok_or("buffer")?;
        gl.bind_buffer(GL::ARRAY_BUFFER, Some(&buf));
        unsafe {
            gl.buffer_data_with_array_buffer_view(GL::ARRAY_BUFFER, &Float32Array::view(&verts), GL::STATIC_DRAW);
        }
        // position, normal, colour, material
        for (loc, size, off) in [(0, 3, 0), (1, 3, 12), (2, 3, 24), (3, 1, 36)] {
            gl.enable_vertex_attrib_array(loc);
            gl.vertex_attrib_pointer_with_i32(loc, size, GL::FLOAT, false, 40, off);
        }
        self.world = Some(WorldMesh { vao, ground, chunks });
        Ok(())
    }

    fn draw_world(&self, eye: V3, vp: &M4) {
        let (gl, Some(w)) = (&self.gl, &self.world) else {
            return;
        };
        // the four sides of the view, as planes facing in (from the rows of the view-projection matrix)
        let row = |i: usize| [vp[i], vp[4 + i], vp[8 + i], vp[12 + i]];
        let (r0, r1, r3) = (row(0), row(1), row(3));
        let sides = [0, 1, 2, 3].map(|k| {
            let (r, s) = if k < 2 { (r0, if k == 0 { 1.0 } else { -1.0 }) } else { (r1, if k == 2 { 1.0 } else { -1.0 }) };
            [r3[0] + s * r[0], r3[1] + s * r[1], r3[2] + s * r[2], r3[3] + s * r[3]]
        });
        gl.bind_vertex_array(Some(&w.vao));
        gl.uniform1f(self.u_batch.as_ref(), 1.0);
        gl.uniform_matrix4fv_with_f32_array(self.u_model.as_ref(), false, &IDENTITY);
        gl.uniform1f(self.u_unlit.as_ref(), 0.0);
        gl.uniform1f(self.u_shiny.as_ref(), 0.0);
        let mut seen: Vec<(f32, &Chunk)> = w
            .chunks
            .iter()
            .filter_map(|c| {
                // past FOG_END everything is haze already, and a cell wholly outside any side of the view is not
                // seen: its corner furthest into the view is still outside
                let gap = |a: usize| (c.min[a] - eye[a]).max(eye[a] - c.max[a]).max(0.0);
                let d = (gap(0).powi(2) + gap(1).powi(2) + gap(2).powi(2)).sqrt();
                let outside = sides.iter().any(|p| {
                    let far = [0, 1, 2].map(|a| if p[a] >= 0.0 { c.max[a] } else { c.min[a] });
                    p[0] * far[0] + p[1] * far[1] + p[2] * far[2] + p[3] < 0.0
                });
                (d <= self.fog_end && !outside).then_some((d, c))
            })
            .collect();
        // nearest first, so what is hidden behind is turned away by the depth test before it is shaded
        seen.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (_, c) in seen {
            gl.draw_arrays(GL::TRIANGLES, c.first, c.count);
        }
        // the ground goes in after the walls, so it is only shaded where it shows. It writes no depth, so markings
        // on it can't fight each other, and it is pushed back a little so a hill just above it always wins
        gl.depth_mask(false);
        gl.enable(GL::POLYGON_OFFSET_FILL);
        gl.polygon_offset(1.0, 4.0);
        gl.draw_arrays(GL::TRIANGLES, w.ground.0, w.ground.1);
        gl.disable(GL::POLYGON_OFFSET_FILL);
        gl.depth_mask(true);
        gl.uniform1f(self.u_batch.as_ref(), 0.0);
    }

    /// Puts the ground's depth in without drawing it again, so the sky drawn after only fills what is left.
    /// This comes after the zombies: anything of theirs that sinks below the ground still shows, as it did when
    /// the ground wrote no depth.
    fn ground_depth(&self) {
        let (gl, Some(w)) = (&self.gl, &self.world) else {
            return;
        };
        gl.bind_vertex_array(Some(&w.vao));
        gl.uniform1f(self.u_batch.as_ref(), 1.0);
        gl.uniform_matrix4fv_with_f32_array(self.u_model.as_ref(), false, &IDENTITY);
        gl.uniform1f(self.u_pass.as_ref(), 1.0);
        gl.color_mask(false, false, false, false);
        gl.enable(GL::POLYGON_OFFSET_FILL);
        gl.polygon_offset(1.0, 4.0);
        // the plain ground is the first square; the markings lie on it
        gl.draw_arrays(GL::TRIANGLES, w.ground.0, 6);
        gl.disable(GL::POLYGON_OFFSET_FILL);
        gl.color_mask(true, true, true, true);
        gl.uniform1f(self.u_pass.as_ref(), 0.0);
        gl.uniform1f(self.u_batch.as_ref(), 0.0);
    }

    fn draw_sky(&self, fwd: V3, fov: f32, aspect: f32, time: f32) {
        let gl = &self.gl;
        let right = norm(cross(fwd, [0.0, 1.0, 0.0]));
        let up = cross(right, fwd);
        let ty = (fov / 2.0).tan();
        gl.use_program(Some(&self.sky));
        gl.uniform3f(self.s_right.as_ref(), right[0], right[1], right[2]);
        gl.uniform3f(self.s_up.as_ref(), up[0], up[1], up[2]);
        gl.uniform3f(self.s_fwd.as_ref(), fwd[0], fwd[1], fwd[2]);
        gl.uniform2f(self.s_tan.as_ref(), ty * aspect, ty);
        gl.uniform3f(self.s_haze.as_ref(), self.haze[0], self.haze[1], self.haze[2]);
        gl.uniform1f(self.s_t.as_ref(), time);
        self.s_finish.set(gl, self.width.max(1), self.height.max(1), time, false);
        gl.uniform1f(self.s_dark.as_ref(), self.dark);
        // it lies on the far plane: where anything has been drawn it fails the depth test
        gl.depth_func(GL::LEQUAL);
        gl.depth_mask(false);
        gl.bind_vertex_array(None);
        gl.draw_arrays(GL::TRIANGLES, 0, 3);
        gl.depth_mask(true);
        gl.depth_func(GL::LESS);
    }

    fn make_targets(gl: &GL, w: u32, h: u32, samples: i32) -> Result<Targets, String> {
        let (wi, hi) = (w as i32, h as i32);
        let rb_c = gl.create_renderbuffer().ok_or("renderbuffer")?;
        gl.bind_renderbuffer(GL::RENDERBUFFER, Some(&rb_c));
        gl.renderbuffer_storage_multisample(GL::RENDERBUFFER, samples, GL::RGBA8, wi, hi);
        let rb_d = gl.create_renderbuffer().ok_or("renderbuffer")?;
        gl.bind_renderbuffer(GL::RENDERBUFFER, Some(&rb_d));
        gl.renderbuffer_storage_multisample(GL::RENDERBUFFER, samples, GL::DEPTH_COMPONENT24, wi, hi);
        let msaa = gl.create_framebuffer().ok_or("framebuffer")?;
        gl.bind_framebuffer(GL::FRAMEBUFFER, Some(&msaa));
        gl.framebuffer_renderbuffer(GL::FRAMEBUFFER, GL::COLOR_ATTACHMENT0, GL::RENDERBUFFER, Some(&rb_c));
        gl.framebuffer_renderbuffer(GL::FRAMEBUFFER, GL::DEPTH_ATTACHMENT, GL::RENDERBUFFER, Some(&rb_d));
        if gl.check_framebuffer_status(GL::FRAMEBUFFER) != GL::FRAMEBUFFER_COMPLETE {
            return Err("the multisampled framebuffer is incomplete".into());
        }
        gl.bind_framebuffer(GL::FRAMEBUFFER, None);
        Ok(Targets { w, h, samples, msaa, rb: [rb_c, rb_d] })
    }

    /// Puts the finished picture on the canvas, averaging each pixel's samples on the way.
    fn present(&self, t: &Targets) {
        let gl = &self.gl;
        gl.bind_framebuffer(GL::READ_FRAMEBUFFER, Some(&t.msaa));
        gl.bind_framebuffer(GL::DRAW_FRAMEBUFFER, None);
        let (w, h) = (t.w as i32, t.h as i32);
        gl.blit_framebuffer(0, 0, w, h, 0, 0, w, h, GL::COLOR_BUFFER_BIT, GL::NEAREST);
        gl.bind_framebuffer(GL::FRAMEBUFFER, None);
    }

    /// Gun `idx` alone on a flat grey `bg`, from its right side and a little above, sized to fill the picture: the
    /// loadout's pictures are made from these. Lit by the sun from over the viewer's shoulder, with no shadows,
    /// no hands, and none of the vignette or grain.
    fn draw_showcase(&self, t: &Targets, idx: usize, bg: f32, aspect: f32) {
        let gl = &self.gl;
        // nothing is drawn over the background, so it is graded here
        let c = grade_rgb([bg; 3]);
        gl.clear_color(c[0], c[1], c[2], 1.0);
        gl.clear(GL::COLOR_BUFFER_BIT | GL::DEPTH_BUFFER_BIT);
        gl.use_program(Some(&self.prog));
        self.finish.set(gl, t.w, t.h, 0.0, true);
        gl.uniform2f(self.u_fogr.as_ref(), 1e4, 2e4);
        gl.uniform1f(self.u_batch.as_ref(), 0.0);
        gl.uniform1f(self.u_shon.as_ref(), 0.0);
        gl.uniform1f(self.u_dark.as_ref(), 0.0);
        gl.uniform1f(self.u_flash.as_ref(), 0.0);
        let fov = 0.5f32;
        gl.uniform_matrix4fv_with_f32_array(self.u_vp.as_ref(), false, &perspective(fov, aspect, 0.05, 50.0));
        gl.uniform3f(self.u_eye.as_ref(), 0.0, 0.0, 0.0);
        // a camera facing away from the sun, so it lights the side being looked at
        let fwd = norm([-SUN[0], -0.45, -SUN[2]]);
        let (s, u) = view_axes(fwd, 0.0);
        gl.uniform1f(self.u_view.as_ref(), 1.0);
        gl.uniform_matrix3fv_with_f32_array(self.u_envrot.as_ref(), false, &[s[0], s[1], s[2], u[0], u[1], u[2], -fwd[0], -fwd[1], -fwd[2]]);
        gl.uniform3f(self.u_eyew.as_ref(), 0.0, 1.6, 0.0);

        let gun = &self.guns[idx];
        let (lo, hi) = gun.bounds;
        let centre = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0, (lo[2] + hi[2]) / 2.0];
        // far enough back for the gun's length to fit across, or its height to fit up the picture
        let half = (fov / 2.0).tan();
        let dist = ((hi[2] - lo[2]) / 2.0 / (half * aspect)).max((hi[1] - lo[1]) / 2.0 / half) * 1.12;
        let model = mul(
            &translate(0.0, 0.0, -dist),
            &mul(&rot_x(0.10), &mul(&rot_y(-std::f32::consts::FRAC_PI_2 + 0.18), &translate(-centre[0], -centre[1], -centre[2]))),
        );
        self.draw_gun(idx, &model, 0.0, 0.0, 0.0, 0.0, 0.0, 0, false);
        gl.uniform1f(self.u_view.as_ref(), 0.0);
        self.present(t);
    }

    /// The sun's view of the area around `centre`, snapped to whole texels so shadows don't shimmer as it moves.
    fn light_vp(&self, centre: V3) -> M4 {
        let l = norm(SUN);
        let f = [-l[0], -l[1], -l[2]];
        let s = norm(cross(f, [0.0, 1.0, 0.0]));
        let u = cross(s, f);
        let texel = 2.0 * SHADOW_HALF / SHADOW_SIZE as f32;
        let (a, b) = (dot(centre, s), dot(centre, u));
        let (da, db) = ((a / texel).round() * texel - a, (b / texel).round() * texel - b);
        let c = [centre[0] + s[0] * da + u[0] * db, centre[1] + s[1] * da + u[1] * db, centre[2] + s[2] * da + u[2] * db];
        let eye = [c[0] + l[0] * 110.0, c[1] + l[1] * 110.0, c[2] + l[2] * 110.0];
        mul(&ortho(SHADOW_HALF, 220.0), &look_dir(eye, f))
    }

    /// Draws everything that blocks the sun into the shadow map: the nearby walls and the zombies queued by `render`.
    fn shadow_pass(&self, centre: V3, lvp: &M4) {
        let gl = &self.gl;
        gl.bind_texture(GL::TEXTURE_2D, None);
        gl.bind_framebuffer(GL::FRAMEBUFFER, Some(&self.shadow_fbo));
        gl.viewport(0, 0, SHADOW_SIZE, SHADOW_SIZE);
        gl.color_mask(false, false, false, false);
        gl.clear(GL::DEPTH_BUFFER_BIT);
        gl.uniform1f(self.u_pass.as_ref(), 1.0);
        self.shadow_pass.set(true);
        gl.uniform_matrix4fv_with_f32_array(self.u_vp.as_ref(), false, lvp);
        gl.enable(GL::POLYGON_OFFSET_FILL);
        gl.polygon_offset(2.0, 4.0);
        if let Some(w) = &self.world {
            gl.bind_vertex_array(Some(&w.vao));
            gl.uniform1f(self.u_batch.as_ref(), 1.0);
            gl.uniform_matrix4fv_with_f32_array(self.u_model.as_ref(), false, &IDENTITY);
            for c in &w.chunks {
                // anything within reach of the covered area, plus the length of the longest shadow
                let gap = |a: usize| (c.min[a] - centre[a]).max(centre[a] - c.max[a]).max(0.0);
                if (gap(0).powi(2) + gap(2).powi(2)).sqrt() < SHADOW_HALF * 1.45 + 40.0 {
                    gl.draw_arrays(GL::TRIANGLES, c.first, c.count);
                }
            }
            gl.uniform1f(self.u_batch.as_ref(), 0.0);
        }
        self.flush();
        gl.disable(GL::POLYGON_OFFSET_FILL);
        self.shadow_pass.set(false);
        gl.color_mask(true, true, true, true);
        gl.uniform1f(self.u_pass.as_ref(), 0.0);
        gl.bind_framebuffer(GL::FRAMEBUFFER, None);
    }

    pub fn render(&self, g: &Game) {
        let gl = &self.gl;
        let eye = g.me().eye();
        let aspect = self.width as f32 / self.height.max(1) as f32;
        let p = g.me();
        let fov = p.fov();
        let fwd = p.forward();

        // zombies are queued once and drawn twice: into the sun's shadow map, then into the picture
        self.list_e.borrow_mut().clear();
        self.jobs.borrow_mut().clear();
        let foot = p.body.pos;
        let centre = [foot[0] + fwd[0] * 18.0, foot[1], foot[2] + fwd[2] * 18.0];
        // nearest first, so a zombie behind another is mostly turned away by the depth test
        let mut near: Vec<(f32, usize)> = g
            .bots
            .iter()
            .enumerate()
            .filter(|(_, b)| b.alive || (b.corpse && b.dead_t < crate::sim::CORPSE_T))
            .map(|(i, b)| ((b.body.pos[0] - eye[0]).hypot(b.body.pos[2] - eye[2]), i))
            .collect();
        near.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (i, b) in near.into_iter().map(|(_, i)| (i, &g.bots[i])) {
            let to = [b.body.pos[0] - eye[0], b.body.pos[1] - eye[1], b.body.pos[2] - eye[2]];
            let near_light = (b.body.pos[0] - centre[0]).hypot(b.body.pos[2] - centre[2]) < SHADOW_HALF * 1.4;
            if dot(to, fwd) >= -3.0 || near_light {
                self.draw_zombie(g, i, b);
            }
        }
        // the other players, playing online
        for (_, m) in g.players.iter().enumerate().filter(|(i, _)| *i != g.local) {
            self.draw_mate(g, m);
        }
        gl.use_program(Some(&self.prog));
        let lvp = self.light_vp(centre);
        gl.uniform_matrix4fv_with_f32_array(self.u_lvp.as_ref(), false, &lvp);
        self.shadow_pass(centre, &lvp);

        let q = &QUALITY[(self.quality as usize).min(QUALITY.len() - 1)];
        let samples = q.samples.min(self.max_samples);
        let (w, h) = (self.width.max(1), self.height.max(1));
        if self.targets.borrow().as_ref().map_or(true, |t| t.w != w || t.h != h || t.samples != samples) {
            if let Some(old) = self.targets.borrow_mut().take() {
                gl.delete_framebuffer(Some(&old.msaa));
                for r in &old.rb {
                    gl.delete_renderbuffer(Some(r));
                }
            }
            *self.targets.borrow_mut() = Some(Self::make_targets(gl, w, h, samples).expect("render targets"));
        }
        let targets = self.targets.borrow();
        let t = targets.as_ref().expect("render targets");
        gl.bind_framebuffer(GL::FRAMEBUFFER, Some(&t.msaa));
        gl.viewport(0, 0, w as i32, h as i32);
        if let Some((gun, bg)) = self.showcase.get() {
            self.draw_showcase(t, gun, bg, aspect);
            return;
        }
        gl.clear_color(self.haze[0], self.haze[1], self.haze[2], 1.0);
        gl.clear(GL::COLOR_BUFFER_BIT | GL::DEPTH_BUFFER_BIT);

        gl.use_program(Some(&self.prog));
        self.finish.set(gl, w, h, g.time, false);
        gl.uniform3f(self.u_fogc.as_ref(), self.haze[0], self.haze[1], self.haze[2]);
        gl.uniform2f(self.u_fogr.as_ref(), self.fog_start, self.fog_end);
        gl.uniform1f(self.u_batch.as_ref(), 0.0);
        gl.active_texture(GL::TEXTURE0);
        gl.bind_texture(GL::TEXTURE_2D, Some(&self.shadow_tex));
        gl.uniform1i(self.u_shadow.as_ref(), 0);
        gl.uniform1f(self.u_shon.as_ref(), 1.0);
        gl.uniform1f(self.u_soft.as_ref(), if q.soft_shadows { 1.0 } else { 0.0 });
        gl.uniform1f(self.u_dark.as_ref(), self.dark);
        gl.uniform1f(self.u_flash.as_ref(), if self.dark > 0.0 { 1.0 } else { 0.0 });
        gl.uniform3f(self.u_fdir.as_ref(), fwd[0], fwd[1], fwd[2]);
        // the head bobs with the stride; shots still go from the true eye, and rolling around the line of sight
        // leaves the crosshair on it
        let (side, up, roll) = head_bob(p);
        // a blast close by shakes the view
        let k = p.shake * p.shake * 0.06;
        let (side, up, roll) = (side + (g.time * 53.0).sin() * k, up + (g.time * 61.0).cos() * k, roll + (g.time * 47.0).sin() * k * 0.5);
        let (s, u) = view_axes(norm(fwd), roll);
        let eye = [eye[0] + s[0] * side + u[0] * up, eye[1] + s[1] * side + u[1] * up, eye[2] + s[2] * side + u[2] * up];
        let vp = mul(&perspective(fov, aspect, NEAR, FAR), &look_rolled(eye, fwd, roll));
        gl.uniform_matrix4fv_with_f32_array(self.u_vp.as_ref(), false, &vp);
        gl.uniform3f(self.u_eye.as_ref(), eye[0], eye[1], eye[2]);
        let cam = Camera { eye, right: s, up: u, fwd: norm(fwd), roll };
        self.tick_brass(g);

        self.draw_world(eye, &vp);
        self.flush();
        for d in &g.debris {
            // drops shrink away at the end of their life
            let k = (d.life / 0.6).min(1.0);
            let h = [d.half[0] * k, d.half[1] * k, d.half[2] * k];
            let spin = mul(&rot_y(d.rot[1]), &mul(&rot_x(d.rot[0]), &rot_z(d.rot[2])));
            let model = mul(&translate(d.pos[0], d.pos[1], d.pos[2]), &mul(&spin, &scale(h[0], h[1], h[2])));
            self.draw_mesh(&self.sphere_vao, self.sphere_count, &model, d.color, d.glow, 0.3, 0.0);
        }
        self.draw_grenades(g);
        for (_, m) in g.players.iter().enumerate().filter(|(i, _)| *i != g.local) {
            self.draw_mate_gun(m);
        }
        self.draw_brass(None);
        for t in &g.tracers {
            self.draw_lines(&[t.a[0], t.a[1], t.a[2], t.b[0], t.b[1], t.b[2]], [1.0, 0.95, 0.6], 1.0);
        }
        // the sky goes in last, behind everything, so only the part that shows is worked out
        self.ground_depth();
        self.draw_sky(fwd, fov, aspect, g.time);
        gl.use_program(Some(&self.prog));

        // viewmodel: drawn in view space on a cleared depth buffer so it never clips into walls
        gl.clear(GL::DEPTH_BUFFER_BIT);
        // the gun keeps its own field of view so zooming the world doesn't magnify it
        gl.uniform_matrix4fv_with_f32_array(self.u_vp.as_ref(), false, &perspective(1.2, aspect, 0.05, 200.0));
        gl.uniform3f(self.u_eye.as_ref(), 0.0, 0.0, 0.0);
        gl.uniform1f(self.u_flash.as_ref(), 0.0);
        // the gun is lit and shadowed as if it were out in the world, so it needs the camera's orientation
        gl.uniform1f(self.u_view.as_ref(), 1.0);
        gl.uniform_matrix3fv_with_f32_array(self.u_envrot.as_ref(), false, &[s[0], s[1], s[2], u[0], u[1], u[2], -fwd[0], -fwd[1], -fwd[2]]);
        gl.uniform3f(self.u_eyew.as_ref(), eye[0], eye[1], eye[2]);
        // the grenade in hand instead of the gun; just after a throw the gun is still down out of sight while the
        // hand follows through
        self.draw_grenade_view(g);
        self.draw_brass(Some(&cam));
        if !p.aiming && !(p.gun().scope && p.ads > 0.6) {
            self.draw_viewmodel(g, &cam);
        }
        gl.uniform1f(self.u_view.as_ref(), 0.0);
        self.present(t);
    }

    /// Grenades out in the world: flying ones, the ones going off, and where the one in hand would fly.
    fn draw_grenades(&self, g: &Game) {
        for n in &g.grenades {
            let spin = mul(&rot_y(n.rot[1]), &mul(&rot_x(n.rot[0]), &rot_z(n.rot[2])));
            // the pin and the spoon are gone once it's thrown; drawn half again its size so it reads at a distance
            let model = mul(&translate(n.pos[0], n.pos[1], n.pos[2]), &mul(&spin, &scale(1.5, 1.5, 1.5)));
            self.draw_pieces(&model, &self.grenade.body);
        }
        for b in &g.blasts {
            self.draw_blast(b);
        }
        let p = g.me();
        if p.aiming && p.alive() {
            self.draw_arc(&p.arc, p.arc_axis);
        }
    }

    /// Where a grenade about to be thrown would fly: a white line up to the first thing it hits, and a ring lying
    /// on that, flat on the ground or against the wall.
    fn draw_arc(&self, arc: &[V3], axis: usize) {
        let Some(&end) = arc.last() else { return };
        let mut pts = Vec::new();
        // from the hand, low on the right of the view
        for w in arc.windows(2) {
            pts.extend_from_slice(&w[0]);
            pts.extend_from_slice(&w[1]);
        }
        // just off the surface, so it isn't lost in it
        let mut c = end;
        if axis == 1 {
            c[1] += 0.02;
        }
        let (u, v) = match axis {
            0 => (1, 2),
            2 => (0, 1),
            _ => (0, 2),
        };
        let n = 24;
        for i in 0..n {
            for a in [i, i + 1] {
                let t = a as f32 / n as f32 * std::f32::consts::TAU;
                let mut p = c;
                p[u] += t.cos() * 0.4;
                p[v] += t.sin() * 0.4;
                pts.extend_from_slice(&p);
            }
        }
        self.draw_lines(&pts, [1.0, 1.0, 1.0], 1.0);
    }

    /// A grenade going off: a white-hot ball of puffs that swells in a blink, cools through orange to dark smoke,
    /// rises and shrinks away.
    fn draw_blast(&self, b: &crate::sim::Blast) {
        let s = b.t / crate::sim::BLAST_T;
        let swell = (b.t / 0.12).min(1.0).sqrt();
        let size = swell * (1.0 - s).powf(0.6);
        // hot to begin with, then cooling: glowing yellow, orange, red, and lit grey smoke
        let heat = (1.0 - b.t / 0.45).clamp(0.0, 1.0);
        let fire = [1.0, 0.45 + 0.5 * heat, 0.12 + 0.6 * heat * heat];
        let smoke = [0.22, 0.21, 0.2];
        let k = (b.t / 0.35).min(1.0);
        let color = [0, 1, 2].map(|i| fire[i] + (smoke[i] - fire[i]) * k * k);
        let glow = heat.powf(0.5);
        // puffs around the middle, the same every time so the cloud doesn't flicker
        for i in 0..7 {
            let a = i as f32 * 2.4;
            let (off, up) = if i == 0 { (0.0, 0.0) } else { (0.75, (i % 3) as f32 * 0.35 - 0.2) };
            let r = (if i == 0 { 1.5 } else { 1.0 + 0.15 * (i % 2) as f32 }) * size;
            let rise = 0.6 + 1.6 * s;
            let c = [b.pos[0] + a.cos() * off * swell, b.pos[1] + rise + up * swell, b.pos[2] + a.sin() * off * swell];
            let model = mul(&translate(c[0], c[1], c[2]), &scale(r, r * 0.9, r));
            self.draw_mesh(&self.sphere_vao, self.sphere_count, &model, color, glow, 0.0, 0.0);
        }
    }

    /// A zombie: a smooth skinned human body (see `body`) moved by a jointed skeleton, with clothes, hair and a face.
    /// Walkers shamble with a dragging leg and a lolling head, runners sprint with pumping arms, brutes are broad
    /// and heavy, and the arms rise to strike. This works out the pose and queues the body; `flush` draws it.
    fn draw_zombie(&self, g: &Game, i: usize, b: &Bot) {
        use crate::body as body;
        use std::f32::consts::PI;
        let (p, pp) = (b.body.pos, g.me().body.pos);
        let d2 = (pp[0] - p[0]).powi(2) + (pp[2] - p[2]).powi(2);
        let speed = (b.body.vel[0].powi(2) + b.body.vel[2].powi(2)).sqrt();
        let dead = !b.alive;
        let speed = if dead { 0.0 } else { speed };
        let yaw = b.face;
        let (runner, brute) = (b.kind == 1, b.kind == 2);
        let id = (b.phase * 1000.0) as usize + i;
        // every zombie has its own height and build
        let (sx, sy) = if brute { (1.3, 1.18) } else if runner { (0.93, 0.98) } else { (1.0, 1.0) };
        let (sx, sy) = (sx * (0.92 + 0.14 * ((id * 17) % 7) as f32 / 6.0), sy * (0.95 + 0.1 * ((id * 37) % 11) as f32 / 10.0));
        // a shot zombie crumples backwards, lies there, then shrinks away
        let dying = if dead { (b.dead_t / 0.7).min(1.0).powi(2) } else { 0.0 };
        let fade = if dead { ((crate::sim::CORPSE_T - b.dead_t) / 0.8).clamp(0.0, 1.0) } else { 1.0 };
        let (sx, sy) = (sx * fade, sy * fade);
        let flinch = b.hit_t;
        let base = mul(&mul(&mul(&translate(p[0], p[1] + 0.1 * dying, p[2]), &rot_y(-yaw)), &rot_x(1.5 * dying)), &scale(sx, sy, sx));
        let amp = if b.body.on_ground { (speed / 3.0).min(1.0) } else { 0.0 };
        let ph = g.time * b.speed * 3.0 + b.phase;
        let seed = b.phase * 3.7;
        self.seed.set(seed);
        // 0 -> 1 -> 0 through a swing of the arms
        let strike = (b.swing_t * PI).sin();

        // ---- skeleton: every bone is the rest pose moved by joint angles
        let bob = (ph * 2.0).cos().abs() * 0.025 * amp;
        // walkers surge forward in lurches instead of walking evenly
        let lurch = if runner { 0.0 } else { (ph * 0.5 + seed).sin().max(0.0).powi(2) * 0.16 * amp };
        let lean = if runner { 0.35 } else { 0.2 } + lurch;
        // climbing (see `sim::Climb`): how far through, and the edge's height above the feet and distance ahead, in
        // the body's own measure. It hangs upright against the wall, then folds over the edge as it gets a knee onto it
        let climb = b.climbing().map(|(s, up, ahead)| (s, up / sy, ahead / sx));
        let over = climb.map_or(0.0, |(s, ..)| ((s - crate::sim::CLIMB_OVER) / (1.0 - crate::sim::CLIMB_OVER)).clamp(0.0, 1.0));
        let lean = if climb.is_some() { (0.3 + 0.7 * (PI * over).sin().powf(0.7)) / 1.4 } else { lean };
        // and it rolls from side to side with each heave, the arms taking turns
        let sway = climb.map_or(0.0, |(s, ..)| 0.12 * (s * 4.0 * PI).sin() * (1.0 - over));
        let pelvis = mul(&mul(&base, &translate(ph.sin() * 0.04 * amp, -bob, 0.0)), &around([0.0, 0.94, 0.0], &mul(&rot_y(ph.sin() * 0.14 * amp), &mul(&rot_z(ph.sin() * 0.05 * amp + sway), &rot_x(-0.08)))));
        // never quite the same twice: a few sines at odd rates, for the arms and spine to wander by
        let wander = |rate: f32, k: f32| {
            let t = g.time * rate + seed * k;
            (t.sin() * 0.5 + (t * 1.73 + 1.3).sin() * 0.3 + (t * 2.71 + 4.1).sin() * 0.2) * (1.0 - dying)
        };
        // the head's jerks (see `Bot::head`, which its hit box follows): the back bends most of the way there and
        // the neck the rest, half a metre and a hand's width below the head respectively
        let [jr, ju, jf] = b.head.map(|v| v * (1.0 - dying));
        let spine = mul(&pelvis, &around([0.0, 0.98, 0.0], &rot_x(-(lean * 0.6 + 0.12 * strike) + 0.55 * flinch)));
        let chest = mul(&spine, &around([0.0, 1.16, 0.0], &mul(&rot_y(-ph.sin() * 0.3 * amp + 0.12 * wander(0.9, 1.0)), &mul(&rot_z((ph * 0.5).sin() * 0.04 * amp + 0.05 * wander(1.1, 2.0) - 0.6 * jr / 0.5), &rot_x(-(lean * 0.8 + 0.2 * strike) - 0.7 * jf / 0.5)))));
        let roll = (ph * 0.35 + seed).sin() * 0.28 - 0.4 * jr / 0.15;
        // on the wall it looks up at where it is going
        let look_up = if climb.is_some() { 0.25 * (1.0 - over) } else { 0.0 };
        let head = mul(&chest, &around([0.0, 1.54, 0.0], &mul(&rot_y((ph * 0.3 + seed).sin() * 0.3 * (1.0 - strike) + (g.time * 6.0 + seed).sin().powi(15) * 0.18), &mul(&rot_z(roll), &rot_x((if runner { 0.3 } else { 0.15 }) + 0.7 * flinch + 4.0 * ju + look_up)))));
        let mouth = 0.2 + 0.5 * strike + 0.1 * (ph * 0.8).sin().abs();
        let jaw = mul(&head, &around(body::head_to_rest([0.0, 1.60, 0.03]), &rot_x(-mouth)));
        // the eyes and teeth are placed on the face as modelled, which the body shrinks onto its neck
        let (hk, hp) = (body::HEAD_K, body::HEAD_PIVOT);
        let modelled = mul(&translate(0.0, hp + body::HEAD_LIFT, 0.0), &mul(&scale(hk, hk, hk), &translate(0.0, -hp, 0.0)));

        let mut bones = [IDENTITY; body::BONES];
        bones[body::PELVIS] = pelvis;
        bones[body::SPINE] = spine;
        bones[body::CHEST] = chest;
        bones[body::HEAD] = head;
        bones[body::JAW] = jaw;
        // legs: hips swing, knees fold while the leg is carried forward; the right leg drags
        for (k, side) in [-1.0f32, 1.0].iter().enumerate() {
            let side = *side;
            let phi = ph + if side > 0.0 { PI } else { 0.0 };
            let kk = if side > 0.0 && !runner { 0.6 } else { 1.0 };
            let a = (if runner { 0.95 } else if brute { 0.45 } else { 0.55 }) * amp * kk;
            let (hip_a, knee_a) = (a * phi.sin(), 0.06 + (if runner { 1.0 } else { 0.55 }) * amp * kk * phi.cos().max(0.0));
            let (hip_a, knee_a) = match climb {
                Some((s, ..)) => {
                    // the feet scrabble at the wall in front, out of step, while it hauls; then one knee comes up onto
                    // the top, the other leg after it, and both straighten as it stands
                    let t = g.time * 9.0 + side * PI + seed;
                    let tuck = (0.9 + 0.3 * t.sin(), 1.0 + 0.4 * (t + 0.8).sin());
                    let tuck = if s < crate::sim::CLIMB_GRAB { (0.3, 0.6) } else { tuck };
                    let lag = if side < 0.0 { 0.0 } else { 0.25 };
                    let k = (PI * ((over - lag) / (0.8 - lag * 0.6)).clamp(0.0, 1.0)).sin();
                    let up = (1.7 * k, 2.1 * k);
                    let w = ((over - lag) / 0.15).clamp(0.0, 1.0);
                    (tuck.0 + (up.0 - tuck.0) * w, tuck.1 + (up.1 - tuck.1) * w)
                }
                None => (hip_a, knee_a),
            };
            let hip = mul(&pelvis, &around([0.0, 0.92, 0.0], &rot_x(hip_a)));
            let knee = mul(&hip, &around([0.0, 0.50, 0.0], &rot_x(-knee_a)));
            let ankle = mul(&knee, &around([0.0, 0.09, 0.0], &rot_x(-(hip_a - knee_a) * 0.7)));
            bones[body::HIP + k] = hip;
            bones[body::KNEE + k] = knee;
            bones[body::ANKLE + k] = ankle;

            // arms: walkers reach out, runners pump, brutes hang heavy. The body is modelled with its arms held out,
            // so each arm bone first brings its arm back down to the sides, then poses it
            let (sh_a, el_a) = if runner {
                (0.5 + (ph + if side > 0.0 { 0.0 } else { PI }).sin() * 0.9 * amp.max(0.4), 1.2)
            } else if brute {
                (0.9 + (ph * 0.5 + side).sin() * 0.1 + 0.9 * strike, 0.3)
            } else {
                (1.15 + 0.35 * side * seed.sin() + 0.14 * (ph * 0.7 + side * 1.3).sin() + 1.0 * strike, 0.45 + 0.2 * (ph * 0.9 + side).sin() - 0.5 * strike)
            };
            // the arms wander up and down and out from the body on their own, never in step
            let sh_a = (sh_a + 0.22 * wander(1.3, 5.0 + side)) * (1.0 - 0.75 * dying) + 0.9 * flinch;
            let el_a = el_a + 0.25 * wander(1.7, 9.0 + side);
            let splay = 0.12 + 0.1 * wander(0.8, 13.0 + side);
            // climbing, the hands hold the edge while the body moves under them, elbows out: the arm is bent to
            // reach it from the shoulder, which leans forward with the back (it is `lean` * 1.4 and the pelvis's tilt)
            let (sh_a, el_a, wr_a, splay) = match climb {
                Some((s, up, ahead)) => {
                    let tilt = 0.08 + lean * 1.4;
                    let (sy0, sf) = (0.94 + 0.47 * tilt.cos(), 0.47 * tilt.sin());
                    let (f, h) = (ahead + 0.15 - sf, up - sy0);
                    let (l1, l2) = (0.28, 0.38);
                    let d = f.hypot(h).clamp(0.12, l1 + l2 - 0.001);
                    let bend = PI - ((l1 * l1 + l2 * l2 - d * d) / (2.0 * l1 * l2)).clamp(-1.0, 1.0).acos();
                    let open = ((l1 * l1 + d * d - l2 * l2) / (2.0 * l1 * d)).clamp(-1.0, 1.0).acos();
                    let hold = (f.atan2(-h) - open + tilt, bend, 0.9 - bend * 0.5);
                    // it reaches up as it hops for the edge, and lets go once a knee is up on it
                    let first = if side < 0.0 { 0.0 } else { 0.4 };
                    let on = ((s / crate::sim::CLIMB_GRAB - first) * 2.0).clamp(0.0, 1.0) * (1.0 - ((over - 0.45) / 0.35).clamp(0.0, 1.0));
                    let mix = |a: f32, b: f32| a + (b - a) * on;
                    (mix(sh_a, hold.0), mix(el_a, hold.1), mix(0.35, hold.2), mix(splay, 0.35 + 0.08 * wander(2.0, side)))
                }
                None => (sh_a, el_a, 0.35, splay),
            };
            let sh = mul(&chest, &around([side * body::ARM_X, 1.41, 0.0], &mul(&rot_z(side * splay), &rot_x(sh_a))));
            let el = mul(&sh, &around([0.0, 1.13, 0.0], &rot_x(el_a)));
            let wr = mul(&el, &around([0.0, 0.87, 0.0], &rot_x(wr_a)));
            let down = around([side * body::ARM_X, body::SHOULDER_Y, 0.0], &rot_z(-side * body::ARM_ANGLE));
            bones[body::SHOULDER + k] = mul(&sh, &down);
            bones[body::ELBOW + k] = mul(&el, &down);
            bones[body::WRIST + k] = mul(&wr, &down);
        }

        // ---- looks
        let skin = [[0.70, 0.57, 0.50], [0.58, 0.44, 0.37], [0.78, 0.66, 0.58], [0.40, 0.30, 0.25]][(id / 3) % 4];
        let skin = if brute { [skin[0] * 0.92, skin[1] * 0.85, skin[2] * 0.82] } else { [skin[0] * 0.9, skin[1] * 0.86, skin[2] * 0.86] };
        let shirt = [[0.36, 0.17, 0.14], [0.20, 0.28, 0.36], [0.40, 0.40, 0.34], [0.18, 0.30, 0.21], [0.52, 0.47, 0.33], [0.62, 0.60, 0.56], [0.78, 0.40, 0.08], [0.09, 0.09, 0.10]][id % 8];
        let pants = [[0.18, 0.22, 0.30], [0.34, 0.31, 0.24], [0.10, 0.10, 0.11]][(id / 5) % 3];
        let shoe = [[0.10, 0.09, 0.08], [0.22, 0.14, 0.09], [0.16, 0.16, 0.18]][(id / 2) % 3];
        let hair = [[0.10, 0.07, 0.05], [0.04, 0.035, 0.03], [0.45, 0.34, 0.17], [0.40, 0.38, 0.36], [0.30, 0.13, 0.07], [0.17, 0.12, 0.08]][(id / 7) % 6];
        let (bone, dark) = ([0.80, 0.76, 0.62], [0.04, 0.02, 0.02]);
        let _ = dark;
        // some sleeves are long, some short, some gone; some shirts stop short of the belly; one arm is often bare
        let sleeve = [0.0, 1.2, 1.2, 0.0][id % 4];
        let hem = if id % 2 == 0 { 0.9 } else { 1.08 };
        let bare = if id % 3 == 0 { 0.0 } else if id % 2 == 0 { -1.0 } else { 1.0 };
        let torn = if id % 2 == 1 { 1.0 } else { 0.0 };
        let mut wounds = [[0.0f32; 4]; 3];
        let mut sites: Vec<[f32; 4]> = Vec::new();
        if id % 3 != 2 {
            sites.push([-0.045, 1.485, -0.04, 0.045]);
        }
        if id % 4 == 1 {
            sites.push([0.07, 1.31, -0.095, 0.06]);
        }
        if id % 5 == 2 {
            sites.push([-0.03, 1.10, -0.10, 0.05]);
            let brow = body::head_to_rest([0.05, 1.70, -0.09]);
            sites.push([brow[0], brow[1], brow[2], 0.03]);
        }
        if bare != 0.0 {
            let at = body::arm_rest(bare, [body::ARM_X, 1.03, -0.045]);
            sites.push([at[0], at[1], at[2], 0.04]);
        }
        if torn > 0.0 {
            sites.push([0.083, 0.30, -0.045, 0.045]);
        }
        for (w, s) in wounds.iter_mut().zip(&sites) {
            *w = *s;
        }
        let d = d2.sqrt();
        // detail by how big the zombie is in the picture: the distance suits one 1600 pixels tall at the hip's
        // field of view, and comes nearer for coarser pictures, where the near mesh's triangles would be smaller
        // than a pixel. It never goes further, so a bigger picture or a zoomed sight costs no more than that
        let px = (self.height as f32 / 1600.0 * 0.6f32.tan() / (g.me().fov() / 2.0).tan()).min(1.0);
        let lod = if d < 35.0 * px { 0 } else { 1 };
        self.jobs.borrow_mut().push(SkinJob {
            bones: bones.iter().flatten().copied().collect(),
            lod,
            skin,
            shirt,
            pants,
            shoe,
            hair,
            cut: [hem, sleeve, bare, torn],
            hair_kind: [if id % 7 != 0 { 1.0 } else { 0.0 }, if id % 3 == 1 { 1.0 } else { 0.0 }],
            wounds,
            seed,
        });

        // ---- the face: wet bloodshot eyes, teeth and a tongue, which sit in the head and jaw
        if d < 28.0 {
            let r0 = IDENTITY;
            let (head, jaw) = (mul(&head, &modelled), mul(&jaw, &modelled));
            let iris = [[0.35, 0.20, 0.10], [0.20, 0.30, 0.35], [0.30, 0.35, 0.15], [0.10, 0.07, 0.05]][(id / 4) % 4];
            for sx in [-1.0f32, 1.0] {
                self.ell(&head, [sx * 0.04, 1.668, -0.082], [0.0108, 0.0102, 0.0102], &r0, iris, 14.0, 0.0, 0.0);
            }
            for k in -2..=2 {
                let x = k as f32 * 0.0095;
                let z = -0.0985 + x.abs() * 1.3;
                self.ell(&head, [x, 1.5835, z], [0.0046, 0.0075, 0.0035], &r0, bone, 0.0, 0.3, 0.0);
                self.ell(&jaw, [x, 1.566, z], [0.0046, 0.0068, 0.0035], &r0, bone, 0.0, 0.3, 0.0);
            }
            self.ell(&jaw, [0.0, 1.5625, -0.075], [0.013, 0.005, 0.018], &r0, [0.55, 0.16, 0.18], 11.0, 0.0, 0.0);
        }
    }

    /// Another player: the zombies' body (see `draw_zombie`) standing straight in clean clothes, a colour of shirt
    /// each, holding their gun up where they aim, legs going as they move, and lying flat while they are down.
    /// Queues the body, as `draw_zombie` does; `draw_mate_gun` draws the gun.
    fn draw_mate(&self, g: &Game, p: &crate::sim::Player) {
        use crate::body as body;
        use std::f32::consts::PI;
        let at = p.body.pos;
        let id = p.id as usize;
        let speed = p.body.vel[0].hypot(p.body.vel[2]);
        let amp = if p.body.on_ground { (speed / 5.0).min(1.0) } else { 0.3 };
        let down = if p.alive() { 0.0 } else { 1.0 };
        let base = mul(&mul(&translate(at[0], at[1] + 0.1 * down, at[2]), &rot_y(-p.yaw)), &rot_x(1.5 * down));
        let ph = g.time * (4.0 + speed * 1.1) + id as f32;
        let bob = (ph * 2.0).cos().abs() * 0.03 * amp;
        let pelvis = mul(&mul(&base, &translate(0.0, -bob, 0.0)), &around([0.0, 0.94, 0.0], &rot_y(ph.sin() * 0.1 * amp)));
        let spine = mul(&pelvis, &around([0.0, 0.98, 0.0], &rot_x(-0.08 - 0.1 * amp)));
        let chest = mul(&spine, &around([0.0, 1.16, 0.0], &rot_x(-0.25 * p.pitch * (1.0 - down))));
        let head = mul(&chest, &around([0.0, 1.54, 0.0], &rot_x(-0.6 * p.pitch * (1.0 - down))));
        let jaw = mul(&head, &around(body::head_to_rest([0.0, 1.60, 0.03]), &rot_x(-0.05)));
        let mut bones = [IDENTITY; body::BONES];
        bones[body::PELVIS] = pelvis;
        bones[body::SPINE] = spine;
        bones[body::CHEST] = chest;
        bones[body::HEAD] = head;
        bones[body::JAW] = jaw;
        for (k, side) in [-1.0f32, 1.0].iter().enumerate() {
            let side = *side;
            let phi = ph + if side > 0.0 { PI } else { 0.0 };
            let (hip_a, knee_a) = (0.6 * amp * phi.sin(), 0.06 + 0.7 * amp * phi.cos().max(0.0));
            let hip = mul(&pelvis, &around([0.0, 0.92, 0.0], &rot_x(hip_a)));
            let knee = mul(&hip, &around([0.0, 0.50, 0.0], &rot_x(-knee_a)));
            let ankle = mul(&knee, &around([0.0, 0.09, 0.0], &rot_x(-(hip_a - knee_a) * 0.7)));
            bones[body::HIP + k] = hip;
            bones[body::KNEE + k] = knee;
            bones[body::ANKLE + k] = ankle;
            // both hands on the gun, out in front and following the aim; a grenade in hand, the right arm goes up
            // and back to throw. Down, the arms lie at the sides
            let aim = (1.0 - down) * if p.aiming && side > 0.0 { 0.0 } else { 1.0 };
            let throw = (1.0 - down) * if p.aiming && side > 0.0 { 1.0 } else { 0.0 };
            let sh_a = (0.75 + 0.7 * p.pitch + if side > 0.0 { 0.0 } else { 0.15 }) * aim + 2.6 * throw + 0.1 * down;
            let el_a = (if side > 0.0 { 1.0 } else { 0.55 }) * aim + 1.4 * throw;
            let splay = -0.3 * aim + 0.25 * throw + 0.1 * down;
            let sh = mul(&chest, &around([side * body::ARM_X, 1.41, 0.0], &mul(&rot_z(side * splay), &rot_x(sh_a))));
            let el = mul(&sh, &around([0.0, 1.13, 0.0], &rot_x(el_a)));
            let wr = mul(&el, &around([0.0, 0.87, 0.0], &rot_x(0.2)));
            let rest = around([side * body::ARM_X, body::SHOULDER_Y, 0.0], &rot_z(-side * body::ARM_ANGLE));
            bones[body::SHOULDER + k] = mul(&sh, &rest);
            bones[body::ELBOW + k] = mul(&el, &rest);
            bones[body::WRIST + k] = mul(&wr, &rest);
        }
        let shirt = [[0.16, 0.30, 0.62], [0.75, 0.38, 0.10], [0.22, 0.45, 0.20], [0.50, 0.20, 0.55], [0.70, 0.62, 0.15], [0.12, 0.50, 0.55], [0.62, 0.16, 0.18], [0.85, 0.85, 0.85]][id % 8];
        let skin = [[0.80, 0.62, 0.50], [0.55, 0.40, 0.30], [0.90, 0.74, 0.62], [0.42, 0.30, 0.22]][(id / 2) % 4];
        let hair = [[0.10, 0.07, 0.05], [0.45, 0.34, 0.17], [0.04, 0.035, 0.03], [0.30, 0.13, 0.07]][(id / 3) % 4];
        let d = (at[0] - g.me().body.pos[0]).hypot(at[2] - g.me().body.pos[2]);
        self.jobs.borrow_mut().push(SkinJob {
            bones: bones.iter().flatten().copied().collect(),
            lod: if d < 30.0 { 0 } else { 1 },
            skin,
            shirt,
            pants: [0.17, 0.19, 0.16],
            shoe: [0.08, 0.07, 0.06],
            hair,
            // a whole shirt with long sleeves, and none of the zombies' holes in it
            cut: [0.85, 0.0, 0.0, 0.0],
            hair_kind: [1.0, 0.0],
            wounds: [[0.0; 4]; 3],
            seed: -1.0,
        });
    }

    /// Another player's gun, in their hands at the chest, pointing where they aim.
    fn draw_mate_gun(&self, p: &crate::sim::Player) {
        if !p.alive() || p.aiming {
            return;
        }
        let (sy, cy) = p.yaw.sin_cos();
        let right = [cy, 0.0, sy];
        let at = p.body.pos;
        let c = [at[0] + right[0] * 0.12 + sy * 0.3, at[1] + 1.33 + 0.3 * p.pitch.sin(), at[2] + right[2] * 0.12 - cy * 0.3];
        let spec = self.guns[p.weapon].spec;
        let gun = mul(&mul(&translate(c[0], c[1], c[2]), &mul(&rot_y(-p.yaw), &rot_x(p.pitch))), &translate(spec.hold[0], spec.hold[1], spec.hold[2]));
        let reload = if p.reload_t > 0.0 { 0.5 } else { 0.0 };
        self.draw_gun(p.weapon, &gun, 0.0, reload, 0.0, 0.0, 0.0, 0, false);
    }

    fn draw_piece(&self, model: &M4, p: &GpuPiece) {
        let gl = &self.gl;
        gl.bind_vertex_array(Some(&p.vao));
        gl.uniform_matrix4fv_with_f32_array(self.u_model.as_ref(), false, model);
        gl.uniform3f(self.u_color.as_ref(), p.color[0], p.color[1], p.color[2]);
        gl.uniform1f(self.u_unlit.as_ref(), p.glow);
        gl.uniform1f(self.u_shiny.as_ref(), p.shiny);
        gl.uniform1f(self.u_mat.as_ref(), p.mat);
        gl.draw_elements_with_i32(GL::TRIANGLES, p.count, GL::UNSIGNED_INT, 0);
    }

    fn draw_pieces(&self, model: &M4, pieces: &[GpuPiece]) {
        for p in pieces {
            self.draw_piece(model, p);
        }
    }

    /// A gun with its hands: the magazine comes out and goes back in during a reload while the left hand goes to
    /// fetch it, the trigger is pulled when it fires, and the pump, bolt or slide cycles after a shot.
    /// `turn` is how many chambers a cylinder has turned through, fractions while it is turning, and `fired` how
    /// many rounds have gone since the last reload.
    #[allow(clippy::too_many_arguments)]
    fn draw_gun(&self, idx: usize, gun: &M4, recoil: f32, reload: f32, mag_out: f32, cycle: f32, turn: f32, fired: usize, hands: bool) {
        use crate::gunmodel::Action;
        let g = &self.guns[idx];
        let s = &g.spec;
        let at = |v: [f32; 3], k: f32| translate(v[0] * k, v[1] * k, v[2] * k);
        self.draw_pieces(gun, &g.body);
        let pull = ((recoil - 0.6) * 5.0).clamp(0.0, 1.0);
        self.draw_pieces(&mul(gun, &around(s.trigger_pivot, &rot_x(-0.35 * pull))), &g.trigger);
        let pose = action_pose(s, recoil, reload, cycle, turn);
        let action = mul(gun, &pose);
        self.draw_pieces(&action, &g.action);
        if let Some(r) = &s.rounds {
            // one round in each chamber, riding in the part that opens. Only the player's own reload is played
            // through: someone else's gun is just shown open, its rounds home
            let played = if hands { reload } else { 0.0 };
            let (open, turn) = (mul(gun, &opened(s, reload)), turned(s, turn));
            for k in 0..r.chambers.len() {
                if let Some((brought, at, spent)) = round_at(r, k, fired, played) {
                    self.draw_pieces(&mul(&open, &mul(&brought, &mul(&turn, &at))), &g.mag[..if spent { r.case } else { r.parts }]);
                }
            }
            if let Some(at) = loader_at(r, played) {
                self.draw_pieces(&mul(&open, &at), &g.mag[r.parts..]);
            }
        } else {
            let mag = mul(gun, &mul(&at(s.mag_out, mag_out), &around(s.mag_pivot, &rot_x(s.mag_tilt * mag_out))));
            self.draw_pieces(&mag, &g.mag);
        }
        if !hands {
            return;
        }

        self.draw_pieces(&mul(gun, &at(s.right_at, 1.0)), &self.hands.right);
        // the left hand lets go, takes the magazine and comes back; on a pump it rides the forend
        let go = ease(0.0, 0.2, reload) - ease(0.78, 1.0, reload);
        let reach = if s.pistol_hold { [0.0, -0.07, 0.02] } else { [0.0, -0.085, 0.23] };
        let mut hand = mul(gun, &mul(&at(s.left_at, 1.0), &mul(&at(reach, go), &at(s.mag_out, 0.6 * mag_out))));
        if s.action == Action::Pump {
            hand = mul(&hand, &pose);
        }
        if let Some(r) = &s.rounds {
            // a break action's hand is on the forend, so it rides the barrels down, and lifts them shut again by it;
            // a revolver's stays upright as it brings the speedloader along the swung out cylinder
            let (hold, along) = r.hand_at(fired, reload);
            let (base, along) = if s.action == Action::Break { (action, along) } else { (*gun, turn_dir(&opened(s, reload), along)) };
            let to = std::array::from_fn(|i| hold[i] + along[i]);
            hand = mul(&base, &mul(&at(s.left_at, 1.0), &at(to, 1.0)));
        }
        self.draw_pieces(&hand, if s.pistol_hold { &self.hands.pistol_left } else { &self.hands.left });
    }

    /// Throws out into the world the rounds a reload empties from the chambers: `action` is where the part that
    /// opens is, in the camera's space. Each goes a little differently, out to its own side, and carried along by
    /// the player's own run.
    fn eject(&self, g: &Game, action: &M4, r: &Rounds, fired: usize, cam: &Camera) {
        let p = g.me();
        let mut brass = self.brass.borrow_mut();
        for (k, c) in r.chambers.iter().enumerate() {
            let spent = k < fired;
            if !(spent || r.all) {
                continue;
            }
            let m = mul(action, &translate(c[0], c[1], c[2] + r.out));
            let rot = [cam.dir([m[0], m[1], m[2]]), cam.dir([m[4], m[5], m[6]]), cam.dir([m[8], m[9], m[10]])];
            let h = |n: f32| ((g.time * 12.9898 + k as f32 * 78.233 + n * 37.719).sin() * 43758.547).rem_euclid(1.0);
            let side = if c[0] < 0.0 { -1.0 } else { 1.0 };
            let t = [r.throw[0] * side + (h(1.0) - 0.5) * 0.4, r.throw[1] * (0.8 + 0.4 * h(2.0)) + (h(4.0) - 0.5) * 0.3, r.throw[2] * (0.8 + 0.4 * h(3.0))];
            let along = |i: usize| rot[0][i] * t[0] + rot[1][i] * t[1] + rot[2][i] * t[2];
            let vel = [along(0) + p.body.vel[0], along(1) + p.body.vel[1], along(2) + p.body.vel[2]];
            // end over end, mostly, with a little twist
            let (flip, twist) = (side * (5.0 + 7.0 * h(5.0)), (h(6.0) - 0.5) * 6.0);
            let spin = [rot[0][0] * flip + rot[1][0] * twist, rot[0][1] * flip + rot[1][1] * twist, rot[0][2] * flip + rot[1][2] * twist];
            brass.push(crate::brass::Brass { gun: p.weapon, spent, pos: cam.point([m[12], m[13], m[14]]), vel, rot, spin, radius: r.radius, age: 0.0, resting: false });
        }
        let extra = brass.len().saturating_sub(crate::brass::MAX);
        brass.drain(..extra);
    }

    /// Moves the brass on to the game's time; a new game clears it away.
    fn tick_brass(&self, g: &Game) {
        let last = self.brass_t.replace(g.time);
        let mut brass = self.brass.borrow_mut();
        if g.time < last {
            brass.clear();
        }
        let dt = (g.time - last).clamp(0.0, 0.05);
        for b in brass.iter_mut() {
            b.step(dt, &g.world);
        }
        brass.retain(|b| b.age < crate::brass::LIFE);
    }

    /// The brass just thrown, with the gun in the camera's space, or (with no camera) the rest, out in the world.
    fn draw_brass(&self, cam: Option<&Camera>) {
        let view = cam.map(Camera::view);
        for b in self.brass.borrow().iter().filter(|b| (b.age < BRASS_WITH_GUN) == view.is_some()) {
            let g = &self.guns[b.gun];
            let Some(r) = &g.spec.rounds else { continue };
            let (x, y, z, k) = (b.rot[0], b.rot[1], b.rot[2], b.size());
            let model = [x[0] * k, x[1] * k, x[2] * k, 0.0, y[0] * k, y[1] * k, y[2] * k, 0.0, z[0] * k, z[1] * k, z[2] * k, 0.0, b.pos[0], b.pos[1], b.pos[2], 1.0];
            let model = view.map_or(model, |v| mul(&v, &model));
            self.draw_pieces(&model, if b.spent { &g.mag[..r.case] } else { &g.mag[..] });
        }
    }

    /// The grenade in the right hand while aiming it, coming up from below as it's taken out; once it's thrown,
    /// the empty hand following through forward and down out of the view.
    fn draw_grenade_view(&self, g: &Game) {
        let p = g.me();
        let follow = 1.0 - p.throw_t / crate::sim::THROW_TIME; // 0 the moment it leaves the hand
        if !p.aiming && !(p.throw_t > 0.0 && follow < 0.4) {
            return;
        }
        let speed = (p.body.vel[0].powi(2) + p.body.vel[2].powi(2)).sqrt();
        let walk = if p.body.on_ground { (speed / 6.0).min(1.5) } else { 0.0 };
        let th = p.gait / crate::sim::STRIDE * std::f32::consts::PI - 0.4;
        let bob_x = th.sin() * 0.012 * walk;
        let bob_y = -(2.0 * th).cos() * 0.007 * walk + (g.time * 1.5).sin() * 0.002;
        // out from below, overshooting a touch as it settles
        let up = if p.aiming { 1.0 - (1.0 - (p.aim_t / 0.2).min(1.0)).powi(3) } else { 1.0 };
        let s = if p.aiming { 0.0 } else { follow / 0.4 };
        let held = mul(
            &translate(0.15 + bob_x - 0.10 * s, -0.095 + bob_y - 0.22 * (1.0 - up) + 0.05 * (s * std::f32::consts::PI).sin() - 0.2 * s * s, -0.31 - 0.30 * s),
            &mul(&rot_y(0.45 - 0.6 * s), &mul(&rot_x(0.3 - 0.7 * (1.0 - up) - 1.3 * s), &rot_z(0.3 + 0.4 * (1.0 - up)))),
        );
        self.draw_pieces(&held, &self.grenade.hand);
        if p.aiming {
            self.draw_pieces(&held, &self.grenade.body);
            self.draw_pieces(&held, &self.grenade.spoon);
            self.draw_pieces(&held, &self.grenade.pin);
        }
    }

    fn draw_viewmodel(&self, g: &Game, view: &Camera) {
        let p = g.me();
        let w = p.gun();
        let t = g.time;
        let speed = (p.body.vel[0].powi(2) + p.body.vel[2].powi(2)).sqrt();
        let walk = if p.body.on_ground { (speed / 6.0).min(1.5) } else { 0.0 } * (1.0 - 0.8 * p.ads);
        let recoil = p.cooldown / w.delay; // 1 right after a shot, fading to 0
        let reload = if p.reload_t > 0.0 { 1.0 - p.reload_t / w.reload } else { 0.0 };
        let dip = (reload * std::f32::consts::PI).sin(); // gun lowers and tilts while reloading
        let spec = self.guns[p.weapon].spec;
        // a magazine comes out and goes back in; rounds a gun shows one by one go in their own way (see `round_at`)
        let mag_out = if spec.rounds.is_none() && (0.3..0.7).contains(&reload) {
            ((reload - 0.3) / 0.4 * std::f32::consts::PI).sin()
        } else {
            0.0
        };
        // the muzzle tips up for rounds to come out: a flick to throw them clear, or held up for them to drop
        let tip = spec.rounds.map_or(0.0, |r| r.tip(reload));
        // pump / bolt cycles back and forth between shots
        let cycle = if w.delay > 0.5 && (0.2..0.75).contains(&recoil) {
            ((0.75 - recoil) / 0.55 * std::f32::consts::PI).sin()
        } else {
            0.0
        };
        // swinging up from below when the gun was just selected
        let raise = (p.equip_t / crate::sim::EQUIP_TIME).powi(2);
        // a long gun needs more kick to read as one, and a gun that flips hard shoves back hard too
        let kick = if w.pellets > 1 || w.scope { 0.09f32 } else { 0.05 }.max(spec.flip * 0.2);
        let (flash_z, hold) = (spec.muzzle_z, spec.hold);
        // where the eye sits when aiming, above the gun's origin
        let sight_y = spec.sight_y + hold[1];

        let ads = p.ads;
        let lerp = |a: f32, b: f32| a + (b - a) * ads;
        // the gun swings in time with the feet, lagging the head a little, and much harder at a sprint
        let th = p.gait / crate::sim::STRIDE * std::f32::consts::PI - 0.4;
        let big = 1.0 + 1.6 * p.sprint_t;
        let bob_x = th.sin() * 0.012 * walk * big;
        let bob_y = -(2.0 * th).cos() * 0.007 * walk * big + (t * 1.5).sin() * 0.002 * (1.0 - ads);
        // sprinting carries the gun low and canted across the body; firing brings it straight back up
        let carry = p.sprint_t * (1.0 - ads) * (1.0 - recoil).max(0.0) * (1.0 - dip);
        // how a reload carries the gun: most go low and cant over; a break action or a cylinder is held up and
        // turned toward the eye instead, so the open breech can be seen
        use crate::gunmodel::Action;
        let (lower, pitch, roll, across) = match spec.action {
            Action::Break => (0.01, 0.05, 0.55, 0.06),
            Action::Cylinder => (0.0, 0.25, 0.75, 0.07),
            _ => (0.10, -0.25, 0.5, 0.0),
        };
        let gun = mul(
            &translate(
                lerp(self.gun_offset.get().map_or(0.15, |o| o[0]) + bob_x - 0.04 * carry - across * dip, 0.0),
                lerp(self.gun_offset.get().map_or(-0.17, |o| o[1]) + bob_y - lower * dip - 0.35 * raise - 0.05 * carry + 0.03 * tip, -sight_y),
                lerp(self.gun_offset.get().map_or(-0.38, |o| o[2]), -0.30) + kick * recoil + 0.03 * carry,
            ),
            &mul(
                &rot_y(self.gun_yaw.get() * (1.0 - ads) + 0.55 * carry + th.sin() * 0.03 * walk * p.sprint_t),
                &mul(
                    &rot_x((spec.flip * recoil + pitch * dip + tip) * (1.0 - 0.5 * ads) + 0.8 * raise - 0.22 * carry),
                    &rot_z(roll * dip + 0.45 * carry + th.sin() * 0.05 * walk * p.sprint_t),
                ),
            ),
        );
        let gun = mul(&gun, &translate(hold[0], hold[1], hold[2]));
        // a cylinder turns on to the next chamber as the shot settles; six-fold, so a full one looks like an empty one
        let fired = (w.mag - p.ammo[p.weapon]) as f32;
        let settle = (((1.0 - recoil) / 0.4).clamp(0.0, 1.0)).powi(2);
        let turn = if recoil > 0.0 { fired - 1.0 + settle } else { fired };
        let spent = fired as usize;
        self.draw_gun(p.weapon, &gun, recoil, reload, mag_out, cycle, turn, spent, true);
        let (weapon, before) = self.last_reload.replace((p.weapon, reload));
        if let Some(r) = &spec.rounds {
            if weapon == p.weapon && before < r.eject && reload >= r.eject {
                self.eject(g, &mul(&gun, &action_pose(&spec, recoil, reload, cycle, turn)), r, spent, view);
            }
        }

        if recoil > 0.55 && w.delay < 0.5 || recoil > 0.9 {
            // muzzle flash, spun a different amount each shot
            let spin = around([0.0, 0.012, flash_z], &rot_z(p.ammo[p.weapon] as f32 * 1.7));
            let big = if w.pellets > 1 { 1.5 } else { 1.0 };
            let flash = [
                part([-0.02 * big, -0.01, flash_z - 0.06], [0.02 * big, 0.034, flash_z + 0.01], [1.0, 0.95, 0.6], 0.0),
                part([-0.07 * big, 0.008, flash_z - 0.04], [0.07 * big, 0.016, flash_z], [1.0, 0.7, 0.2], 0.0),
                part([-0.004, -0.04 * big, flash_z - 0.04], [0.004, 0.064 * big, flash_z], [1.0, 0.7, 0.2], 0.0),
            ];
            for f in flash {
                self.draw_part(&gun, &f.rot(spin), 1.0);
            }
        }
    }
}
