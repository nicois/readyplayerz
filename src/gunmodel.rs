//! The carbine the player holds: hard-surface parts built from bevelled boxes and cylinders, and gloved hands and
//! sleeves shaped as smooth surfaces. Everything is in gun space: -z is the muzzle, +z the stock, +y up, x to the
//! right, with the bore along the z axis.

use crate::body::{cone, ell, len, smin, surface_nets};
use crate::sim::V3;
use std::f32::consts::TAU;

type V = V3;

/// Materials the shader knows for hard parts (see the fragment shader).
pub const ANODISED: f32 = 20.0;
pub const POLYMER: f32 = 21.0;
pub const STEEL: f32 = 23.0;
pub const RUBBER: f32 = 24.0;
pub const CLOTH: f32 = 10.0;

/// Where the muzzle is, and how high above the bore the sights' line of sight is.
pub const MUZZLE_Z: f32 = -0.645;
pub const SIGHT_Y: f32 = 0.0705;

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
}

fn basis(y: V) -> (V, V) {
    let h = if y[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
    let u = norm(cross(y, h));
    (u, cross(y, u))
}

/// One draw: a mesh with the look it is drawn in.
pub struct Piece {
    pub mesh: Mesh,
    pub mat: f32,
    pub color: [f32; 3],
    pub shiny: f32,
}

fn piece(mesh: Mesh, mat: f32, color: [f32; 3], shiny: f32) -> Piece {
    Piece { mesh, mat, color, shiny }
}

/// All the parts of the carbine and the hands that hold it, grouped by what moves together.
pub struct Carbine {
    pub body: Vec<Piece>,
    pub mag: Vec<Piece>,
    pub trigger: Vec<Piece>,
    pub right_hand: Vec<Piece>,
    pub left_hand: Vec<Piece>,
}

impl Carbine {
    pub fn new() -> Carbine {
        Carbine { body: body(), mag: magazine(), trigger: trigger(), right_hand: right_hand(), left_hand: left_hand() }
    }
}

fn body() -> Vec<Piece> {
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
    guard.bx([-0.0026, -0.0845, -0.071], [0.0026, -0.062, -0.0665], 0.0006);
    guard.bx([-0.0026, -0.0845, -0.071], [0.0026, -0.0805, 0.056], 0.0006);
    guard.bx([-0.0026, -0.0845, 0.052], [0.0026, -0.055, 0.057], 0.0006);

    // pistol grip, tilted back, with a thin front strap and a base screw
    let g = |x: f32, y: f32, z: f32| [x, y, z];
    let (w, yt, yb) = (0.0165, -0.052, -0.158);
    grip.hex(
        [
            g(-w, yb, 0.090), g(w, yb, 0.090), g(-w, yt, 0.058), g(w, yt, 0.058),
            g(-w, yb, 0.130), g(w, yb, 0.130), g(-w, yt, 0.096), g(w, yt, 0.096),
        ],
        0.0035,
    );
    grip.bx([-0.0055, -0.1595, 0.102], [0.0055, -0.1575, 0.114], 0.0004);

    // buffer tube and the adjustable stock behind it, with a rubber pad
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
    dark.bx([-0.0012, 0.062, -0.4295], [0.0012, SIGHT_Y, -0.4255], 0.0003);
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
    dark.tube([0.0, SIGHT_Y, 0.0645], [0.0, SIGHT_Y, 0.0735], 0.0076, 0.0034, 18);
    dark.bx([-0.0112, 0.0615, 0.0635], [-0.0078, 0.0795, 0.0735], 0.0007);
    dark.bx([0.0078, 0.0615, 0.0635], [0.0112, 0.0795, 0.0735], 0.0007);
    // a dark ejection port on the right
    dark.bx([0.0169, 0.0105, 0.012], [0.0178, 0.0275, 0.052], 0.0004);

    vec![
        piece(recv, ANODISED, [0.27, 0.28, 0.31], 0.7),
        piece(rails, ANODISED, [0.24, 0.25, 0.28], 0.7),
        piece(hand_guard, ANODISED, [0.20, 0.21, 0.23], 0.6),
        piece(grip, POLYMER, [0.11, 0.11, 0.12], 0.5),
        piece(guard, ANODISED, [0.27, 0.28, 0.31], 0.7),
        piece(barrel, STEEL, [0.26, 0.26, 0.28], 0.9),
        piece(tube_parts, ANODISED, [0.16, 0.16, 0.18], 0.7),
        piece(dark, RUBBER, [0.025, 0.025, 0.03], 0.3),
    ]
}

/// A curved 30-round magazine, in segments that follow its bend forward.
fn magazine() -> Vec<Piece> {
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

fn trigger() -> Vec<Piece> {
    let mut t = Mesh::default();
    t.bx([-0.0022, -0.064, -0.0345], [0.0022, -0.040, -0.0275], 0.0006);
    vec![piece(t, STEEL, [0.28, 0.28, 0.30], 0.8)]
}

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

fn right_hand() -> Vec<Piece> {
    let glove = smooth(&right_hand_sdf, [-0.05, -0.18, -0.03], [0.07, -0.03, 0.2], 0.0026);
    let sleeve = smooth(&right_sleeve_sdf, [0.0, -0.33, 0.12], [0.2, -0.09, 0.58], 0.004);
    vec![piece(glove, CLOTH, [0.06, 0.06, 0.065], 0.12), piece(sleeve, CLOTH, [0.20, 0.24, 0.17], 0.0)]
}

fn left_hand() -> Vec<Piece> {
    let glove = smooth(&left_hand_sdf, [-0.08, -0.08, -0.34], [0.06, 0.04, -0.17], 0.0026);
    let sleeve = smooth(&left_sleeve_sdf, [-0.26, -0.30, -0.24], [0.0, 0.0, 0.2], 0.004);
    vec![piece(glove, CLOTH, [0.06, 0.06, 0.065], 0.12), piece(sleeve, CLOTH, [0.20, 0.24, 0.17], 0.0)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_carbine_is_built_and_sized_like_a_rifle() {
        let t = std::time::Instant::now();
        let c = Carbine::new();
        println!("built in {:?}", t.elapsed());
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        let mut tris = 0;
        for p in c.body.iter().chain(&c.mag).chain(&c.trigger).chain(&c.right_hand).chain(&c.left_hand) {
            assert!(!p.mesh.idx.is_empty(), "an empty piece");
            assert!(p.mesh.idx.iter().all(|i| (*i as usize) < p.mesh.verts.len() / 6));
            assert!(p.mesh.verts.iter().all(|v| v.is_finite()), "a vertex is not a number");
            tris += p.mesh.idx.len() / 3;
        }
        for p in c.body.iter() {
            for v in p.mesh.verts.chunks(6) {
                for a in 0..3 {
                    lo[a] = lo[a].min(v[a]);
                    hi[a] = hi[a].max(v[a]);
                }
            }
        }
        println!("{tris} triangles, from {lo:?} to {hi:?}");
        assert!(hi[2] - lo[2] > 0.9 && hi[2] - lo[2] < 1.05, "length {}", hi[2] - lo[2]);
    }
}
