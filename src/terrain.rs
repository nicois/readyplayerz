//! Ground that isn't flat: a height at every point of a square grid, and flat triangles between the points,
//! the same triangles the renderer draws, so feet stand exactly on the ground that is seen.

use crate::sim::V3;

/// Spacing of the grid's points: fine enough for rolling hills, and a quarter the triangles of 2 m.
pub const STEP: f32 = 4.0;
/// Side, in squares, of the blocks whose highest point is kept, so a ray over low ground can skip them.
const BLOCK: usize = 4;

pub struct Terrain {
    /// the grid runs from -half to half on x and z
    pub half: f32,
    /// points per side
    pub n: usize,
    h: Vec<f32>,
    blocks: usize,
    top: Vec<f32>,
}

impl Terrain {
    /// The grid with the height `f(x, z)` at each of its points.
    pub fn new(half: f32, f: impl Fn(f32, f32) -> f32) -> Terrain {
        let n = (2.0 * half / STEP).round() as usize + 1;
        let mut h = Vec::with_capacity(n * n);
        for j in 0..n {
            for i in 0..n {
                h.push(f(-half + i as f32 * STEP, -half + j as f32 * STEP));
            }
        }
        let blocks = (n - 1).div_ceil(BLOCK);
        let mut top = vec![f32::MIN; blocks * blocks];
        for bj in 0..blocks {
            for bi in 0..blocks {
                for j in bj * BLOCK..=((bj + 1) * BLOCK).min(n - 1) {
                    for i in bi * BLOCK..=((bi + 1) * BLOCK).min(n - 1) {
                        top[bj * blocks + bi] = top[bj * blocks + bi].max(h[j * n + i]);
                    }
                }
            }
        }
        Terrain {
            half,
            n,
            h,
            blocks,
            top,
        }
    }

    /// The height at grid point (i, j).
    pub fn at(&self, i: usize, j: usize) -> f32 {
        self.h[j * self.n + i]
    }

    /// Where on the grid (x, z) is, in squares, kept inside it.
    fn cell(&self, x: f32, z: f32) -> (usize, usize, f32, f32) {
        let last = (self.n - 1) as f32 - 1e-3;
        let (fx, fz) = (
            ((x + self.half) / STEP).clamp(0.0, last),
            ((z + self.half) / STEP).clamp(0.0, last),
        );
        let (i, j) = (fx as usize, fz as usize);
        (i, j, fx - i as f32, fz - j as f32)
    }

    /// The height of the ground at (x, z); past the edge of the grid, that of its edge.
    pub fn height(&self, x: f32, z: f32) -> f32 {
        let (i, j, tx, tz) = self.cell(x, z);
        let (h00, h10, h01, h11) = (
            self.at(i, j),
            self.at(i + 1, j),
            self.at(i, j + 1),
            self.at(i + 1, j + 1),
        );
        // each square is two triangles, split from (i, j) to (i + 1, j + 1)
        if tx > tz {
            h00 + (h10 - h00) * tx + (h11 - h10) * tz
        } else {
            h00 + (h01 - h00) * tz + (h11 - h01) * tx
        }
    }

    /// Nothing in the block of squares around (x, z) is higher than this.
    fn ceiling(&self, x: f32, z: f32) -> f32 {
        let (i, j, _, _) = self.cell(x, z);
        self.top[(j / BLOCK).min(self.blocks - 1) * self.blocks + (i / BLOCK).min(self.blocks - 1)]
    }

    /// Distance along the ray to where it meets the ground, or `max` if it doesn't that close; 0 if it starts
    /// underground.
    pub fn raycast(&self, o: V3, d: V3, max: f32) -> f32 {
        let below = |t: f32| {
            let p = [o[0] + d[0] * t, o[1] + d[1] * t, o[2] + d[2] * t];
            p[1] < self.ceiling(p[0], p[2]) && p[1] <= self.height(p[0], p[2])
        };
        if below(0.0) {
            return 0.0;
        }
        // march in short steps, then close in on the crossing between the last step above and the first below
        let mut last = 0.0;
        while last < max {
            let t = (last + 0.5).min(max);
            if below(t) {
                let (mut a, mut b) = (last, t);
                for _ in 0..12 {
                    let m = (a + b) / 2.0;
                    if below(m) {
                        b = m;
                    } else {
                        a = m;
                    }
                }
                return a;
            }
            last = t;
        }
        max
    }
}

/// Smooth noise from about -1 to 1 that changes over a distance of about 1, the same for the same `seed`.
pub fn noise(x: f32, z: f32, seed: u32) -> f32 {
    let hash = |i: i32, j: i32| {
        let mut h = (i as u32).wrapping_mul(0x8da6_b343)
            ^ (j as u32).wrapping_mul(0xd816_3841)
            ^ seed.wrapping_mul(0xcb1a_b31f);
        h ^= h >> 13;
        h = h.wrapping_mul(0x5bd1_e995);
        h ^= h >> 15;
        (h & 0xffff) as f32 / 32767.5 - 1.0
    };
    let (i, j) = (x.floor() as i32, z.floor() as i32);
    let (tx, tz) = (x - x.floor(), z - z.floor());
    let (sx, sz) = (tx * tx * (3.0 - 2.0 * tx), tz * tz * (3.0 - 2.0 * tz));
    let a = hash(i, j) + (hash(i + 1, j) - hash(i, j)) * sx;
    let b = hash(i, j + 1) + (hash(i + 1, j + 1) - hash(i, j + 1)) * sx;
    a + (b - a) * sz
}

/// Noise with finer detail laid over coarser: `octaves` layers, each half the size and strength of the last.
pub fn fbm(x: f32, z: f32, seed: u32, octaves: u32) -> f32 {
    let (mut sum, mut amp, mut f, mut norm) = (0.0, 1.0, 1.0, 0.0);
    for k in 0..octaves {
        sum += amp * noise(x * f, z * f, seed.wrapping_add(k * 101));
        norm += amp;
        amp *= 0.5;
        f *= 2.0;
    }
    sum / norm
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hills() -> Terrain {
        Terrain::new(20.0, |x, z| 3.0 + 2.0 * (x * 0.3).sin() + z * 0.1)
    }

    #[test]
    fn heights_on_the_grid_points_are_the_ones_given() {
        let t = hills();
        for (x, z) in [(-20.0, -20.0), (0.0, 0.0), (8.0, -12.0), (16.0, 20.0)] {
            let want = 3.0 + 2.0 * (x * 0.3f32).sin() + z * 0.1;
            assert!(
                (t.height(x, z) - want).abs() < 1e-3,
                "at {x}, {z}: {} not {want}",
                t.height(x, z)
            );
        }
    }

    #[test]
    fn a_ray_down_lands_on_the_ground_and_one_across_hits_the_slope() {
        let t = hills();
        let g = t.height(1.3, 2.7);
        let d = t.raycast([1.3, 30.0, 2.7], [0.0, -1.0, 0.0], 100.0);
        assert!(
            (30.0 - d - g).abs() < 0.01,
            "landed at {} not {g}",
            30.0 - d
        );
        assert_eq!(
            t.raycast([0.0, 40.0, 0.0], [1.0, 0.0, 0.0], 15.0),
            15.0,
            "a ray over the top hit something"
        );
        assert_eq!(
            t.raycast([0.0, -5.0, 0.0], [1.0, 0.0, 0.0], 15.0),
            0.0,
            "a ray from underground missed"
        );
        let flat = Terrain::new(20.0, |x, _| if x > 5.0 { 10.0 } else { 0.0 });
        let d = flat.raycast([0.0, 1.0, 0.0], [1.0, 0.0, 0.0], 30.0);
        assert!((4.0..6.0).contains(&d), "hit the rise at {d}");
    }

    #[test]
    fn noise_is_smooth_and_bounded() {
        for k in 0..400 {
            let (x, z) = (k as f32 * 0.37, k as f32 * 0.21 - 30.0);
            let n = fbm(x, z, 9, 4);
            assert!((-1.0..=1.0).contains(&n));
            assert!((fbm(x + 0.01, z, 9, 4) - n).abs() < 0.1);
        }
    }
}
