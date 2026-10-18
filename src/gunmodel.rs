//! The guns the player holds: hard-surface parts built from bevelled boxes and cylinders, and gloved hands and
//! sleeves shaped as smooth surfaces. Everything is in gun space: -z is the muzzle, +z the stock, +y up, x to the
//! right, with the bore along the z axis.
//!
//! The long guns all share the carbine's pistol grip and the hands are built once, so one right hand fits every
//! grip and the left hand is moved to each gun's forend. The pistol has a two-handed hold of its own.

use crate::body::{cone, ell, len, smin, surface_nets};
use crate::sim::V3;
use std::f32::consts::TAU;

type V = V3;

/// Materials the shader knows for hard parts (see the fragment shader).
pub const ANODISED: f32 = 20.0;
pub const POLYMER: f32 = 21.0;
pub const WOOD: f32 = 22.0;
pub const STEEL: f32 = 23.0;
pub const RUBBER: f32 = 24.0;
pub const CLOTH: f32 = 10.0;

fn add(a: V, b: V) -> V {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn sub(a: V, b: V) -> V {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn mulv(a: V, k: f32) -> V {
    [a[0] * k, a[1] * k, a[2] * k]
}
fn dot(a: V, b: V) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V, b: V) -> V {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn norm(a: V) -> V {
    let l = len(a).max(1e-9);
    mulv(a, 1.0 / l)
}
/// Rotates `p` about the x axis through the origin by `a`.
fn xr(p: V, a: f32) -> V {
    let (s, c) = a.sin_cos();
    [p[0], p[1] * c - p[2] * s, p[1] * s + p[2] * c]
}

/// Triangles as position and normal for every vertex.
#[derive(Default, Clone)]
pub struct Mesh {
    pub verts: Vec<f32>,
    pub idx: Vec<u32>,
}

const FACES: [[usize; 4]; 6] = [[0, 2, 6, 4], [1, 5, 7, 3], [0, 4, 5, 1], [2, 3, 7, 6], [0, 1, 3, 2], [4, 6, 7, 5]];

/// The eight corners of an axis-aligned box, numbered x + 2y + 4z.
pub fn corners(min: V, max: V) -> [V; 8] {
    let mut c = [[0.0; 3]; 8];
    for (i, p) in c.iter_mut().enumerate() {
        *p = [if i & 1 == 0 { min[0] } else { max[0] }, if i & 2 == 0 { min[1] } else { max[1] }, if i & 4 == 0 { min[2] } else { max[2] }];
    }
    c
}

impl Mesh {
    fn push(&mut self, p: V, n: V) -> u32 {
        let i = (self.verts.len() / 6) as u32;
        self.verts.extend_from_slice(&[p[0], p[1], p[2], n[0], n[1], n[2]]);
        i
    }

    fn quad(&mut self, p: [V; 4], n: [V; 4]) {
        let i = [self.push(p[0], n[0]), self.push(p[1], n[1]), self.push(p[2], n[2]), self.push(p[3], n[3])];
        self.idx.extend_from_slice(&[i[0], i[1], i[2], i[0], i[2], i[3]]);
    }

    fn tri(&mut self, p: [V; 3], n: V) {
        let i = [self.push(p[0], n), self.push(p[1], n), self.push(p[2], n)];
        self.idx.extend_from_slice(&i);
    }

    /// A six-sided solid with every edge cut off at 45 degrees, `bevel` metres in: flat faces, bevelled rims that
    /// catch the light, and a triangle at each corner.
    pub fn hex(&mut self, c: [V; 8], bevel: f32) {
        let mut cen = [0.0; 3];
        for p in &c {
            cen = add(cen, *p);
        }
        cen = mulv(cen, 0.125);
        let mut fnrm = [[0.0; 3]; 6];
        let mut fcen = [[0.0; 3]; 6];
        for (f, l) in FACES.iter().enumerate() {
            let p = [c[l[0]], c[l[1]], c[l[2]], c[l[3]]];
            fcen[f] = mulv(add(add(p[0], p[1]), add(p[2], p[3])), 0.25);
            let mut n = norm(cross(sub(p[1], p[0]), sub(p[3], p[0])));
            if dot(n, sub(fcen[f], cen)) < 0.0 {
                n = mulv(n, -1.0);
            }
            fnrm[f] = n;
        }
        // each face pulled in from its edges by the bevel
        let mut iv = [[[0.0f32; 3]; 4]; 6];
        for f in 0..6 {
            let p = [c[FACES[f][0]], c[FACES[f][1]], c[FACES[f][2]], c[FACES[f][3]]];
            let mut m = [[0.0f32; 3]; 4];
            for i in 0..4 {
                let e = norm(sub(p[(i + 1) % 4], p[i]));
                let mut d = cross(fnrm[f], e);
                if dot(d, sub(fcen[f], p[i])) < 0.0 {
                    d = mulv(d, -1.0);
                }
                m[i] = norm(d);
            }
            for i in 0..4 {
                let (m1, m2) = (m[(i + 3) % 4], m[i]);
                iv[f][i] = add(p[i], mulv(add(m1, m2), bevel / (1.0 + dot(m1, m2))));
            }
        }
        for f in 0..6 {
            self.quad(iv[f], [fnrm[f]; 4]);
        }
        for f in 0..6 {
            for s in 0..4 {
                let (a, b) = (FACES[f][s], FACES[f][(s + 1) % 4]);
                for g in (f + 1)..6 {
                    if let (Some(sa), Some(sb)) = (FACES[g].iter().position(|x| *x == a), FACES[g].iter().position(|x| *x == b)) {
                        let n = norm(add(fnrm[f], fnrm[g]));
                        self.quad([iv[f][s], iv[f][(s + 1) % 4], iv[g][sb], iv[g][sa]], [n; 4]);
                    }
                }
            }
        }
        for corner in 0..8 {
            let (mut pts, mut n) = (Vec::new(), [0.0; 3]);
            for f in 0..6 {
                if let Some(s) = FACES[f].iter().position(|x| *x == corner) {
                    pts.push(iv[f][s]);
                    n = add(n, fnrm[f]);
                }
            }
            self.tri([pts[0], pts[1], pts[2]], norm(n));
        }
    }

    pub fn bx(&mut self, min: V, max: V, bevel: f32) {
        self.hex(corners(min, max), bevel);
    }

    /// A box turned about the x axis through `pivot`.
    pub fn bx_rot(&mut self, min: V, max: V, bevel: f32, pivot: V, a: f32) {
        let mut c = corners(min, max);
        for p in &mut c {
            *p = add(xr(sub(*p, pivot), a), pivot);
        }
        self.hex(c, bevel);
    }

    /// A cone or cylinder from `a` to `b`, radius `r0` at `a` and `r1` at `b`, with flat ends.
    pub fn cyl(&mut self, a: V, b: V, r0: f32, r1: f32, seg: usize) {
        let (y, l) = (norm(sub(b, a)), len(sub(b, a)));
        let (u, v) = basis(y);
        let slope = (r0 - r1) / l.max(1e-6);
        for i in 0..seg {
            let (t0, t1) = (i as f32 / seg as f32 * TAU, (i + 1) as f32 / seg as f32 * TAU);
            let d0 = add(mulv(u, t0.cos()), mulv(v, t0.sin()));
            let d1 = add(mulv(u, t1.cos()), mulv(v, t1.sin()));
            let p = [add(a, mulv(d0, r0)), add(a, mulv(d1, r0)), add(b, mulv(d1, r1)), add(b, mulv(d0, r1))];
            let (n0, n1) = (norm(add(d0, mulv(y, slope))), norm(add(d1, mulv(y, slope))));
            self.quad(p, [n0, n1, n1, n0]);
            if r0 > 0.0 {
                self.tri([a, add(a, mulv(d1, r0)), add(a, mulv(d0, r0))], mulv(y, -1.0));
            }
            if r1 > 0.0 {
                self.tri([b, add(b, mulv(d0, r1)), add(b, mulv(d1, r1))], y);
            }
        }
    }

    /// A pipe: outer wall, inner wall and the rings that close its ends.
    pub fn tube(&mut self, a: V, b: V, r_out: f32, r_in: f32, seg: usize) {
        let y = norm(sub(b, a));
        let (u, v) = basis(y);
        for i in 0..seg {
            let (t0, t1) = (i as f32 / seg as f32 * TAU, (i + 1) as f32 / seg as f32 * TAU);
            let d0 = add(mulv(u, t0.cos()), mulv(v, t0.sin()));
            let d1 = add(mulv(u, t1.cos()), mulv(v, t1.sin()));
            self.quad([add(a, mulv(d0, r_out)), add(a, mulv(d1, r_out)), add(b, mulv(d1, r_out)), add(b, mulv(d0, r_out))], [d0, d1, d1, d0]);
            self.quad([add(a, mulv(d0, r_in)), add(a, mulv(d1, r_in)), add(b, mulv(d1, r_in)), add(b, mulv(d0, r_in))], [mulv(d0, -1.0), mulv(d1, -1.0), mulv(d1, -1.0), mulv(d0, -1.0)]);
            self.quad([add(a, mulv(d0, r_in)), add(a, mulv(d1, r_in)), add(a, mulv(d1, r_out)), add(a, mulv(d0, r_out))], [mulv(y, -1.0); 4]);
            self.quad([add(b, mulv(d0, r_in)), add(b, mulv(d1, r_in)), add(b, mulv(d1, r_out)), add(b, mulv(d0, r_out))], [y; 4]);
        }
    }

    /// The top rail every gun mounts sights on: a base and its cross slots, from `z0` to `z1`, standing on `y`.
    fn rail(&mut self, y: f32, z0: f32, z1: f32) {
        self.bx([-0.0105, y, z0], [0.0105, y + 0.0075, z1], 0.0008);
        let n = ((z1 - z0 - 0.003) / 0.0108) as usize;
        for k in 0..n {
            let z = z0 + 0.003 + k as f32 * 0.0108;
            self.bx([-0.0105, y + 0.0075, z], [0.0105, y + 0.0105, z + 0.0056], 0.0006);
        }
    }
}

fn basis(y: V) -> (V, V) {
    let h = if y[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
    let u = norm(cross(y, h));
    (u, cross(y, u))
}

/// One draw: a mesh with the look it is drawn in. `glow` 1 draws it unlit, for sight dots.
pub struct Piece {
    pub mesh: Mesh,
    pub mat: f32,
    pub color: [f32; 3],
    pub shiny: f32,
    pub glow: f32,
}

fn piece(mesh: Mesh, mat: f32, color: [f32; 3], shiny: f32) -> Piece {
    Piece { mesh, mat, color, shiny, glow: 0.0 }
}

fn glow(mesh: Mesh, color: [f32; 3]) -> Piece {
    Piece { mesh, mat: 0.0, color, shiny: 0.0, glow: 1.0 }
}

/// What cycles after a shot.
#[derive(Clone, Copy, PartialEq)]
pub enum Action {
    Fixed,
    /// the forend racks back and forward, taking the left hand with it
    Pump,
    /// the bolt handle lifts and pulls back
    Bolt,
    /// the slide kicks back with every shot
    Slide,
}

/// How a gun sits in the hands and moves.
#[derive(Clone, Copy)]
pub struct Spec {
    pub muzzle_z: f32,
    /// how high above the bore the sights' line of sight is
    pub sight_y: f32,
    pub action: Action,
    /// the trigger swings back about this point
    pub trigger_pivot: V,
    /// the magazine during a reload: where it is when fully out, and how far it tips about `mag_pivot`
    pub mag_out: V,
    pub mag_pivot: V,
    pub mag_tilt: f32,
    /// moves the shared hands onto this gun: the right from the carbine's grip, the left from its handguard
    pub right_at: V,
    pub left_at: V,
    /// held in both hands round the grip instead of by a forend
    pub pistol_hold: bool,
    /// moves the whole gun in the view, e.g. a pistol is held out at arm's length
    pub hold: V,
}

/// All the parts of one gun, grouped by what moves together.
pub struct Gun {
    pub body: Vec<Piece>,
    pub mag: Vec<Piece>,
    pub trigger: Vec<Piece>,
    pub action: Vec<Piece>,
    pub spec: Spec,
}

/// The hands, built once and moved onto each gun.
pub struct Hands {
    pub right: Vec<Piece>,
    pub left: Vec<Piece>,
    pub pistol_left: Vec<Piece>,
}

impl Hands {
    pub fn new() -> Hands {
        Hands { right: right_hand(), left: left_hand(), pistol_left: pistol_left_hand() }
    }
}

/// Every gun, in the order of `sim::WEAPONS`.
pub fn guns() -> Vec<Gun> {
    vec![carbine(), smg(), shotgun(), sniper(), pistol()]
}

const GLOVE: [f32; 3] = [0.06, 0.06, 0.065];
const SLEEVE: [f32; 3] = [0.20, 0.24, 0.17];
const BLACK: [f32; 3] = [0.025, 0.025, 0.03];
const GLASS: [f32; 3] = [0.04, 0.07, 0.10];
const BRASS: [f32; 3] = [0.55, 0.42, 0.18];

/// The carbine's grip and trigger: where the shared right hand closes, so every long gun has them in this place.
const TRIGGER_PIVOT: V = [0.0, -0.062, -0.031];

/// The pistol grip, tilted back, with a thin front strap and a base screw; `dy` raises it.
fn pistol_grip(grip: &mut Mesh, dy: f32) {
    let g = |x: f32, y: f32, z: f32| [x, y + dy, z];
    let (w, yt, yb) = (0.0165, -0.052, -0.158);
    grip.hex(
        [
            g(-w, yb, 0.090), g(w, yb, 0.090), g(-w, yt, 0.058), g(w, yt, 0.058),
            g(-w, yb, 0.130), g(w, yb, 0.130), g(-w, yt, 0.096), g(w, yt, 0.096),
        ],
        0.0035,
    );
    grip.bx([-0.0055, -0.1595 + dy, 0.102], [0.0055, -0.1575 + dy, 0.114], 0.0004);
}

/// A rifle's trigger guard, from just ahead of the trigger back to the grip.
fn trigger_guard(guard: &mut Mesh) {
    guard.bx([-0.0026, -0.0845, -0.071], [0.0026, -0.062, -0.0665], 0.0006);
    guard.bx([-0.0026, -0.0845, -0.071], [0.0026, -0.0805, 0.056], 0.0006);
    guard.bx([-0.0026, -0.0845, 0.052], [0.0026, -0.055, 0.057], 0.0006);
}

fn rifle_trigger() -> Vec<Piece> {
    let mut t = Mesh::default();
    t.bx([-0.0022, -0.064, -0.0345], [0.0022, -0.040, -0.0275], 0.0006);
    vec![piece(t, STEEL, [0.28, 0.28, 0.30], 0.8)]
}

/// A straight box magazine from `top` down to `bottom`, `z0` to `z1` deep, leaning forward by `lean`, with feed
/// lips, ribs down the sides and a base plate.
fn box_mag(hw: f32, top: f32, bottom: f32, z0: f32, z1: f32, lean: f32, color: [f32; 3]) -> Vec<Piece> {
    let (mut m, mut lips, mut plate) = (Mesh::default(), Mesh::default(), Mesh::default());
    let pivot = [0.0, top, (z0 + z1) * 0.5];
    m.bx_rot([-hw, bottom, z0], [hw, top, z1], 0.0012, pivot, lean);
    for side in [-1.0f32, 1.0] {
        m.bx_rot([side * hw - 0.0008, bottom + 0.012, z0 + 0.008], [side * hw + 0.0008, top - 0.02, z0 + 0.014], 0.0005, pivot, lean);
    }
    lips.bx_rot([-hw * 0.8, top, z0 + 0.004], [hw * 0.8, top + 0.007, z1 - 0.006], 0.0006, pivot, lean);
    plate.bx_rot([-hw - 0.0012, bottom - 0.008, z0 - 0.002], [hw + 0.0012, bottom, z1 + 0.002], 0.0012, pivot, lean);
    vec![piece(m, POLYMER, color, 0.45), piece(lips, STEEL, [0.30, 0.30, 0.32], 0.8), piece(plate, ANODISED, [0.07, 0.07, 0.08], 0.6)]
}

// ---- carbine ----------------------------------------------------------------------------------

fn carbine() -> Gun {
    let sight_y = 0.0705;
    let (mut recv, mut rails, mut grip, mut guard, mut barrel, mut dark, mut tube_parts) =
        (Mesh::default(), Mesh::default(), Mesh::default(), Mesh::default(), Mesh::default(), Mesh::default(), Mesh::default());

    // upper receiver with its flat top rail, port cover, forward assist and charging handle
    recv.bx([-0.0175, -0.010, -0.165], [0.0175, 0.040, 0.095], 0.0018);
    rails.bx([-0.0105, 0.040, -0.165], [0.0105, 0.0475, 0.095], 0.0008);
    for k in 0..24 {
        let z = -0.162 + k as f32 * 0.0108;
        rails.bx([-0.0105, 0.0475, z], [0.0105, 0.0505, z + 0.0056], 0.0006);
    }
    recv.bx([0.0175, 0.008, -0.040], [0.0192, 0.031, 0.040], 0.0007);
    recv.cyl([0.0175, 0.018, 0.062], [0.0262, 0.018, 0.062], 0.0052, 0.0048, 14);
    recv.bx([-0.0145, 0.0405, 0.084], [0.0145, 0.0475, 0.118], 0.001);
    recv.bx([0.0105, 0.030, 0.092], [0.0125, 0.0405, 0.116], 0.0008);
    // lower receiver with magwell, trigger guard, selector and release
    recv.bx([-0.0175, -0.062, -0.115], [0.0175, -0.010, 0.062], 0.0018);
    recv.bx([-0.0145, -0.036, 0.062], [0.0145, -0.004, 0.116], 0.0014);
    recv.bx([-0.0158, -0.098, -0.088], [0.0158, -0.060, -0.002], 0.0016);
    recv.cyl([0.0175, -0.044, -0.012], [0.0232, -0.044, -0.012], 0.0046, 0.0046, 12);
    recv.cyl([-0.0175, -0.030, 0.050], [-0.0235, -0.030, 0.050], 0.0058, 0.0058, 12);
    recv.bx([-0.0245, -0.034, 0.050], [-0.0225, -0.027, 0.074], 0.0006);
    recv.bx([-0.0195, -0.058, 0.002], [-0.0175, -0.050, 0.018], 0.0006);
    trigger_guard(&mut guard);
    pistol_grip(&mut grip, 0.0);

    // buffer tube and the adjustable stock behind it, with a rubber pad
    let g = |x: f32, y: f32, z: f32| [x, y, z];
    tube_parts.cyl([0.0, -0.004, 0.10], [0.0, -0.004, 0.272], 0.0124, 0.0124, 22);
    let (xf, xb, zf, zb) = (0.0150, 0.0176, 0.150, 0.288);
    grip.hex(
        [
            g(-xf, -0.026, zf), g(xf, -0.026, zf), g(-xf, 0.0115, zf), g(xf, 0.0115, zf),
            g(-xb, -0.062, zb), g(xb, -0.062, zb), g(-xb, 0.026, zb), g(xb, 0.026, zb),
        ],
        0.0034,
    );
    dark.bx([-0.0186, -0.064, 0.288], [0.0186, 0.028, 0.3015], 0.0022);

    // round handguard with a barrel nut, a continuing top rail and slots down both sides
    recv.cyl([0.0, 0.0, -0.172], [0.0, 0.0, -0.150], 0.0245, 0.0245, 24);
    let mut hand_guard = Mesh::default();
    hand_guard.cyl([0.0, 0.0, -0.405], [0.0, 0.0, -0.172], 0.0215, 0.0215, 28);
    hand_guard.cyl([0.0, 0.0, -0.411], [0.0, 0.0, -0.405], 0.0228, 0.0228, 28);
    hand_guard.bx([-0.0105, 0.016, -0.405], [0.0105, 0.0262, -0.172], 0.0008);
    for k in 0..21 {
        let z = -0.402 + k as f32 * 0.0108;
        hand_guard.bx([-0.0105, 0.0262, z], [0.0105, 0.0292, z + 0.0056], 0.0006);
    }
    for k in 0..3 {
        let z = -0.385 + k as f32 * 0.062;
        for side in [-1.0f32, 1.0] {
            dark.bx([side * 0.0206 - 0.0005, -0.0042, z], [side * 0.0206 + 0.0005, 0.0042, z + 0.036], 0.0004);
        }
    }

    // barrel, gas block, flip-up front sight, and the muzzle device with its slots
    barrel.cyl([0.0, 0.0, -0.595], [0.0, 0.0, -0.405], 0.0078, 0.0078, 18);
    dark.bx([-0.0098, -0.0098, -0.442], [0.0098, 0.0098, -0.411], 0.0014);
    dark.bx([-0.0052, 0.0098, -0.436], [0.0052, 0.062, -0.419], 0.0009);
    dark.bx([-0.0012, 0.062, -0.4295], [0.0012, sight_y, -0.4255], 0.0003);
    dark.bx([-0.0098, 0.062, -0.4325], [-0.0052, 0.0715, -0.4235], 0.0006);
    dark.bx([0.0052, 0.062, -0.4325], [0.0098, 0.0715, -0.4235], 0.0006);
    tube_parts.tube([0.0, 0.0, -0.642], [0.0, 0.0, -0.585], 0.0096, 0.0062, 20);
    for k in 0..3 {
        let z = -0.633 + k as f32 * 0.016;
        dark.bx([-0.0102, -0.0028, z], [-0.0088, 0.0028, z + 0.009], 0.0004);
        dark.bx([0.0088, -0.0028, z], [0.0102, 0.0028, z + 0.009], 0.0004);
        dark.bx([-0.0028, 0.0088, z], [0.0028, 0.0102, z + 0.009], 0.0004);
    }
    // rear sight: a base on the rail, a stem, and an aperture with two protecting wings
    dark.bx([-0.0098, 0.0505, 0.058], [0.0098, 0.0615, 0.0875], 0.0012);
    dark.bx([-0.0042, 0.0615, 0.0635], [0.0042, 0.0668, 0.0725], 0.0006);
    dark.tube([0.0, sight_y, 0.0645], [0.0, sight_y, 0.0735], 0.0076, 0.0034, 18);
    dark.bx([-0.0112, 0.0615, 0.0635], [-0.0078, 0.0795, 0.0735], 0.0007);
    dark.bx([0.0078, 0.0615, 0.0635], [0.0112, 0.0795, 0.0735], 0.0007);
    // a dark ejection port on the right
    dark.bx([0.0169, 0.0105, 0.012], [0.0178, 0.0275, 0.052], 0.0004);

    let body = vec![
        piece(recv, ANODISED, [0.27, 0.28, 0.31], 0.7),
        piece(rails, ANODISED, [0.24, 0.25, 0.28], 0.7),
        piece(hand_guard, ANODISED, [0.20, 0.21, 0.23], 0.6),
        piece(grip, POLYMER, [0.11, 0.11, 0.12], 0.5),
        piece(guard, ANODISED, [0.27, 0.28, 0.31], 0.7),
        piece(barrel, STEEL, [0.26, 0.26, 0.28], 0.9),
        piece(tube_parts, ANODISED, [0.16, 0.16, 0.18], 0.7),
        piece(dark, RUBBER, BLACK, 0.3),
    ];
    Gun {
        body,
        mag: curved_magazine(),
        trigger: rifle_trigger(),
        action: Vec::new(),
        spec: Spec {
            muzzle_z: -0.645,
            sight_y,
            action: Action::Fixed,
            trigger_pivot: TRIGGER_PIVOT,
            mag_out: [0.0, -0.24, 0.05],
            mag_pivot: [0.0, -0.09, -0.045],
            mag_tilt: 0.3,
            right_at: [0.0; 3],
            left_at: [0.0; 3],
            pistol_hold: false,
            hold: [0.0; 3],
        },
    }
}

/// A curved 30-round magazine, in segments that follow its bend forward.
fn curved_magazine() -> Vec<Piece> {
    let mut m = Mesh::default();
    let mut lips = Mesh::default();
    let mut plate = Mesh::default();
    // each segment starts where the one above ended and leans a little further forward
    let (seg_len, mut a, mut c) = (0.031f32, 0.0f32, [0.0f32, -0.074, -0.0455]);
    for k in 0..5 {
        let (hw, hd) = (0.0129 - 0.0002 * k as f32, 0.0182);
        let mut cs = corners([-hw, -seg_len, -hd], [hw, 0.0, hd]);
        for p in &mut cs {
            *p = add(xr(*p, a), c);
        }
        m.hex(cs, 0.0012);
        c = add(c, xr([0.0, -seg_len * 0.97, 0.0], a));
        a += 0.05;
    }
    lips.bx([-0.0122, -0.0715, -0.0600], [0.0122, -0.0620, -0.0310], 0.0008);
    let mut cs = corners([-0.0140, -0.0095, -0.0198], [0.0140, 0.0, 0.0198]);
    for p in &mut cs {
        *p = add(xr(*p, a - 0.05), add(c, [0.0, -0.002, 0.0]));
    }
    plate.hex(cs, 0.0012);
    vec![piece(m, POLYMER, [0.12, 0.125, 0.135], 0.45), piece(lips, STEEL, [0.30, 0.30, 0.32], 0.8), piece(plate, ANODISED, [0.07, 0.07, 0.08], 0.6)]
}

// ---- SMG --------------------------------------------------------------------------------------

/// A compact 9mm: squared-off receiver, short slotted handguard, a suppressor, a red dot and a folding stock.
fn smg() -> Gun {
    let sight_y = 0.066;
    let (mut recv, mut rails, mut grip, mut guard, mut dark, mut steel, mut hg, mut can, mut optic, mut dot) = (
        Mesh::default(), Mesh::default(), Mesh::default(), Mesh::default(), Mesh::default(),
        Mesh::default(), Mesh::default(), Mesh::default(), Mesh::default(), Mesh::default(),
    );

    // upper receiver with a full-length rail, an ejection port, and the charging handle up front on the left
    recv.bx([-0.0165, -0.012, -0.175], [0.0165, 0.034, 0.075], 0.0018);
    rails.rail(0.034, -0.172, 0.072);
    dark.bx([0.0158, 0.006, -0.02], [0.0170, 0.024, 0.025], 0.0004);
    steel.bx([-0.0245, 0.010, -0.150], [-0.0165, 0.021, -0.128], 0.0012);
    steel.bx([-0.0275, 0.008, -0.146], [-0.0240, 0.023, -0.132], 0.0010);
    // lower: trigger housing, a magwell ahead of the trigger guard, selectors both sides and the mag release
    recv.bx([-0.0165, -0.062, -0.140], [0.0165, -0.012, 0.070], 0.0018);
    recv.bx([-0.0145, -0.098, -0.137], [0.0145, -0.060, -0.075], 0.0016);
    recv.bx([-0.014, -0.036, 0.070], [0.014, -0.004, 0.090], 0.0014);
    for side in [-1.0f32, 1.0] {
        recv.cyl([side * 0.0165, -0.044, -0.012], [side * 0.0222, -0.044, -0.012], 0.0046, 0.0046, 12);
        recv.bx([side * 0.0165 - 0.001, -0.050, -0.012], [side * 0.0165 + 0.001, -0.046, 0.004], 0.0004);
    }
    dark.bx([0.0145, -0.072, -0.072], [0.0170, -0.062, -0.062], 0.0006);
    trigger_guard(&mut guard);
    pistol_grip(&mut grip, 0.0);

    // square handguard with slots down the sides and underneath, and the rail carried on along the top
    hg.bx([-0.0195, -0.024, -0.318], [0.0195, 0.0225, -0.175], 0.003);
    hg.rail(0.0225, -0.318, -0.178);
    for k in 0..3 {
        let z = -0.305 + k as f32 * 0.042;
        for side in [-1.0f32, 1.0] {
            dark.bx([side * 0.0195 - 0.0005, -0.012, z], [side * 0.0195 + 0.0005, 0.006, z + 0.028], 0.0004);
        }
        dark.bx([-0.006, -0.0245, z], [0.006, -0.0235, z + 0.028], 0.0004);
    }

    // a stub of barrel, the suppressor's mount, its body with two grip rings, and a tapered end cap
    steel.cyl([0.0, 0.0, -0.335], [0.0, 0.0, -0.318], 0.0080, 0.0080, 16);
    steel.cyl([0.0, 0.0, -0.350], [0.0, 0.0, -0.335], 0.0130, 0.0130, 20);
    can.cyl([0.0, 0.0, -0.505], [0.0, 0.0, -0.350], 0.0175, 0.0175, 28);
    can.cyl([0.0, 0.0, -0.513], [0.0, 0.0, -0.505], 0.0150, 0.0175, 28);
    for z in [-0.368f32, -0.376, -0.486] {
        dark.tube([0.0, 0.0, z], [0.0, 0.0, z + 0.003], 0.0178, 0.0170, 28);
    }
    dark.cyl([0.0, 0.0, -0.5133], [0.0, 0.0, -0.5130], 0.0042, 0.0042, 14);

    // the red dot: a mount on the rail, a riser, an open tube with its turrets, and the dot itself
    dark.bx([-0.012, 0.0435, -0.040], [0.012, 0.049, 0.008], 0.001);
    dark.bx([-0.007, 0.049, -0.034], [0.007, sight_y - 0.012, 0.002], 0.001);
    optic.tube([0.0, sight_y, -0.036], [0.0, sight_y, 0.006], 0.0135, 0.0112, 26);
    optic.cyl([0.0, sight_y + 0.0128, -0.016], [0.0, sight_y + 0.019, -0.016], 0.0052, 0.0052, 12);
    optic.cyl([0.0128, sight_y, -0.016], [0.019, sight_y, -0.016], 0.0052, 0.0052, 12);
    dot.bx([-0.0006, sight_y - 0.0006, -0.0345], [0.0006, sight_y + 0.0006, -0.0335], 0.0001);

    // the folding stock: a hinge block, two rods and a lower strut out to a padded butt plate
    recv.bx([-0.0145, -0.014, 0.070], [0.0145, 0.016, 0.088], 0.0012);
    for x in [-0.0105f32, 0.0105] {
        steel.cyl([x, 0.006, 0.086], [x, -0.002, 0.268], 0.0040, 0.0040, 10);
    }
    steel.cyl([0.0, -0.030, 0.090], [0.0, -0.046, 0.268], 0.0040, 0.0040, 10);
    recv.bx([-0.0165, -0.062, 0.265], [0.0165, 0.016, 0.279], 0.002);
    dark.bx([-0.0172, -0.064, 0.279], [0.0172, 0.018, 0.291], 0.002);

    let body = vec![
        piece(recv, ANODISED, [0.22, 0.23, 0.25], 0.65),
        piece(rails, ANODISED, [0.20, 0.21, 0.23], 0.7),
        piece(hg, ANODISED, [0.17, 0.18, 0.19], 0.6),
        piece(grip, POLYMER, [0.11, 0.11, 0.12], 0.5),
        piece(guard, ANODISED, [0.22, 0.23, 0.25], 0.7),
        piece(steel, STEEL, [0.24, 0.24, 0.26], 0.9),
        piece(can, ANODISED, [0.10, 0.10, 0.11], 0.5),
        piece(optic, ANODISED, [0.08, 0.08, 0.09], 0.6),
        piece(dark, RUBBER, BLACK, 0.3),
        glow(dot, [1.0, 0.12, 0.08]),
    ];
    Gun {
        body,
        mag: box_mag(0.0105, -0.072, -0.235, -0.130, -0.082, 0.05, [0.12, 0.125, 0.135]),
        trigger: rifle_trigger(),
        action: Vec::new(),
        spec: Spec {
            muzzle_z: -0.513,
            sight_y,
            action: Action::Fixed,
            trigger_pivot: TRIGGER_PIVOT,
            mag_out: [0.0, -0.26, 0.03],
            mag_pivot: [0.0, -0.15, -0.106],
            mag_tilt: 0.15,
            right_at: [0.0; 3],
            left_at: [0.0, 0.0, 0.02],
            pistol_hold: false,
            hold: [0.0; 3],
        },
    }
}

// ---- shotgun ----------------------------------------------------------------------------------

/// A pump gun with a pistol grip: steel receiver with a side saddle of shells, a barrel over its magazine tube,
/// a ghost ring and a fibre-optic front post, and a wooden stock and forend. The forend is the part that pumps.
fn shotgun() -> Gun {
    let sight_y = 0.047;
    let (mut recv, mut rails, mut grip, mut guard, mut barrel, mut steel, mut dark, mut wood, mut shells, mut brass, mut fibre) = (
        Mesh::default(), Mesh::default(), Mesh::default(), Mesh::default(), Mesh::default(), Mesh::default(),
        Mesh::default(), Mesh::default(), Mesh::default(), Mesh::default(), Mesh::default(),
    );

    // receiver: loading port underneath, ejection port on the right, a short rail on top
    recv.bx([-0.0175, -0.040, -0.135], [0.0175, 0.028, 0.075], 0.002);
    dark.bx([-0.011, -0.0405, -0.130], [0.011, -0.039, -0.086], 0.0004);
    dark.bx([0.0172, 0.002, -0.065], [0.0180, 0.022, -0.005], 0.0004);
    rails.rail(0.028, -0.06, 0.072);
    // trigger housing with a cross-bolt safety and the action release ahead of the guard
    recv.bx([-0.016, -0.062, -0.080], [0.016, -0.040, 0.070], 0.0016);
    steel.cyl([-0.018, -0.050, 0.040], [0.018, -0.050, 0.040], 0.0034, 0.0034, 12);
    dark.bx([-0.0185, -0.054, -0.072], [-0.0160, -0.046, -0.058], 0.0005);
    trigger_guard(&mut guard);
    pistol_grip(&mut grip, 0.0);

    // side saddle on the left with four shells, brass up
    recv.bx([-0.0215, -0.032, -0.112], [-0.0175, 0.022, -0.030], 0.001);
    for k in 0..4 {
        let z = -0.101 + k as f32 * 0.0195;
        shells.cyl([-0.0272, -0.030, z], [-0.0272, 0.012, z], 0.0062, 0.0062, 14);
        brass.cyl([-0.0272, 0.012, z], [-0.0272, 0.020, z], 0.0065, 0.0065, 14);
    }

    // ghost ring rear sight with protective ears
    dark.bx([-0.012, 0.0385, 0.035], [0.012, 0.044, 0.065], 0.001);
    dark.bx([-0.0028, 0.044, 0.046], [0.0028, sight_y - 0.0075, 0.052], 0.0005);
    dark.tube([0.0, sight_y, 0.045], [0.0, sight_y, 0.053], 0.0085, 0.0048, 18);
    for side in [-1.0f32, 1.0] {
        dark.bx([side * 0.0105 - 0.0016, 0.044, 0.043], [side * 0.0105 + 0.0016, 0.060, 0.055], 0.0007);
    }

    // barrel over the magazine tube, a clamp joining them, the tube's cap, and the muzzle
    barrel.cyl([0.0, 0.0, -0.645], [0.0, 0.0, -0.135], 0.0105, 0.0105, 24);
    dark.cyl([0.0, 0.0, -0.6453], [0.0, 0.0, -0.6450], 0.0076, 0.0076, 18);
    steel.cyl([0.0, -0.024, -0.560], [0.0, -0.024, -0.135], 0.0095, 0.0095, 20);
    steel.cyl([0.0, -0.024, -0.578], [0.0, -0.024, -0.560], 0.0108, 0.0108, 20);
    for k in 0..4 {
        let z = -0.576 + k as f32 * 0.004;
        dark.tube([0.0, -0.024, z], [0.0, -0.024, z + 0.0012], 0.0110, 0.0100, 20);
    }
    recv.bx([-0.0125, -0.0365, -0.550], [0.0125, 0.0125, -0.528], 0.002);
    // front post on a ramp, with ears and a fibre-optic tip
    dark.bx([-0.005, 0.0095, -0.628], [0.005, 0.019, -0.596], 0.0012);
    dark.bx([-0.0015, 0.019, -0.615], [0.0015, sight_y - 0.003, -0.607], 0.0003);
    for side in [-1.0f32, 1.0] {
        dark.bx([side * 0.0068 - 0.0013, 0.019, -0.618], [side * 0.0068 + 0.0013, 0.050, -0.604], 0.0005);
    }
    fibre.bx([-0.0013, sight_y - 0.003, -0.6135], [0.0013, sight_y, -0.6085], 0.0002);

    // wooden stock from the back of the receiver, with a rubber pad. It drops toward the butt, so aiming down the
    // low ghost ring the comb sits well under the eye instead of running back into it
    let g = |x: f32, y: f32, z: f32| [x, y, z];
    wood.hex(
        [
            g(-0.016, -0.040, 0.072), g(0.016, -0.040, 0.072), g(-0.016, 0.026, 0.072), g(0.016, 0.026, 0.072),
            g(-0.019, -0.114, 0.312), g(0.019, -0.114, 0.312), g(-0.019, 0.002, 0.312), g(0.019, 0.002, 0.312),
        ],
        0.004,
    );
    dark.bx([-0.0196, -0.116, 0.312], [0.0196, 0.004, 0.327], 0.003);

    // the pump: a grooved wooden forend round the tube, and the action bars running back into the receiver
    let (mut fore, mut bars, mut grooves) = (Mesh::default(), Mesh::default(), Mesh::default());
    fore.bx([-0.0215, -0.047, -0.402], [0.0215, -0.004, -0.240], 0.007);
    for k in 0..6 {
        let z = -0.386 + k as f32 * 0.024;
        grooves.bx([-0.0219, -0.043, z], [0.0219, -0.008, z + 0.003], 0.004);
    }
    for x in [-0.0115f32, 0.0115] {
        bars.bx([x - 0.0015, -0.021, -0.245], [x + 0.0015, -0.014, -0.120], 0.0005);
    }

    let body = vec![
        piece(recv, ANODISED, [0.12, 0.12, 0.13], 0.7),
        piece(rails, ANODISED, [0.14, 0.14, 0.15], 0.7),
        piece(grip, POLYMER, [0.11, 0.11, 0.12], 0.5),
        piece(guard, ANODISED, [0.12, 0.12, 0.13], 0.7),
        piece(barrel, STEEL, [0.17, 0.17, 0.19], 0.9),
        piece(steel, STEEL, [0.20, 0.20, 0.22], 0.85),
        piece(wood, WOOD, [0.34, 0.20, 0.10], 0.35),
        piece(shells, POLYMER, [0.55, 0.07, 0.05], 0.5),
        piece(brass, STEEL, BRASS, 0.9),
        piece(dark, RUBBER, BLACK, 0.3),
        glow(fibre, [1.0, 0.45, 0.08]),
    ];
    Gun {
        body,
        mag: Vec::new(),
        trigger: rifle_trigger(),
        action: vec![piece(fore, WOOD, [0.34, 0.20, 0.10], 0.35), piece(grooves, WOOD, [0.16, 0.09, 0.05], 0.2), piece(bars, STEEL, [0.24, 0.24, 0.26], 0.9)],
        spec: Spec {
            muzzle_z: -0.645,
            sight_y,
            action: Action::Pump,
            trigger_pivot: TRIGGER_PIVOT,
            mag_out: [0.0; 3],
            mag_pivot: [0.0; 3],
            mag_tilt: 0.0,
            right_at: [0.0; 3],
            left_at: [0.0, -0.0255, -0.056],
            pistol_hold: false,
            hold: [0.0; 3],
        },
    }
}

// ---- sniper -----------------------------------------------------------------------------------

/// A bolt gun in a tan chassis: round steel action, heavy barrel with a ported brake, a folded bipod, a big scope
/// in two rings, and a stock with a cheek riser. The bolt handle is the part that cycles.
fn sniper() -> Gun {
    let sight_y = 0.058;
    let (mut action, mut rails, mut chassis, mut grip, mut guard, mut barrel, mut brake, mut dark, mut scope, mut glass, mut steel) = (
        Mesh::default(), Mesh::default(), Mesh::default(), Mesh::default(), Mesh::default(), Mesh::default(),
        Mesh::default(), Mesh::default(), Mesh::default(), Mesh::default(), Mesh::default(),
    );

    // the action: a steel cylinder with a recoil lug, an ejection port, a bolt shroud, and a rail on top
    action.cyl([0.0, 0.0, -0.135], [0.0, 0.0, 0.070], 0.0165, 0.0165, 28);
    action.cyl([0.0, 0.0, -0.147], [0.0, 0.0, -0.135], 0.0185, 0.0185, 28);
    action.cyl([0.0, 0.0, 0.070], [0.0, 0.0, 0.096], 0.0125, 0.0105, 20);
    dark.bx([0.0150, 0.001, -0.060], [0.0170, 0.012, -0.004], 0.0004);
    rails.rail(0.0145, -0.140, 0.072);

    // chassis: the bed under the action, a magwell ahead of the guard, and a long free-floating forend
    chassis.bx([-0.0195, -0.062, -0.147], [0.0195, -0.008, 0.075], 0.002);
    chassis.bx([-0.0175, -0.080, -0.130], [0.0175, -0.060, -0.072], 0.0015);
    chassis.bx([-0.023, -0.034, -0.505], [0.023, 0.012, -0.147], 0.003);
    for k in 0..5 {
        let z = -0.480 + k as f32 * 0.064;
        for side in [-1.0f32, 1.0] {
            dark.bx([side * 0.023 - 0.0005, -0.022, z], [side * 0.023 + 0.0005, 0.0, z + 0.040], 0.0004);
        }
    }
    dark.bx([0.0175, -0.076, -0.070], [0.0195, -0.066, -0.060], 0.0005);
    trigger_guard(&mut guard);
    pistol_grip(&mut grip, 0.0);

    // a heavy barrel, the brake with ports through it, and the bore
    barrel.cyl([0.0, 0.0, -0.800], [0.0, 0.0, -0.147], 0.0095, 0.0115, 24);
    brake.bx([-0.0135, -0.0135, -0.865], [0.0135, 0.0135, -0.800], 0.003);
    for k in 0..3 {
        let z = -0.852 + k as f32 * 0.018;
        dark.bx([-0.0137, -0.0065, z], [0.0137, 0.0065, z + 0.010], 0.0006);
    }
    dark.cyl([0.0, 0.0, -0.8653], [0.0, 0.0, -0.8650], 0.0055, 0.0055, 16);

    // the bipod folded up under the forend
    chassis.bx([-0.013, -0.043, -0.490], [0.013, -0.034, -0.462], 0.0015);
    for x in [-0.0095f32, 0.0095] {
        steel.cyl([x, -0.041, -0.468], [x, -0.043, -0.300], 0.0042, 0.0034, 10);
        dark.cyl([x, -0.043, -0.302], [x, -0.044, -0.285], 0.0052, 0.0046, 10);
    }

    // stock with a cheek riser and a pad
    let g = |x: f32, y: f32, z: f32| [x, y, z];
    chassis.hex(
        [
            g(-0.016, -0.050, 0.075), g(0.016, -0.050, 0.075), g(-0.016, 0.006, 0.075), g(0.016, 0.006, 0.075),
            g(-0.0185, -0.092, 0.342), g(0.0185, -0.092, 0.342), g(-0.0185, 0.018, 0.342), g(0.0185, 0.018, 0.342),
        ],
        0.0035,
    );
    chassis.bx([-0.0135, 0.014, 0.175], [0.0135, 0.034, 0.320], 0.004);
    dark.bx([-0.0195, -0.094, 0.342], [0.0195, 0.022, 0.357], 0.003);

    // the scope: main tube, objective bell and eyepiece, turret saddle and turrets, glass at both ends, two rings
    scope.cyl([0.0, sight_y, -0.155], [0.0, sight_y, 0.060], 0.0152, 0.0152, 28);
    scope.cyl([0.0, sight_y, -0.192], [0.0, sight_y, -0.155], 0.0275, 0.0152, 28);
    scope.cyl([0.0, sight_y, -0.255], [0.0, sight_y, -0.192], 0.0285, 0.0275, 28);
    scope.cyl([0.0, sight_y, 0.060], [0.0, sight_y, 0.086], 0.0152, 0.0200, 28);
    scope.cyl([0.0, sight_y, 0.086], [0.0, sight_y, 0.140], 0.0205, 0.0205, 28);
    dark.tube([0.0, sight_y, 0.140], [0.0, sight_y, 0.149], 0.0212, 0.0170, 28);
    dark.tube([0.0, sight_y, -0.262], [0.0, sight_y, -0.255], 0.0290, 0.0255, 28);
    glass.cyl([0.0, sight_y, -0.2552], [0.0, sight_y, -0.2548], 0.0262, 0.0262, 28);
    glass.cyl([0.0, sight_y, 0.1390], [0.0, sight_y, 0.1394], 0.0180, 0.0180, 28);
    scope.bx([-0.0172, sight_y - 0.0172, -0.068], [0.0172, sight_y + 0.0172, -0.022], 0.006);
    scope.cyl([0.0, sight_y + 0.016, -0.045], [0.0, sight_y + 0.030, -0.045], 0.0105, 0.0105, 20);
    scope.cyl([0.016, sight_y, -0.045], [0.030, sight_y, -0.045], 0.0105, 0.0105, 20);
    scope.cyl([-0.016, sight_y, -0.045], [-0.026, sight_y, -0.045], 0.0085, 0.0085, 18);
    dark.cyl([0.0, sight_y + 0.030, -0.045], [0.0, sight_y + 0.0318, -0.045], 0.0095, 0.0095, 20);
    dark.cyl([0.030, sight_y, -0.045], [0.0318, sight_y, -0.045], 0.0095, 0.0095, 20);
    for z in [-0.118f32, 0.028] {
        dark.tube([0.0, sight_y, z], [0.0, sight_y, z + 0.016], 0.0188, 0.0151, 24);
        dark.bx([-0.0095, 0.0250, z], [0.0095, sight_y - 0.0168, z + 0.016], 0.001);
        dark.bx([0.0115, sight_y - 0.024, z + 0.004], [0.0215, sight_y - 0.016, z + 0.012], 0.0008);
    }

    // the bolt: a collar round the action, the handle angled down and back, and a tactical knob
    let mut bolt = Mesh::default();
    bolt.cyl([0.0, 0.0, 0.048], [0.0, 0.0, 0.064], 0.0172, 0.0172, 24);
    bolt.cyl([0.012, 0.0, 0.056], [0.042, -0.016, 0.068], 0.0042, 0.0042, 12);
    let mut knob = Mesh::default();
    knob.cyl([0.038, -0.0175, 0.0685], [0.055, -0.0245, 0.0725], 0.0088, 0.0088, 16);

    let body = vec![
        piece(action, STEEL, [0.15, 0.15, 0.16], 0.85),
        piece(rails, ANODISED, [0.16, 0.16, 0.17], 0.7),
        piece(chassis, ANODISED, [0.36, 0.31, 0.24], 0.5),
        piece(grip, POLYMER, [0.11, 0.11, 0.12], 0.5),
        piece(guard, ANODISED, [0.36, 0.31, 0.24], 0.6),
        piece(barrel, STEEL, [0.20, 0.20, 0.22], 0.9),
        piece(brake, ANODISED, [0.10, 0.10, 0.11], 0.6),
        piece(steel, STEEL, [0.22, 0.22, 0.24], 0.85),
        piece(scope, ANODISED, [0.08, 0.08, 0.09], 0.6),
        piece(glass, STEEL, GLASS, 1.2),
        piece(dark, RUBBER, BLACK, 0.3),
    ];
    Gun {
        body,
        mag: box_mag(0.0135, -0.066, -0.118, -0.124, -0.078, 0.0, [0.09, 0.09, 0.10]),
        trigger: rifle_trigger(),
        action: vec![piece(bolt, STEEL, [0.26, 0.26, 0.28], 0.9), piece(knob, POLYMER, [0.07, 0.07, 0.08], 0.4)],
        spec: Spec {
            muzzle_z: -0.865,
            sight_y,
            action: Action::Bolt,
            trigger_pivot: TRIGGER_PIVOT,
            mag_out: [0.0, -0.16, 0.0],
            mag_pivot: [0.0, -0.10, -0.10],
            mag_tilt: 0.12,
            right_at: [0.0; 3],
            left_at: [0.0, -0.011, -0.105],
            pistol_hold: false,
            hold: [0.0; 3],
        },
    }
}

// ---- pistol -----------------------------------------------------------------------------------

/// The pistol's grip sits this much higher than the rifles', so the bore runs close over the hand.
const PISTOL_GRIP_UP: f32 = 0.017;

/// A striker-fired polymer pistol: frame with an accessory rail, a steel slide with serrations, an ejection port and
/// three-dot sights, and a magazine that drops out of the grip. The slide is the part that cycles.
fn pistol() -> Gun {
    let sight_y = 0.0205;
    let up = PISTOL_GRIP_UP;
    let (mut frame, mut grip, mut dark, mut barrel) = (Mesh::default(), Mesh::default(), Mesh::default(), Mesh::default());

    // frame: dust cover with rail slots, a beavertail, a squared trigger guard, slide stop and mag release
    frame.bx([-0.0135, -0.040, -0.108], [0.0135, -0.016, 0.098], 0.002);
    frame.bx([-0.0125, -0.040, 0.090], [0.0125, -0.030, 0.114], 0.002);
    for k in 0..3 {
        let z = -0.098 + k as f32 * 0.012;
        dark.bx([-0.0138, -0.040, z], [0.0138, -0.0375, z + 0.005], 0.0003);
    }
    frame.bx([-0.0055, -0.062, -0.076], [0.0055, -0.058, 0.066], 0.0012);
    frame.bx([-0.0055, -0.062, -0.076], [0.0055, -0.040, -0.068], 0.0014);
    dark.bx([-0.0150, -0.024, -0.040], [-0.0135, -0.018, -0.004], 0.0004);
    dark.bx([-0.0175, -0.047, 0.052], [-0.0160, -0.039, 0.064], 0.0005);
    pistol_grip(&mut grip, up);
    // grip texture: shallow panels on both sides
    for side in [-1.0f32, 1.0] {
        let x = side * 0.0168;
        grip.bx_rot([x - 0.0006, -0.130 + up, 0.090], [x + 0.0006, -0.060 + up, 0.112], 0.0003, [0.0, -0.095 + up, 0.1], -0.3);
    }
    // the barrel inside the slide, seen at the muzzle and when the slide runs back
    barrel.cyl([0.0, 0.0, -0.117], [0.0, 0.0, -0.040], 0.0062, 0.0062, 16);
    dark.cyl([0.0, 0.0, -0.1173], [0.0, 0.0, -0.1170], 0.0046, 0.0046, 14);

    // the slide, with serrations front and back, an ejection port showing the barrel hood, and the sights
    let (mut slide, mut sdark, mut hood, mut dots) = (Mesh::default(), Mesh::default(), Mesh::default(), Mesh::default());
    slide.bx([-0.0125, -0.016, -0.118], [0.0125, 0.0145, 0.100], 0.0022);
    for k in 0..6 {
        let z = 0.062 + k as f32 * 0.0062;
        sdark.bx([-0.0127, -0.010, z], [0.0127, 0.010, z + 0.0026], 0.0003);
    }
    for k in 0..4 {
        let z = -0.106 + k as f32 * 0.0062;
        sdark.bx([-0.0127, -0.010, z], [0.0127, 0.010, z + 0.0026], 0.0003);
    }
    sdark.bx([0.001, 0.0140, -0.032], [0.0128, 0.0147, 0.012], 0.0003);
    hood.bx([-0.008, 0.0135, -0.030], [0.008, 0.0150, 0.006], 0.0006);
    sdark.bx([-0.0016, 0.0145, -0.112], [0.0016, sight_y, -0.104], 0.0004);
    dots.bx([-0.0009, 0.0172, -0.1040], [0.0009, 0.0190, -0.1036], 0.0001);
    sdark.bx([-0.0098, 0.0145, 0.086], [0.0098, 0.0168, 0.097], 0.0005);
    for side in [-1.0f32, 1.0] {
        sdark.bx([side * 0.0061 - 0.0037, 0.0168, 0.086], [side * 0.0061 + 0.0037, 0.0215, 0.097], 0.0005);
        dots.bx([side * 0.0061 - 0.0009, 0.0180, 0.0970], [side * 0.0061 + 0.0009, 0.0198, 0.0974], 0.0001);
    }

    // the magazine, hidden in the grip until it drops, with a base plate below it
    let (mut mag, mut plate) = (Mesh::default(), Mesh::default());
    let g = |x: f32, y: f32, z: f32| [x, y + up, z];
    mag.hex(
        [
            g(-0.0118, -0.158, 0.093), g(0.0118, -0.158, 0.093), g(-0.0118, -0.055, 0.061), g(0.0118, -0.055, 0.061),
            g(-0.0118, -0.158, 0.127), g(0.0118, -0.158, 0.127), g(-0.0118, -0.055, 0.093), g(0.0118, -0.055, 0.093),
        ],
        0.001,
    );
    plate.bx([-0.0170, -0.165 + up, 0.087], [0.0170, -0.158 + up, 0.134], 0.0015);

    let body = vec![
        piece(frame, POLYMER, [0.10, 0.10, 0.11], 0.45),
        piece(grip, POLYMER, [0.09, 0.09, 0.10], 0.4),
        piece(barrel, STEEL, [0.30, 0.30, 0.32], 0.9),
        piece(dark, RUBBER, BLACK, 0.3),
    ];
    let mut trig = Mesh::default();
    trig.bx([-0.0028, -0.056, -0.036], [0.0028, -0.040, -0.029], 0.0008);
    Gun {
        body,
        mag: vec![piece(mag, POLYMER, [0.07, 0.07, 0.08], 0.4), piece(plate, POLYMER, [0.08, 0.08, 0.09], 0.45)],
        trigger: vec![piece(trig, POLYMER, [0.08, 0.08, 0.09], 0.4)],
        action: vec![
            piece(slide, STEEL, [0.13, 0.13, 0.14], 0.8),
            piece(sdark, RUBBER, BLACK, 0.3),
            piece(hood, STEEL, [0.30, 0.30, 0.32], 0.9),
            glow(dots, [0.85, 0.95, 0.85]),
        ],
        spec: Spec {
            muzzle_z: -0.118,
            sight_y,
            action: Action::Slide,
            trigger_pivot: [0.0, -0.040, -0.032],
            mag_out: [0.0, -0.15, 0.047],
            mag_pivot: [0.0, -0.09, 0.1],
            mag_tilt: 0.0,
            right_at: [0.0, up, 0.0],
            left_at: [0.0; 3],
            pistol_hold: true,
            hold: [0.0, 0.03, -0.17],
        },
    }
}

// ---- hands ------------------------------------------------------------------------------------

/// Builds a smooth mesh of `f` over a box, as position and normal.
fn smooth(f: &dyn Fn(V) -> f32, lo: V, hi: V, h: f32) -> Mesh {
    let (pts, nrm, idx) = surface_nets(f, lo, hi, h);
    let mut m = Mesh::default();
    for (p, n) in pts.iter().zip(&nrm) {
        m.verts.extend_from_slice(&[p[0], p[1], p[2], n[0], n[1], n[2]]);
    }
    m.idx = idx;
    m
}

/// A finger as a chain of cones along `pts`, radius `r` falling to `r * taper`.
fn finger(p: V, pts: &[V], r: f32, taper: f32) -> f32 {
    let mut d = 1.0f32;
    for i in 0..pts.len() - 1 {
        let t0 = i as f32 / (pts.len() - 1) as f32;
        let t1 = (i + 1) as f32 / (pts.len() - 1) as f32;
        let k = cone(p, pts[i], pts[i + 1], r * (1.0 - (1.0 - taper) * t0), r * (1.0 - (1.0 - taper) * t1));
        d = smin(d, k, 0.006);
    }
    d
}

fn right_hand_sdf(p: V) -> f32 {
    // the back of the hand and the heel of the palm against the rear of the grip
    let mut d = ell(p, [0.034, -0.092, 0.113], [0.021, 0.040, 0.031]);
    d = smin(d, cone(p, [0.036, -0.122, 0.128], [0.040, -0.150, 0.172], 0.026, 0.027), 0.02);
    // three fingers wrapped round the front of the grip
    for (i, y) in [-0.078f32, -0.098, -0.118].iter().enumerate() {
        let t = (-0.052 - y) / 0.106 * 0.03;
        let pts = [[0.034, *y, 0.104 + t * 0.6], [0.0275, *y, 0.080 + t], [0.014, *y, 0.058 + t - 0.0088], [-0.003, *y, 0.058 + t - 0.0075]];
        d = smin(d, finger(p, &pts, 0.0098 - 0.0005 * i as f32, 0.9), 0.008);
    }
    // the index finger laid along the side of the receiver
    d = smin(d, finger(p, &[[0.034, -0.062, 0.104], [0.0268, -0.060, 0.062], [0.0266, -0.059, 0.022], [0.0266, -0.0585, -0.012]], 0.0092, 0.9), 0.008);
    // and the thumb over the back of the grip to the far side
    d = smin(d, finger(p, &[[0.022, -0.060, 0.122], [0.0, -0.058, 0.119], [-0.0235, -0.056, 0.098], [-0.0262, -0.054, 0.055]], 0.0105, 0.9), 0.008);
    d
}

fn right_sleeve_sdf(p: V) -> f32 {
    cone(p, [0.043, -0.128, 0.162], [0.125, -0.255, 0.47], 0.031, 0.05)
}

fn left_hand_sdf(p: V) -> f32 {
    // the palm under the handguard, four fingers round its right side, the thumb up the left
    let mut d = ell(p, [-0.012, -0.040, -0.265], [0.030, 0.017, 0.042]);
    d = smin(d, cone(p, [-0.04, -0.046, -0.215], [-0.055, -0.056, -0.185], 0.027, 0.028), 0.02);
    for (i, z) in [-0.232f32, -0.254, -0.276, -0.298].iter().enumerate() {
        let r = 0.031;
        let pts = [[-0.018, -0.040, *z], [0.0, -r, *z], [0.016, -0.0265, *z], [0.027, -0.0155, *z], [0.031, 0.0, *z], [0.0265, 0.0155, *z]];
        d = smin(d, finger(p, &pts, 0.0096 - 0.0004 * i as f32, 0.85), 0.008);
    }
    d = smin(d, finger(p, &[[-0.028, -0.034, -0.226], [-0.032, -0.012, -0.236], [-0.0305, 0.008, -0.262], [-0.0285, 0.0125, -0.292]], 0.0108, 0.9), 0.008);
    d
}

fn left_sleeve_sdf(p: V) -> f32 {
    cone(p, [-0.052, -0.054, -0.19], [-0.17, -0.20, 0.09], 0.031, 0.05)
}

/// The support hand on a pistol, in the pistol's own gun space: the palm against the left of the grip, four
/// fingers wrapped over the front of the right hand's, and the thumb forward along the frame.
fn pistol_left_sdf(p: V) -> f32 {
    let up = PISTOL_GRIP_UP;
    let mut d = ell(p, [-0.033, -0.082 + up, 0.090], [0.016, 0.036, 0.032]);
    d = smin(d, cone(p, [-0.040, -0.112 + up, 0.108], [-0.048, -0.136 + up, 0.140], 0.024, 0.026), 0.02);
    for (i, y) in [-0.083f32, -0.101, -0.119, -0.135].iter().enumerate() {
        let y = y + up;
        let pts = [[-0.037, y, 0.076], [-0.035, y, 0.047], [-0.016, y, 0.034], [0.006, y, 0.031], [0.025, y, 0.036]];
        d = smin(d, finger(p, &pts, 0.0094 - 0.0005 * i as f32, 0.85), 0.008);
    }
    d = smin(d, finger(p, &[[-0.034, -0.069 + up, 0.076], [-0.026, -0.059 + up, 0.048], [-0.0215, -0.051 + up, 0.012]], 0.0102, 0.9), 0.008);
    d
}

fn pistol_left_sleeve_sdf(p: V) -> f32 {
    let up = PISTOL_GRIP_UP;
    cone(p, [-0.046, -0.130 + up, 0.130], [-0.150, -0.270 + up, 0.46], 0.029, 0.05)
}

fn right_hand() -> Vec<Piece> {
    let glove = smooth(&right_hand_sdf, [-0.05, -0.18, -0.03], [0.07, -0.03, 0.2], 0.0026);
    let sleeve = smooth(&right_sleeve_sdf, [0.0, -0.33, 0.12], [0.2, -0.09, 0.58], 0.004);
    vec![piece(glove, CLOTH, GLOVE, 0.12), piece(sleeve, CLOTH, SLEEVE, 0.0)]
}

fn left_hand() -> Vec<Piece> {
    let glove = smooth(&left_hand_sdf, [-0.08, -0.08, -0.34], [0.06, 0.04, -0.17], 0.0026);
    let sleeve = smooth(&left_sleeve_sdf, [-0.26, -0.30, -0.24], [0.0, 0.0, 0.2], 0.004);
    vec![piece(glove, CLOTH, GLOVE, 0.12), piece(sleeve, CLOTH, SLEEVE, 0.0)]
}

fn pistol_left_hand() -> Vec<Piece> {
    let up = PISTOL_GRIP_UP;
    let glove = smooth(&pistol_left_sdf, [-0.07, -0.16 + up, 0.0], [0.045, -0.03 + up, 0.17], 0.0026);
    let sleeve = smooth(&pistol_left_sleeve_sdf, [-0.23, -0.34 + up, 0.08], [0.0, -0.08 + up, 0.53], 0.004);
    vec![piece(glove, CLOTH, GLOVE, 0.12), piece(sleeve, CLOTH, SLEEVE, 0.0)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::WEAPONS;

    fn check(pieces: &[Piece]) -> usize {
        let mut tris = 0;
        for p in pieces {
            assert!(!p.mesh.idx.is_empty(), "an empty piece");
            assert!(p.mesh.idx.iter().all(|i| (*i as usize) < p.mesh.verts.len() / 6));
            assert!(p.mesh.verts.iter().all(|v| v.is_finite()), "a vertex is not a number");
            tris += p.mesh.idx.len() / 3;
        }
        tris
    }

    #[test]
    fn every_gun_is_built_sized_for_what_it_is_and_has_its_muzzle_in_front() {
        let t = std::time::Instant::now();
        let (guns, hands) = (guns(), Hands::new());
        println!("built in {:?}", t.elapsed());
        assert_eq!(guns.len(), WEAPONS.len());
        println!("hands: {} triangles", check(&hands.right) + check(&hands.left) + check(&hands.pistol_left));
        // overall length, front to back, for each gun in order
        let lengths = [(0.9, 1.05), (0.7, 0.85), (0.9, 1.05), (1.1, 1.3), (0.18, 0.27)];
        for (i, g) in guns.iter().enumerate() {
            let tris = check(&g.body) + check(&g.mag) + check(&g.trigger) + check(&g.action);
            let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
            for p in g.body.iter().chain(&g.action) {
                for v in p.mesh.verts.chunks(6) {
                    for a in 0..3 {
                        lo[a] = lo[a].min(v[a]);
                        hi[a] = hi[a].max(v[a]);
                    }
                }
            }
            let name = WEAPONS[i].name;
            println!("{name}: {tris} triangles, from {lo:?} to {hi:?}");
            let l = hi[2] - lo[2];
            assert!(l > lengths[i].0 && l < lengths[i].1, "{name} is {l} long");
            assert!((lo[2] - g.spec.muzzle_z).abs() < 0.01, "{name}'s muzzle is at {} but its front at {}", g.spec.muzzle_z, lo[2]);
        }
    }
}
