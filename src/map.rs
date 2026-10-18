//! The arena: a ruined town round a clock tower in the middle, an industrial yard to the north,
//! houses and a church to the south, a hill with a radio mast to the east, trenches and a water tower to the
//! west, and a landmark in each corner. Everything is boxes, so it is cheap to draw and to collide with.
//!
//! The layout is authored; the seed only varies colours, which houses are ruined, and the rubble.

use crate::sim::{overlaps, Aabb, Rng, V3, HALF};

pub struct Map {
    pub walls: Vec<Aabb>,
    /// one per wall, same order
    pub colors: Vec<[f32; 3]>,
    /// surface look per wall, see `material`
    pub mats: Vec<u8>,
    /// flat ground markings (roads, paving, grass): drawn, never collided with
    pub decals: Vec<(Aabb, [f32; 3])>,
    /// open ground to spawn on
    pub spawns: Vec<V3>,
}

type Col = [f32; 3];

const CONCRETE: Col = [0.60, 0.59, 0.56];
const DARK_CONCRETE: Col = [0.38, 0.38, 0.39];
const STONE: Col = [0.52, 0.50, 0.46];
const BRICK: Col = [0.52, 0.27, 0.22];
const BRICK_LIGHT: Col = [0.62, 0.40, 0.31];
const PLASTER: Col = [0.72, 0.66, 0.54];
const WOOD: Col = [0.40, 0.29, 0.19];
const RUST: Col = [0.52, 0.30, 0.18];
const STEEL: Col = [0.34, 0.37, 0.41];
const ROOF: Col = [0.30, 0.27, 0.26];
const YELLOW: Col = [0.78, 0.62, 0.16];
const BLACK: Col = [0.10, 0.10, 0.11];
const CONTAINERS: [Col; 6] = [
    [0.62, 0.22, 0.18],
    [0.20, 0.32, 0.55],
    [0.22, 0.45, 0.30],
    [0.78, 0.60, 0.20],
    [0.74, 0.74, 0.70],
    [0.80, 0.42, 0.16],
];
const CAR_COLORS: [Col; 4] = [[0.45, 0.14, 0.12], [0.20, 0.26, 0.34], [0.55, 0.52, 0.45], [0.18, 0.30, 0.22]];

const PAVING: Col = [0.38, 0.37, 0.34];
const ASPHALT: Col = [0.14, 0.14, 0.15];
const YARD: Col = [0.30, 0.30, 0.30];
const GRASS: Col = [0.20, 0.27, 0.16];
const DIRT: Col = [0.30, 0.25, 0.18];
const MUD: Col = [0.20, 0.16, 0.12];
pub const GROUND: Col = [0.23, 0.22, 0.19];

/// What a surface of this colour is made of, which the renderer turns into a pattern:
/// 0 plain, 1 concrete, 2 brick, 3 plaster, 4 painted metal, 5 wood, 6 dirt and asphalt, 7 paving, 8 grass.
pub fn material(c: [f32; 3]) -> u8 {
    if [CONCRETE, DARK_CONCRETE, STONE, ROOF].contains(&c) {
        1
    } else if [BRICK, BRICK_LIGHT].contains(&c) {
        2
    } else if c == PLASTER {
        3
    } else if [RUST, STEEL, YELLOW].contains(&c) || CONTAINERS.contains(&c) || CAR_COLORS.contains(&c) {
        4
    } else if c == WOOD {
        5
    } else if [ASPHALT, DIRT, MUD, GROUND].contains(&c) {
        6
    } else if [PAVING, YARD].contains(&c) {
        7
    } else if c == GRASS {
        8
    } else {
        0
    }
}

/// One step up and along; stairs are made of these.
const RISE: f32 = 0.3;
const TREAD: f32 = 0.6;
/// Clear height of a storey, and the pitch from one floor to the next (a 0.3 slab in between).
const CLEAR: f32 = 3.0;
const STORY: f32 = 3.3;
const T: f32 = 0.4;

/// A hole in a wall: centre along the wall, width, and sill / head heights above the wall's base.
struct Op {
    c: f32,
    w: f32,
    sill: f32,
    head: f32,
}

/// The four boundary walls, which tests use for an empty arena.
pub fn perimeter() -> Vec<Aabb> {
    let (edge, len) = (HALF + 0.5, 2.0 * HALF + 2.0);
    vec![
        block(0.0, -edge, len, 8.0, 1.0),
        block(0.0, edge, len, 8.0, 1.0),
        block(-edge, 0.0, 1.0, 8.0, len),
        block(edge, 0.0, 1.0, 8.0, len),
    ]
}

pub fn block(cx: f32, cz: f32, sx: f32, sy: f32, sz: f32) -> Aabb {
    Aabb { min: [cx - sx / 2.0, 0.0, cz - sz / 2.0], max: [cx + sx / 2.0, sy, cz + sz / 2.0] }
}

struct B {
    w: Vec<Aabb>,
    c: Vec<Col>,
    m: Vec<u8>,
    decals: Vec<(Aabb, Col)>,
    /// footprints that random rubble must stay out of
    keep_out: Vec<Aabb>,
    rng: Rng,
    /// how battered the building being built is, 0 to 1
    ruin: f32,
}

impl B {
    fn tint(&mut self, c: Col) -> Col {
        let k = self.rng.range(0.9, 1.1);
        [c[0] * k, c[1] * k, c[2] * k]
    }

    fn put(&mut self, min: V3, max: V3, col: Col) {
        if (0..3).any(|i| max[i] - min[i] < 0.01) {
            return;
        }
        let mat = material(col);
        let col = self.tint(col);
        self.w.push(Aabb { min, max });
        self.c.push(col);
        self.m.push(mat);
    }

    fn bx(&mut self, x0: f32, y0: f32, z0: f32, x1: f32, y1: f32, z1: f32, col: Col) {
        self.put([x0, y0, z0], [x1, y1, z1], col);
    }

    fn block(&mut self, cx: f32, cz: f32, sx: f32, sy: f32, sz: f32, col: Col) {
        self.bx(cx - sx / 2.0, 0.0, cz - sz / 2.0, cx + sx / 2.0, sy, cz + sz / 2.0, col);
    }

    fn decal(&mut self, x0: f32, z0: f32, x1: f32, z1: f32, col: Col) {
        self.decals.push((Aabb { min: [x0, 0.0, z0], max: [x1, 0.02, z1] }, col));
    }

    fn reserve(&mut self, x0: f32, z0: f32, x1: f32, z1: f32) {
        self.keep_out.push(Aabb { min: [x0, 0.0, z0], max: [x1, 1.0, z1] });
    }

    fn pick(&mut self, lo: f32, hi: f32) -> f32 {
        if hi <= lo {
            (lo + hi) / 2.0
        } else {
            self.rng.range(lo, hi)
        }
    }

    fn one_of<'a, X>(&mut self, xs: &'a [X]) -> &'a X {
        &xs[((self.rng.f() * xs.len() as f32) as usize).min(xs.len() - 1)]
    }

    // ---- walls, floors and stairs ------------------------------------------------------------

    /// A wall along `axis` (0 = x, 2 = z) from `a0` to `a1`, centred on `at` on the other axis,
    /// with openings cut out of it.
    fn wall(&mut self, axis: usize, a0: f32, a1: f32, at: f32, thick: f32, y0: f32, y1: f32, ops: &[Op], col: Col) {
        let mut sorted: Vec<&Op> = ops.iter().collect();
        sorted.sort_by(|a, b| a.c.partial_cmp(&b.c).unwrap());
        let mut cur = a0;
        for o in sorted {
            let (lo, hi) = (o.c - o.w / 2.0, o.c + o.w / 2.0);
            self.seg(axis, cur, lo, at, thick, y0, y1, col);
            self.seg(axis, lo, hi, at, thick, y0, (y0 + o.sill).min(y1), col);
            self.seg(axis, lo, hi, at, thick, y0 + o.head, y1, col);
            cur = hi;
        }
        self.seg(axis, cur, a1, at, thick, y0, y1, col);
    }

    fn seg(&mut self, axis: usize, lo: f32, hi: f32, at: f32, thick: f32, ylo: f32, yhi: f32, col: Col) {
        if hi - lo < 0.05 || yhi - ylo < 0.05 {
            return;
        }
        let mut yhi = yhi;
        // ruins: some pieces lose their tops or go missing altogether
        if self.ruin > 0.0 {
            if hi - lo < 4.0 && self.rng.f() < self.ruin * 0.12 {
                return;
            }
            if yhi - ylo > 1.5 && self.rng.f() < self.ruin * 0.4 {
                yhi = ylo + (yhi - ylo) * self.rng.range(0.35, 0.8);
            }
        }
        let h = thick / 2.0;
        if axis == 0 {
            self.put([lo, ylo, at - h], [hi, yhi, at + h], col);
        } else {
            self.put([at - h, ylo, lo], [at + h, yhi, hi], col);
        }
    }

    /// A slab with a rectangular hole (x0, z0, x1, z1) in it, e.g. over a stairwell.
    fn slab(&mut self, r: [f32; 4], y0: f32, y1: f32, hole: Option<[f32; 4]>, col: Col) {
        let [x0, z0, x1, z1] = r;
        match hole {
            None => self.bx(x0, y0, z0, x1, y1, z1, col),
            Some([hx0, hz0, hx1, hz1]) => {
                self.bx(x0, y0, z0, hx0, y1, z1, col);
                self.bx(hx1, y0, z0, x1, y1, z1, col);
                self.bx(hx0, y0, z0, hx1, y1, hz0, col);
                self.bx(hx0, y0, hz1, hx1, y1, z1, col);
            }
        }
    }

    /// Stairs from (x, z), the middle of the bottom edge, going in `dir` (0 +x, 1 -x, 2 +z, 3 -z). Solid ones are
    /// built up from `base`; open ones are a zig-zag of steel with room underneath, like a real flight indoors.
    fn stairs(&mut self, x: f32, z: f32, dir: u8, steps: usize, width: f32, base: f32, open: bool, col: Col) {
        let hw = width / 2.0;
        for i in 1..=steps {
            let (a, b) = ((i - 1) as f32 * TREAD, i as f32 * TREAD);
            let top = base + RISE * i as f32;
            let y = if open { top - RISE } else { base };
            let (min, max) = match dir {
                0 => ([x + a, y, z - hw], [x + b, top, z + hw]),
                1 => ([x - b, y, z - hw], [x - a, top, z + hw]),
                2 => ([x - hw, y, z + a], [x + hw, top, z + b]),
                _ => ([x - hw, y, z - b], [x + hw, top, z - a]),
            };
            self.put(min, max, col);
        }
    }

    /// A building with `stories` floors (only if it is wide enough for the stairs), doors on the sides
    /// marked in `doors` (north = -z, south, west = -x, east), windows all round, and a flat roof.
    /// Stairs climb along the north wall on even floors and the south wall on odd ones, from every floor of a
    /// building with more than one, so the roof can always be reached.
    fn building(&mut self, x0: f32, z0: f32, x1: f32, z1: f32, stories: usize, col: Col, ruin: f32, doors: [bool; 4]) {
        let stories = if x1 - x0 >= 14.0 && z1 - z0 >= 8.0 { stories } else { 1 };
        let climb = stories > 1;
        self.ruin = ruin;
        self.reserve(x0 - 1.0, z0 - 1.0, x1 + 1.0, z1 + 1.0);
        let n_lo = if stories > 1 { x0 + 9.5 } else { x0 + 3.0 };
        let door_at = [self.pick(n_lo, x1 - 2.5), self.pick(x0 + 4.0, x1 - 4.0), self.pick(z0 + 4.0, z1 - 2.5), self.pick(z0 + 4.0, z1 - 2.5)];
        let (w, d) = (x1 - x0, z1 - z0);

        for s in 0..stories {
            let y0 = s as f32 * STORY;
            let y1 = y0 + CLEAR;
            let (xf, lane, dir) = if s % 2 == 0 { (x0 + T / 2.0 + 1.6, z0 + T / 2.0 + 0.8, 0) } else { (x1 - T / 2.0 - 1.6, z1 - T / 2.0 - 0.8, 1) };
            let run = 11.0 * TREAD;
            let (f0, f1) = if dir == 0 { (xf, xf + run) } else { (xf - run, xf) };
            for side in 0..4 {
                let (axis, a0, a1, at) = match side {
                    0 => (0, x0, x1, z0),
                    1 => (0, x0, x1, z1),
                    2 => (2, z0, z1, x0),
                    _ => (2, z0, z1, x1),
                };
                let mut ops = Vec::new();
                if s == 0 && doors[side] {
                    ops.push(Op { c: door_at[side], w: 2.4, sill: 0.0, head: 2.4 });
                }
                let n = ((a1 - a0 - 2.0) / 3.6).floor() as i32;
                for k in 0..n {
                    let c = a0 + 1.0 + (k as f32 + 0.5) * (a1 - a0 - 2.0) / n as f32;
                    // the wall the flight runs up stays whole, so the steps never cut across a window
                    let behind = climb && side == dir as usize && c + 1.1 > f0 && c - 1.1 < f1;
                    if !behind && ops.iter().all(|o| (o.c - c).abs() > 2.8) {
                        ops.push(Op { c, w: 1.6, sill: 1.0, head: 2.3 });
                    }
                }
                self.wall(axis, a0, a1, at, T, y0, y1, &ops, col);
            }
            if s == 0 && w >= 18.0 && d >= 8.0 {
                // a partition with a door splits the ground floor into two rooms
                let x = x0 + w * 0.62;
                self.wall(2, z0, z1, x, T, y0, y1, &[Op { c: z1 - 3.5, w: 2.4, sill: 0.0, head: 2.4 }], col);
            }

            let last = s + 1 == stories;
            let hole = climb.then(|| {
                let run = 10.0 * TREAD;
                // a little wider than the stairs, on the open side, so a body that drifts off the lane keeps its head clear
                if dir == 0 {
                    [xf, lane - 0.8, xf + run, lane + 1.3]
                } else {
                    [xf - run, lane - 1.3, xf, lane + 0.8]
                }
            });
            if climb {
                self.stairs(xf, lane, dir, 11, 1.6, y0, true, STEEL);
            }
            let top = [x0 - T / 2.0, z0 - T / 2.0, x1 + T / 2.0, z1 + T / 2.0];
            let hole = if hole.is_none() && last && self.rng.f() < ruin * 0.8 {
                // the roof has fallen in somewhere
                let hx = self.pick(x0 + 2.0, x1 - 6.0);
                let hz = self.pick(z0 + 1.5, z1 - 5.5);
                Some([hx, hz, hx + 4.0, hz + 4.0])
            } else {
                hole
            };
            self.slab(top, y1, y1 + (STORY - CLEAR), hole, ROOF);
            if last && ruin < 0.3 {
                // parapet
                let (py, ph) = (y1 + 0.3, 0.7);
                self.bx(top[0], py, top[1], top[2], py + ph, top[1] + 0.3, col);
                self.bx(top[0], py, top[3] - 0.3, top[2], py + ph, top[3], col);
                self.bx(top[0], py, top[1], top[0] + 0.3, py + ph, top[3], col);
                self.bx(top[2] - 0.3, py, top[1], top[2], py + ph, top[3], col);
            }
        }
        // a crate in two corners
        self.block(x0 + 1.4, z1 - 1.4, 1.2, 1.0, 1.2, WOOD);
        self.block(x1 - 1.4, z1 - 1.4, 1.2, 1.0, 1.2, WOOD);
    }

    fn warehouse(&mut self, x0: f32, z0: f32, x1: f32, z1: f32, h: f32, col: Col) {
        self.reserve(x0 - 1.0, z0 - 1.0, x1 + 1.0, z1 + 1.0);
        self.ruin = 0.0;
        let n = ((x1 - x0 - 4.0) / 6.0).floor() as i32;
        for at in [z0, z1] {
            let ops: Vec<Op> = (0..n)
                .map(|k| Op { c: x0 + 3.0 + (k as f32 + 0.5) * (x1 - x0 - 6.0) / n as f32, w: 3.0, sill: 4.2, head: h - 0.8 })
                .collect();
            self.wall(0, x0, x1, at, 0.5, 0.0, h, &ops, col);
        }
        for at in [x0, x1] {
            self.wall(2, z0, z1, at, 0.5, 0.0, h, &[Op { c: (z0 + z1) / 2.0, w: 5.0, sill: 0.0, head: 4.5 }], col);
        }
        self.bx(x0 - 0.5, h, z0 - 0.5, x1 + 0.5, h + 0.4, z1 + 0.5, ROOF);
        // storage racks in two rows with a gap in the middle of each
        let mid = (x0 + x1) / 2.0;
        for rz in [z0 + 4.5, z0 + 11.5] {
            self.bx(x0 + 4.0, 0.0, rz - 0.5, mid - 2.0, 3.0, rz + 0.5, RUST);
            self.bx(mid + 2.0, 0.0, rz - 0.5, x1 - 4.0, 3.0, rz + 0.5, RUST);
        }
    }

    fn container_yard(&mut self, x0: f32, z0: f32, cols: usize, rows: usize, px: f32, pz: f32) {
        self.reserve(x0 - 1.0, z0 - 3.0, x0 + cols as f32 * px, z0 + rows as f32 * pz);
        for r in 0..rows {
            for c in 0..cols {
                if self.rng.f() < 0.15 {
                    continue;
                }
                let (cx, cz) = (x0 + 6.0 + c as f32 * px, z0 + r as f32 * pz);
                let col = *self.one_of(&CONTAINERS);
                self.block(cx, cz, 12.0, 2.7, 2.5, col);
                if self.rng.f() < 0.3 {
                    // a second container over half the length, reached by a stair alongside
                    let col = *self.one_of(&CONTAINERS);
                    self.bx(cx, 2.7, cz - 1.25, cx + 6.0, 5.4, cz + 1.25, col);
                    self.stairs(cx - 6.0, cz + 1.85, 0, 9, 1.2, 0.0, false, STEEL);
                }
            }
        }
    }

    fn car(&mut self, cx: f32, cz: f32, along_x: bool) {
        let col = *self.one_of(&CAR_COLORS);
        let (sx, sz) = if along_x { (2.1, 0.9) } else { (0.9, 2.1) };
        self.bx(cx - sx, 0.3, cz - sz, cx + sx, 1.0, cz + sz, col);
        let (cx2, cz2) = if along_x { (1.0, 0.8) } else { (0.8, 1.0) };
        self.bx(cx - cx2, 1.0, cz - cz2, cx + cx2, 1.55, cz + cz2, BLACK);
        self.bx(cx - sx + 0.3, 0.0, cz - sz + 0.2, cx + sx - 0.3, 0.3, cz + sz - 0.2, BLACK);
    }

    // ---- zones ---------------------------------------------------------------------------------

    fn perimeter(&mut self) {
        for a in perimeter() {
            self.put(a.min, a.max, DARK_CONCRETE);
        }
        // buttresses so the boundary reads as a wall and not a horizon
        for i in -4..=4 {
            let p = i as f32 * 22.0;
            let e = HALF - 1.0;
            self.bx(p - 1.0, 0.0, -e - 1.0, p + 1.0, 9.0, -e, CONCRETE);
            self.bx(p - 1.0, 0.0, e, p + 1.0, 9.0, e + 1.0, CONCRETE);
            self.bx(-e - 1.0, 0.0, p - 1.0, -e, 9.0, p + 1.0, CONCRETE);
            self.bx(e, 0.0, p - 1.0, e + 1.0, 9.0, p + 1.0, CONCRETE);
        }
    }

    fn roads(&mut self) {
        self.decal(-6.0, -98.0, 6.0, 98.0, ASPHALT);
        self.decal(-48.0, -6.0, 48.0, 6.0, ASPHALT);
        self.decal(-98.0, 60.0, 98.0, 68.0, ASPHALT);
    }

    /// The clock tower: a stone shaft on a stepped base, with a steel stair winding round the outside of it up to a
    /// round balcony under the clock, the highest lookout on the map.
    fn clock_tower(&mut self) {
        self.ruin = 0.0;
        self.block(0.0, 0.0, 12.0, 0.3, 12.0, DARK_CONCRETE);
        self.block(0.0, 0.0, 9.0, 0.6, 9.0, DARK_CONCRETE);
        // half the shaft's width, the stair's width, where the stair starts and the balcony's deck
        let (a, w, base, deck) = (3.0, 1.4, 0.6, 21.9);
        self.block(0.0, 0.0, 2.0 * a, deck - 0.3, 2.0 * a, STONE);

        // The stair is laid out as if on the south face, as (u0, v0, u1, v1) with u along the face the way it
        // climbs and v out from the middle, and each flight is turned onto its face: a quarter turn per face.
        let face = |k: usize, [u0, v0, u1, v1]: [f32; 4]| {
            let (mut p, mut q) = ([u0, v0], [u1, v1]);
            for _ in 0..k % 4 {
                p = [p[1], -p[0]];
                q = [q[1], -q[0]];
            }
            [p[0].min(q[0]), p[1].min(q[1]), p[0].max(q[0]), p[1].max(q[1])]
        };
        // from the middle of the south face: half a flight, five whole ones round, and a last one up the north face
        // through the deck, with a landing at each corner
        let mut steps = Vec::new();
        for k in 0..7 {
            let (from, treads) = if k == 0 { (0.0, 5) } else { (-a, 10) };
            for i in 0..treads {
                let u = from + i as f32 * TREAD;
                steps.push((k, [u, a, u + TREAD, a + w], false));
            }
            if k < 6 {
                steps.push((k, [a, a, a + w, a + w], true));
            }
        }
        let rail = 0.06;
        for (i, &(k, r, corner)) in steps.iter().enumerate() {
            let top = base + RISE * (i + 1) as f32;
            let [x0, z0, x1, z1] = face(k, r);
            self.bx(x0, top - RISE, z0, x1, top, z1, STEEL);
            if i + 1 == steps.len() {
                break;
            }
            // a handrail along the open side, round the corners, and a post every other step
            let [u0, v0, u1, v1] = r;
            let mut bars = vec![[u0, v1 - rail, u1, v1]];
            if corner {
                bars.push([u1 - rail, v0, u1, v1]);
            }
            for b in bars {
                let [x0, z0, x1, z1] = face(k, b);
                self.bx(x0, top + 0.85, z0, x1, top + 1.0, z1, STEEL);
            }
            if i % 2 == 0 {
                let [x0, z0, x1, z1] = face(k, [u0, v1 - rail, u0 + rail, v1]);
                self.bx(x0, top, z0, x1, top + 0.85, z1, STEEL);
            }
        }

        // the balcony: a round steel deck, open where the last flight comes up through it (so nobody's head hits
        // it on the way), with a railing all round low enough to shoot over
        let (r, hole) = (5.7, face(6, [-a, a, 2.4, a + w]));
        let mut zs: Vec<f32> = (0..=22).map(|i| -r + i as f32 * (2.0 * r / 22.0)).chain([hole[1], hole[3]]).collect();
        zs.sort_by(f32::total_cmp);
        for z in zs.windows(2) {
            let (z0, z1) = (z[0], z[1]);
            let near = if z0 < 0.0 && z1 > 0.0 { 0.0 } else { z0.abs().min(z1.abs()) };
            let hw = (r * r - near * near).max(0.0).sqrt();
            let cut = z0 >= hole[1] - 0.001 && z1 <= hole[3] + 0.001;
            let spans = if cut { vec![(-hw, hole[0]), (hole[2], hw)] } else { vec![(-hw, hw)] };
            for (x0, x1) in spans {
                self.bx(x0, deck - 0.3, z0, x1, deck, z1, STEEL);
            }
        }
        let (rr, n) = (5.55, 48);
        let chord = std::f32::consts::TAU * rr / n as f32;
        for i in 0..n {
            let (s, c) = ((i as f32 + 0.5) / n as f32 * std::f32::consts::TAU).sin_cos();
            let (x, z) = (rr * c, rr * s);
            // each piece of the ring is a box along the circle, a little longer than its share so they overlap
            let (hx, hz) = ((s.abs() * chord * 0.6).max(0.035), (c.abs() * chord * 0.6).max(0.035));
            for (y0, y1) in [(0.45, 0.52), (0.95, 1.05)] {
                self.bx(x - hx, deck + y0, z - hz, x + hx, deck + y1, z + hz, STEEL);
            }
            if i % 3 == 0 {
                self.bx(x - 0.04, deck, z - 0.04, x + 0.04, deck + 0.95, z + 0.04, STEEL);
            }
        }

        // the clock room on the deck, with a face on each side, and its roof and spire
        let top = deck + 4.1;
        self.bx(-2.0, deck, -2.0, 2.0, top, 2.0, STONE);
        for (dx, dz) in [(0.0, 2.1), (0.0, -2.1)] {
            self.bx(dx - 1.2, deck + 0.8, dz - 0.2, dx + 1.2, deck + 3.2, dz + 0.2, PLASTER);
        }
        for (dx, dz) in [(2.1, 0.0), (-2.1, 0.0)] {
            self.bx(dx - 0.2, deck + 0.8, dz - 1.2, dx + 0.2, deck + 3.2, dz + 1.2, PLASTER);
        }
        self.bx(-2.6, top, -2.6, 2.6, top + 0.5, 2.6, STONE);
        self.bx(-1.6, top + 0.5, -1.6, 1.6, top + 4.0, 1.6, ROOF);
        self.bx(-0.5, top + 4.0, -0.5, 0.5, top + 8.0, 0.5, STEEL);
    }

    fn town_square(&mut self) {
        self.reserve(-27.0, -27.0, 27.0, 27.0);
        self.decal(-26.0, -26.0, 26.0, 26.0, PAVING);
        self.clock_tower();
        // fountains and benches
        for sx in [-14.0, 14.0] {
            let (r, t) = (3.0, 0.6);
            self.bx(sx - r, 0.0, -r, sx + r, 0.8, -r + t, STONE);
            self.bx(sx - r, 0.0, r - t, sx + r, 0.8, r, STONE);
            self.bx(sx - r, 0.0, -r + t, sx - r + t, 0.8, r - t, STONE);
            self.bx(sx + r - t, 0.0, -r + t, sx + r, 0.8, r - t, STONE);
            self.block(sx, 0.0, 0.8, 1.8, 0.8, STONE);
        }
        for (x, z) in [(-8.0, 18.0), (8.0, 18.0), (-8.0, -18.0), (8.0, -18.0), (-20.0, 10.0), (20.0, -10.0)] {
            self.block(x, z, 3.0, 0.7, 0.7, WOOD);
        }
        // four ruined shops and flats round the plaza
        let c = self.one_of(&[BRICK, BRICK_LIGHT, PLASTER]).to_owned();
        self.building(-46.0, -24.0, -30.0, -10.0, 2, c, 0.5, [false, true, false, true]);
        let c = self.one_of(&[BRICK, BRICK_LIGHT, PLASTER]).to_owned();
        self.building(30.0, -24.0, 46.0, -10.0, 2, c, 0.5, [false, true, true, false]);
        let c = self.one_of(&[BRICK, BRICK_LIGHT, PLASTER]).to_owned();
        self.building(-46.0, 10.0, -30.0, 24.0, 2, c, 0.6, [true, false, false, true]);
        let c = self.one_of(&[BRICK, BRICK_LIGHT, PLASTER]).to_owned();
        self.building(30.0, 10.0, 46.0, 24.0, 2, c, 0.4, [true, false, true, false]);
        // wrecks along the roads
        for (x, z, ax) in [(-3.0, -40.0, false), (3.0, 38.0, false), (-30.0, 3.0, true), (36.0, -3.0, true), (2.5, 52.0, false)] {
            self.car(x, z, ax);
        }
    }

    fn north_yard(&mut self) {
        self.decal(-62.0, -98.0, 62.0, -48.0, YARD);
        self.warehouse(-58.0, -96.0, -20.0, -80.0, 7.0, STEEL);
        self.warehouse(20.0, -96.0, 58.0, -80.0, 7.0, STEEL);
        self.container_yard(-58.0, -74.0, 3, 3, 15.0, 7.0);
        self.container_yard(16.0, -74.0, 3, 3, 15.0, 7.0);
        // a gantry crane straddling the road, with a container hanging from it
        let (x, z) = (0.0, -66.0);
        for dx in [-9.0, 9.0] {
            for dz in [-3.0, 3.0] {
                self.block(x + dx, z + dz, 1.6, 16.0, 1.6, YELLOW);
            }
        }
        self.bx(x - 11.0, 14.0, z - 4.2, x + 11.0, 16.4, z + 4.2, YELLOW);
        self.bx(x - 1.5, 12.5, z - 1.5, x + 1.5, 14.0, z + 1.5, BLACK);
        self.bx(x - 0.1, 10.7, z - 0.1, x + 0.1, 12.5, z + 0.1, BLACK);
        self.bx(x - 6.0, 8.0, z - 1.25, x + 6.0, 10.7, z + 1.25, CONTAINERS[0]);
        self.reserve(x - 12.0, z - 5.0, x + 12.0, z + 5.0);
        for (x, z) in [(-12.0, -52.0), (12.0, -54.0), (-4.0, -88.0), (5.0, -76.0)] {
            self.car(x, z, false);
        }
    }

    fn south_town(&mut self) {
        self.decal(-98.0, 48.0, -36.0, 96.0, DIRT);
        self.decal(36.0, 48.0, 98.0, 64.0, DIRT);
        let cols = [BRICK, BRICK_LIGHT, PLASTER, WOOD, STONE];
        for x0 in [-92.0, -72.0, -52.0, 38.0, 58.0, 78.0] {
            let c = *self.one_of(&cols);
            let stories = if self.rng.f() < 0.55 { 2 } else { 1 };
            let ruin = self.rng.range(0.3, 0.9);
            let west = self.rng.f() < 0.5;
            self.building(x0, 50.0, x0 + 14.0, 60.0, stories, c, ruin, [false, true, west, !west]);
        }
        for x0 in [-92.0, -72.0, -52.0] {
            let c = *self.one_of(&cols);
            let stories = if self.rng.f() < 0.55 { 2 } else { 1 };
            let ruin = self.rng.range(0.3, 0.9);
            self.building(x0, 70.0, x0 + 14.0, 80.0, stories, c, ruin, [true, false, true, false]);
        }
        // the church: a roofless nave with tall broken walls, a solid bell tower, pews and an altar
        self.reserve(-12.0, 54.0, 12.0, 92.0);
        self.decal(-14.0, 54.0, 14.0, 92.0, PAVING);
        self.ruin = 0.7;
        let ops: Vec<Op> = (0..4).map(|k| Op { c: 67.0 + k as f32 * 6.0, w: 2.2, sill: 1.5, head: 6.5 }).collect();
        for x in [-9.0, 9.0] {
            self.wall(2, 64.0, 88.0, x, 0.8, 0.0, 10.0, &ops, STONE);
        }
        self.wall(0, -9.0, 9.0, 88.0, 0.8, 0.0, 11.0, &[Op { c: 0.0, w: 2.2, sill: 2.0, head: 7.0 }], STONE);
        self.wall(0, -9.0, 9.0, 64.0, 0.8, 0.0, 10.0, &[Op { c: 0.0, w: 3.4, sill: 0.0, head: 5.0 }], STONE);
        self.block(0.0, 59.0, 6.0, 24.0, 6.0, STONE);
        self.block(0.0, 59.0, 4.0, 28.0, 4.0, ROOF);
        self.block(0.0, 59.0, 1.0, 32.0, 1.0, STEEL);
        for row in 0..5 {
            let z = 69.0 + row as f32 * 3.4;
            self.bx(-7.5, 0.0, z - 0.4, -1.5, 0.9, z + 0.4, WOOD);
            self.bx(1.5, 0.0, z - 0.4, 7.5, 0.9, z + 0.4, WOOD);
        }
        self.block(0.0, 85.5, 3.0, 1.1, 1.2, STONE);
        for (x, z, ax) in [(-20.0, 64.0, true), (24.0, 66.0, true), (-60.0, 64.0, true), (50.0, 64.0, true)] {
            self.car(x, z, ax);
        }
    }

    fn east_hill(&mut self) {
        self.decal(48.0, -60.0, 98.0, 40.0, GRASS);
        // a stepped hill: each ring is a step you can walk up
        for i in 0..14 {
            let half = 22.0 - 1.4 * i as f32;
            self.block(75.0, 0.0, 2.0 * half, RISE * (i + 1) as f32, 2.0 * half, STONE);
        }
        let top = RISE * 14.0;
        // a sniper's nest on top, and the radio mast
        self.bx(71.0, top, -4.0, 79.0, top + 1.2, -3.4, CONCRETE);
        self.bx(71.0, top, -4.0, 71.6, top + 1.2, 4.0, CONCRETE);
        self.bx(78.4, top, -4.0, 79.0, top + 1.2, 4.0, CONCRETE);
        self.block(75.0, 0.0, 1.4, top + 34.0, 1.4, STEEL);
        for (y, w) in [(top + 12.0, 6.0), (top + 22.0, 4.5), (top + 30.0, 3.0)] {
            self.bx(75.0 - w, y, -0.3, 75.0 + w, y + 0.5, 0.3, STEEL);
        }
        self.reserve(52.0, -23.0, 98.0, 23.0);
        // a farm and a pair of broken walls out on the grass
        self.building(60.0, -56.0, 74.0, -46.0, 1, WOOD, 0.7, [false, true, true, false]);
        self.building(60.0, 30.0, 74.0, 40.0, 1, PLASTER, 0.8, [true, false, false, true]);
        self.ruin = 0.8;
        self.wall(0, 84.0, 96.0, -40.0, 0.5, 0.0, 2.6, &[], STONE);
        self.wall(2, 28.0, 38.0, 88.0, 0.5, 0.0, 2.6, &[], STONE);
    }

    fn west_trench(&mut self) {
        self.decal(-98.0, -32.0, -48.0, 32.0, MUD);
        // sandbagged zig-zag: parallel walls with a gap at alternating ends
        self.ruin = 0.0;
        self.reserve(-99.0, -31.0, -49.0, 31.0);
        for k in 0..15 {
            let z = -28.0 + 4.0 * k as f32;
            let c = if k % 2 == 0 { -50.0 - 1.5 } else { -96.0 + 1.5 };
            self.wall(0, -96.0, -50.0, z, 0.8, 0.0, 2.2, &[Op { c, w: 3.0, sill: 0.0, head: 9.0 }], WOOD);
        }
        self.wall(2, -28.4, 28.4, -96.4, 0.8, 0.0, 2.2, &[], WOOD);
        // a concrete bunker north of it
        self.building(-96.0, -50.0, -80.0, -38.0, 1, DARK_CONCRETE, 0.0, [false, true, false, true]);
        // a water tower on legs: you can walk under it
        let (x, z) = (-78.0, 42.0);
        for (dx, dz) in [(-3.5, -3.5), (3.5, -3.5), (-3.5, 3.5), (3.5, 3.5)] {
            self.block(x + dx, z + dz, 1.0, 11.0, 1.0, STEEL);
        }
        self.bx(x - 5.0, 11.0, z - 5.0, x + 5.0, 12.0, z + 5.0, RUST);
        self.bx(x - 4.5, 12.0, z - 4.5, x + 4.5, 17.0, z + 4.5, RUST);
        self.bx(x - 2.0, 17.0, z - 2.0, x + 2.0, 19.0, z + 2.0, STEEL);
        self.reserve(x - 7.0, z - 7.0, x + 7.0, z + 7.0);
    }

    fn corners(&mut self) {
        // NW: a walled fortress with towers and a three-storey keep
        let (fx0, fz0, fx1, fz1) = (-96.0, -96.0, -62.0, -62.0);
        self.decal(fx0, fz0, fx1, fz1, DIRT);
        self.reserve(fx0 - 1.0, fz0 - 1.0, fx1 + 1.0, fz1 + 1.0);
        self.ruin = 0.3;
        let gate = |c: f32| [Op { c, w: 4.0, sill: 0.0, head: 4.5 }];
        self.wall(0, fx0, fx1, fz0, 1.0, 0.0, 6.0, &[], CONCRETE);
        self.wall(0, fx0, fx1, fz1, 1.0, 0.0, 6.0, &gate(-79.0), CONCRETE);
        self.wall(2, fz0, fz1, fx0, 1.0, 0.0, 6.0, &[], CONCRETE);
        self.wall(2, fz0, fz1, fx1, 1.0, 0.0, 6.0, &gate(-79.0), CONCRETE);
        for (x, z) in [(fx0, fz0), (fx1, fz0), (fx0, fz1), (fx1, fz1)] {
            self.block(x, z, 5.0, 10.0, 5.0, STONE);
        }
        self.building(-86.0, -86.0, -70.0, -76.0, 3, STONE, 0.2, [false, true, false, true]);
        for (x, z) in [(-90.0, -68.0), (-66.0, -90.0), (-92.0, -80.0)] {
            self.block(x, z, 2.5, 1.2, 1.2, WOOD);
        }
        // NE: an oil depot, tanks and pipe runs round a control building
        self.decal(64.0, -98.0, 98.0, -62.0, YARD);
        self.reserve(63.0, -99.0, 99.0, -61.0);
        for (x, z) in [(72.0, -90.0), (90.0, -90.0), (72.0, -70.0), (90.0, -70.0)] {
            self.block(x, z, 12.0, 5.0, 12.0, RUST);
            self.block(x, z, 4.0, 6.0, 4.0, STEEL);
        }
        self.bx(66.0, 0.0, -80.6, 96.0, 0.9, -79.4, STEEL);
        self.bx(80.6, 0.0, -96.0, 81.4, 0.9, -64.0, STEEL);
        self.building(77.0, -63.5, 85.0, -61.5, 1, CONCRETE, 0.0, [false, false, false, false]);
        // SE: a gas station and a market
        self.decal(40.0, 66.0, 98.0, 98.0, ASPHALT);
        self.reserve(40.0, 66.0, 98.0, 99.0);
        for (x, z) in [(46.0, 72.0), (64.0, 72.0), (46.0, 82.0), (64.0, 82.0)] {
            self.block(x, z, 0.8, 4.6, 0.8, STEEL);
        }
        self.bx(44.0, 4.6, 70.0, 66.0, 5.0, 84.0, YELLOW);
        for x in [49.0, 55.0, 61.0] {
            self.block(x, 77.0, 1.0, 1.4, 0.7, RUST);
        }
        self.building(70.0, 70.0, 84.0, 80.0, 1, PLASTER, 0.3, [false, true, true, false]);
        for k in 0..5 {
            let x = 46.0 + k as f32 * 9.0;
            self.block(x, 91.0, 4.0, 1.6, 2.0, WOOD);
            self.bx(x - 2.4, 2.6, 89.6, x + 2.4, 2.8, 92.4, YELLOW);
            self.block(x - 2.0, 89.4, 0.2, 2.6, 0.2, WOOD);
            self.block(x + 2.0, 89.4, 0.2, 2.6, 0.2, WOOD);
        }
        // SW: a substation, transformers behind a fence and two pylons
        self.decal(-98.0, 84.0, -60.0, 98.0, YARD);
        self.reserve(-99.0, 83.0, -59.0, 99.0);
        for (x, z) in [(-92.0, 90.0), (-84.0, 90.0), (-76.0, 90.0), (-68.0, 90.0)] {
            self.block(x, z, 3.0, 3.5, 3.0, STEEL);
            self.block(x, z, 1.0, 5.0, 1.0, BLACK);
        }
        self.wall(0, -98.0, -60.0, 84.0, 0.2, 0.0, 2.4, &[Op { c: -79.0, w: 4.0, sill: 0.0, head: 9.0 }], STEEL);
        for x in [-90.0, -66.0] {
            self.block(x, 78.0, 1.0, 24.0, 1.0, STEEL);
            self.bx(x - 4.0, 20.0, 77.6, x + 4.0, 20.6, 78.4, STEEL);
            self.bx(x - 3.0, 16.0, 77.6, x + 3.0, 16.6, 78.4, STEEL);
        }
    }

    /// Low rubble and boulders wherever nothing else stands, so no stretch of ground is empty.
    fn rubble(&mut self) {
        let want = 170;
        let mut placed = 0;
        for _ in 0..want * 40 {
            if placed == want {
                break;
            }
            let (sx, sz) = (self.rng.range(1.5, 4.0), self.rng.range(1.5, 4.0));
            let sy = if self.rng.f() < 0.75 { self.rng.range(0.4, 1.2) } else { self.rng.range(1.2, 2.4) };
            let reach = HALF - 4.0;
            let (x, z) = (self.rng.range(-reach, reach), self.rng.range(-reach, reach));
            let near = Aabb { min: [x - sx / 2.0 - 3.0, 0.0, z - sz / 2.0 - 3.0], max: [x + sx / 2.0 + 3.0, 1.0, z + sz / 2.0 + 3.0] };
            if self.keep_out.iter().any(|k| overlaps(&near, k)) {
                continue;
            }
            let flat = Aabb { min: [near.min[0], 0.0, near.min[2]], max: [near.max[0], 1.0, near.max[2]] };
            if self.w.iter().any(|w| overlaps(&flat, &Aabb { min: [w.min[0], 0.0, w.min[2]], max: [w.max[0], 1.0, w.max[2]] })) {
                continue;
            }
            let col = *self.one_of(&[STONE, CONCRETE, DARK_CONCRETE, BRICK]);
            self.block(x, z, sx, sy, sz, col);
            placed += 1;
        }
    }

    /// Open ground on a loose grid, to spawn on.
    fn spawns(&mut self) -> Vec<V3> {
        let mut out = Vec::new();
        let mut x = -90.0;
        while x <= 90.0 {
            let mut z = -90.0;
            while z <= 90.0 {
                let (px, pz) = (x + self.rng.range(-4.0, 4.0), z + self.rng.range(-4.0, 4.0));
                let probe = Aabb { min: [px - 1.2, 0.0, pz - 1.2], max: [px + 1.2, 1.8, pz + 1.2] };
                if !self.w.iter().any(|w| overlaps(&probe, w)) {
                    out.push([px, 0.0, pz]);
                }
                z += 15.0;
            }
            x += 15.0;
        }
        out
    }
}

pub fn build(seed: u32) -> Map {
    let mut b = B { w: Vec::new(), c: Vec::new(), m: Vec::new(), decals: Vec::new(), keep_out: Vec::new(), rng: Rng(seed.wrapping_mul(2_654_435_761).max(1)), ruin: 0.0 };
    b.perimeter();
    b.roads();
    b.town_square();
    b.north_yard();
    b.south_town();
    b.east_hill();
    b.west_trench();
    b.corners();
    b.rubble();
    let spawns = b.spawns();
    Map { walls: b.w, colors: b.c, mats: b.m, decals: b.decals, spawns }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn everything_stays_inside_the_arena() {
        for seed in 1..8 {
            let m = build(seed);
            assert_eq!(m.walls.len(), m.colors.len());
            for w in &m.walls {
                assert!(w.min[0] >= -HALF - 1.1 && w.max[0] <= HALF + 1.1 && w.min[2] >= -HALF - 1.1 && w.max[2] <= HALF + 1.1, "seed {seed}: {w:?}");
                assert!((0..3).all(|i| w.max[i] > w.min[i]), "seed {seed}: empty box {w:?}");
            }
            assert!(m.walls.len() > 600, "seed {seed}: only {} walls", m.walls.len());
            assert!(m.spawns.len() > 30, "seed {seed}: only {} spawn points", m.spawns.len());
        }
    }

    #[test]
    fn most_of_the_map_is_connected() {
        use crate::grid::Grid;
        use crate::nav::Nav;
        for seed in 1..4 {
            let m = build(seed);
            let world = Grid::new(m.walls, 4.0);
            let mut nav = Nav::new(&world, HALF);
            nav.set_goal([0.0, 0.0, 14.0]);
            let (open, reached) = nav.coverage();
            assert!(reached * 100 >= open * 98, "seed {seed}: only {reached} of {open} open cells can reach the middle");
            // and every authored area can be reached from the middle
            for (name, p) in [("north yard", [-40.0, 0.0, -78.0]), ("church", [0.0, 0.0, 76.0]), ("fortress yard", [-79.0, 0.0, -66.0]), ("hill foot", [52.0, 0.0, 0.0]), ("trench mouth", [-49.0, 0.0, -26.0]), ("gas station", [58.0, 0.0, 74.0]), ("clock tower balcony", [0.0, 21.9, 4.8]), ("a roof by the plaza", [-38.0, 6.6, -17.0])] {
                assert!(nav.reachable(p), "seed {seed}: {name} is cut off");
            }
        }
    }

    fn walk(g: &mut crate::sim::Game, secs: f32) {
        for _ in 0..(secs / crate::sim::STEP) as usize {
            g.tick(crate::sim::STEP);
        }
    }

    fn lone_building(stories: usize) -> (crate::sim::Game, Aabb) {
        let mut b = B { w: Vec::new(), c: Vec::new(), m: Vec::new(), decals: Vec::new(), keep_out: Vec::new(), rng: Rng(7), ruin: 0.0 };
        for a in perimeter() {
            b.put(a.min, a.max, CONCRETE);
        }
        let (x0, z0, x1, z1) = (-8.0, -5.0, 8.0, 5.0);
        b.building(x0, z0, x1, z1, stories, BRICK, 0.0, [false, true, false, false]);
        let walls = b.w;
        let c = vec![[0.5; 3]; walls.len()];
        let mats = vec![0; walls.len()];
        let mut g = crate::sim::Game::with_map(1, Map { walls, colors: c, mats, decals: Vec::new(), spawns: Vec::new() });
        g.bots.clear();
        (g, Aabb { min: [x0, 0.0, z0], max: [x1, 0.0, z1] })
    }

    #[test]
    fn zombies_climb_the_stairs_to_a_player_on_an_upper_floor() {
        let (mut g, b) = lone_building(3);
        g.player.hp = 1e6;
        // the player stands on the second floor at the east end, a zombie waits on the ground floor
        g.player.body.pos = [b.max[0] - 2.0, 3.3, 0.0];
        let mut z = crate::sim::Bot::zombie([b.min[0] + 2.5, 0.0, 0.0], 0.0);
        z.hp = 1e6;
        g.bots.push(z);
        let mut top = 0.0f32;
        let mut bitten = false;
        for _ in 0..(40.0 / crate::sim::STEP) as usize {
            g.tick(crate::sim::STEP);
            if let Some(z) = g.bots.first() {
                top = top.max(z.body.pos[1]);
            }
            if g.player.hp < 100.0 {
                bitten = true;
                break;
            }
        }
        assert!(top > 3.0, "the zombie never got above {top}");
        assert!(bitten, "the zombie never reached the player upstairs");
    }

    #[test]
    fn the_stairs_in_a_building_lead_up_to_the_next_floor_and_the_roof() {
        let (mut g, b) = lone_building(3);
        // foot of the first flight, hugging the north wall and facing east
        g.player.body.pos = [b.min[0] + 0.8, 0.0, b.min[2] + 1.0];
        g.player.yaw = std::f32::consts::FRAC_PI_2;
        g.input.w = true;
        walk(&mut g, 4.0);
        let y = g.player.body.pos[1];
        assert!((3.2..3.5).contains(&y), "first flight ended at height {y}");
        // second flight runs back along the south wall from the east end
        g.player.body.pos = [b.max[0] - 0.8, 3.3, b.max[2] - 1.0];
        g.player.body.vel = [0.0; 3];
        g.player.yaw = -std::f32::consts::FRAC_PI_2;
        walk(&mut g, 4.0);
        let y = g.player.body.pos[1];
        assert!((6.5..6.8).contains(&y), "second flight ended at height {y}");
    }

    #[test]
    fn a_zombie_below_the_side_of_a_flight_goes_round_and_up_it() {
        let (mut g, b) = lone_building(2);
        // the player stands near the top of the open flight; the zombie is on the floor beside it, where it can
        // see them over the edge of the steps but can't jump up
        g.player.body.pos = [-1.0, 3.0, b.min[2] + 1.0];
        g.bots.push(crate::sim::Bot::zombie([-1.0, 0.0, -1.0], 0.0));
        for _ in 0..(10.0 / crate::sim::STEP) as usize {
            g.tick(crate::sim::STEP);
            if g.player.hp < 100.0 {
                return;
            }
        }
        panic!("the zombie never got up the stairs to the player: it is at {:?}", g.bots[0].body.pos);
    }

    #[test]
    fn running_up_and_down_the_stairs_is_smooth() {
        let (mut g, b) = lone_building(2);
        g.player.body.pos = [b.min[0] + 0.8, 0.0, b.min[2] + 1.0];
        g.input.w = true;
        g.input.sprint = true;
        // up the flight, then turn round at the top and run back down it
        for (yaw, to) in [(std::f32::consts::FRAC_PI_2, 3.3), (-std::f32::consts::FRAC_PI_2, 0.0)] {
            g.player.yaw = yaw;
            let mut eye = g.player.eye()[1];
            let mut airborne = 0;
            for _ in 0..(2.5 / crate::sim::STEP) as usize {
                g.tick(crate::sim::STEP);
                let e = g.player.eye()[1];
                // a sprint up a smooth slope this steep rises 0.075 a tick; a bare step would jump 0.3
                assert!((e - eye).abs() < 0.12, "the eye jumped {} in one tick at {:?}", e - eye, g.player.body.pos);
                eye = e;
                airborne += !g.player.body.on_ground as usize;
            }
            assert_eq!(airborne, 0, "left the steps for {airborne} ticks");
            let y = g.player.body.pos[1];
            assert!((y - to).abs() < 0.05, "ended at height {y}, not {to}");
        }
    }
}

#[cfg(test)]
mod tower {
    use super::*;

    #[test]
    fn the_stair_round_the_clock_tower_climbs_all_the_way_to_the_balcony() {
        let mut g = crate::sim::Game::new(1);
        g.bots.clear();
        g.player.hp = 1e6;
        g.player.body.pos = [-1.5, 0.6, 3.7];
        // once round the corners and on to the east face's, then up the last flight on the north face and out
        // onto the deck
        let c = 3.7;
        let route = [[c, c], [c, -c], [-c, -c], [-c, c], [c, c], [c, -c], [-2.7, -c], [-4.6, 0.0]];
        g.input.w = true;
        for (i, to) in route.iter().enumerate() {
            for _ in 0..(8.0 / crate::sim::STEP) as usize {
                let p = g.player.body.pos;
                let (dx, dz) = (to[0] - p[0], to[1] - p[2]);
                if dx.hypot(dz) < 0.25 {
                    break;
                }
                g.player.yaw = dx.atan2(-dz);
                g.tick(crate::sim::STEP);
            }
            let p = g.player.body.pos;
            assert!((to[0] - p[0]).hypot(to[1] - p[2]) < 0.3, "stuck on the way to corner {i} at {p:?}");
        }
        let y = g.player.body.pos[1];
        assert!((21.8..22.0).contains(&y), "ended at height {y}, not on the deck");
    }
}

#[cfg(test)]
mod debug {
    use super::*;
    #[test]
    #[ignore]
    fn stairs_trace() {
        let mut b = B { w: Vec::new(), c: Vec::new(), m: Vec::new(), decals: Vec::new(), keep_out: Vec::new(), rng: Rng(7), ruin: 0.0 };
        for a in perimeter() {
            b.put(a.min, a.max, CONCRETE);
        }
        b.building(-8.0, -5.0, 8.0, 5.0, 3, BRICK, 0.0, [false, true, false, false]);
        let walls = b.w;
        let c = vec![[0.5; 3]; walls.len()];
        let mats = vec![0; walls.len()];
        let mut g = crate::sim::Game::with_map(1, Map { walls, colors: c, mats, decals: Vec::new(), spawns: Vec::new() });
        g.bots.clear();
        g.player.body.pos = [-7.2, 0.0, -3.6];
        g.player.yaw = std::f32::consts::FRAC_PI_2;
        g.input.w = true;
        for i in 0..240 {
            g.tick(crate::sim::STEP);
            if i % 15 == 0 {
                println!("{:.2} {:?} vel {:?} hp {}", i as f32 / 60.0, g.player.body.pos, g.player.body.vel, g.player.hp);
            }
        }
    }

    #[test]
    #[ignore]
    fn ascii() {
        use crate::grid::Grid;
        use crate::nav::Nav;
        let m = build(1);
        let world = Grid::new(m.walls, 4.0);
        let mut nav = Nav::new(&world, HALF);
        nav.set_goal([0.0, 0.0, 14.0]);
        println!("{:?}", nav.coverage());
        for zi in 0..100 {
            let mut row = String::new();
            for xi in 0..100 {
                let p = [-99.5 + 2.0 * xi as f32, 0.0, -99.5 + 2.0 * zi as f32];
                row.push(if !nav.open_at(p) { '#' } else if nav.reachable(p) { '.' } else { 'X' });
            }
            println!("{row}");
        }
    }
}
