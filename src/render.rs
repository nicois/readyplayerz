//! WebGL2 renderer: flat-shaded boxes and lines, one shader.

use crate::sim::{Bot, Game, V3};
use js_sys::Float32Array;
use web_sys::{
    WebGl2RenderingContext as GL, WebGlProgram, WebGlShader, WebGlUniformLocation, WebGlVertexArrayObject,
};

type M4 = [f32; 16];

const VERT: &str = r#"#version 300 es
layout(location=0) in vec3 a_pos;
layout(location=1) in vec3 a_nrm;
uniform mat4 u_vp;
uniform mat4 u_model;
out vec3 v_nrm;
out vec3 v_pos;
void main() {
    vec4 w = u_model * vec4(a_pos, 1.0);
    v_pos = w.xyz;
    v_nrm = mat3(u_model) * a_nrm;
    gl_Position = u_vp * w;
}"#;

const FRAG: &str = r#"#version 300 es
precision highp float;
in vec3 v_nrm;
in vec3 v_pos;
uniform vec3 u_color;
uniform float u_unlit;
uniform float u_shiny;
uniform vec3 u_eye;
out vec4 o;
void main() {
    vec3 n = normalize(v_nrm);
    vec3 l = normalize(vec3(0.45, 1.0, 0.3));
    float d = max(dot(n, l), 0.0);
    float spec = pow(max(dot(n, normalize(l + normalize(u_eye - v_pos))), 0.0), 36.0) * u_shiny;
    vec3 c = mix(u_color * (0.38 + 0.62 * d) + spec, u_color, u_unlit);
    float f = clamp((length(v_pos - u_eye) - 25.0) / 55.0, 0.0, 1.0);
    o = vec4(mix(c, vec3(0.07, 0.09, 0.16), f), 1.0);
}"#;

const WALL_COLORS: [[f32; 3]; 4] = [[0.36, 0.40, 0.62], [0.30, 0.55, 0.60], [0.55, 0.38, 0.58], [0.62, 0.50, 0.34]];

pub struct Renderer {
    gl: GL,
    prog: WebGlProgram,
    cube_vao: WebGlVertexArrayObject,
    line_vao: WebGlVertexArrayObject,
    line_buf: web_sys::WebGlBuffer,
    u_vp: Option<WebGlUniformLocation>,
    u_model: Option<WebGlUniformLocation>,
    u_color: Option<WebGlUniformLocation>,
    u_unlit: Option<WebGlUniformLocation>,
    u_shiny: Option<WebGlUniformLocation>,
    u_eye: Option<WebGlUniformLocation>,
    pub width: u32,
    pub height: u32,
}

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
    let f = norm(fwd);
    let s = norm(cross(f, [0.0, 1.0, 0.0]));
    let u = cross(s, f);
    [
        s[0], u[0], -f[0], 0.0, s[1], u[1], -f[1], 0.0, s[2], u[2], -f[2], 0.0, -dot(s, eye), -dot(u, eye), dot(f, eye),
        1.0,
    ]
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

const GUNMETAL: [f32; 3] = [0.30, 0.32, 0.36];
const DARK: [f32; 3] = [0.12, 0.12, 0.14];
const POLYMER: [f32; 3] = [0.20, 0.21, 0.23];
const GLOVE: [f32; 3] = [0.14, 0.14, 0.15];
const SLEEVE: [f32; 3] = [0.26, 0.32, 0.22];
const WOOD: [f32; 3] = [0.30, 0.20, 0.12];

impl Renderer {
    pub fn new(gl: GL, width: u32, height: u32) -> Result<Renderer, String> {
        let vs = compile(&gl, GL::VERTEX_SHADER, VERT)?;
        let fs = compile(&gl, GL::FRAGMENT_SHADER, FRAG)?;
        let prog = gl.create_program().ok_or("create_program failed")?;
        gl.attach_shader(&prog, &vs);
        gl.attach_shader(&prog, &fs);
        gl.link_program(&prog);
        if !gl.get_program_parameter(&prog, GL::LINK_STATUS).as_bool().unwrap_or(false) {
            return Err(gl.get_program_info_log(&prog).unwrap_or_default());
        }

        let cube_vao = gl.create_vertex_array().ok_or("vao")?;
        gl.bind_vertex_array(Some(&cube_vao));
        let cube_buf = gl.create_buffer().ok_or("buffer")?;
        gl.bind_buffer(GL::ARRAY_BUFFER, Some(&cube_buf));
        let verts = cube_verts();
        unsafe {
            gl.buffer_data_with_array_buffer_view(GL::ARRAY_BUFFER, &Float32Array::view(&verts), GL::STATIC_DRAW);
        }
        Self::layout(&gl);

        let line_vao = gl.create_vertex_array().ok_or("vao")?;
        gl.bind_vertex_array(Some(&line_vao));
        let line_buf = gl.create_buffer().ok_or("buffer")?;
        gl.bind_buffer(GL::ARRAY_BUFFER, Some(&line_buf));
        Self::layout(&gl);

        gl.enable(GL::DEPTH_TEST);
        let u = |n: &str| gl.get_uniform_location(&prog, n);
        Ok(Renderer {
            u_vp: u("u_vp"),
            u_model: u("u_model"),
            u_color: u("u_color"),
            u_unlit: u("u_unlit"),
            u_shiny: u("u_shiny"),
            u_eye: u("u_eye"),
            gl,
            prog,
            cube_vao,
            line_vao,
            line_buf,
            width,
            height,
        })
    }

    fn layout(gl: &GL) {
        gl.enable_vertex_attrib_array(0);
        gl.vertex_attrib_pointer_with_i32(0, 3, GL::FLOAT, false, 24, 0);
        gl.enable_vertex_attrib_array(1);
        gl.vertex_attrib_pointer_with_i32(1, 3, GL::FLOAT, false, 24, 12);
    }

    fn draw_model(&self, model: &M4, color: [f32; 3], unlit: f32, shiny: f32) {
        let gl = &self.gl;
        gl.bind_vertex_array(Some(&self.cube_vao));
        gl.uniform_matrix4fv_with_f32_array(self.u_model.as_ref(), false, model);
        gl.uniform3f(self.u_color.as_ref(), color[0], color[1], color[2]);
        gl.uniform1f(self.u_unlit.as_ref(), unlit);
        gl.uniform1f(self.u_shiny.as_ref(), shiny);
        gl.draw_arrays(GL::TRIANGLES, 0, 36);
    }

    fn draw_box(&self, min: V3, max: V3, color: [f32; 3], unlit: f32) {
        self.draw_model(&box_model(min, max), color, unlit, 0.0);
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

    pub fn render(&self, g: &Game) {
        let gl = &self.gl;
        gl.viewport(0, 0, self.width as i32, self.height as i32);
        gl.clear_color(0.07, 0.09, 0.16, 1.0);
        gl.clear(GL::COLOR_BUFFER_BIT | GL::DEPTH_BUFFER_BIT);
        gl.use_program(Some(&self.prog));

        let eye = g.player.eye();
        let aspect = self.width as f32 / self.height.max(1) as f32;
        let p = &g.player;
        let fov = 1.2 * (1.0 + (p.gun().zoom - 1.0) * p.ads);
        let vp = mul(&perspective(fov, aspect, 0.05, 200.0), &look_dir(eye, g.player.forward()));
        gl.uniform_matrix4fv_with_f32_array(self.u_vp.as_ref(), false, &vp);
        gl.uniform3f(self.u_eye.as_ref(), eye[0], eye[1], eye[2]);

        self.draw_box([-20.0, -0.3, -20.0], [20.0, 0.0, 20.0], [0.11, 0.13, 0.20], 0.0);
        let mut grid = Vec::new();
        for i in -10..=10 {
            let c = i as f32 * 2.0;
            grid.extend_from_slice(&[c, 0.01, -20.0, c, 0.01, 20.0, -20.0, 0.01, c, 20.0, 0.01, c]);
        }
        self.draw_lines(&grid, [0.20, 0.24, 0.36], 1.0);

        for (i, w) in g.walls.iter().enumerate() {
            self.draw_box(w.min, w.max, WALL_COLORS[i % WALL_COLORS.len()], 0.0);
        }
        for (i, b) in g.bots.iter().enumerate().filter(|(_, b)| b.alive) {
            self.draw_soldier(g, i, b);
        }
        for d in &g.debris {
            // pieces shrink away at the end of their life
            let k = (d.life / if d.glow { d.max_life } else { 0.6 }).min(1.0);
            let h = [d.half[0] * k, d.half[1] * k, d.half[2] * k];
            let spin = mul(&rot_y(d.rot[1]), &mul(&rot_x(d.rot[0]), &rot_z(d.rot[2])));
            let model = mul(&translate(d.pos[0], d.pos[1], d.pos[2]), &mul(&spin, &box_model([-h[0], -h[1], -h[2]], h)));
            self.draw_model(&model, d.color, if d.glow { 1.0 } else { 0.0 }, 0.1);
        }
        for t in &g.tracers {
            let c = if t.enemy { [1.0, 0.35, 0.3] } else { [1.0, 0.95, 0.6] };
            self.draw_lines(&[t.a[0], t.a[1], t.a[2], t.b[0], t.b[1], t.b[2]], c, 1.0);
        }

        // viewmodel: drawn in view space on a cleared depth buffer so it never clips into walls
        gl.clear(GL::DEPTH_BUFFER_BIT);
        // the gun keeps its own field of view so zooming the world doesn't magnify it
        gl.uniform_matrix4fv_with_f32_array(self.u_vp.as_ref(), false, &perspective(1.2, aspect, 0.05, 200.0));
        gl.uniform3f(self.u_eye.as_ref(), 0.0, 0.0, 0.0);
        if !(p.gun().scope && p.ads > 0.6) {
            self.draw_viewmodel(g);
        }
    }

    /// An enemy built from the same cube: legs that swing with speed, vest, helmet and a rifle held across the chest.
    fn draw_soldier(&self, g: &Game, i: usize, b: &Bot) {
        let p = b.body.pos;
        let yaw = (g.player.body.pos[0] - p[0]).atan2(-(g.player.body.pos[2] - p[2]));
        let base = mul(&translate(p[0], p[1], p[2]), &rot_y(-yaw));
        let speed = (b.body.vel[0].powi(2) + b.body.vel[2].powi(2)).sqrt();
        let amp = if b.body.on_ground { (speed / 3.0).min(1.0) } else { 0.0 };
        let ph = g.time * 9.0 + b.phase;
        let swing = if b.body.on_ground { ph.sin() * 0.7 * amp } else { 0.5 };
        let upper = mul(&base, &translate(0.0, (ph * 2.0).cos().abs() * 0.03 * amp, 0.0));

        let t = (i % 3) as f32 * 0.035;
        let shirt = [0.22 + t, 0.24, 0.27 - t];
        let pants = [0.15, 0.16 + t, 0.18];
        let vest = [0.13, 0.14, 0.17];
        let red = [0.78, 0.14, 0.17];
        let skin = [0.80, 0.60, 0.46];
        let helmet = [0.11, 0.12, 0.14];
        let glove = [0.08, 0.08, 0.09];
        let steel = [0.10, 0.10, 0.11];

        // ground shadow
        self.draw_box([p[0] - 0.4, p[1] + 0.01, p[2] - 0.4], [p[0] + 0.4, p[1] + 0.02, p[2] + 0.4], [0.03, 0.04, 0.08], 1.0);

        for (side, angle) in [(-1.0f32, swing), (1.0, -swing)] {
            let hip = around([side * 0.115, 0.88, 0.0], &rot_x(angle));
            let (a, c) = (side * 0.03, side * 0.20);
            let (x0, x1) = (a.min(c), a.max(c));
            self.draw_part(&base, &part([x0, 0.0, -0.09], [x1, 0.88, 0.09], pants, 0.05).rot(hip), 0.0);
            self.draw_part(&base, &part([x0 - 0.01, 0.0, -0.17], [x1 + 0.01, 0.10, 0.10], steel, 0.2).rot(hip), 0.0);
            self.draw_part(&base, &part([x0 - 0.005, 0.40, -0.10], [x1 + 0.005, 0.52, -0.085], vest, 0.1).rot(hip), 0.0); // knee pad
        }

        // arms reach forward to the rifle: right hand on the grip, left on the handguard
        let arm = |side: f32, up: f32, turn: f32| {
            let pivot = [side * 0.31, 1.31, 0.0];
            let r = around(pivot, &mul(&rot_y(turn), &rot_x(up)));
            [
                part([side * 0.31 - 0.055, 0.84, -0.055], [side * 0.31 + 0.055, 1.31, 0.055], shirt, 0.05).rot(r),
                part([side * 0.31 - 0.05, 0.75, -0.05], [side * 0.31 + 0.05, 0.85, 0.05], glove, 0.05).rot(r),
            ]
        };
        let parts = [
            // torso, vest with chest plate and belt
            part([-0.24, 0.85, -0.13], [0.24, 1.38, 0.13], shirt, 0.05),
            part([-0.27, 0.95, -0.155], [0.27, 1.36, 0.155], vest, 0.15),
            part([-0.15, 1.05, -0.175], [0.15, 1.29, -0.155], red, 0.3),
            part([-0.26, 0.82, -0.145], [0.26, 0.90, 0.145], steel, 0.2),
            part([-0.27, 1.34, -0.07], [-0.20, 1.40, 0.07], vest, 0.1), // shoulder pads
            part([0.20, 1.34, -0.07], [0.27, 1.40, 0.07], vest, 0.1),
            // neck, head, helmet, visor
            part([-0.05, 1.38, -0.05], [0.05, 1.47, 0.05], skin, 0.05),
            part([-0.11, 1.45, -0.115], [0.11, 1.66, 0.115], skin, 0.1),
            part([-0.135, 1.60, -0.15], [0.135, 1.78, 0.14], helmet, 0.45),
            part([-0.10, 1.51, -0.13], [0.10, 1.60, -0.115], [0.02, 0.03, 0.05], 0.9),
            // rifle
            part([0.055, 1.10, -0.18], [0.105, 1.19, 0.12], steel, 0.4),
            part([0.072, 1.14, -0.60], [0.088, 1.17, -0.18], [0.14, 0.14, 0.16], 0.6),
            part([0.065, 1.00, -0.12], [0.095, 1.10, -0.06], steel, 0.3),
            part([0.06, 1.08, 0.12], [0.10, 1.17, 0.26], [0.09, 0.09, 0.10], 0.1),
        ];
        for p in &parts {
            self.draw_part(&upper, p, 0.0);
        }
        for p in arm(1.0, 1.25, 0.40).iter().chain(arm(-1.0, 1.35, -0.62).iter()) {
            self.draw_part(&upper, p, 0.0);
        }
    }

    fn draw_viewmodel(&self, g: &Game) {
        let p = &g.player;
        let w = p.gun();
        let t = g.time;
        let speed = (p.body.vel[0].powi(2) + p.body.vel[2].powi(2)).sqrt();
        let walk = if p.body.on_ground { (speed / 6.0).min(1.5) } else { 0.0 } * (1.0 - 0.8 * p.ads);
        let recoil = p.cooldown / w.delay; // 1 right after a shot, fading to 0
        let reload = if p.reload_t > 0.0 { 1.0 - p.reload_t / w.reload } else { 0.0 };
        let dip = (reload * std::f32::consts::PI).sin(); // gun lowers and tilts while reloading
        let mag_out = if (0.3..0.7).contains(&reload) { ((reload - 0.3) / 0.4 * std::f32::consts::PI).sin() } else { 0.0 };
        // pump / bolt cycles back and forth between shots
        let cycle = if w.delay > 0.5 && (0.2..0.75).contains(&recoil) {
            ((0.75 - recoil) / 0.55 * std::f32::consts::PI).sin()
        } else {
            0.0
        };
        // swinging up from below when the gun was just selected
        let raise = (p.equip_t / crate::sim::EQUIP_TIME).powi(2);
        // a long gun needs more kick to read as one
        let kick = if w.pellets > 1 || w.scope { 0.09 } else { 0.05 };

        let (parts, flash_z, sight_y) = weapon_parts(p.weapon, mag_out, cycle);

        let ads = p.ads;
        let lerp = |a: f32, b: f32| a + (b - a) * ads;
        let bob_x = (t * 6.0).cos() * 0.010 * walk;
        let bob_y = (t * 12.0).sin() * 0.006 * walk + (t * 1.5).sin() * 0.002 * (1.0 - ads);
        let gun = mul(
            &translate(
                lerp(0.15 + bob_x, 0.0),
                lerp(-0.17 + bob_y - 0.10 * dip - 0.35 * raise, -sight_y - 0.004),
                lerp(-0.38, -0.30) + kick * recoil,
            ),
            &mul(
                &rot_y(0.05 * (1.0 - ads)),
                &mul(&rot_x((0.07 * recoil - 0.25 * dip) * (1.0 - 0.5 * ads) + 0.8 * raise), &rot_z(0.5 * dip)),
            ),
        );
        for part in &parts {
            self.draw_part(&gun, part, 0.0);
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

/// Trigger hand, pistol grip and guard, shared by every gun.
fn grip_parts(v: &mut Vec<Part>) {
    let grip_rot = around([0.0, -0.04, 0.075], &rot_x(-0.3));
    v.extend([
        part([-0.017, -0.15, 0.055], [0.017, -0.04, 0.095], POLYMER, 0.15).rot(grip_rot),
        part([-0.005, -0.066, -0.04], [0.005, -0.058, 0.055], GUNMETAL, 0.4),
        part([-0.005, -0.066, -0.045], [0.005, -0.04, -0.037], GUNMETAL, 0.4),
        part([-0.003, -0.058, -0.01], [0.003, -0.04, -0.004], DARK, 0.5),
        part([-0.022, -0.14, 0.045], [0.036, -0.05, 0.10], GLOVE, 0.05).rot(grip_rot),
        part([-0.04, -0.14, 0.07], [0.04, -0.06, 0.5], SLEEVE, 0.0)
            .rot(around([0.0, -0.1, 0.07], &mul(&rot_y(0.15), &rot_x(0.55)))),
    ]);
}

/// Support hand under the front of the gun, from `z0` to `z1`, plus the arm running back from it.
/// `slide` moves it along the gun (a pumping forend).
fn support_hand(v: &mut Vec<Part>, z0: f32, z1: f32, slide: f32) {
    let shift = translate(0.0, 0.0, slide);
    let pivot = z0 + 0.08;
    v.push(part([-0.036, -0.078, z0], [0.036, -0.024, z1], GLOVE, 0.05).rot(shift));
    v.push(
        part([-0.04, -0.085, pivot], [0.04, -0.02, 0.30], SLEEVE, 0.0)
            .rot(mul(&shift, &around([0.0, -0.05, pivot], &mul(&rot_y(-0.45), &rot_x(0.35))))),
    );
}

/// Parts for one gun in gun space (-z muzzle, +z stock, +y up), the z of its muzzle, and the height of
/// its sights above the barrel axis, which is where the eye sits when aiming.
fn weapon_parts(idx: usize, mag_out: f32, cycle: f32) -> (Vec<Part>, f32, f32) {
    let mut v: Vec<Part> = Vec::new();
    let mag_drop = translate(0.0, -0.2 * mag_out, 0.0);
    let (flash_z, sight_y) = match idx {
        // carbine
        0 => {
            let mag_rot = mul(&mag_drop, &around([0.0, -0.04, -0.035], &rot_x(0.18)));
            v.extend([
                // receiver and top rail
                part([-0.03, -0.04, -0.14], [0.03, 0.03, 0.12], GUNMETAL, 0.6),
                part([-0.026, 0.03, -0.16], [0.026, 0.052, 0.10], [0.36, 0.38, 0.42], 0.6),
                part([-0.012, 0.052, -0.14], [0.012, 0.058, 0.08], DARK, 0.2),
                part([0.03, 0.0, -0.03], [0.04, 0.02, 0.03], DARK, 0.3), // charging handle
                // handguard with top rail, barrel and muzzle brake
                part([-0.032, -0.032, -0.42], [0.032, 0.034, -0.14], POLYMER, 0.25),
                part([-0.013, 0.034, -0.42], [0.013, 0.040, -0.14], DARK, 0.2),
                part([-0.008, 0.002, -0.60], [0.008, 0.022, -0.42], [0.14, 0.14, 0.16], 0.8),
                part([-0.013, -0.003, -0.65], [0.013, 0.027, -0.60], [0.12, 0.12, 0.13], 0.8),
                // front post with ears, rear aperture
                part([-0.003, 0.040, -0.435], [0.003, 0.085, -0.425], DARK, 0.3),
                part([-0.015, 0.040, -0.438], [-0.009, 0.078, -0.422], DARK, 0.3),
                part([0.009, 0.040, -0.438], [0.015, 0.078, -0.422], DARK, 0.3),
                part([-0.02, 0.052, 0.05], [0.02, 0.058, 0.065], DARK, 0.3),
                part([-0.02, 0.052, 0.05], [-0.013, 0.078, 0.065], DARK, 0.3),
                part([0.013, 0.052, 0.05], [0.02, 0.078, 0.065], DARK, 0.3),
                // stock
                part([-0.022, -0.055, 0.12], [0.022, 0.02, 0.34], [0.26, 0.22, 0.17], 0.1),
                part([-0.024, -0.062, 0.34], [0.024, 0.028, 0.355], [0.05, 0.05, 0.05], 0.05),
                // curved magazine, drops out while reloading
                part([-0.016, -0.17, -0.06], [0.016, -0.04, -0.01], [0.20, 0.21, 0.23], 0.3).rot(mag_rot),
                part([-0.018, -0.18, -0.062], [0.018, -0.17, -0.008], DARK, 0.3).rot(mag_rot),
            ]);
            support_hand(&mut v, -0.34, -0.22, 0.0);
            (-0.66, 0.070)
        }
        // SMG: short and boxy, suppressed, red dot, wire stock, long straight magazine
        1 => {
            v.extend([
                part([-0.03, -0.04, -0.12], [0.03, 0.035, 0.10], GUNMETAL, 0.6),
                part([-0.022, 0.035, -0.12], [0.022, 0.045, 0.08], DARK, 0.2),
                part([-0.032, -0.032, -0.30], [0.032, 0.034, -0.12], POLYMER, 0.25),
                part([-0.008, 0.002, -0.40], [0.008, 0.022, -0.30], [0.14, 0.14, 0.16], 0.8),
                part([-0.018, -0.004, -0.52], [0.018, 0.03, -0.40], DARK, 0.5), // suppressor
                // red dot: base, housing and lens
                part([-0.015, 0.045, -0.02], [0.015, 0.055, 0.05], DARK, 0.3),
                part([-0.02, 0.055, -0.02], [0.02, 0.085, 0.05], DARK, 0.4),
                part([-0.016, 0.058, -0.024], [0.016, 0.082, -0.02], [0.75, 0.10, 0.10], 0.9),
                // wire stock
                part([-0.012, -0.03, 0.10], [-0.006, 0.01, 0.30], DARK, 0.3),
                part([0.006, -0.03, 0.10], [0.012, 0.01, 0.30], DARK, 0.3),
                part([-0.02, -0.05, 0.30], [0.02, 0.02, 0.315], DARK, 0.1),
                // straight stick magazine
                part([-0.015, -0.22, -0.06], [0.015, -0.04, -0.01], [0.20, 0.21, 0.23], 0.3).rot(mag_drop),
                part([-0.017, -0.23, -0.062], [0.017, -0.22, -0.008], DARK, 0.3).rot(mag_drop),
            ]);
            support_hand(&mut v, -0.28, -0.16, 0.0);
            (-0.54, 0.075)
        }
        // pump shotgun: long barrel over a tube magazine, wooden forend that racks back after each shot
        2 => {
            let pump = translate(0.0, 0.0, 0.09 * cycle);
            v.extend([
                part([-0.03, -0.045, -0.10], [0.03, 0.035, 0.12], GUNMETAL, 0.6),
                part([-0.014, 0.0, -0.62], [0.014, 0.03, -0.10], GUNMETAL, 0.7),
                part([-0.006, 0.03, -0.60], [0.006, 0.034, -0.10], DARK, 0.2), // rib
                part([-0.003, 0.034, -0.62], [0.003, 0.048, -0.61], [0.9, 0.8, 0.4], 0.3), // bead
                part([-0.012, -0.036, -0.55], [0.012, -0.012, -0.10], DARK, 0.6),
                part([-0.015, -0.04, -0.57], [0.015, -0.008, -0.55], DARK, 0.5), // tube cap
                part([-0.032, -0.06, -0.40], [0.032, -0.008, -0.22], WOOD, 0.25).rot(pump),
                part([-0.024, -0.07, 0.12], [0.024, 0.02, 0.36], WOOD, 0.2),
                part([-0.026, -0.075, 0.36], [0.026, 0.026, 0.375], [0.05, 0.05, 0.05], 0.05),
            ]);
            support_hand(&mut v, -0.38, -0.25, 0.09 * cycle);
            (-0.64, 0.050)
        }
        // bolt-action sniper: heavy barrel, brake, big scope, bolt handle that cycles after a shot
        _ => {
            let bolt = translate(0.0, 0.0, 0.05 * cycle);
            v.extend([
                part([-0.03, -0.04, -0.10], [0.03, 0.03, 0.14], GUNMETAL, 0.6),
                part([-0.028, -0.03, -0.45], [0.028, 0.03, -0.10], POLYMER, 0.25),
                part([-0.011, 0.0, -0.78], [0.011, 0.026, -0.45], [0.16, 0.17, 0.19], 0.8),
                part([-0.016, -0.004, -0.84], [0.016, 0.03, -0.78], DARK, 0.7), // muzzle brake
                // scope with mounts and turret
                part([-0.02, 0.05, -0.20], [0.02, 0.09, 0.08], DARK, 0.6),
                part([-0.028, 0.045, -0.26], [0.028, 0.095, -0.20], DARK, 0.6),
                part([-0.024, 0.047, 0.08], [0.024, 0.093, 0.14], DARK, 0.6),
                part([-0.01, 0.03, -0.12], [0.01, 0.05, -0.08], GUNMETAL, 0.5),
                part([-0.01, 0.03, 0.02], [0.01, 0.05, 0.06], GUNMETAL, 0.5),
                part([-0.006, 0.09, -0.06], [0.006, 0.104, -0.04], GUNMETAL, 0.5),
                // bolt handle
                part([0.03, 0.004, 0.03], [0.075, 0.016, 0.045], GUNMETAL, 0.6).rot(bolt),
                part([0.07, -0.012, 0.025], [0.087, 0.026, 0.05], DARK, 0.4).rot(bolt),
                // stock with cheek riser and pad
                part([-0.024, -0.075, 0.14], [0.024, 0.03, 0.36], [0.17, 0.19, 0.17], 0.1),
                part([-0.02, 0.03, 0.20], [0.02, 0.05, 0.33], [0.17, 0.19, 0.17], 0.1),
                part([-0.026, -0.08, 0.36], [0.026, 0.034, 0.375], [0.05, 0.05, 0.05], 0.05),
                // short box magazine
                part([-0.016, -0.09, -0.05], [0.016, -0.04, 0.03], DARK, 0.3).rot(mag_drop),
            ]);
            support_hand(&mut v, -0.36, -0.24, 0.0);
            (-0.86, 0.070)
        }
    };
    grip_parts(&mut v);
    (v, flash_z, sight_y)
}
