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

    /// A smooth solid of revolution about the z axis through the profile `(z, r)`, in order from one end to the
    /// other. Each ring's normal follows the profile through it, so a curve reads as one; an end at radius 0 closes.
    pub fn lathe(&mut self, profile: &[(f32, f32)], seg: usize) {
        let n = profile.len();
        let slope = |i: usize| {
            let (a, b) = (profile[i.saturating_sub(1)], profile[(i + 1).min(n - 1)]);
            let (dz, dr) = (b.0 - a.0, b.1 - a.1);
            let l = dz.hypot(dr).max(1e-9);
            // out from the axis, or along it at an end
            let (nz, nr) = (dr / l, -dz / l);
            if nr < 0.0 { (-nz, -nr) } else { (nz, nr) }
        };
        for i in 0..n - 1 {
            let ((z0, r0), (z1, r1), s0, s1) = (profile[i], profile[i + 1], slope(i), slope(i + 1));
            for k in 0..seg {
                let (t0, t1) = (k as f32 / seg as f32 * TAU, (k + 1) as f32 / seg as f32 * TAU);
                let (d0, d1) = ([t0.cos(), t0.sin()], [t1.cos(), t1.sin()]);
                let at = |d: [f32; 2], z: f32, r: f32| [d[0] * r, d[1] * r, z];
                let nrm = |d: [f32; 2], s: (f32, f32)| [d[0] * s.1, d[1] * s.1, s.0];
                self.quad(
                    [at(d0, z0, r0), at(d0, z1, r1), at(d1, z1, r1), at(d1, z0, r0)],
                    [nrm(d0, s0), nrm(d0, s1), nrm(d1, s1), nrm(d1, s0)],
                );
            }
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
    /// the barrels tip down about `Spec::pivot` to reload, taking the shells in them along
    Break,
    /// the cylinder turns a chamber about its axis through `Spec::pivot` with every shot, and swings out about
    /// `Spec::swing` to reload
    Cylinder,
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
    /// how far the muzzle flips up on a shot, in radians
    pub flip: f32,
    /// the hinge of a break action, or the axis of a cylinder (see `Action`)
    pub pivot: V,
    /// the axis a cylinder swings out about
    pub swing: V,
    /// the rounds of a break action or a cylinder, each in its own chamber (see `Rounds`)
    pub rounds: Option<Rounds>,
}

/// A gun whose rounds sit where they can be seen, one in each chamber: the first `parts` of `Gun::mag` are then one
/// round, drawn in each, and any after them a speedloader. A reload goes: open, the spent ones are pulled out and
/// thrown clear, fresh ones come up in the hand turned side on, so they can be seen, swing into line behind the
/// chambers and are pushed home, close. The times are fractions of the reload.
#[derive(Clone, Copy)]
pub struct Rounds {
    /// where each round's base sits, in the frame of the part that opens; the first fires first
    pub chambers: &'static [V],
    /// how many of the pieces are one round, and how many of those the case, all that is left once it has fired
    pub parts: usize,
    pub case: usize,
    /// how thick a round is, to lie on the floor rather than in it
    pub radius: f32,
    /// when the spent ones start coming out, how far out they are by the time they are thrown clear, and when
    pub pull: f32,
    pub out: f32,
    pub eject: f32,
    /// whether the live ones are thrown out along with the spent, as a cylinder's ejector does
    pub all: bool,
    /// how fast one is thrown clear, in its own frame: +z is back out of the chamber; and how far the muzzle is
    /// tipped up as they go, flicking them clear or letting them drop
    pub throw: V,
    pub flick: f32,
    /// when the first fresh one comes up, how long after it each next one does, and how long each takes to go in
    pub fill: f32,
    pub stagger: f32,
    pub load: f32,
    /// where a fresh one comes up from, as an offset from its chamber, and turned how far about x from in line;
    /// where it is in line, just behind, to go straight in from
    pub from: V,
    pub tilt: f32,
    pub entry: V,
    /// the middle of the speedloader that brings them all at once, if there is one: it is turned about it, and
    /// drawn there
    pub loader: Option<V>,
    /// where the left hand is from its hold when it has a fresh round at its chamber
    pub hand: V,
}

/// Which way a speedloader is taken away once it has let its rounds go, in the frame of the part that opens.
const LOADER_AWAY: V = [0.0, -0.10, 0.12];

/// 0 up to `a`, 1 from `b`, smoothly between.
pub fn ease(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// How a reload of `Rounds` goes, by how far through it is (`reload`, 0 to 1), with `fired` of them spent.
impl Rounds {
    /// How many chambers a reload empties, and so fills again.
    pub fn emptied(&self, fired: usize) -> usize {
        if self.all { self.chambers.len() } else { fired.min(self.chambers.len()) }
    }

    /// When the last fresh one of `n` is home.
    pub fn loaded(&self, n: usize) -> f32 {
        self.fill + n.saturating_sub(1) as f32 * self.stagger + self.load
    }

    /// How far fresh round `j` has gone in, 0 to 1, once it has started. On a speedloader they all go together.
    pub fn progress(&self, j: usize, reload: f32) -> Option<f32> {
        let j = if self.loader.is_some() { 0 } else { j };
        let start = self.fill + j as f32 * self.stagger;
        (reload >= start).then(|| ((reload - start) / self.load).min(1.0))
    }

    /// Where a fresh round is `u` of the way in: brought up turned side on so it shows, swung into line at
    /// `entry`, then pushed straight in. As an offset from its chamber, and a turn about x.
    pub fn loading(&self, u: f32) -> (V, f32) {
        let (a, b) = (ease(0.0, 0.6, u), ease(0.6, 1.0, u));
        let (f, e) = (self.from, self.entry);
        (std::array::from_fn(|i| (f[i] + (e[i] - f[i]) * a) * (1.0 - b)), self.tilt * (1.0 - a))
    }

    /// Where the speedloader has been taken from its rounds once they are home, 0 there at first.
    pub fn loader_away(&self, reload: f32) -> V {
        let done = self.loaded(1);
        let k = if self.loader.is_some() { ease(done + 0.03, done + 0.15, reload) } else { 0.0 };
        LOADER_AWAY.map(|v| v * k)
    }

    /// Whether the speedloader is still in the hand.
    pub fn loader_held(&self, reload: f32) -> bool {
        self.loader.is_some() && reload >= self.fill && reload < self.loaded(1) + 0.15
    }

    /// How far the muzzle is tipped up for the rounds to come out.
    pub fn tip(&self, reload: f32) -> f32 {
        self.flick * (ease(self.pull - 0.05, self.eject, reload) - ease(self.eject + 0.03, self.fill + 0.04, reload))
    }

    /// Where the left hand is from its hold: down for fresh rounds while the spent ones come out, then with each
    /// fresh one as it goes in (back down for the next in between), and with the speedloader as it is taken away.
    /// As a move from the hold, and one along with the rounds in the frame of the part that opens.
    pub fn hand_at(&self, fired: usize, reload: f32) -> (V, V) {
        let n = self.emptied(fired).max(1);
        let done = self.loaded(n) + if self.loader.is_some() { 0.15 } else { 0.0 };
        let fetch = ease(self.pull - 0.1, self.eject, reload) - ease(self.fill - 0.1, self.fill, reload);
        let carry = ease(self.fill - 0.1, self.fill, reload) - ease(done + 0.02, done + 0.12, reload);
        // the round it has now: the first not yet in
        let j = (0..n).find(|&j| self.progress(j, reload).is_none_or(|u| u < 1.0)).unwrap_or(n - 1);
        let o = match self.progress(j, reload) {
            Some(u) => self.loading(u).0,
            // back down for the next, after the one before has gone in
            None if j > 0 => {
                let start = self.fill + j as f32 * self.stagger;
                self.from.map(|v| v * ease(start - self.stagger + self.load, start, reload))
            }
            None => self.from,
        };
        let away = self.loader_away(reload);
        (std::array::from_fn(|i| self.hand[i] * carry + [0.0, -0.16, 0.04][i] * fetch), std::array::from_fn(|i| (o[i] + away[i]) * carry))
    }
}

/// What every gun's spec has unless it says otherwise.
const SPEC: Spec = Spec {
    muzzle_z: 0.0,
    sight_y: 0.0,
    action: Action::Fixed,
    trigger_pivot: TRIGGER_PIVOT,
    mag_out: [0.0; 3],
    mag_pivot: [0.0; 3],
    mag_tilt: 0.0,
    right_at: [0.0; 3],
    left_at: [0.0; 3],
    pistol_hold: false,
    hold: [0.0; 3],
    flip: 0.07,
    pivot: [0.0; 3],
    swing: [0.0; 3],
    rounds: None,
};

/// A block that changes size along its length, as for a stock: `x` half widths and `y` bottom and top at `z0`
/// and at `z1`.
fn taper(m: &mut Mesh, z0: f32, x0: f32, y0: [f32; 2], z1: f32, x1: f32, y1: [f32; 2], bevel: f32) {
    m.hex(
        [
            [-x0, y0[0], z0], [x0, y0[0], z0], [-x0, y0[1], z0], [x0, y0[1], z0],
            [-x1, y1[0], z1], [x1, y1[0], z1], [-x1, y1[1], z1], [x1, y1[1], z1],
        ],
        bevel,
    );
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
    vec![carbine(), smg(), shotgun(), sniper(), ak47(), uzi(), double_barrel(), mosin(), pistol(), revolver(), deagle(), baseball_bat()]
}

const GLOVE: [f32; 3] = [0.06, 0.06, 0.065];
const SLEEVE: [f32; 3] = [0.20, 0.24, 0.17];
const BLACK: [f32; 3] = [0.025, 0.025, 0.03];
const GLASS: [f32; 3] = [0.04, 0.07, 0.10];
const BRASS: [f32; 3] = [0.55, 0.42, 0.18];
/// blued steel, nearly black with a cold sheen
const BLUED: [f32; 3] = [0.09, 0.095, 0.11];

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
            ..SPEC
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
            ..SPEC
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
            ..SPEC
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
            ..SPEC
        },
    }
}

// ---- AK-47 ------------------------------------------------------------------------------------

/// A stamped steel receiver with a ribbed dust cover, red wood furniture, the gas tube over the barrel, a tangent
/// rear sight, a hooded front post and a slant brake, and a deeply curved magazine.
fn ak47() -> Gun {
    let sight_y = 0.056;
    let (mut recv, mut steel, mut dark, mut wood, mut grip, mut guard) =
        (Mesh::default(), Mesh::default(), Mesh::default(), Mesh::default(), Mesh::default(), Mesh::default());
    const RED_WOOD: [f32; 3] = [0.30, 0.12, 0.05];

    // receiver, and the dust cover over it with a rib down the middle and its rear catch
    recv.bx([-0.0170, -0.048, -0.150], [0.0170, 0.012, 0.100], 0.0016);
    recv.bx([-0.0160, 0.012, -0.135], [0.0160, 0.026, 0.098], 0.0055);
    steel.bx([-0.0035, 0.025, -0.120], [0.0035, 0.029, 0.090], 0.0012);
    steel.bx([-0.0045, 0.010, 0.096], [0.0045, 0.024, 0.108], 0.0015);
    // the charging handle out on the right, and its slot
    dark.bx([0.0168, 0.000, -0.090], [0.0174, 0.010, 0.050], 0.0003);
    steel.bx([0.0170, 0.001, 0.034], [0.0270, 0.009, 0.046], 0.0018);
    // the long selector lever on the right, stamped rivets, and the magwell
    steel.bx([0.0170, -0.004, -0.030], [0.0188, 0.006, 0.080], 0.0006);
    steel.bx([0.0170, -0.010, 0.064], [0.0188, 0.006, 0.084], 0.0008);
    for (y, z) in [(-0.040f32, -0.120f32), (-0.040, 0.020), (-0.020, 0.070), (-0.040, 0.080)] {
        dark.cyl([0.0168, y, z], [0.0176, y, z], 0.0018, 0.0018, 8);
        dark.cyl([-0.0168, y, z], [-0.0176, y, z], 0.0018, 0.0018, 8);
    }
    recv.bx([-0.0155, -0.060, -0.098], [0.0155, -0.046, -0.042], 0.0012);
    // the trigger housing under the back half, which the grip and the guard hang from
    recv.bx([-0.0150, -0.062, -0.042], [0.0150, -0.046, 0.100], 0.0015);
    // the paddle magazine release ahead of the guard
    steel.bx([-0.004, -0.066, -0.040], [0.004, -0.052, -0.030], 0.0008);
    trigger_guard(&mut guard);
    pistol_grip(&mut grip, 0.0);

    // the rear sight block and its leaf with the notch, just ahead of the receiver
    steel.bx([-0.0130, 0.004, -0.205], [0.0130, 0.034, -0.150], 0.002);
    steel.bx([-0.0080, 0.034, -0.198], [0.0080, 0.041, -0.158], 0.001);
    for side in [-1.0f32, 1.0] {
        dark.bx([side * 0.0042 - 0.0026, 0.041, -0.166], [side * 0.0042 + 0.0026, sight_y, -0.160], 0.0004);
    }
    dark.bx([-0.0016, 0.041, -0.166], [0.0016, sight_y - 0.0045, -0.160], 0.0003);

    // the wooden handguard: a lower half round the barrel and an upper one over the gas tube, with a steel retainer
    wood.bx([-0.0215, -0.034, -0.385], [0.0215, 0.006, -0.205], 0.008);
    wood.bx([-0.0140, 0.004, -0.360], [0.0140, 0.026, -0.208], 0.006);
    for k in 0..3 {
        let z = -0.330 + k as f32 * 0.035;
        dark.bx([-0.0145, 0.014, z], [0.0145, 0.020, z + 0.010], 0.002);
    }
    steel.bx([-0.0225, -0.036, -0.215], [0.0225, 0.010, -0.203], 0.002);
    steel.bx([-0.0225, -0.036, -0.392], [0.0225, 0.008, -0.383], 0.002);

    // barrel, gas tube and gas block, the front sight with its hood, and the slant brake
    steel.cyl([0.0, 0.0, -0.610], [0.0, 0.0, -0.385], 0.0082, 0.0082, 18);
    steel.cyl([0.0, 0.022, -0.440], [0.0, 0.022, -0.360], 0.0080, 0.0080, 16);
    steel.bx([-0.0100, -0.010, -0.465], [0.0100, 0.030, -0.440], 0.002);
    steel.bx([-0.0100, -0.012, -0.590], [0.0100, 0.012, -0.565], 0.002);
    steel.bx([-0.0018, 0.012, -0.584], [0.0018, sight_y, -0.578], 0.0004);
    dark.tube([0.0, sight_y - 0.003, -0.589], [0.0, sight_y - 0.003, -0.574], 0.0105, 0.0090, 18);
    dark.bx([-0.0060, 0.012, -0.589], [0.0060, sight_y - 0.012, -0.574], 0.0008);
    dark.cyl([0.0, 0.0, -0.645], [0.0, 0.0, -0.610], 0.0105, 0.0098, 18);
    dark.bx([-0.0035, 0.0096, -0.640], [0.0035, 0.0110, -0.618], 0.0003);
    dark.cyl([0.0, 0.0, -0.6455], [0.0, 0.0, -0.6450], 0.0052, 0.0052, 14);

    // the wooden stock, dropping toward the butt so it sits under the eye, with a steel butt plate
    taper(&mut wood, 0.100, 0.0150, [-0.044, 0.012], 0.345, 0.0185, [-0.112, -0.008], 0.005);
    steel.bx([-0.0195, -0.115, 0.345], [0.0195, -0.004, 0.352], 0.002);
    dark.bx([-0.0188, -0.070, 0.180], [0.0188, -0.058, 0.205], 0.001);

    let body = vec![
        piece(recv, STEEL, [0.13, 0.13, 0.14], 0.65),
        piece(steel, STEEL, [0.16, 0.16, 0.17], 0.8),
        piece(wood, WOOD, RED_WOOD, 0.4),
        piece(grip, WOOD, RED_WOOD, 0.4),
        piece(guard, STEEL, [0.13, 0.13, 0.14], 0.7),
        piece(dark, RUBBER, BLACK, 0.3),
    ];
    Gun {
        body,
        mag: ak_magazine(),
        trigger: rifle_trigger(),
        action: Vec::new(),
        spec: Spec {
            muzzle_z: -0.645,
            sight_y,
            mag_out: [0.0, -0.22, -0.02],
            mag_pivot: [0.0, -0.06, -0.07],
            mag_tilt: 0.45,
            left_at: [0.0, -0.007, -0.03],
            flip: 0.09,
            ..SPEC
        },
    }
}

/// The AK's steel-reinforced magazine, curving far forward, with ribs down its sides.
fn ak_magazine() -> Vec<Piece> {
    let (mut m, mut ribs, mut lips) = (Mesh::default(), Mesh::default(), Mesh::default());
    let (seg_len, mut a, mut c) = (0.030f32, 0.0f32, [0.0f32, -0.058, -0.070]);
    for k in 0..6 {
        let (hw, hd) = (0.0135 - 0.0002 * k as f32, 0.0205);
        let mut cs = corners([-hw, -seg_len, -hd], [hw, 0.0, hd]);
        for p in &mut cs {
            *p = add(xr(*p, a), c);
        }
        m.hex(cs, 0.0014);
        let mut rib = corners([-hw - 0.001, -seg_len * 0.8, -0.004], [hw + 0.001, -seg_len * 0.2, 0.004]);
        for p in &mut rib {
            *p = add(xr(*p, a), c);
        }
        ribs.hex(rib, 0.0008);
        c = add(c, xr([0.0, -seg_len * 0.97, 0.0], a));
        a += 0.085;
    }
    lips.bx([-0.0120, -0.058, -0.086], [0.0120, -0.050, -0.054], 0.0008);
    vec![piece(m, POLYMER, [0.36, 0.15, 0.06], 0.45), piece(ribs, POLYMER, [0.30, 0.12, 0.05], 0.4), piece(lips, STEEL, [0.18, 0.18, 0.19], 0.8)]
}

// ---- Uzi --------------------------------------------------------------------------------------

/// A stubby boxy 9mm: a stamped receiver with a ribbed top cover and its cocking knob, sights in protective ears
/// at both ends, a stub of barrel with its nut, and the magazine loaded up through the grip.
fn uzi() -> Gun {
    let sight_y = 0.052;
    let (mut recv, mut cover, mut steel, mut dark, mut grip, mut guard) =
        (Mesh::default(), Mesh::default(), Mesh::default(), Mesh::default(), Mesh::default(), Mesh::default());

    // the receiver box, pressed ribs down its sides, and the top cover with its grooves and the cocking knob
    recv.bx([-0.0200, -0.045, -0.165], [0.0200, 0.024, 0.140], 0.003);
    for side in [-1.0f32, 1.0] {
        dark.bx([side * 0.0200 - 0.0006, -0.026, -0.150], [side * 0.0200 + 0.0006, -0.020, 0.120], 0.0003);
        dark.bx([side * 0.0200 - 0.0006, 0.004, -0.150], [side * 0.0200 + 0.0006, 0.008, 0.120], 0.0003);
    }
    cover.bx([-0.0195, 0.022, -0.160], [0.0195, 0.032, 0.135], 0.004);
    for k in 0..9 {
        let z = -0.140 + k as f32 * 0.016;
        dark.bx([-0.0170, 0.0315, z], [0.0170, 0.0325, z + 0.006], 0.0003);
    }
    steel.bx([-0.0035, 0.030, 0.010], [0.0035, 0.038, 0.034], 0.0015);
    dark.cyl([0.0, 0.038, 0.022], [0.0, 0.044, 0.022], 0.0072, 0.0068, 14);
    // the grip frame and trigger housing under it, and the selector on the left above the grip
    recv.bx([-0.0170, -0.062, -0.080], [0.0170, -0.044, 0.135], 0.002);
    steel.bx([-0.0215, -0.040, 0.050], [-0.0195, -0.030, 0.072], 0.0006);
    trigger_guard(&mut guard);
    pistol_grip(&mut grip, 0.0);
    // the grip safety on the back of the grip
    steel.bx_rot([-0.0080, -0.130, 0.105], [0.0080, -0.066, 0.114], 0.0015, [0.0, -0.1, 0.11], -0.29);

    // sights: a post in ears up front, an aperture in ears at the back
    steel.bx([-0.0120, 0.032, -0.160], [0.0120, 0.038, -0.140], 0.0012);
    steel.bx([-0.0016, 0.038, -0.153], [0.0016, sight_y, -0.147], 0.0003);
    for side in [-1.0f32, 1.0] {
        steel.bx([side * 0.0090 - 0.0018, 0.038, -0.156], [side * 0.0090 + 0.0018, sight_y + 0.004, -0.144], 0.0006);
    }
    steel.bx([-0.0120, 0.032, 0.110], [0.0120, 0.038, 0.132], 0.0012);
    steel.bx([-0.0040, 0.038, 0.116], [0.0040, sight_y - 0.0045, 0.124], 0.0005);
    dark.tube([0.0, sight_y, 0.116], [0.0, sight_y, 0.124], 0.0060, 0.0028, 16);
    for side in [-1.0f32, 1.0] {
        steel.bx([side * 0.0095 - 0.0018, 0.038, 0.114], [side * 0.0095 + 0.0018, sight_y + 0.007, 0.126], 0.0006);
    }

    // the barrel nut and the stub of barrel past it
    steel.cyl([0.0, 0.0, -0.182], [0.0, 0.0, -0.165], 0.0150, 0.0150, 20);
    for k in 0..8 {
        let a = k as f32 / 8.0 * TAU;
        let (s, c) = a.sin_cos();
        dark.bx([c * 0.0150 - 0.0012, s * 0.0150 - 0.0012, -0.181], [c * 0.0150 + 0.0012, s * 0.0150 + 0.0012, -0.166], 0.0003);
    }
    steel.cyl([0.0, 0.0, -0.228], [0.0, 0.0, -0.182], 0.0078, 0.0078, 16);
    dark.cyl([0.0, 0.0, -0.2283], [0.0, 0.0, -0.2280], 0.0050, 0.0050, 14);
    // the stock's folded hinge block at the back
    steel.bx([-0.0160, -0.040, 0.140], [0.0160, 0.016, 0.152], 0.002);

    let body = vec![
        piece(recv, ANODISED, [0.13, 0.13, 0.14], 0.55),
        piece(cover, ANODISED, [0.11, 0.11, 0.12], 0.55),
        piece(steel, STEEL, [0.16, 0.16, 0.17], 0.8),
        piece(grip, POLYMER, [0.08, 0.08, 0.09], 0.4),
        piece(guard, ANODISED, [0.13, 0.13, 0.14], 0.6),
        piece(dark, RUBBER, BLACK, 0.3),
    ];
    Gun {
        body,
        // straight up the grip, leaning back with it
        mag: box_mag(0.0108, -0.060, -0.200, 0.066, 0.094, -0.29, [0.10, 0.10, 0.11]),
        trigger: rifle_trigger(),
        action: Vec::new(),
        spec: Spec {
            muzzle_z: -0.228,
            sight_y,
            mag_out: [0.0, -0.20, 0.05],
            mag_pivot: [0.0, -0.13, 0.08],
            mag_tilt: 0.0,
            left_at: [0.0, -0.016, 0.165],
            flip: 0.06,
            ..SPEC
        },
    }
}

// ---- double barrel ----------------------------------------------------------------------------

/// A side-by-side: two blued barrels with a rib and a bead between them, a case-hardened action with its top
/// lever, two triggers, and walnut. The barrels and forend are the part that breaks open, the shells ride in them.
fn double_barrel() -> Gun {
    let sight_y = 0.020;
    let pivot = [0.0, -0.030, -0.034];
    let (mut action_body, mut steel, mut dark, mut wood, mut grip, mut guard) =
        (Mesh::default(), Mesh::default(), Mesh::default(), Mesh::default(), Mesh::default(), Mesh::default());
    const WALNUT: [f32; 3] = [0.25, 0.13, 0.06];

    // the action: a block behind the breech, rounded under, with the top lever, safety and the hinge pin
    action_body.bx([-0.0225, -0.062, -0.034], [0.0225, 0.014, 0.080], 0.004);
    action_body.cyl([-0.0225, pivot[1], pivot[2]], [0.0225, pivot[1], pivot[2]], 0.0075, 0.0075, 16);
    steel.bx([-0.0040, 0.0135, 0.040], [0.0040, 0.0175, 0.082], 0.0012);
    steel.bx_rot([0.0000, 0.0135, 0.074], [0.0200, 0.0185, 0.086], 0.0015, [0.0, 0.015, 0.08], 0.0);
    steel.bx([-0.0050, 0.0135, 0.090], [0.0050, 0.0175, 0.104], 0.0012);
    dark.cyl([-0.0228, pivot[1], pivot[2]], [-0.0232, pivot[1], pivot[2]], 0.0040, 0.0040, 12);
    trigger_guard(&mut guard);
    pistol_grip(&mut grip, 0.0);

    // walnut stock, dropping well under the low rib so it stays out of the way of the bead, with a black pad
    taper(&mut wood, 0.080, 0.0175, [-0.060, -0.012], 0.345, 0.0190, [-0.128, -0.046], 0.006);
    dark.bx([-0.0200, -0.131, 0.345], [0.0200, -0.043, 0.360], 0.003);

    // the barrels and everything fixed to them: breech face, rib and bead, the lump under the hinge, and the forend
    let (mut barrels, mut rib, mut fore, mut bead) = (Mesh::default(), Mesh::default(), Mesh::default(), Mesh::default());
    let mut fore_dark = Mesh::default();
    for x in [-0.0108f32, 0.0108] {
        barrels.cyl([x, 0.0, -0.665], [x, 0.0, -0.036], 0.0108, 0.0112, 22);
        fore_dark.cyl([x, 0.0, -0.6653], [x, 0.0, -0.6650], 0.0086, 0.0086, 18);
    }
    barrels.bx([-0.0215, -0.012, -0.090], [0.0215, 0.010, -0.036], 0.003);
    barrels.bx([-0.0100, -0.040, -0.075], [0.0100, -0.010, -0.036], 0.002);
    rib.bx([-0.0050, 0.0080, -0.660], [0.0050, 0.0150, -0.040], 0.0012);
    rib.bx([-0.0060, -0.0150, -0.660], [0.0060, -0.0080, -0.110], 0.0012);
    bead.cyl([0.0, 0.0150, -0.652], [0.0, sight_y - 0.0025, -0.652], 0.0016, 0.0016, 10);
    bead.cyl([0.0, sight_y - 0.0025, -0.652], [0.0, sight_y, -0.652], 0.0024, 0.0006, 10);
    fore.bx([-0.0215, -0.034, -0.330], [0.0215, -0.006, -0.110], 0.007);
    fore_dark.bx([-0.0060, -0.0355, -0.140], [0.0060, -0.0325, -0.115], 0.0008);

    // a shell, one in each chamber: a brass head at the breech and a red hull inside
    let (mut hull, mut head) = (Mesh::default(), Mesh::default());
    hull.cyl([0.0, 0.0, -0.0755], [0.0, 0.0, -0.0055], 0.0090, 0.0090, 16);
    head.cyl([0.0, 0.0, -0.0055], [0.0, 0.0, 0.0], 0.0100, 0.0100, 16);

    let body = vec![
        piece(action_body, STEEL, [0.30, 0.28, 0.25], 0.8),
        piece(steel, STEEL, BLUED, 0.85),
        piece(wood, WOOD, WALNUT, 0.45),
        piece(grip, WOOD, WALNUT, 0.45),
        piece(guard, STEEL, BLUED, 0.8),
        piece(dark, RUBBER, BLACK, 0.3),
    ];
    let mut trig = Mesh::default();
    trig.bx([-0.0022, -0.064, -0.0345], [0.0022, -0.040, -0.0275], 0.0006);
    trig.bx([-0.0022, -0.066, -0.0115], [0.0022, -0.040, -0.0045], 0.0006);
    Gun {
        body,
        mag: vec![piece(hull, POLYMER, [0.55, 0.07, 0.05], 0.5), piece(head, STEEL, BRASS, 0.9)],
        trigger: vec![piece(trig, STEEL, [0.28, 0.28, 0.30], 0.8)],
        action: vec![
            piece(barrels, STEEL, BLUED, 0.9),
            piece(rib, STEEL, BLUED, 0.85),
            piece(fore, WOOD, WALNUT, 0.45),
            piece(fore_dark, RUBBER, BLACK, 0.3),
            glow(bead, [0.95, 0.9, 0.75]),
        ],
        spec: Spec {
            muzzle_z: -0.665,
            sight_y,
            action: Action::Break,
            left_at: [0.0, -0.013, 0.04],
            // held a little further out, so aiming along the low rib the stock's wrist isn't right under the eye
            hold: [0.0, 0.0, -0.06],
            flip: 0.16,
            pivot,
            // the extractors ease the shells out as the barrels drop, then the ejectors kick the spent ones up and
            // back over the shoulder; an unfired one stays in
            rounds: Some(Rounds {
                chambers: &[[0.0108, 0.0, -0.0345], [-0.0108, 0.0, -0.0345]],
                parts: 2,
                case: 2,
                radius: 0.0100,
                pull: 0.12,
                out: 0.015,
                eject: 0.21,
                all: false,
                throw: [0.5, 2.0, 0.9],
                flick: 0.18,
                // one shell and then the other, each up from under the breech lying across, in line with its nose
                // at the mouth, and in
                fill: 0.38,
                stagger: 0.24,
                load: 0.17,
                from: [0.0, -0.07, 0.16],
                tilt: 1.2,
                entry: [0.0, 0.0, 0.085],
                loader: None,
                hand: [0.0, -0.045, 0.27],
            }),
            ..SPEC
        },
    }
}

// ---- Mosin ------------------------------------------------------------------------------------

/// A long wooden bolt gun: round receiver, a straight bolt handle with a ball knob, a hooded front post and a
/// tangent rear sight, barrel bands, and a full-length stock with a handguard over the barrel.
fn mosin() -> Gun {
    let sight_y = 0.030;
    let (mut recv, mut steel, mut dark, mut wood, mut grip, mut guard) =
        (Mesh::default(), Mesh::default(), Mesh::default(), Mesh::default(), Mesh::default(), Mesh::default());
    const BIRCH: [f32; 3] = [0.33, 0.18, 0.08];

    // the round receiver with its flat sides and the ejection port, and the box magazine ahead of the guard
    recv.cyl([0.0, 0.0, -0.150], [0.0, 0.0, 0.040], 0.0150, 0.0150, 24);
    recv.bx([-0.0130, -0.024, -0.150], [0.0130, -0.004, 0.040], 0.002);
    dark.bx([0.0120, 0.002, -0.060], [0.0152, 0.011, 0.010], 0.0004);
    steel.bx([-0.0135, -0.088, -0.120], [0.0135, -0.060, -0.050], 0.003);
    trigger_guard(&mut guard);
    pistol_grip(&mut grip, 0.0);

    // the stock: a long forestock up to the nose cap, the body under the action, and the butt dropping away
    wood.bx([-0.0190, -0.044, -0.640], [0.0190, -0.002, -0.150], 0.007);
    wood.bx([-0.0190, -0.062, -0.155], [0.0190, -0.006, 0.065], 0.006);
    taper(&mut wood, 0.060, 0.0170, [-0.064, -0.012], 0.360, 0.0190, [-0.130, -0.036], 0.006);
    steel.bx([-0.0195, -0.133, 0.360], [0.0195, -0.033, 0.368], 0.002);
    // the handguard over the barrel, and the steel bands holding it all together
    wood.bx([-0.0130, -0.002, -0.470], [0.0130, 0.016, -0.275], 0.005);
    for (z, w) in [(-0.485f32, 0.010f32), (-0.635, 0.012)] {
        steel.bx([-0.0205, -0.046, z], [0.0205, 0.018, z + w], 0.003);
    }
    steel.bx([-0.0200, -0.046, -0.655], [0.0200, 0.010, -0.640], 0.003);

    // the barrel, stepping down toward the muzzle, and the bore
    steel.cyl([0.0, 0.0, -0.360], [0.0, 0.0, -0.150], 0.0110, 0.0110, 20);
    steel.cyl([0.0, 0.0, -0.885], [0.0, 0.0, -0.360], 0.0082, 0.0094, 20);
    dark.cyl([0.0, 0.0, -0.8853], [0.0, 0.0, -0.8850], 0.0045, 0.0045, 14);
    // the front post in its hood, at the muzzle
    steel.bx([-0.0060, 0.006, -0.880], [0.0060, 0.012, -0.858], 0.0012);
    steel.bx([-0.0014, 0.012, -0.873], [0.0014, sight_y, -0.866], 0.0003);
    dark.tube([0.0, sight_y - 0.002, -0.881], [0.0, sight_y - 0.002, -0.857], 0.0110, 0.0094, 18);
    // the tangent rear sight: a base on the barrel, and the leaf with its notch
    steel.bx([-0.0120, 0.008, -0.270], [0.0120, 0.018, -0.200], 0.002);
    steel.bx([-0.0090, 0.018, -0.226], [0.0090, sight_y - 0.006, -0.208], 0.001);
    for side in [-1.0f32, 1.0] {
        dark.bx([side * 0.0050 - 0.0030, sight_y - 0.006, -0.216], [side * 0.0050 + 0.0030, sight_y + 0.001, -0.210], 0.0004);
    }

    // the bolt: its body in the receiver, the cocking piece behind, and the handle straight out with a ball
    let (mut bolt, mut knob) = (Mesh::default(), Mesh::default());
    bolt.cyl([0.0, 0.0, 0.034], [0.0, 0.0, 0.074], 0.0110, 0.0110, 18);
    bolt.cyl([0.0, 0.0, 0.074], [0.0, 0.0, 0.094], 0.0080, 0.0070, 14);
    bolt.cyl([0.010, 0.0, 0.050], [0.052, -0.006, 0.056], 0.0038, 0.0034, 12);
    knob.cyl([0.050, -0.006, 0.0558], [0.0640, -0.008, 0.0578], 0.0085, 0.0070, 16);

    let body = vec![
        piece(recv, STEEL, BLUED, 0.85),
        piece(steel, STEEL, [0.12, 0.12, 0.13], 0.85),
        piece(wood, WOOD, BIRCH, 0.4),
        piece(grip, WOOD, BIRCH, 0.4),
        piece(guard, STEEL, BLUED, 0.8),
        piece(dark, RUBBER, BLACK, 0.3),
    ];
    Gun {
        body,
        mag: Vec::new(),
        trigger: rifle_trigger(),
        action: vec![piece(bolt, STEEL, [0.30, 0.30, 0.32], 0.9), piece(knob, STEEL, [0.26, 0.26, 0.28], 0.9)],
        spec: Spec {
            muzzle_z: -0.885,
            sight_y,
            action: Action::Bolt,
            left_at: [0.0, -0.020, -0.03],
            flip: 0.11,
            ..SPEC
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
            ..SPEC
        },
    }
}

// ---- revolver ---------------------------------------------------------------------------------

/// A six-inch magnum: a blued frame round a fluted six-shot cylinder, a barrel with a vented rib and a full
/// underlug, a ramp front sight with a red insert, the hammer, and walnut grips. The cylinder is the part that turns.
fn revolver() -> Gun {
    let sight_y = 0.0195;
    let up = PISTOL_GRIP_UP;
    let axis = [0.0, -0.0125, 0.0];
    let (mut frame, mut steel, mut dark, mut grip, mut red) =
        (Mesh::default(), Mesh::default(), Mesh::default(), Mesh::default(), Mesh::default());

    // the frame: the front the barrel screws into, the top strap and the bottom of the window the cylinder sits in,
    // the recoil shield behind it and the frame back to the grip, with a rounded trigger guard
    frame.bx([-0.0105, -0.034, -0.066], [0.0105, 0.0135, -0.043], 0.002);
    frame.bx([-0.0085, 0.006, -0.046], [0.0085, 0.0145, 0.024], 0.002);
    frame.bx([-0.0105, -0.041, -0.050], [0.0105, -0.032, 0.022], 0.002);
    frame.bx([-0.0120, -0.041, 0.012], [0.0120, 0.012, 0.034], 0.003);
    frame.bx([-0.0110, -0.046, 0.030], [0.0110, 0.004, 0.094], 0.004);
    frame.bx([-0.0055, -0.064, -0.048], [0.0055, -0.060, 0.052], 0.0015);
    frame.bx([-0.0055, -0.064, -0.050], [0.0055, -0.040, -0.044], 0.0015);
    // the cylinder latch on the left, the hammer and its spur, and the screws
    steel.bx([-0.0128, -0.012, 0.022], [-0.0110, -0.003, 0.040], 0.0006);
    steel.bx([-0.0035, -0.004, 0.034], [0.0035, 0.016, 0.047], 0.0012);
    steel.bx_rot([-0.0035, 0.012, 0.042], [0.0035, 0.019, 0.064], 0.0015, [0.0, 0.015, 0.045], -0.25);
    for (y, z) in [(-0.020f32, 0.060f32), (-0.030, -0.054)] {
        dark.cyl([-0.0112, y, z], [0.0112, y, z], 0.0018, 0.0018, 10);
    }
    pistol_grip(&mut grip, up);
    for side in [-1.0f32, 1.0] {
        dark.cyl([side * 0.0164, -0.085 + up, 0.104], [side * 0.0170, -0.085 + up, 0.104], 0.0040, 0.0040, 12);
    }

    // the barrel with its rib on top, vents in the rib, the underlug, the bore and the front sight
    steel.cyl([0.0, 0.0, -0.205], [0.0, 0.0, -0.066], 0.0085, 0.0085, 20);
    steel.bx([-0.0045, 0.0045, -0.205], [0.0045, 0.0120, -0.066], 0.0012);
    for k in 0..5 {
        let z = -0.190 + k as f32 * 0.025;
        dark.bx([-0.0047, 0.0070, z], [0.0047, 0.0110, z + 0.012], 0.0003);
    }
    steel.bx([-0.0072, -0.033, -0.205], [0.0072, -0.004, -0.066], 0.0025);
    dark.cyl([0.0, 0.0, -0.2053], [0.0, 0.0, -0.2050], 0.0046, 0.0046, 14);
    dark.cyl([0.0, -0.022, -0.2053], [0.0, -0.022, -0.2050], 0.0030, 0.0030, 10);
    steel.bx([-0.0014, 0.012, -0.199], [0.0014, sight_y, -0.184], 0.0003);
    red.bx([-0.0015, sight_y - 0.004, -0.1872], [0.0015, sight_y - 0.0005, -0.1840], 0.0002);
    // the rear sight: a notch in a block on the top strap
    for side in [-1.0f32, 1.0] {
        steel.bx([side * 0.0035 - 0.0018, 0.0145, 0.010], [side * 0.0035 + 0.0018, sight_y, 0.020], 0.0004);
    }
    steel.bx([-0.0018, 0.0145, 0.010], [0.0018, sight_y - 0.0035, 0.020], 0.0003);

    // the cylinder: six chambers round the axis, flutes between them, the chamber mouths and the crane out the front
    let (mut cyl, mut cdark) = (Mesh::default(), Mesh::default());
    let c = |r: f32, a: f32, z: f32| [r * a.cos(), axis[1] + r * a.sin(), z];
    cyl.cyl(c(0.0, 0.0, -0.043), c(0.0, 0.0, 0.012), 0.0195, 0.0195, 30);
    for k in 0..6 {
        let a = std::f32::consts::FRAC_PI_2 + k as f32 / 6.0 * TAU;
        cdark.cyl(c(0.0125, a, -0.0433), c(0.0125, a, -0.0430), 0.0046, 0.0046, 12);
        cdark.cyl(c(0.0187, a + TAU / 12.0, -0.034), c(0.0187, a + TAU / 12.0, 0.004), 0.0030, 0.0030, 8);
    }
    cyl.cyl(c(0.0, 0.0, -0.050), c(0.0, 0.0, -0.043), 0.0045, 0.0045, 12);

    // a round, one in each chamber: the rimmed brass case, its primer, and the copper bullet in its mouth
    let (mut case, mut primer, mut bullet) = (Mesh::default(), Mesh::default(), Mesh::default());
    case.cyl([0.0, 0.0, -0.0012], [0.0, 0.0, 0.0004], 0.0058, 0.0058, 14);
    case.cyl([0.0, 0.0, -0.0330], [0.0, 0.0, -0.0012], 0.0047, 0.0047, 14);
    primer.cyl([0.0, 0.0, 0.0004], [0.0, 0.0, 0.0006], 0.0019, 0.0019, 10);
    bullet.cyl([0.0, 0.0, -0.0400], [0.0, 0.0, -0.0330], 0.0045, 0.0042, 14);
    bullet.cyl([0.0, 0.0, -0.0445], [0.0, 0.0, -0.0400], 0.0018, 0.0042, 14);
    // the speedloader they come in on: a black body gripping the six rims, and the knurled knob that lets them go
    let loader_at = [0.0, axis[1], 0.0124];
    let (mut loader, mut knob) = (Mesh::default(), Mesh::default());
    loader.cyl([0.0, 0.0, 0.0006], [0.0, 0.0, 0.0080], 0.0195, 0.0185, 30);
    knob.cyl([0.0, 0.0, 0.0080], [0.0, 0.0, 0.0200], 0.0075, 0.0075, 18);
    for k in 0..12 {
        let (s, c) = (k as f32 / 12.0 * TAU).sin_cos();
        knob.cyl([0.0075 * c, 0.0075 * s, 0.0082], [0.0075 * c, 0.0075 * s, 0.0198], 0.0011, 0.0011, 6);
    }

    let body = vec![
        piece(frame, STEEL, BLUED, 0.9),
        piece(steel, STEEL, [0.11, 0.11, 0.13], 0.9),
        piece(grip, WOOD, [0.27, 0.13, 0.06], 0.5),
        piece(dark, RUBBER, BLACK, 0.3),
        glow(red, [1.0, 0.15, 0.08]),
    ];
    let mut trig = Mesh::default();
    trig.bx([-0.0028, -0.056, -0.036], [0.0028, -0.040, -0.029], 0.0008);
    Gun {
        body,
        mag: vec![
            piece(case, STEEL, BRASS, 0.9),
            piece(primer, STEEL, [0.62, 0.60, 0.58], 0.9),
            piece(bullet, STEEL, [0.62, 0.34, 0.20], 0.8),
            piece(loader, POLYMER, [0.05, 0.05, 0.055], 0.4),
            piece(knob, STEEL, [0.20, 0.20, 0.22], 0.8),
        ],
        trigger: vec![piece(trig, STEEL, [0.22, 0.22, 0.24], 0.8)],
        action: vec![piece(cyl, STEEL, BLUED, 0.9), piece(cdark, RUBBER, BLACK, 0.3)],
        spec: Spec {
            muzzle_z: -0.2053,
            sight_y,
            action: Action::Cylinder,
            trigger_pivot: [0.0, -0.040, -0.032],
            right_at: [0.0, up, 0.0],
            pistol_hold: true,
            hold: [0.0, 0.03, -0.17],
            flip: 0.32,
            pivot: axis,
            swing: [-0.0105, -0.032, 0.0],
            // muzzle up, the ejector rod pushes all six out together and they drop free
            rounds: Some(Rounds {
                // round the axis from the top, the way the cylinder turns them under the hammer
                chambers: &[
                    [0.0, 0.0, 0.0124],
                    [-0.010825, -0.00625, 0.0124],
                    [-0.010825, -0.01875, 0.0124],
                    [0.0, -0.0250, 0.0124],
                    [0.010825, -0.01875, 0.0124],
                    [0.010825, -0.00625, 0.0124],
                ],
                parts: 3,
                case: 2,
                radius: 0.0058,
                pull: 0.22,
                out: 0.022,
                eject: 0.30,
                all: true,
                throw: [0.0, 0.0, 0.7],
                flick: 0.75,
                // all six on the speedloader, up from below turned so they show, in line with the bullets' noses at
                // the chamber mouths, and in
                fill: 0.36,
                stagger: 0.0,
                load: 0.30,
                // in the swung out cylinder's frame, which is turned most of a quarter about the bore: this is up
                // from below and out to the left, the side it swings out to, turned about the upright to show them
                from: [-0.083, 0.039, 0.12],
                tilt: 0.9,
                entry: [0.0, 0.0, 0.05],
                loader: Some(loader_at),
                hand: [-0.035, 0.05, -0.01],
            }),
            ..SPEC
        },
    }
}

// ---- Desert Eagle -----------------------------------------------------------------------------

/// A big gas-operated magnum: a long frame, a fixed barrel with its triangular top running into the slide's flat
/// top line, a slide with an exposed hammer and safeties, and a magazine in the grip. The slide is the part that
/// cycles, the barrel stays put.
fn deagle() -> Gun {
    let sight_y = 0.028;
    let up = PISTOL_GRIP_UP;
    let (mut frame, mut grip, mut dark, mut barrel, mut dots) =
        (Mesh::default(), Mesh::default(), Mesh::default(), Mesh::default(), Mesh::default());

    // frame: a long dust cover, the beavertail, a squared trigger guard with a hooked front, the slide stop
    frame.bx([-0.0145, -0.042, -0.122], [0.0145, -0.016, 0.098], 0.0025);
    frame.bx([-0.0130, -0.042, 0.090], [0.0130, -0.030, 0.118], 0.002);
    frame.bx([-0.0060, -0.066, -0.080], [0.0060, -0.061, 0.066], 0.0014);
    frame.bx([-0.0060, -0.068, -0.082], [0.0060, -0.042, -0.072], 0.0016);
    frame.bx([-0.0060, -0.072, -0.086], [0.0060, -0.066, -0.078], 0.0014);
    dark.bx([-0.0165, -0.026, -0.030], [-0.0145, -0.019, 0.010], 0.0005);
    dark.bx([-0.0180, -0.050, 0.052], [-0.0160, -0.041, 0.064], 0.0005);
    pistol_grip(&mut grip, up);
    for side in [-1.0f32, 1.0] {
        let x = side * 0.0168;
        grip.bx_rot([x - 0.0006, -0.130 + up, 0.090], [x + 0.0006, -0.060 + up, 0.112], 0.0003, [0.0, -0.095 + up, 0.1], -0.3);
    }

    // the fixed barrel: a block over the frame and a top that narrows to a flat with a groove down it, the bore,
    // and the front post with its dot
    let (z0, z1) = (-0.168f32, -0.020f32);
    barrel.bx([-0.0145, -0.016, z0], [0.0145, 0.007, z1], 0.002);
    barrel.hex(
        [
            [-0.0145, 0.006, z0], [0.0145, 0.006, z0], [-0.0070, 0.021, z0], [0.0070, 0.021, z0],
            [-0.0145, 0.006, z1], [0.0145, 0.006, z1], [-0.0070, 0.021, z1], [0.0070, 0.021, z1],
        ],
        0.0015,
    );
    dark.bx([-0.0018, 0.0205, -0.150], [0.0018, 0.0213, -0.030], 0.0003);
    dark.cyl([0.0, 0.0, z0 - 0.0003], [0.0, 0.0, z0], 0.0064, 0.0064, 16);
    barrel.bx([-0.0017, 0.021, -0.162], [0.0017, sight_y, -0.150], 0.0004);
    dots.bx([-0.0009, sight_y - 0.0035, -0.1500], [0.0009, sight_y - 0.0017, -0.1496], 0.0001);

    // the slide: serrations at the back, an ejection port, ambidextrous safeties, the rear sight, the hammer
    let (mut slide, mut sdark, mut sdots) = (Mesh::default(), Mesh::default(), Mesh::default());
    slide.bx([-0.0145, -0.016, -0.020], [0.0145, 0.021, 0.106], 0.0022);
    for k in 0..7 {
        let z = 0.060 + k as f32 * 0.0062;
        sdark.bx([-0.0147, -0.010, z], [0.0147, 0.012, z + 0.0026], 0.0003);
    }
    sdark.bx([0.0142, 0.002, -0.014], [0.0150, 0.017, 0.028], 0.0004);
    for side in [-1.0f32, 1.0] {
        slide.cyl([side * 0.0145, 0.004, 0.090], [side * 0.0185, 0.004, 0.090], 0.0042, 0.0042, 12);
        slide.bx([side * 0.0160 - 0.0012, 0.004, 0.084], [side * 0.0160 + 0.0012, 0.013, 0.096], 0.0005);
        slide.bx([side * 0.0050 - 0.0030, 0.021, 0.092], [side * 0.0050 + 0.0030, sight_y, 0.104], 0.0004);
        sdots.bx([side * 0.0050 - 0.0009, sight_y - 0.0035, 0.1040], [side * 0.0050 + 0.0009, sight_y - 0.0017, 0.1044], 0.0001);
    }
    slide.bx([-0.0080, 0.021, 0.092], [0.0080, sight_y - 0.0045, 0.104], 0.0004);
    sdark.bx([-0.0030, -0.008, 0.104], [0.0030, 0.012, 0.118], 0.0010);

    // the magazine, hidden in the grip until it drops
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

    // brushed stainless all over, with a black grip
    const SILVER: [f32; 3] = [0.62, 0.63, 0.66];
    let body = vec![
        piece(frame, STEEL, [0.56, 0.57, 0.60], 1.0),
        piece(grip, POLYMER, [0.07, 0.07, 0.075], 0.4),
        piece(barrel, STEEL, SILVER, 1.1),
        piece(dark, RUBBER, BLACK, 0.3),
        glow(dots, [0.85, 0.95, 0.85]),
    ];
    let mut trig = Mesh::default();
    trig.bx([-0.0028, -0.058, -0.038], [0.0028, -0.040, -0.030], 0.0008);
    Gun {
        body,
        mag: vec![piece(mag, STEEL, [0.16, 0.16, 0.17], 0.7), piece(plate, STEEL, [0.50, 0.51, 0.54], 0.9)],
        trigger: vec![piece(trig, STEEL, [0.50, 0.51, 0.54], 0.9)],
        action: vec![
            piece(slide, STEEL, SILVER, 1.1),
            piece(sdark, RUBBER, BLACK, 0.3),
            glow(sdots, [0.85, 0.95, 0.85]),
        ],
        spec: Spec {
            muzzle_z: z0 - 0.0003,
            sight_y,
            action: Action::Slide,
            trigger_pivot: [0.0, -0.040, -0.032],
            mag_out: [0.0, -0.15, 0.047],
            mag_pivot: [0.0, -0.09, 0.1],
            right_at: [0.0, up, 0.0],
            pistol_hold: true,
            hold: [0.0, 0.03, -0.17],
            flip: 0.65,
            ..SPEC
        },
    }
}

// ---- baseball bat -----------------------------------------------------------------------------

/// A 33-inch wooden bat's outline, as (z, radius) from the knob to the end of the barrel: the knob, the thin
/// handle, the long taper, the barrel, and the rounded end. Along the bore, so the end of the barrel is its muzzle.
const BAT: [(f32, f32); 20] = [
    (0.016, 0.0), (0.016, 0.017), (0.0145, 0.0235), (0.011, 0.0262), (0.006, 0.026), (0.001, 0.0215), (-0.005, 0.0152),
    (-0.014, 0.0127), (-0.20, 0.0129), (-0.30, 0.0146), (-0.38, 0.0182), (-0.46, 0.0242), (-0.53, 0.0294),
    (-0.59, 0.0324), (-0.66, 0.0335), (-0.79, 0.0336), (-0.818, 0.0329), (-0.831, 0.0298), (-0.839, 0.021),
    (-0.8415, 0.0),
];
/// Where the hands hold it: the left (bottom) hand just over the knob, the right above it.
pub const BAT_LEFT_Z: f32 = -0.054;
pub const BAT_RIGHT_Z: f32 = -0.137;
/// Where the bat meets what it is swung at, so the blood is where it would be: the sweet spot of the barrel.
const BAT_SWEET: (f32, f32) = (-0.78, -0.58);
const ASH: [f32; 3] = [0.74, 0.57, 0.36];

/// The bat's radius at `z`, along its outline.
fn bat_r(z: f32) -> f32 {
    let i = BAT.iter().position(|p| p.0 <= z).unwrap_or(BAT.len() - 1).max(1);
    let ((z0, r0), (z1, r1)) = (BAT[i - 1], BAT[i]);
    r0 + (r1 - r0) * ((z - z0) / (z1 - z0)).clamp(0.0, 1.0)
}

/// The outline from `z0` down to `z1`, a little proud of the wood, for what is wrapped round or painted on it.
fn bat_band(z0: f32, z1: f32, proud: f32) -> Vec<(f32, f32)> {
    let mut p = vec![(z0, bat_r(z0) - 0.001), (z0, bat_r(z0) + proud)];
    p.extend(BAT.iter().filter(|q| q.0 < z0 && q.0 > z1).map(|q| (q.0, q.1 + proud)));
    p.extend([(z1, bat_r(z1) + proud), (z1, bat_r(z1) - 0.001)]);
    p
}

/// A wooden baseball bat: pale lacquered ash with its grain, the end of the barrel dipped black, a maker's band
/// burned in where the taper starts, and the handle wrapped in black grip tape, its ridges spiralling up to a
/// finishing band. It has no moving parts; the hands round it come from `bat_hands`.
fn baseball_bat() -> Gun {
    let mut wood = Mesh::default();
    wood.lathe(&BAT, 28);
    // the dipped end: lacquer from a little short of the end, round over it
    let mut dip = Mesh::default();
    let mut end = vec![(-0.74, bat_r(-0.74) - 0.001), (-0.74, bat_r(-0.74) + 0.0006)];
    end.extend(BAT.iter().filter(|q| q.0 < -0.74).map(|q| (q.0, q.1 + if q.1 > 0.0 { 0.0006 } else { 0.0 })));
    dip.lathe(&end, 28);
    // the maker's band: two thin burned rings and a wider one between them
    let mut brand = Mesh::default();
    for (z0, z1) in [(-0.497, -0.500), (-0.504, -0.516), (-0.520, -0.523)] {
        brand.lathe(&bat_band(z0, z1, 0.0003), 28);
    }
    // grip tape over the knob and up the handle, and its finishing band
    let (mut tape, mut ridge) = (Mesh::default(), Mesh::default());
    let mut over: Vec<(f32, f32)> = BAT.iter().filter(|q| q.0 > -0.014).map(|q| (q.0 + 0.0016, q.1 + if q.1 > 0.0 { 0.0016 } else { 0.0 })).collect();
    over.extend(bat_band(-0.014, -0.215, 0.0011).into_iter().skip(1));
    tape.lathe(&over, 24);
    tape.lathe(&bat_band(-0.215, -0.228, 0.0016), 24);
    let (turns, steps) = (9.0, 360);
    let mut last: Option<V> = None;
    for k in 0..=steps {
        let t = k as f32 / steps as f32;
        let (z, a) = (-0.012 - 0.2 * t, t * turns * TAU);
        let r = bat_r(z) + 0.0011;
        let p = [r * a.cos(), r * a.sin(), z];
        if let Some(q) = last {
            ridge.cyl(q, p, 0.0009, 0.0009, 5);
        }
        last = Some(p);
    }
    Gun {
        body: vec![
            piece(wood, WOOD, ASH, 0.5),
            piece(dip, POLYMER, [0.035, 0.03, 0.028], 0.7),
            piece(brand, POLYMER, [0.10, 0.06, 0.03], 0.2),
            piece(tape, RUBBER, [0.035, 0.035, 0.04], 0.15),
            piece(ridge, RUBBER, [0.06, 0.06, 0.065], 0.2),
        ],
        mag: Vec::new(),
        trigger: Vec::new(),
        action: Vec::new(),
        spec: Spec { muzzle_z: BAT[BAT.len() - 1].0, trigger_pivot: [0.0; 3], flip: 0.0, ..SPEC },
    }
}

/// One hand round the bat's handle at `zc`, in the bat's space: the back of the hand on the side `side` of x, the
/// forearm running off along +y, four fingers round under the handle from the knuckles, and the thumb over the top
/// to meet them. The left hand is the right one turned over, so both close the same way round, as on a real grip.
fn bat_hand_sdf(p: V, side: f32, zc: f32) -> f32 {
    let p = [p[0] * side, p[1], p[2] - zc];
    let rr = 0.0141 + 0.0092;
    // the palm and the back of the hand, running from the wrist down to the knuckles by the handle
    let mut d = ell(p, [0.025, 0.036, 0.0], [0.0155, 0.040, 0.040]);
    d = smin(d, cone(p, [0.026, 0.064, 0.004], [0.030, 0.104, 0.010], 0.024, 0.026), 0.02);
    // four fingers, the index nearest the barrel, each round the handle from its knuckle to its tip
    for (i, z) in [-0.029f32, -0.010, 0.009, 0.027].iter().enumerate() {
        let end = 3.75 - 0.12 * i as f32;
        let pts: Vec<V> = (0..7)
            .map(|k| 0.35 - end * k as f32 / 6.0)
            .map(|a: f32| {
                let r = rr + 0.004 * (1.0 - (a - 0.35).abs().min(1.0));
                [r * a.cos(), r * a.sin(), *z]
            })
            .collect();
        d = smin(d, finger(p, &pts, 0.0094 - 0.0004 * i as f32, 0.85), 0.007);
    }
    // the thumb, from the heel of the palm over the top of the handle to the fingertips
    let thumb: Vec<V> = [(0.022f32, 0.050f32, -0.030f32), (0.016, 0.030, -0.036), (0.004, rr + 0.003, -0.036), (-0.012, rr - 0.001, -0.034), (-0.022, 0.012, -0.031)]
        .iter()
        .map(|&(x, y, z)| [x, y, z])
        .collect();
    smin(d, finger(p, &thumb, 0.0104, 0.88), 0.008)
}

fn bat_sleeve_sdf(p: V, side: f32, zc: f32) -> f32 {
    cone([p[0] * side, p[1], p[2] - zc], [0.030, 0.100, 0.012], [0.075, 0.40, 0.09], 0.031, 0.05)
}

/// Both hands round the bat, and their sleeves (see `bat_hand_sdf`).
pub fn bat_hands() -> Vec<Piece> {
    let mut out = Vec::new();
    for (side, zc) in [(1.0f32, BAT_RIGHT_Z), (-1.0, BAT_LEFT_Z)] {
        // x from `a` to `b` on the right hand's side, turned over for the left
        let span = |a: f32, b: f32| ((a * side).min(b * side), (a * side).max(b * side));
        let (x0, x1) = span(-0.045, 0.065);
        let glove = smooth(&|p| bat_hand_sdf(p, side, zc), [x0, -0.045, zc - 0.06], [x1, 0.14, zc + 0.06], 0.0024);
        let (x0, x1) = span(0.0, 0.14);
        let sleeve = smooth(&|p| bat_sleeve_sdf(p, side, zc), [x0, 0.06, zc - 0.06], [x1, 0.46, zc + 0.16], 0.004);
        out.push(piece(glove, CLOTH, GLOVE, 0.12));
        out.push(piece(sleeve, CLOTH, SLEEVE, 0.0));
    }
    out
}

/// A small fixed scatter, the same every time: `n` numbers 0 to 1 from `seed`.
fn scatter(seed: u32, n: usize) -> Vec<f32> {
    let mut x = seed.wrapping_mul(2654435761).max(1);
    (0..n)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            (x >> 8) as f32 / 16_777_216.0
        })
        .collect()
}

/// A splash of blood on the bat at angle `a` round it and `z` along it, `half` across and along: a thin lens
/// wrapped round the wood so it lies on it all over, the inner half hidden in it.
fn splash(m: &mut Mesh, a: f32, z: f32, half: [f32; 2]) {
    const THICK: f32 = 0.0011;
    let (rings, seg) = (6, 12);
    let at = |i: usize, j: usize| {
        let (th, ph) = (i as f32 / rings as f32 * std::f32::consts::PI, j as f32 / seg as f32 * TAU);
        let k = [th.cos(), th.sin() * ph.cos(), th.sin() * ph.sin()];
        let zz = z + k[2] * half[1];
        let ang = a + k[1] * half[0] / bat_r(zz).max(0.01);
        let (out, round) = ([ang.cos(), ang.sin(), 0.0], [-ang.sin(), ang.cos(), 0.0]);
        let p = add(mulv(out, bat_r(zz) + 0.0002 + k[0] * THICK), [0.0, 0.0, zz]);
        let n = norm(add(add(mulv(out, k[0] / THICK), mulv(round, k[1] / half[0])), [0.0, 0.0, k[2] / half[1]]));
        (p, n)
    };
    for i in 0..rings {
        for j in 0..seg {
            let q = [at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1)];
            m.quad(q.map(|v| v.0), q.map(|v| v.1));
        }
    }
}

/// Blood on the bat's barrel, in coats, each over the last: a few spots, then smears, then most of the sweet spot
/// soaked and running down towards the handle. Drawn up to how bloodied it is (see `sim::Player::gore`). Each
/// splash is a flat lens lying on the wood, drawn out along the bat.
pub fn bat_blood() -> Vec<Piece> {
    let (z1, z0) = BAT_SWEET;
    // fresh and bright first, darker and browner as it builds up and dries
    const RED: [[f32; 3]; 4] = [[0.30, 0.015, 0.02], [0.24, 0.012, 0.015], [0.19, 0.012, 0.012], [0.14, 0.012, 0.01]];
    (0..4u32)
        .map(|coat| {
            let n = 22;
            let r = scatter(coat + 7, 5 * n);
            let mut m = Mesh::default();
            for k in 0..n {
                let (u, v, w, x, y) = (r[5 * k], r[5 * k + 1], r[5 * k + 2], r[5 * k + 3], r[5 * k + 4]);
                // round the side it strikes with, spreading wider and further down coat by coat, until it is all
                // the way round
                let a = -1.9 + (u - 0.5) * (2.6 + 1.3 * coat as f32);
                let z = z1 + (z0 - z1 + 0.07 * coat as f32) * v;
                // spatter and splashes, each flung round the bat or smeared along it
                let s = 0.002 + 0.009 * w * w + 0.0012 * coat as f32;
                let (across, along) = if y < 0.5 { (1.0 + 1.5 * x, 1.0) } else { (1.0, 1.0 + 2.0 * x) };
                // the last coat runs: long drips down towards the handle
                let along = if coat == 3 && y > 0.8 { 5.0 + 5.0 * x } else { along };
                splash(&mut m, a, z + s * (along - 1.0), [s * across, s * along]);
            }
            piece(m, 11.0, RED[coat as usize], 0.0)
        })
        .collect()
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

// ---- grenade ----------------------------------------------------------------------------------

/// A fragmentation grenade, in its own space about the middle of its body: +y is the fuze, and the spoon runs
/// down the +z side, which faces the eye in the hand. Grouped by what comes away: the pin is pulled out first and
/// the spoon flies off when it is thrown.
pub struct HandGrenade {
    pub body: Vec<Piece>,
    pub spoon: Vec<Piece>,
    pub pin: Vec<Piece>,
    /// the right hand round it, palm on its right, fingers over the front and the thumb holding the spoon down
    pub hand: Vec<Piece>,
}

/// The radius of the grenade's body.
pub const GRENADE_R: f32 = 0.032;
const OLIVE: [f32; 3] = [0.15, 0.17, 0.095];

/// An M67: a smooth steel ball painted olive with a yellow band under the fuze, the fuze's neck and head, the
/// striker's housing, the spoon bent over it and down the side, and a split pin with its pull ring.
pub fn grenade() -> HandGrenade {
    let r = GRENADE_R;
    let ball = smooth(&|p| len(p) - r, [-0.036; 3], [0.036; 3], 0.0016);
    // a thin shell standing proud of the paint, hollow so there is nothing to build inside the ball
    let band = smooth(&|p| ((len(p) - r - 0.0002).abs() - 0.0005).max((p[1] - 0.021).abs() - 0.0018), [-0.036, 0.016, -0.036], [0.036, 0.026, 0.036], 0.0008);
    let mut fuze = Mesh::default();
    fuze.cyl([0.0, 0.026, 0.0], [0.0, 0.036, 0.0], 0.0118, 0.0112, 20);
    fuze.cyl([0.0, 0.036, 0.0], [0.0, 0.0465, 0.0], 0.0094, 0.0090, 20);
    fuze.bx([-0.0075, 0.034, 0.003], [0.0075, 0.0465, 0.0165], 0.0012);
    let mut dark = Mesh::default();
    // the threads where the fuze screws in
    for k in 0..3 {
        let y = 0.0275 + k as f32 * 0.003;
        dark.cyl([0.0, y, 0.0], [0.0, y + 0.0012, 0.0], 0.0121, 0.0121, 20);
    }

    // the spoon: flat over the top of the fuze, down past the housing and bent round the body to below its middle
    let mut spoon = Mesh::default();
    spoon.bx([-0.0065, 0.0465, -0.007], [0.0065, 0.0485, 0.0195], 0.0006);
    spoon.bx([-0.0065, 0.029, 0.0175], [0.0065, 0.0485, 0.0195], 0.0006);
    for k in 0..6 {
        let a = 0.55 + k as f32 * 0.27;
        let w = 0.0065 - 0.0004 * k as f32;
        spoon.bx_rot([-w, r + 0.0015, -0.0052], [w, r + 0.0033, 0.0052], 0.0006, [0.0; 3], a);
    }

    // the pin through the fuze head, and its ring hanging off the left
    let mut pin = Mesh::default();
    pin.cyl([-0.016, 0.040, 0.004], [0.013, 0.040, 0.004], 0.0011, 0.0011, 8);
    let ring_at = [-0.019, 0.040, -0.008];
    let ring = smooth(
        &|p| {
            let d = [p[1] - ring_at[1], p[2] - ring_at[2]];
            let q = [(d[0] * d[0] + d[1] * d[1]).sqrt() - 0.0115, p[0] - ring_at[0]];
            (q[0] * q[0] + q[1] * q[1]).sqrt() - 0.0013
        },
        [-0.023, 0.026, -0.022],
        [-0.015, 0.054, 0.006],
        0.0006,
    );

    HandGrenade {
        body: vec![
            piece(ball, POLYMER, OLIVE, 0.3),
            piece(band, POLYMER, [0.62, 0.50, 0.08], 0.3),
            piece(fuze, STEEL, [0.30, 0.30, 0.28], 0.7),
            piece(dark, STEEL, [0.12, 0.12, 0.11], 0.5),
        ],
        spoon: vec![piece(spoon, POLYMER, [0.17, 0.19, 0.11], 0.45)],
        pin: vec![piece(pin, STEEL, [0.45, 0.45, 0.43], 0.9), piece(ring, STEEL, [0.45, 0.45, 0.43], 0.9)],
        hand: grenade_hand(),
    }
}

fn grenade_hand_sdf(p: V) -> f32 {
    let r = GRENADE_R;
    // the palm against the right of the ball, and the wrist behind it
    let mut d = ell(p, [0.041, -0.004, 0.006], [0.015, 0.038, 0.034]);
    d = smin(d, cone(p, [0.044, -0.028, 0.024], [0.056, -0.066, 0.062], 0.024, 0.026), 0.02);
    // four fingers from the knuckles at the front of the palm round the far side of the ball, the little one shortest
    for (i, y) in [0.019f32, 0.002, -0.015, -0.030].iter().enumerate() {
        let rr = (r * r - y * y).max(0.0).sqrt() + 0.0094;
        let end = if i == 3 { 2.1 } else { 2.5 - 0.1 * i as f32 };
        let pts: Vec<V> = (0..6).map(|k| -0.2 + (end + 0.2) * k as f32 / 5.0).map(|a: f32| [rr * a.cos(), *y, -rr * a.sin()]).collect();
        d = smin(d, finger(p, &pts, 0.0096 - 0.0005 * i as f32, 0.85), 0.007);
    }
    // the thumb round the near side and up over the spoon
    let thumb: Vec<V> = [(0.35f32, -0.020f32), (0.85, -0.010), (1.35, 0.000), (1.8, 0.008)]
        .iter()
        .map(|(a, y)| {
            let rr = ((r + 0.0033).powi(2) - y * y).max(0.0).sqrt() + 0.0102;
            [rr * a.cos(), *y, rr * a.sin()]
        })
        .collect();
    smin(d, finger(p, &thumb, 0.0108, 0.88), 0.01)
}

fn grenade_sleeve_sdf(p: V) -> f32 {
    cone(p, [0.060, -0.062, 0.058], [0.14, -0.20, 0.36], 0.031, 0.05)
}

fn grenade_hand() -> Vec<Piece> {
    let glove = smooth(&grenade_hand_sdf, [-0.05, -0.08, -0.05], [0.08, 0.04, 0.09], 0.0024);
    let sleeve = smooth(&grenade_sleeve_sdf, [0.0, -0.27, 0.0], [0.21, -0.02, 0.43], 0.004);
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

    /// A gun that shows its rounds one by one has a chamber for each it holds, and a case to leave when one fires.
    #[test]
    fn a_gun_that_shows_its_rounds_has_a_chamber_for_each() {
        for (g, w) in guns().iter().zip(WEAPONS.iter()) {
            let Some(r) = &g.spec.rounds else { continue };
            assert_eq!(r.chambers.len(), w.mag as usize, "{}", w.name);
            assert!(r.case > 0 && r.case <= r.parts && r.parts <= g.mag.len(), "{}", w.name);
            assert_eq!(r.loader.is_some(), r.parts < g.mag.len(), "{}: a speedloader is the pieces after a round", w.name);
            // all of them in before the gun starts to close (see `render::action_pose`)
            let n = r.chambers.len();
            assert!(r.pull < r.eject && r.eject < r.fill && r.loaded(n) <= 0.8, "{}'s reload is out of order", w.name);
        }
    }

    /// The left hand goes smoothly through a reload of rounds, however many were spent: from one round going in, it
    /// goes back down for the next rather than jumping there.
    #[test]
    fn the_hand_never_jumps_through_a_reload() {
        for (g, w) in guns().iter().zip(WEAPONS.iter()) {
            let Some(r) = &g.spec.rounds else { continue };
            for fired in 1..=r.chambers.len() {
                let at = |t: f32| {
                    let (hold, along) = r.hand_at(fired, t);
                    add(hold, along)
                };
                let steps = 2000;
                for i in 0..steps {
                    let (t0, t1) = (i as f32 / steps as f32, (i + 1) as f32 / steps as f32);
                    let jump = len(sub(at(t1), at(t0)));
                    assert!(jump < 0.003, "{} with {fired} spent: the hand jumps {jump} m at {t0}", w.name);
                }
                // and is back at its hold once the reload is done
                assert!(len(at(1.0)) < 1e-4, "{} with {fired} spent: the hand ends {:?} off its hold", w.name, at(1.0));
            }
        }
    }

    #[test]
    fn every_gun_is_built_sized_for_what_it_is_and_has_its_muzzle_in_front() {
        let t = std::time::Instant::now();
        let (guns, hands) = (guns(), Hands::new());
        println!("built in {:?}", t.elapsed());
        assert_eq!(guns.len(), WEAPONS.len());
        println!("hands: {} triangles", check(&hands.right) + check(&hands.left) + check(&hands.pistol_left));
        // overall length, front to back, for each gun in order
        let lengths = [
            (0.9, 1.05), (0.7, 0.85), (0.9, 1.05), (1.1, 1.3), (0.95, 1.05), (0.33, 0.42), (0.98, 1.08), (1.2, 1.3),
            (0.18, 0.27), (0.3, 0.37), (0.26, 0.31), (0.84, 0.87),
        ];
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

    /// The bat is as thick as one at the barrel and the handle, the hands close round the handle and nowhere else,
    /// and each coat of blood is there to draw.
    #[test]
    fn the_bat_is_shaped_like_one_and_held_by_the_handle() {
        let t = std::time::Instant::now();
        let (hands, blood) = (bat_hands(), bat_blood());
        println!("bat hands and blood built in {:?}", t.elapsed());
        println!("bat: {} triangles of hands, {} of blood", check(&hands), check(&blood));
        assert_eq!(blood.len(), 4);
        assert!((bat_r(-0.7) - 0.0335).abs() < 0.001 && (bat_r(-0.1) - 0.0128).abs() < 0.001);
        for p in &hands {
            for v in p.mesh.verts.chunks(6) {
                assert!(v[2] < 0.0 && v[2] > -0.3 || v[1] > 0.06, "a hand off the handle at {v:?}");
            }
        }
    }

    #[test]
    fn the_grenade_is_built_the_size_of_one_with_its_fuze_on_top() {
        let g = grenade();
        let tris = check(&g.body) + check(&g.spoon) + check(&g.pin) + check(&g.hand);
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        for p in g.body.iter().chain(&g.spoon) {
            for v in p.mesh.verts.chunks(6) {
                for a in 0..3 {
                    lo[a] = lo[a].min(v[a]);
                    hi[a] = hi[a].max(v[a]);
                }
            }
        }
        println!("grenade: {tris} triangles, from {lo:?} to {hi:?}");
        assert!((hi[0] - lo[0] - 2.0 * GRENADE_R).abs() < 0.004, "{} wide", hi[0] - lo[0]);
        assert!(hi[1] > 0.045 && hi[1] < 0.05 && lo[1] < -0.03, "from {} to {} high", lo[1], hi[1]);
    }
}
