//! The suburb: a 250 m square of houses on a grid of streets. A main road along the north runs past a strip
//! mall, a gas station and a big hardware store; south of it three rows of streets are lined with houses, with a
//! park in the middle, and past them are a school with its sports field, a cul-de-sac and a strip of woods
//! under a water tower. All round it the land rises through wooded foothills, with a fire lookout, a cabin and a
//! radio mast up in them, to mountains that close the map in.
//!
//! The streets and the big buildings are authored; the seed picks each house's plan, colours, garage, cars and
//! garden, and the lie of the hills.

use super::*;
use crate::terrain::fbm;

/// Half the side of the map.
const SIZE: f32 = 190.0;
/// The flat suburb reaches this far from the middle on x and z; the hills begin past it.
const CORE: f32 = 125.0;
const WALK: f32 = 2.0;

/// A level spot cut into the hills to build on: where, how far it is flat, and how high.
struct Pad {
    x: f32,
    z: f32,
    r: f32,
    h: f32,
}

fn smooth(a: f32, b: f32, t: f32) -> f32 {
    let u = ((t - a) / (b - a)).clamp(0.0, 1.0);
    u * u * (3.0 - 2.0 * u)
}

/// Height of the land at (x, z) before anything is levelled: flat in the suburb, then rolling foothills gentle
/// enough to walk, then mountains too steep to, rising nearer in some places than others.
fn land(x: f32, z: f32, seed: u32) -> f32 {
    let d = (x.abs() - CORE).max(0.0).hypot((z.abs() - CORE).max(0.0));
    if d <= 0.0 {
        return 0.0;
    }
    let foot = 6.0 * smooth(0.0, 26.0, d)
        + 5.0 * (fbm(x / 40.0, z / 40.0, seed, 3) * 0.5 + 0.5) * smooth(0.0, 18.0, d);
    // ridges with sharp crests, peaks and saddles between them, some ranges higher than others, rising gently
    // at the foot and steeply to the top
    let start = 30.0 + 14.0 * fbm(x / 90.0, z / 90.0, seed + 7, 2);
    let ridge = 1.0 - fbm(x / 45.0, z / 45.0, seed + 13, 3).abs() * 1.6;
    let range = 0.6 + 0.4 * (fbm(x / 120.0, z / 120.0, seed + 5, 2) * 0.5 + 0.5);
    let peak = 25.0 + 70.0 * ridge.max(0.0) * range;
    let crags = 6.0 * fbm(x / 12.0, z / 12.0, seed + 21, 3);
    let rise = smooth(start, start + 34.0, d).powf(1.4);
    (foot + (peak + crags) * rise) * smooth(0.0, 8.0, d)
}

/// The land with the pads levelled into it, each blending back into the slope around it.
fn height(x: f32, z: f32, seed: u32, pads: &[Pad]) -> f32 {
    pads.iter().fold(land(x, z, seed), |h, p| {
        h + (p.h - h) * (1.0 - smooth(p.r, p.r + 12.0, (x - p.x).hypot(z - p.z)))
    })
}

/// A house lot seen from its street: `u` runs along the street (world x) from `x0` to `x1`, `v` back from the
/// front edge at `zf` into the lot, which lies to the south of the street if `dir` is 1 and the north if -1.
#[derive(Clone, Copy)]
struct Lot {
    x0: f32,
    x1: f32,
    zf: f32,
    dir: f32,
    depth: f32,
    /// fence the back of the lot, which two lots back to back share, so only one of them builds it
    back: bool,
    /// last in its row, so it also fences its east side
    last: bool,
}

impl Lot {
    fn z(&self, v: f32) -> f32 {
        self.zf + self.dir * v
    }

    /// The z span from `v0` to `v1` into the lot, lowest first.
    fn zs(&self, v0: f32, v1: f32) -> (f32, f32) {
        let (a, b) = (self.z(v0), self.z(v1));
        (a.min(b), a.max(b))
    }
}

/// Windows along a wall from `a0` to `a1`, about every 3 m, clear of the openings already in `ops`.
fn windows(ops: &mut Vec<Op>, a0: f32, a1: f32) {
    let n = ((a1 - a0 - 1.0) / 3.0).floor().max(0.0) as i32;
    for k in 0..n {
        let c = a0 + 0.5 + (k as f32 + 0.5) * (a1 - a0 - 1.0) / n as f32;
        if ops
            .iter()
            .all(|o| (o.c - c).abs() > (o.w + 1.4) / 2.0 + 0.4)
        {
            ops.push(Op {
                c,
                w: 1.4,
                sill: 0.9,
                head: 2.2,
            });
        }
    }
}

/// A street: its asphalt, and whether it runs along x.
struct Street {
    r: [f32; 4],
    along_x: bool,
}

impl B {
    /// A box in a lot's own coordinates.
    fn lot_box(&mut self, l: &Lot, u0: f32, y0: f32, v0: f32, u1: f32, y1: f32, v1: f32, col: Col) {
        let (z0, z1) = l.zs(v0, v1);
        self.bx(u0, y0, z0, u1, y1, z1, col);
    }

    /// A wall running back into a lot at `u`, from `v0` to `v1`, with openings placed along v.
    fn lot_side(
        &mut self,
        l: &Lot,
        u: f32,
        v0: f32,
        v1: f32,
        thick: f32,
        y0: f32,
        y1: f32,
        ops: &[Op],
        col: Col,
    ) {
        let (z0, z1) = l.zs(v0, v1);
        let ops: Vec<Op> = ops
            .iter()
            .map(|o| Op {
                c: l.z(o.c),
                w: o.w,
                sill: o.sill,
                head: o.head,
            })
            .collect();
        self.wall(2, z0, z1, u, thick, y0, y1, &ops, col);
    }

    fn tree(&mut self, x: f32, z: f32, s: f32) {
        // on whatever the ground is doing, its foot sunk a little so a slope never shows under it
        let g = self.ground_at(x, z);
        self.bx(
            x - 0.18,
            g - 0.3,
            z - 0.18,
            x + 0.18,
            g + 2.6 * s,
            z + 0.18,
            WOOD,
        );
        // two crossed boxes and a smaller one on top, so it reads round from every side
        let (r, q, y) = (1.8 * s, 1.25 * s, g + 2.3 * s);
        self.bx(x - r, y, z - q, x + r, y + 1.5 * s, z + q, LEAF);
        self.bx(x - q, y - 0.2 * s, z - r, x + q, y + 1.7 * s, z + r, LEAF);
        let t = 0.9 * s;
        self.bx(x - t, y + 1.5 * s, z - t, x + t, y + 2.5 * s, z + t, LEAF);
    }

    /// A pine: a tall trunk and tiers narrowing to a point, the lowest clear of a head.
    fn pine(&mut self, x: f32, z: f32, s: f32) {
        let g = self.ground_at(x, z);
        self.bx(
            x - 0.2,
            g - 0.4,
            z - 0.2,
            x + 0.2,
            g + 3.0 * s,
            z + 0.2,
            WOOD,
        );
        for k in 0..4 {
            let (r, y) = ((1.7 - 0.38 * k as f32) * s, g + (2.0 + 1.25 * k as f32) * s);
            self.bx(x - r, y, z - r, x + r, y + 1.35 * s, z + r, HEDGE);
        }
        self.bx(
            x - 0.25 * s,
            g + 7.0 * s,
            z - 0.25 * s,
            x + 0.25 * s,
            g + 7.8 * s,
            z + 0.25 * s,
            HEDGE,
        );
    }

    /// Trees dotted over a rectangle wherever there is room, `n` of them if they fit.
    fn woods(&mut self, x0: f32, z0: f32, x1: f32, z1: f32, n: usize) {
        let mut placed = 0;
        for _ in 0..n * 30 {
            if placed == n {
                break;
            }
            let (x, z, s) = (
                self.rng.range(x0 + 2.0, x1 - 2.0),
                self.rng.range(z0 + 2.0, z1 - 2.0),
                self.rng.range(0.9, 1.4),
            );
            let r = 2.4 * s;
            let near = Aabb {
                min: [x - r, 0.0, z - r],
                max: [x + r, 1.0, z + r],
            };
            if self.keep_out.iter().any(|k| overlaps(&near, k))
                || self.w.iter().any(|w| {
                    w.min[1] < 6.0
                        && overlaps(
                            &near,
                            &Aabb {
                                min: [w.min[0], 0.0, w.min[2]],
                                max: [w.max[0], 1.0, w.max[2]],
                            },
                        )
                })
            {
                continue;
            }
            self.tree(x, z, s);
            placed += 1;
        }
    }

    /// A street light at the edge of a road, its arm reaching `(dx, dz)` out over it.
    fn lamp(&mut self, x: f32, z: f32, dx: f32, dz: f32) {
        self.bx(x - 0.1, 0.0, z - 0.1, x + 0.1, 6.5, z + 0.1, STEEL);
        let (ax, az) = (x + dx * 1.8, z + dz * 1.8);
        self.bx(
            x.min(ax) - 0.06,
            6.3,
            z.min(az) - 0.06,
            x.max(ax) + 0.06,
            6.42,
            z.max(az) + 0.06,
            STEEL,
        );
        self.bx(ax - 0.25, 6.15, az - 0.25, ax + 0.25, 6.3, az + 0.25, BLACK);
    }

    // ---- houses --------------------------------------------------------------------------------

    /// A house on a lot: one or two storeys under a stepped gable roof, often with a garage beside it, a
    /// driveway, a lawn out front and a fenced garden behind. Gives back a spot upstairs, if it has one.
    fn house(&mut self, l: Lot) -> Option<V3> {
        let w = l.x1 - l.x0;
        let garage = self.rng.f() < 0.75;
        let two = self.rng.f() < 0.5;
        let left = self.rng.f() < 0.5;
        // house and garage across the lot, with 2 m spare at each side
        let hw = if garage {
            w - 10.0
        } else {
            (w - 7.0).min(13.0)
        };
        let (h0, g0) = match (garage, left) {
            (true, true) => (l.x0 + 8.0, l.x0 + 2.0),
            (true, false) => (l.x0 + 2.0, l.x0 + 2.0 + hw),
            (false, true) => (l.x1 - 2.0 - hw, 0.0),
            (false, false) => (l.x0 + 2.0, 0.0),
        };
        let h1 = h0 + hw;
        let (v0, v1) = {
            let sv = self.rng.range(5.0, 6.5);
            (sv, sv + self.rng.range(9.0, 10.0))
        };
        let (za, zb) = l.zs(v0, v1);
        self.decal(h0, za, h1, zb, WOOD);
        let r = self.rng.f();
        let col = if r < 0.65 {
            *self.one_of(&SIDING)
        } else if r < 0.9 {
            *self.one_of(&[BRICK, BRICK_LIGHT])
        } else {
            PLASTER
        };
        let roof = *self.one_of(&[SHINGLE, SHINGLE, TILE]);
        self.ruin = if self.rng.f() < 0.3 {
            self.rng.range(0.1, 0.35)
        } else {
            0.0
        };
        let stories = if two { 2 } else { 1 };
        // the garage's wall is the house's; the stairs go up the other side
        let (inner, outer) = if left { (h0, h1) } else { (h1, h0) };
        let lane = if outer == h1 {
            h1 - T / 2.0 - 0.8
        } else {
            h0 + T / 2.0 + 0.8
        };
        let foot = v0 + T / 2.0 + 0.9;
        let door = self.pick(h0 + 2.0, h1 - 2.0);
        let patio = self.pick(h0 + 2.0, h1 - 2.0);
        let part = v0 + (v1 - v0) * 0.5;
        let part_door = self.pick(h0 + 2.0, h1 - 3.5);
        // the partition stops short of the stairs, which climb through where it would be
        let (p0, p1) = if !two {
            (h0, h1)
        } else if outer == h1 {
            (h0, h1 - 2.0)
        } else {
            (h0 + 2.0, h1)
        };
        let part_door = part_door.clamp(p0 + 1.4, p1 - 1.4);

        for s in 0..stories {
            let y0 = s as f32 * STORY;
            let y1 = y0 + CLEAR;
            let mut front = Vec::new();
            let mut back = Vec::new();
            if s == 0 {
                front.push(Op {
                    c: door,
                    w: 2.2,
                    sill: 0.0,
                    head: 2.3,
                });
                back.push(Op {
                    c: patio,
                    w: 2.4,
                    sill: 0.0,
                    head: 2.3,
                });
            }
            windows(&mut front, h0, h1);
            windows(&mut back, h0, h1);
            self.wall(0, h0, h1, l.z(v0), T, y0, y1, &front, col);
            self.wall(0, h0, h1, l.z(v1), T, y0, y1, &back, col);
            for u in [h0, h1] {
                let mut ops = Vec::new();
                if s == 0 && garage && u == inner {
                    ops.push(Op {
                        c: v0 + 4.5,
                        w: 2.2,
                        sill: 0.0,
                        head: 2.3,
                    });
                }
                // the wall the flight runs up stays whole, so the steps never cut across a window
                if !(two && s == 0 && u == outer) && !(garage && s == 0 && u == inner) {
                    windows(&mut ops, v0, v1);
                }
                self.lot_side(&l, u, v0, v1, T, y0, y1, &ops, col);
            }
            self.wall(
                0,
                p0,
                p1,
                l.z(part),
                T * 0.6,
                y0,
                y1,
                &[Op {
                    c: part_door,
                    w: 2.2,
                    sill: 0.0,
                    head: 2.3,
                }],
                PLASTER,
            );

            let last = s + 1 == stories;
            let top = [h0 - T / 2.0, za - T / 2.0, h1 + T / 2.0, zb + T / 2.0];
            let hole = (two && s == 0).then(|| {
                let (u0, u1) = if outer == h1 {
                    (lane - 1.3, lane + 0.8)
                } else {
                    (lane - 0.8, lane + 1.3)
                };
                let (z0, z1) = l.zs(foot, foot + 10.0 * TREAD);
                [u0, z0, u1, z1]
            });
            self.slab(top, y1, y0 + STORY, hole, if last { PLASTER } else { WOOD });
        }
        if two {
            self.stairs(
                lane,
                l.z(foot),
                if l.dir > 0.0 { 2 } else { 3 },
                11,
                1.6,
                0.0,
                true,
                WOOD,
            );
        }
        self.ruin = 0.0;

        // the roof: slabs stepping in from front and back up to a ridge along the street
        let base = stories as f32 * STORY;
        let mut k = 0;
        loop {
            let (a, b) = (v0 - 0.6 + k as f32 * 0.8, v1 + 0.6 - k as f32 * 0.8);
            if b - a < 1.0 {
                break;
            }
            self.lot_box(
                &l,
                h0 - 0.6,
                base + k as f32 * 0.38,
                a,
                h1 + 0.6,
                base + (k + 1) as f32 * 0.38,
                b,
                roof,
            );
            k += 1;
        }
        if self.rng.f() < 0.4 {
            let (cu, cv) = (if left { h1 - 1.6 } else { h0 + 0.8 }, part);
            self.lot_box(
                &l,
                cu,
                base,
                cv - 0.4,
                cu + 0.8,
                base + k as f32 * 0.38 + 0.9,
                cv + 0.4,
                BRICK,
            );
        }

        // the garage, a car in it or on the drive
        let drive = if garage {
            (g0 + 0.5, g0 + 5.5)
        } else if left {
            (l.x0 + 1.5, h0 - 1.5)
        } else {
            (h1 + 1.5, l.x1 - 1.5)
        };
        let du = (drive.0 + drive.1) / 2.0;
        self.decal(drive.0, l.zs(0.0, v0).0, drive.1, l.zs(0.0, v0).1, PAVING);
        if garage {
            let (gv1, gh) = (v0 + 7.5, 2.8);
            let outside = if left { g0 } else { g0 + 6.0 };
            let (gz0, gz1) = l.zs(v0, gv1);
            self.decal(g0, gz0, g0 + 6.0, gz1, PAVING);
            self.wall(
                0,
                g0,
                g0 + 6.0,
                l.z(v0),
                T,
                0.0,
                gh,
                &[Op {
                    c: g0 + 3.0,
                    w: 4.6,
                    sill: 0.0,
                    head: 2.4,
                }],
                col,
            );
            self.wall(
                0,
                g0,
                g0 + 6.0,
                l.z(gv1),
                T,
                0.0,
                gh,
                &[Op {
                    c: g0 + 3.0,
                    w: 1.4,
                    sill: 1.0,
                    head: 2.0,
                }],
                col,
            );
            self.lot_side(&l, outside, v0, gv1, T, 0.0, gh, &[], col);
            self.lot_box(
                &l,
                g0 - T / 2.0,
                gh,
                v0 - 0.4,
                g0 + 6.0 + T / 2.0,
                gh + 0.3,
                gv1 + T / 2.0,
                roof,
            );
            // a car parked well in, so there is room to walk round its nose, or else shelves along the back
            if self.rng.f() < 0.5 {
                self.car(g0 + 3.0, l.z(v0 + 4.6), false);
            } else {
                self.lot_box(&l, g0 + 0.6, 0.0, gv1 - 0.8, g0 + 5.4, 1.8, gv1 - 0.3, WOOD);
            }
        }
        // a car on the drive would shut a garage door, so only where there is none
        if !garage && self.rng.f() < 0.6 && v0 > 5.4 {
            self.car(du, l.z(v0 - 2.9), false);
        }
        // a path to the door, a mailbox by the drive, shrubs under the front windows
        let (pz0, pz1) = l.zs(WALK, v0);
        self.decal(door - 0.6, pz0, door + 0.6, pz1, PAVING);
        let mu = if du < door {
            drive.1 + 0.6
        } else {
            drive.0 - 0.6
        };
        self.lot_box(
            &l,
            mu - 0.05,
            0.0,
            WALK + 0.4,
            mu + 0.05,
            1.0,
            WALK + 0.5,
            STEEL,
        );
        self.lot_box(
            &l,
            mu - 0.15,
            1.0,
            WALK + 0.2,
            mu + 0.15,
            1.3,
            WALK + 0.7,
            BLACK,
        );
        for u in [h0 + 1.2, h1 - 1.2] {
            if (u - door).abs() > 2.2 {
                self.lot_box(&l, u - 0.7, 0.0, v0 - 1.3, u + 0.7, 0.9, v0 - 0.3, HEDGE);
            }
        }
        if self.rng.f() < 0.5 {
            // a tree in the front lawn, on the side away from the drive
            let u = if du < (l.x0 + l.x1) / 2.0 {
                l.x1 - 3.0
            } else {
                l.x0 + 3.0
            };
            let s = self.rng.range(0.9, 1.2);
            self.tree(u, l.z(WALK + 1.6), s);
        }

        // the garden: a fence round the back, and a shed, a trampoline or a tree in it
        let (fz0, fz1) = l.zs(v1, l.depth);
        if l.back {
            self.wall(0, l.x0, l.x1, l.z(l.depth), 0.12, 0.0, 1.6, &[], WOOD);
        }
        let mut sides = vec![l.x0];
        if l.last {
            sides.push(l.x1);
        }
        for u in sides {
            self.wall(2, fz0, fz1, u, 0.12, 0.0, 1.6, &[], WOOD);
        }
        let yard = l.depth - v1;
        if yard > 5.0 {
            let pick = self.rng.f();
            if pick < 0.35 {
                let u = if left { l.x1 - 3.0 } else { l.x0 + 1.6 };
                self.lot_box(
                    &l,
                    u - 0.5,
                    0.0,
                    l.depth - 2.6,
                    u + 1.4,
                    2.2,
                    l.depth - 0.4,
                    WOOD,
                );
                self.lot_box(
                    &l,
                    u - 0.6,
                    2.2,
                    l.depth - 2.8,
                    u + 1.6,
                    2.4,
                    l.depth - 0.2,
                    roof,
                );
            } else if pick < 0.6 {
                let (u, v) = ((h0 + h1) / 2.0, v1 + yard / 2.0);
                for (du, dv) in [(-1.3, -1.3), (1.3, -1.3), (-1.3, 1.3), (1.3, 1.3)] {
                    self.lot_box(
                        &l,
                        u + du - 0.05,
                        0.0,
                        v + dv - 0.05,
                        u + du + 0.05,
                        0.8,
                        v + dv + 0.05,
                        STEEL,
                    );
                }
                self.lot_box(&l, u - 1.6, 0.8, v - 1.6, u + 1.6, 0.9, v + 1.6, BLACK);
            } else if pick < 0.9 {
                let (s, u) = (self.rng.range(1.0, 1.4), self.pick(l.x0 + 3.5, l.x1 - 3.5));
                self.tree(u, l.z(l.depth - 3.0), s);
            }
        }
        // the lawn, under everything else on the lot
        let (lz0, lz1) = l.zs(0.0, l.depth);
        self.decals.insert(
            0,
            (
                Aabb {
                    min: [l.x0, 0.0, lz0],
                    max: [l.x1, 0.02, lz1],
                },
                GRASS,
            ),
        );
        two.then(|| [(p0 + p1) / 2.0, STORY, l.z((v0 + part) / 2.0)])
    }

    /// A row of lots along a street, filling each stretch between cross streets with as many as fit.
    fn row(&mut self, zf: f32, dir: f32, depth: f32, back: bool, stretches: &[(f32, f32)]) {
        for &(x0, x1) in stretches {
            let n = ((x1 - x0) / 19.0).floor().max(1.0) as usize;
            let w = (x1 - x0) / n as f32;
            for i in 0..n {
                let lot = Lot {
                    x0: x0 + i as f32 * w,
                    x1: x0 + (i + 1) as f32 * w,
                    zf,
                    dir,
                    depth,
                    back,
                    last: i + 1 == n,
                };
                self.house(lot);
            }
        }
    }

    // ---- streets -------------------------------------------------------------------------------

    /// The streets, with pavements along both sides, a dashed line down the middle, lamps and parked cars.
    fn streets(&mut self, streets: &[Street]) {
        for s in streets {
            let [x0, z0, x1, z1] = s.r;
            if s.along_x {
                self.decal(x0, z0 - WALK, x1, z0, PAVING);
                self.decal(x0, z1, x1, z1 + WALK, PAVING);
            } else {
                self.decal(x0 - WALK, z0, x0, z1, PAVING);
                self.decal(x1, z0, x1 + WALK, z1, PAVING);
            }
        }
        for s in streets {
            let [x0, z0, x1, z1] = s.r;
            self.decal(x0, z0, x1, z1, ASPHALT);
        }
        for s in streets {
            let [x0, z0, x1, z1] = s.r;
            let (a0, a1, mid) = if s.along_x {
                (x0, x1, (z0 + z1) / 2.0)
            } else {
                (z0, z1, (x0 + x1) / 2.0)
            };
            let mut a = a0 + 2.0;
            while a + 3.0 < a1 {
                if s.along_x {
                    self.decal(a, mid - 0.08, a + 3.0, mid + 0.08, LINE);
                } else {
                    self.decal(mid - 0.08, a, mid + 0.08, a + 3.0, LINE);
                }
                a += 7.0;
            }
        }
        // nothing stands where another street crosses
        let crossing = |x: f32, z: f32, s: &Street| {
            streets.iter().any(|o| {
                !std::ptr::eq(o, s)
                    && x > o.r[0] - 4.0
                    && x < o.r[2] + 4.0
                    && z > o.r[1] - 4.0
                    && z < o.r[3] + 4.0
            })
        };
        for s in streets {
            let [x0, z0, x1, z1] = s.r;
            let (a0, a1) = if s.along_x { (x0, x1) } else { (z0, z1) };
            let mut a = a0 + 9.0;
            let mut side = 0;
            while a + 4.0 < a1 {
                let mid = if s.along_x {
                    [a, (z0 + z1) / 2.0]
                } else {
                    [(x0 + x1) / 2.0, a]
                };
                if crossing(mid[0], mid[1], s) {
                    a += 3.0;
                    continue;
                }
                // lamps on alternate sides, just off the kerb
                if s.along_x {
                    let (z, dz) = if side == 0 {
                        (z0 - 0.4, 1.0)
                    } else {
                        (z1 + 0.4, -1.0)
                    };
                    self.lamp(a, z, 0.0, dz);
                } else {
                    let (x, dx) = if side == 0 {
                        (x0 - 0.4, 1.0)
                    } else {
                        (x1 + 0.4, -1.0)
                    };
                    self.lamp(x, a, dx, 0.0);
                }
                side ^= 1;
                a += 26.0;
            }
            let mut a = a0 + 14.0;
            while a + 8.0 < a1 {
                let mid = if s.along_x {
                    [a, (z0 + z1) / 2.0]
                } else {
                    [(x0 + x1) / 2.0, a]
                };
                if crossing(mid[0], mid[1], s) {
                    a += 3.0;
                    continue;
                }
                if self.rng.f() < 0.22 {
                    let near = self.rng.f() < 0.5;
                    if s.along_x {
                        self.car(a, if near { z0 + 1.3 } else { z1 - 1.3 }, true);
                    } else {
                        self.car(if near { x0 + 1.3 } else { x1 - 1.3 }, a, false);
                    }
                } else if self.rng.f() < 0.04 {
                    // left in the middle of the road, across it
                    if s.along_x {
                        self.car(a, (z0 + z1) / 2.0, false);
                    } else {
                        self.car((x0 + x1) / 2.0, a, true);
                    }
                }
                a += 11.0;
            }
        }
    }

    // ---- the big buildings ---------------------------------------------------------------------

    /// A row of shops with big windows on a car park, its roof reached by a steel stair round the back.
    fn strip_mall(&mut self, x0: f32, z0: f32, x1: f32, z1: f32) {
        self.reserve(x0 - 1.0, z0 - 2.5, x1 + 1.0, z1 + 2.0);
        let n = ((x1 - x0) / 12.0).floor() as usize;
        let shop = (x1 - x0) / n as f32;
        let col = *self.one_of(&[BRICK_LIGHT, PLASTER, CONCRETE]);
        let mut front = Vec::new();
        let mut back = Vec::new();
        for i in 0..n {
            let a = x0 + i as f32 * shop;
            front.push(Op {
                c: a + 2.2,
                w: 2.2,
                sill: 0.0,
                head: 2.5,
            });
            front.push(Op {
                c: a + shop / 2.0 + 1.6,
                w: shop - 6.4,
                sill: 0.6,
                head: 2.6,
            });
            if i % 2 == 0 {
                back.push(Op {
                    c: a + 2.0,
                    w: 2.2,
                    sill: 0.0,
                    head: 2.3,
                });
            }
        }
        self.wall(0, x0, x1, z1, T, 0.0, CLEAR, &front, col);
        self.wall(0, x0, x1, z0, T, 0.0, CLEAR, &back, col);
        for x in [x0, x1] {
            self.wall(
                2,
                z0,
                z1,
                x,
                T,
                0.0,
                CLEAR,
                &[Op {
                    c: (z0 + z1) / 2.0,
                    w: 1.6,
                    sill: 1.0,
                    head: 2.3,
                }],
                col,
            );
        }
        for i in 1..n {
            let a = x0 + i as f32 * shop;
            // every other pair of shops knocked through at the back
            let ops: Vec<Op> = if i % 2 == 1 {
                vec![Op {
                    c: z0 + 2.5,
                    w: 2.2,
                    sill: 0.0,
                    head: 2.3,
                }]
            } else {
                Vec::new()
            };
            self.wall(2, z0, z1, a, T * 0.6, 0.0, CLEAR, &ops, PLASTER);
            // counters
            self.bx(a - shop + 2.0, 0.0, z0 + 3.0, a - 4.0, 1.0, z0 + 3.8, WOOD);
        }
        let top = [x0 - T / 2.0, z0 - T / 2.0, x1 + T / 2.0, z1 + T / 2.0];
        self.slab(top, CLEAR, STORY, None, ROOF);
        // a tall sign band along the front with a panel for each shop, a low parapet elsewhere with a gap where
        // the stair arrives, and an awning over the pavement
        self.bx(
            top[0],
            STORY,
            top[3] - 0.3,
            top[2],
            STORY + 1.2,
            top[3],
            col,
        );
        for i in 0..n {
            let a = x0 + i as f32 * shop;
            let c = *self.one_of(&CONTAINERS);
            self.bx(
                a + 1.0,
                STORY + 0.2,
                top[3],
                a + shop - 1.0,
                STORY + 1.0,
                top[3] + 0.1,
                c,
            );
        }
        let (sx0, sx1) = (x1 - 9.0, x1 - 7.4);
        self.bx(top[0], STORY, top[1], sx0, STORY + 0.6, top[1] + 0.3, col);
        self.bx(sx1, STORY, top[1], top[2], STORY + 0.6, top[1] + 0.3, col);
        self.bx(
            top[0],
            STORY,
            top[1],
            top[0] + 0.3,
            STORY + 0.6,
            top[3],
            col,
        );
        self.bx(
            top[2] - 0.3,
            STORY,
            top[1],
            top[2],
            STORY + 0.6,
            top[3],
            col,
        );
        self.bx(x0, 2.7, z1 + 0.2, x1, 2.85, z1 + 1.8, STEEL);
        self.stairs(x1 - 15.6, z0 - 1.0, 0, 11, 1.2, 0.0, false, STEEL);
        self.bx(sx0, 0.0, z0 - 1.6, sx1, STORY, z0 - 0.2, STEEL);
    }

    fn gas_station(&mut self, x: f32, z: f32) {
        self.reserve(x - 14.0, z - 8.0, x + 14.0, z + 8.0);
        for (dx, dz) in [(-10.0, -5.0), (10.0, -5.0), (-10.0, 5.0), (10.0, 5.0)] {
            self.block(x + dx, z + dz, 0.8, 4.6, 0.8, STEEL);
        }
        self.bx(x - 12.0, 4.6, z - 7.0, x + 12.0, 5.2, z + 7.0, YELLOW);
        for dx in [-6.0, 0.0, 6.0] {
            for dz in [-2.0, 2.0] {
                self.block(x + dx, z + dz, 1.0, 1.5, 0.7, RUST);
            }
            self.block(x + dx, z, 4.0, 0.2, 1.4, CONCRETE);
        }
    }

    fn water_tower(&mut self, x: f32, z: f32) {
        for (dx, dz) in [(-3.5, -3.5), (3.5, -3.5), (-3.5, 3.5), (3.5, 3.5)] {
            self.block(x + dx, z + dz, 1.0, 13.0, 1.0, STEEL);
        }
        self.bx(x - 5.0, 13.0, z - 5.0, x + 5.0, 14.0, z + 5.0, STEEL);
        self.bx(x - 4.5, 14.0, z - 4.5, x + 4.5, 19.0, z + 4.5, SIDING[1]);
        self.bx(x - 2.0, 19.0, z - 2.0, x + 2.0, 21.0, z + 2.0, STEEL);
        self.reserve(x - 7.0, z - 7.0, x + 7.0, z + 7.0);
    }

    /// The school: a two-storey brick block, a football pitch with a stand, a basketball court and a car park.
    fn school(&mut self) {
        self.decal(-123.0, 72.0, -6.5, 123.0, GRASS);
        self.building(
            -66.0,
            78.0,
            -16.0,
            94.0,
            2,
            BRICK,
            0.15,
            [true, true, true, true],
        );
        // the pitch, lined, with a goal at each end
        let (px0, pz0, px1, pz1) = (-118.0, 78.0, -78.0, 120.0);
        self.reserve(px0, pz0, px1 + 6.0, pz1);
        let l = 0.15;
        for r in [
            [px0, pz0, px1, pz0 + l],
            [px0, pz1 - l, px1, pz1],
            [px0, pz0, px0 + l, pz1],
            [px1 - l, pz0, px1, pz1],
            [
                px0,
                (pz0 + pz1) / 2.0 - l / 2.0,
                px1,
                (pz0 + pz1) / 2.0 + l / 2.0,
            ],
        ] {
            self.decal(r[0], r[1], r[2], r[3], LINE);
        }
        let mx = (px0 + px1) / 2.0;
        for (z, d) in [(pz0, -1.0), (pz1, 1.0)] {
            for dx in [-3.7, 3.7] {
                self.block(mx + dx, z, 0.15, 2.4, 0.15, LINE);
            }
            self.bx(mx - 3.7, 2.3, z - 0.075, mx + 3.7, 2.45, z + 0.075, LINE);
            self.bx(
                mx - 3.7,
                0.0,
                z + d * 1.5 - 0.05,
                mx + 3.7,
                0.1,
                z + d * 1.5 + 0.05,
                STEEL,
            );
        }
        // the stand along the east touchline, four rows rising away from the pitch
        for i in 0..4 {
            let x = px1 + 1.5 + i as f32 * 1.0;
            self.bx(x, 0.0, 86.0, px1 + 5.5, 0.45 * (i + 1) as f32, 112.0, WOOD);
        }
        // the basketball court
        self.decal(-60.0, 100.0, -36.0, 116.0, PAVING);
        self.decal(-60.0, 107.92, -36.0, 108.08, LINE);
        for (z, d) in [(100.6, 1.0), (115.4, -1.0)] {
            self.block(-48.0, z, 0.2, 3.4, 0.2, STEEL);
            self.bx(
                -48.9,
                2.9,
                z + d * 0.2 - 0.05,
                -47.1,
                3.9,
                z + d * 0.2 + 0.05,
                LINE,
            );
        }
        self.decal(-14.0, 98.0, -6.5, 122.0, ASPHALT);
        for z in [102.0, 108.0, 118.0] {
            if self.rng.f() < 0.7 {
                self.car(-10.0, z, true);
            }
        }
    }

    /// The park in the middle: lawns crossed by paths, a pond, a gazebo, a playground and trees.
    fn park(&mut self, x0: f32, z0: f32, x1: f32, z1: f32) {
        self.decal(x0, z0, x1, z1, GRASS);
        let (mx, mz) = ((x0 + x1) / 2.0, (z0 + z1) / 2.0);
        self.decal(mx - 1.2, z0, mx + 1.2, z1, PAVING);
        self.decal(x0, mz - 1.2, x1, mz + 1.2, PAVING);
        // the gazebo where the paths cross: posts, a roof and a rail to shoot over, open on the paths
        let (gx, gz, r) = (mx, mz, 3.5);
        self.reserve(gx - r - 1.0, gz - r - 1.0, gx + r + 1.0, gz + r + 1.0);
        self.decal(gx - r, gz - r, gx + r, gz + r, WOOD);
        for (dx, dz) in [(-r, -r), (r, -r), (-r, r), (r, r)] {
            self.block(gx + dx, gz + dz, 0.3, 3.0, 0.3, WOOD);
        }
        for k in 0..4 {
            let i = k as f32 * 0.6;
            self.bx(
                gx - r - 0.4 + i,
                3.0 + k as f32 * 0.35,
                gz - r - 0.4 + i,
                gx + r + 0.4 - i,
                3.35 + k as f32 * 0.35,
                gz + r + 0.4 - i,
                SHINGLE,
            );
        }
        for (a, b) in [(-r, -1.4), (1.4, r)] {
            self.bx(
                gx + a,
                0.85,
                gz - r - 0.05,
                gx + b,
                1.0,
                gz - r + 0.05,
                WOOD,
            );
            self.bx(
                gx + a,
                0.85,
                gz + r - 0.05,
                gx + b,
                1.0,
                gz + r + 0.05,
                WOOD,
            );
            self.bx(
                gx - r - 0.05,
                0.85,
                gz + a,
                gx - r + 0.05,
                1.0,
                gz + b,
                WOOD,
            );
            self.bx(
                gx + r - 0.05,
                0.85,
                gz + a,
                gx + r + 0.05,
                1.0,
                gz + b,
                WOOD,
            );
        }
        // the pond in the north-west quarter, behind a low stone rim
        let (qx, qz) = ((x0 + mx) / 2.0, (z0 + mz) / 2.0);
        let (pw, pd, t) = (12.0, 8.0, 0.5);
        self.reserve(
            qx - pw / 2.0 - 1.0,
            qz - pd / 2.0 - 1.0,
            qx + pw / 2.0 + 1.0,
            qz + pd / 2.0 + 1.0,
        );
        self.decal(
            qx - pw / 2.0,
            qz - pd / 2.0,
            qx + pw / 2.0,
            qz + pd / 2.0,
            WATER,
        );
        self.bx(
            qx - pw / 2.0,
            0.0,
            qz - pd / 2.0,
            qx + pw / 2.0,
            0.45,
            qz - pd / 2.0 + t,
            STONE,
        );
        self.bx(
            qx - pw / 2.0,
            0.0,
            qz + pd / 2.0 - t,
            qx + pw / 2.0,
            0.45,
            qz + pd / 2.0,
            STONE,
        );
        self.bx(
            qx - pw / 2.0,
            0.0,
            qz - pd / 2.0 + t,
            qx - pw / 2.0 + t,
            0.45,
            qz + pd / 2.0 - t,
            STONE,
        );
        self.bx(
            qx + pw / 2.0 - t,
            0.0,
            qz - pd / 2.0 + t,
            qx + pw / 2.0,
            0.45,
            qz + pd / 2.0 - t,
            STONE,
        );
        // the playground in the south-east quarter: a climbing frame with a slide, and swings
        let (qx, qz) = ((mx + x1) / 2.0, (mz + z1) / 2.0);
        self.reserve(qx - 7.0, qz - 5.0, qx + 7.0, qz + 5.0);
        self.decal(qx - 7.0, qz - 5.0, qx + 7.0, qz + 5.0, DIRT);
        for (dx, dz) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
            self.block(qx - 3.0 + dx, qz + dz, 0.15, 1.8, 0.15, YELLOW);
        }
        self.bx(qx - 4.0, 1.5, qz - 1.0, qx - 2.0, 1.65, qz + 1.0, YELLOW);
        self.stairs(qx - 4.0 - 5.0 * TREAD, qz, 0, 5, 0.8, 0.0, true, STEEL);
        for i in 0..5 {
            let x = qx - 2.0 + i as f32 * 0.6;
            self.bx(
                x,
                1.5 - (i + 1) as f32 * 0.3,
                qz - 0.4,
                x + 0.6,
                1.62 - i as f32 * 0.3,
                qz + 0.4,
                CONTAINERS[0],
            );
        }
        for dz in [-2.0, 2.0] {
            self.block(qx + 4.0, qz + dz, 0.15, 2.4, 0.15, STEEL);
        }
        self.bx(qx + 3.9, 2.4, qz - 2.0, qx + 4.1, 2.55, qz + 2.0, STEEL);
        for dz in [-0.8, 0.8] {
            self.bx(
                qx + 3.75,
                0.5,
                qz + dz - 0.25,
                qx + 4.25,
                0.56,
                qz + dz + 0.25,
                BLACK,
            );
        }
        for (x, z) in [
            (mx - 4.0, z0 + 6.0),
            (mx + 4.0, z1 - 6.0),
            (x0 + 8.0, mz + 3.0),
            (x1 - 8.0, mz - 3.0),
        ] {
            self.block(x, z, 2.4, 0.6, 0.7, WOOD);
        }
        self.reserve(mx - 2.0, z0, mx + 2.0, z1);
        self.reserve(x0, mz - 2.0, x1, mz + 2.0);
        self.woods(x0, z0, x1, z1, 22);
    }
}

impl B {
    /// A fire lookout: a deck on four legs, reached by two flights of stairs, with a rail round it and a roof.
    fn lookout(&mut self, x: f32, z: f32) {
        let a = 2.5;
        for (dx, dz) in [(-a, -a), (a, -a), (-a, a), (a, a)] {
            self.block(x + dx, z + dz, 0.3, 9.5, 0.3, WOOD);
        }
        // up the south side to a landing, then back along the north side and out through the deck
        self.stairs(x - 3.3, z - 1.0, 0, 11, 1.2, 0.0, true, WOOD);
        self.bx(x + 3.3, STORY - 0.3, z - 1.6, x + 4.6, STORY, z + 1.6, WOOD);
        self.stairs(x + 3.3, z + 1.0, 1, 11, 1.2, STORY, true, WOOD);
        let deck = 2.0 * STORY;
        self.slab(
            [x - 3.5, z - 3.5, x + 3.5, z + 3.5],
            deck - 0.3,
            deck,
            Some([x - 3.4, z + 0.3, x + 1.5, z + 1.75]),
            WOOD,
        );
        for [x0, z0, x1, z1] in [
            [x - 3.5, z - 3.5, x + 3.5, z - 3.4],
            [x - 3.5, z + 3.4, x + 3.5, z + 3.5],
            [x - 3.5, z - 3.5, x - 3.4, z + 3.5],
            [x + 3.4, z - 3.5, x + 3.5, z + 3.5],
        ] {
            self.bx(x0, deck + 0.9, z0, x1, deck + 1.05, z1, WOOD);
        }
        self.bx(x - 3.9, 9.5, z - 3.9, x + 3.9, 9.8, z + 3.9, SHINGLE);
        self.bx(x - 2.6, 9.8, z - 2.6, x + 2.6, 10.3, z + 2.6, SHINGLE);
    }

    /// A hunting cabin with a pickup outside and a woodpile.
    fn cabin(&mut self, x: f32, z: f32) {
        self.building(
            x - 7.0,
            z - 5.0,
            x + 7.0,
            z + 5.0,
            1,
            WOOD,
            0.35,
            [true, true, false, true],
        );
        for k in 0..3 {
            let i = k as f32 * 1.4;
            self.bx(
                x - 7.6,
                STORY + k as f32 * 0.4,
                z - 5.6 + i,
                x + 7.6,
                STORY + (k + 1) as f32 * 0.4,
                z + 5.6 - i,
                SHINGLE,
            );
        }
        self.car(x - 2.0, z - 8.5, true);
        self.bx(x - 9.5, 0.0, z - 3.0, x - 8.3, 1.2, z + 3.0, WOOD);
    }

    /// A radio mast with crossbars and a dish, and the hut that runs it.
    fn mast(&mut self, x: f32, z: f32) {
        self.building(
            x - 4.0,
            z - 2.5,
            x + 2.0,
            z + 2.5,
            1,
            CONCRETE,
            0.0,
            [false, true, false, false],
        );
        self.block(x + 5.0, z, 1.2, 42.0, 1.2, STEEL);
        for (y, w) in [(14.0, 5.0), (26.0, 3.8), (36.0, 2.6)] {
            self.bx(
                x + 5.0 - w,
                y,
                z - 0.3,
                x + 5.0 + w,
                y + 0.5,
                z + 0.3,
                STEEL,
            );
        }
        self.bx(x + 5.6, 30.0, z - 1.2, x + 6.0, 32.4, z + 1.2, LINE);
    }

    /// The hills: the lookout, the cabin and the mast on their pads, then pines and boulders over every slope that
    /// isn't sheer.
    fn hills(&mut self, pads: &[Pad]) {
        let [look, cabin, mast] = pads else { return };
        for (p, f) in [
            (look, B::lookout as fn(&mut B, f32, f32)),
            (cabin, B::cabin),
            (mast, B::mast),
        ] {
            self.lift = p.h;
            f(self, p.x, p.z);
            self.reserve(
                p.x - p.r - 2.0,
                p.z - p.r - 2.0,
                p.x + p.r + 2.0,
                p.z + p.r + 2.0,
            );
        }
        self.lift = 0.0;
        let (mut pines, mut rocks) = (0, 0);
        for _ in 0..8000 {
            if pines >= 240 && rocks >= 70 {
                break;
            }
            let (x, z) = (
                self.rng.range(-SIZE + 4.0, SIZE - 4.0),
                self.rng.range(-SIZE + 4.0, SIZE - 4.0),
            );
            if x.abs() < CORE + 4.0 && z.abs() < CORE + 4.0 {
                continue;
            }
            let g = self.ground_at(x, z);
            let slope = (self.ground_at(x + 1.0, z) - self.ground_at(x - 1.0, z))
                .hypot(self.ground_at(x, z + 1.0) - self.ground_at(x, z - 1.0))
                / 2.0;
            if slope > 0.9 || g > 45.0 {
                continue;
            }
            let near = Aabb {
                min: [x - 2.5, 0.0, z - 2.5],
                max: [x + 2.5, 1.0, z + 2.5],
            };
            if self.keep_out.iter().any(|k| overlaps(&near, k))
                || self.w.iter().any(|w| {
                    w.max[1] > g
                        && w.min[0] < near.max[0]
                        && w.max[0] > near.min[0]
                        && w.min[2] < near.max[2]
                        && w.max[2] > near.min[2]
                })
            {
                continue;
            }
            if pines < 240 && (rocks >= 70 || self.rng.f() < 0.78) {
                let s = self.rng.range(0.95, 1.4);
                self.pine(x, z, s);
                pines += 1;
            } else {
                let (sx, sy, sz) = (
                    self.rng.range(1.2, 3.6),
                    self.rng.range(0.8, 2.6),
                    self.rng.range(1.2, 3.6),
                );
                let low = [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)]
                    .iter()
                    .map(|(a, c)| self.ground_at(x + a * sx / 2.0, z + c * sz / 2.0))
                    .fold(g, f32::min);
                self.bx(
                    x - sx / 2.0,
                    low - 0.3,
                    z - sz / 2.0,
                    x + sx / 2.0,
                    g + sy,
                    z + sz / 2.0,
                    STONE,
                );
                rocks += 1;
            }
        }
    }
}

/// The level spots in the hills, for the fire lookout, the cabin and the mast, each at the height the land has
/// there.
fn pads(seed: u32) -> Vec<Pad> {
    [
        (30.0, 146.0, 7.0),
        (-146.0, 40.0, 10.0),
        (141.0, -141.0, 8.0),
    ]
    .into_iter()
    .map(|(x, z, r)| Pad {
        x,
        z,
        r,
        h: land(x, z, seed),
    })
    .collect()
}

pub fn build(seed: u32) -> Map {
    let mut b = B::new(seed, SIZE);
    let pads = pads(seed);
    b.ground = Some(Terrain::new(SIZE, |x, z| height(x, z, seed, &pads)));
    b.perimeter();

    // the street plan: the main road along the north, three streets across, and avenues down between them
    let main = [-123.5, -78.0, 123.5, -64.0];
    let (elm, oak, pine) = (
        [-123.5, -18.5, 123.5, -9.5],
        [-123.5, 40.5, 123.5, 49.5],
        [4.5, 74.0, 123.5, 83.0],
    );
    let (centre, west, east) = (
        [-4.5, -64.0, 4.5, 104.0],
        [-74.5, -64.0, -65.5, 72.0],
        [65.5, -64.0, 74.5, 74.0],
    );
    let streets: Vec<Street> = [
        (main, true),
        (elm, true),
        (oak, true),
        (pine, true),
        (centre, false),
        (west, false),
        (east, false),
    ]
    .into_iter()
    .map(|(r, along_x)| Street { r, along_x })
    .collect();

    // the houses go in first so their lawns lie under the pavements; the park takes one block's two rows
    let all = [(-123.0, -76.5), (-63.5, -6.5), (6.5, 63.5), (76.5, 123.0)];
    let no_park = [(-123.0, -76.5), (-63.5, -6.5), (76.5, 123.0)];
    b.row(elm[1], -1.0, 26.0, true, &all);
    b.row(elm[3], 1.0, 24.5, true, &no_park);
    b.row(oak[1], -1.0, oak[1] - 15.0, false, &no_park);
    b.row(oak[3], 1.0, 72.0 - oak[3], true, &all);
    b.row(pine[3], 1.0, 104.0 - pine[3], true, &[(6.5, 123.0)]);
    b.park(6.5, elm[3] + WALK, 63.5, oak[1] - WALK);

    // the strip between the backs of the first row and the main road: grass, trees and a path
    b.decal(-123.5, -62.0, 123.5, -44.5, GRASS);
    b.decal(-123.5, -54.0, 123.5, -52.4, PAVING);
    for r in [centre, west, east] {
        b.reserve(r[0] - 3.0, -64.0, r[2] + 3.0, -44.0);
    }
    b.reserve(-123.5, -54.5, 123.5, -51.9);
    b.woods(-123.0, -62.0, 123.0, -44.5, 28);

    // the north side of the main road: the strip mall, the gas station and the hardware store, with car parks
    b.decal(-123.5, -102.0, -32.0, -80.0, ASPHALT);
    b.decal(-32.0, -122.0, 30.0, -80.0, ASPHALT);
    b.decal(30.0, -100.0, 123.5, -80.0, ASPHALT);
    for (x0, x1) in [(-118.0, -36.0), (34.0, 120.0)] {
        let mut x = x0;
        while x <= x1 {
            b.decal(x - 0.06, -100.0, x + 0.06, -95.0, LINE);
            b.decal(x - 0.06, -87.0, x + 0.06, -82.0, LINE);
            x += 2.8;
        }
    }
    b.strip_mall(-112.0, -118.0, -40.0, -102.0);
    b.gas_station(-4.0, -92.0);
    b.building(
        6.0,
        -118.0,
        20.0,
        -108.0,
        1,
        PLASTER,
        0.1,
        [false, true, true, false],
    );
    b.warehouse(36.0, -122.0, 100.0, -102.0, 7.5, BRICK_LIGHT);
    b.bx(52.0, 7.9, -101.8, 84.0, 9.6, -101.6, CONTAINERS[1]);
    for _ in 0..14 {
        let (x, z) = if b.rng.f() < 0.55 {
            (b.pick(-116.0, -38.0), *b.one_of(&[-97.5, -84.5]))
        } else {
            (b.pick(36.0, 118.0), *b.one_of(&[-97.5, -84.5]))
        };
        b.car(x, z, false);
    }
    for (x, z) in [(104.0, -116.0), (108.0, -116.0), (-118.0, -121.0)] {
        b.block(x, z, 3.0, 1.6, 1.8, CONTAINERS[2]);
    }

    // the south: the school, the cul-de-sac's turning circle, and the woods under the water tower
    b.school();
    b.decal(-9.0, 95.0, 9.0, 106.0, ASPHALT);
    b.decal(6.5, 104.0, 123.5, 123.5, GRASS);
    b.water_tower(110.0, 114.0);
    b.woods(6.5, 106.0, 123.0, 123.0, 26);

    b.hills(&pads);
    b.streets(&streets);
    b.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::Grid;
    use crate::nav::Nav;

    #[test]
    fn the_suburb_stays_inside_its_walls() {
        for seed in 1..6 {
            let m = build(seed);
            assert_eq!(m.walls.len(), m.colors.len());
            for w in &m.walls {
                assert!(
                    w.min[0] >= -SIZE - 1.1
                        && w.max[0] <= SIZE + 1.1
                        && w.min[2] >= -SIZE - 1.1
                        && w.max[2] <= SIZE + 1.1,
                    "seed {seed}: {w:?}"
                );
                assert!(
                    (0..3).all(|i| w.max[i] > w.min[i]),
                    "seed {seed}: empty box {w:?}"
                );
            }
            assert!(
                m.spawns.len() > 60,
                "seed {seed}: only {} spawn points",
                m.spawns.len()
            );
        }
    }

    #[test]
    fn every_part_of_the_suburb_can_be_walked_to() {
        for seed in 1..4 {
            let m = build(seed);
            let world = Grid::new(m.walls, 4.0).with_ground(m.ground);
            let mut nav = Nav::new(&world, SIZE);
            nav.set_goal([0.0, 0.0, 14.0]);
            // the whole suburb, and most of the foothills: the first 25 m of them, where the land is still gentle
            let (mut town, mut hills) = ([0, 0], [0, 0]);
            for i in 0..(2.0 * SIZE) as usize * (2.0 * SIZE) as usize {
                let (x, z) = (
                    -SIZE + 0.5 + (i % (2 * SIZE as usize)) as f32,
                    -SIZE + 0.5 + (i / (2 * SIZE as usize)) as f32,
                );
                let d = (x.abs() - CORE).max(0.0).hypot((z.abs() - CORE).max(0.0));
                let p = [x, land(x, z, seed), z];
                let tally = if d == 0.0 {
                    &mut town
                } else if d < 25.0 {
                    &mut hills
                } else {
                    continue;
                };
                if nav.open_at(p) {
                    tally[0] += 1;
                    tally[1] += nav.reachable(p) as usize;
                }
            }
            println!("seed {seed}: town {town:?}, foothills {hills:?}");
            assert!(
                town[1] * 100 >= town[0] * 98,
                "seed {seed}: only {} of {} open cells in the suburb can reach the middle",
                town[1],
                town[0]
            );
            assert!(
                hills[1] * 100 >= hills[0] * 85,
                "seed {seed}: only {} of {} open cells in the foothills can reach the middle",
                hills[1],
                hills[0]
            );
            let [look, cabin, mast] = &pads(seed)[..] else {
                panic!()
            };
            let up_in_the_hills = [
                (
                    "fire lookout's deck",
                    [look.x - 1.5, look.h + 2.0 * STORY, look.z - 2.5],
                ),
                ("cabin", [cabin.x, cabin.h, cabin.z]),
                ("mast's hut", [mast.x - 1.0, mast.h, mast.z]),
            ];
            for (name, p) in [
                ("strip mall", [-76.0, 0.0, -110.0]),
                ("strip mall roof", [-60.0, 3.3, -112.0]),
                ("hardware store", [68.0, 0.0, -112.0]),
                ("gas station", [-4.0, 0.0, -92.0]),
                ("school", [-30.0, 0.0, 90.0]),
                ("school roof", [-40.0, 6.6, 86.0]),
                ("pitch", [-98.0, 0.0, 99.0]),
                ("park", [20.0, 0.0, 30.0]),
                ("woods", [60.0, 0.0, 115.0]),
            ]
            .into_iter()
            .chain(up_in_the_hills)
            {
                assert!(nav.reachable(p), "seed {seed}: {name} is cut off");
            }
        }
    }

    #[test]
    fn every_house_can_be_walked_into_and_up() {
        // both ways round, on lots as narrow and as wide as the rows make them
        let mut upstairs = 0;
        for seed in 1..16 {
            let (dir, w) = if seed % 2 == 0 {
                (1.0, 19.0)
            } else {
                (-1.0, 23.25)
            };
            let mut b = B::new(seed, 40.0);
            b.perimeter();
            let up = b.house(Lot {
                x0: -w / 2.0,
                x1: w / 2.0,
                zf: -12.0 * dir,
                dir,
                depth: 25.0,
                back: true,
                last: true,
            });
            let world = Grid::new(b.finish().walls, 4.0);
            let mut nav = Nav::new(&world, 40.0);
            nav.set_goal([0.0, 0.0, -30.0 * dir]);
            let shut: Vec<[f32; 2]> = (0..80 * 80)
                .map(|i| [-39.5 + (i % 80) as f32, -39.5 + (i / 80) as f32])
                .filter(|p| nav.open_at([p[0], 0.0, p[1]]) && !nav.reachable([p[0], 0.0, p[1]]))
                .collect();
            assert!(
                shut.is_empty(),
                "seed {seed}: the ground at {shut:?} is shut off"
            );
            if let Some(at) = up {
                assert!(
                    nav.reachable(at),
                    "seed {seed}: the upstairs at {at:?} can't be reached"
                );
                upstairs += 1;
            }
        }
        assert!(upstairs > 2, "only {upstairs} houses had an upstairs");
    }

    /// How long building the map, its nav field and one route to the player take.
    #[test]
    #[ignore]
    fn timing() {
        let t = std::time::Instant::now();
        let m = build(1);
        println!("{} walls, built in {:?}", m.walls.len(), t.elapsed());
        let t = std::time::Instant::now();
        let world = Grid::new(m.walls, 4.0).with_ground(m.ground);
        let mut nav = Nav::new(&world, SIZE);
        println!("nav field in {:?}", t.elapsed());
        let t = std::time::Instant::now();
        for k in 0..10 {
            nav.follow(&[[k as f32 * 3.0, 0.0, 14.0]], &[]);
        }
        println!("a route to the player in {:?}", t.elapsed() / 10);
        let t = std::time::Instant::now();
        for k in 0..10 {
            nav.set_goal([k as f32 * 3.0, 0.0, 14.0]);
        }
        println!("a route from everywhere in {:?}", t.elapsed() / 10);
        let world = Grid::new(super::build(1).walls, 4.0);
        let mut nav = Nav::new(&world, HALF);
        let t = std::time::Instant::now();
        for k in 0..10 {
            nav.follow(&[[k as f32 * 3.0, 0.0, 14.0]], &[]);
        }
        println!("on the town, a route in {:?}", t.elapsed() / 10);
    }

    /// How high into the hills a body gets from the middle across the terrain's grid with each steepest slope it
    /// can manage, and how much of the ring past the suburb that opens up.
    #[test]
    #[ignore]
    fn how_high_each_slope_gets_you() {
        for seed in 1..4 {
            let t = build(seed).ground.unwrap();
            let n = t.n;
            for limit in [0.75, 0.9, 1.0, 1.1, 1.25, 1.5] {
                let mut seen = vec![false; n * n];
                let mid = n / 2;
                let mut queue = vec![mid * n + mid];
                seen[mid * n + mid] = true;
                let (mut top, mut outside) = (0.0f32, 0);
                while let Some(k) = queue.pop() {
                    let (i, j) = (k % n, k / n);
                    let h = t.at(i, j);
                    top = top.max(h);
                    let (x, z) = (
                        -t.half + i as f32 * crate::terrain::STEP,
                        -t.half + j as f32 * crate::terrain::STEP,
                    );
                    outside += (x.abs().max(z.abs()) > CORE + 25.0) as usize;
                    for (di, dj) in [(1i32, 0i32), (-1, 0), (0, 1), (0, -1)] {
                        let (a, b) = (i as i32 + di, j as i32 + dj);
                        if a < 0 || b < 0 || a >= n as i32 || b >= n as i32 {
                            continue;
                        }
                        let k2 = b as usize * n + a as usize;
                        if !seen[k2]
                            && t.at(a as usize, b as usize) - h <= limit * crate::terrain::STEP
                        {
                            seen[k2] = true;
                            queue.push(k2);
                        }
                    }
                }
                println!("seed {seed} slope {limit}: up to {top:.1} m, {outside} grid points out in the mountains");
            }
        }
    }

    /// Prints the boxes within 2.5 m of AT=x,z on the map made from SEED, and the ground around as in `house_ascii`.
    #[test]
    #[ignore]
    fn near() {
        let seed = std::env::var("SEED")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(1);
        let at: Vec<f32> = std::env::var("AT")
            .unwrap_or("0,0".into())
            .split(',')
            .map(|s| s.parse().unwrap())
            .collect();
        let m = build(seed);
        for (w, c) in m.walls.iter().zip(&m.colors) {
            if w.max[0] > at[0] - 2.5
                && w.min[0] < at[0] + 2.5
                && w.max[2] > at[1] - 2.5
                && w.min[2] < at[1] + 2.5
            {
                println!("{:?} {:?} {c:.2?}", w.min, w.max);
            }
        }
        let world = Grid::new(m.walls, 4.0).with_ground(m.ground);
        let mut nav = Nav::new(&world, SIZE);
        nav.set_goal([at[0], 0.0, at[1]]);
        for zi in -6..=6 {
            let z = (at[1].floor() + 0.5) + zi as f32;
            let row: String = (-10..=10)
                .map(|xi| {
                    let p = [at[0].floor() + 0.5 + xi as f32, 0.0, z];
                    if !nav.open_at(p) {
                        '#'
                    } else if nav.reachable(p) {
                        '.'
                    } else {
                        'X'
                    }
                })
                .collect();
            println!("{z:6.1} {row}");
        }
    }

    /// Prints one house's lot as a map of the ground: # blocked, . reachable, X open but shut off.
    #[test]
    #[ignore]
    fn house_ascii() {
        let seed = std::env::var("SEED")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(2);
        let (dir, w) = if seed % 2 == 0 {
            (1.0, 19.0)
        } else {
            (-1.0, 23.25)
        };
        let mut b = B::new(seed, 40.0);
        b.perimeter();
        b.house(Lot {
            x0: -w / 2.0,
            x1: w / 2.0,
            zf: -12.0 * dir,
            dir,
            depth: 25.0,
            back: true,
            last: true,
        });
        let world = Grid::new(b.finish().walls, 4.0);
        let mut nav = Nav::new(&world, 40.0);
        nav.set_goal([0.0, 0.0, -30.0 * dir]);
        for zi in 0..30 {
            let z = -14.5 + zi as f32;
            let row: String = (0..26)
                .map(|xi| {
                    let p = [-12.5 + xi as f32, 0.0, z];
                    if !nav.open_at(p) {
                        '#'
                    } else if nav.reachable(p) {
                        '.'
                    } else {
                        'X'
                    }
                })
                .collect();
            println!("{z:6.1} {row}");
        }
    }

    /// The mall's stair comes up beside the parapet: a player out on the roof can be seen over it from the top
    /// steps, but has to be reached through the gap at the landing, so nobody may stall on the last steps.
    #[test]
    fn a_horde_gets_up_the_mall_stair_to_a_player_across_the_roof() {
        for at in [[-60.0, -110.0], [-80.0, -110.0], [-52.0, -117.0]] {
            let mut g = crate::sim::Game::on(1, 1);
            g.bots.clear();
            g.players[0].body.pos = [at[0], 3.35, at[1]];
            for k in 0..6 {
                let mut z = crate::sim::Bot::zombie([-62.0 + k as f32, 0.0, -123.0], k as f32);
                z.hp = 1e6;
                g.bots.push(z);
            }
            for _ in 0..(30.0 / crate::sim::STEP) as usize {
                g.players[0].hp = 100.0;
                g.tick(crate::sim::STEP);
                // the waves would bring more
                g.bots.truncate(6);
            }
            for (i, z) in g.bots.iter().enumerate() {
                let p = z.body.pos;
                assert!(
                    (p[0] - at[0]).hypot(p[2] - at[1]) < 3.0,
                    "a zombie stalled on the way to {at:?}: {}",
                    g.describe_bot(i)
                );
            }
        }
    }
}
