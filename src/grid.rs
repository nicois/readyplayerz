//! Walls bucketed into a uniform grid over the ground plane, so collision and shots only look at nearby boxes.

use crate::sim::{ray_aabb, Aabb, V3};
use crate::terrain::Terrain;

pub struct Grid {
    pub walls: Vec<Aabb>,
    /// hills, if the ground isn't flat at height 0
    pub ground: Option<Terrain>,
    cell: f32,
    origin: [f32; 2],
    dims: [usize; 2],
    cells: Vec<Vec<u32>>,
}

impl Grid {
    pub fn new(walls: Vec<Aabb>, cell: f32) -> Grid {
        let mut lo = [f32::INFINITY; 2];
        let mut hi = [f32::NEG_INFINITY; 2];
        for w in &walls {
            for (i, axis) in [0, 2].into_iter().enumerate() {
                lo[i] = lo[i].min(w.min[axis]);
                hi[i] = hi[i].max(w.max[axis]);
            }
        }
        if walls.is_empty() {
            lo = [0.0; 2];
            hi = [0.0; 2];
        }
        let dims = [0, 1].map(|i| (((hi[i] - lo[i]) / cell).ceil() as usize).max(1));
        let mut g = Grid { walls, ground: None, cell, origin: lo, dims, cells: vec![Vec::new(); dims[0] * dims[1]] };
        for i in 0..g.walls.len() {
            let ([x0, z0], [x1, z1]) = g.span(&g.walls[i]);
            for z in z0..=z1 {
                for x in x0..=x1 {
                    g.cells[z * dims[0] + x].push(i as u32);
                }
            }
        }
        g
    }

    pub fn with_ground(mut self, ground: Option<Terrain>) -> Grid {
        self.ground = ground;
        self
    }

    /// Height of the ground at (x, z).
    pub fn ground_at(&self, x: f32, z: f32) -> f32 {
        self.ground.as_ref().map_or(0.0, |t| t.height(x, z))
    }

    /// Distance to the first wall or hillside along the ray, or `max` if there is none that close.
    pub fn raycast(&self, o: V3, d: V3, max: f32) -> f32 {
        let t = self.raycast_walls(o, d, max);
        self.ground.as_ref().map_or(t, |g| g.raycast(o, d, t))
    }

    fn index(&self, v: f32, axis: usize) -> usize {
        (((v - self.origin[axis]) / self.cell).floor().max(0.0) as usize).min(self.dims[axis] - 1)
    }

    /// Inclusive range of cells (x, z) a box touches. Boxes outside the grid clamp to its edge.
    fn span(&self, b: &Aabb) -> ([usize; 2], [usize; 2]) {
        ([self.index(b.min[0], 0), self.index(b.min[2], 1)], [self.index(b.max[0], 0), self.index(b.max[2], 1)])
    }

    /// Calls `f` on every wall in the cells `b` touches until it returns true. Walls spanning several
    /// cells are visited more than once.
    pub fn find(&self, b: &Aabb, mut f: impl FnMut(&Aabb) -> bool) -> bool {
        let ([x0, z0], [x1, z1]) = self.span(b);
        for z in z0..=z1 {
            for x in x0..=x1 {
                for &i in &self.cells[z * self.dims[0] + x] {
                    if f(&self.walls[i as usize]) {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Indices into `walls` of everything within `radius` of `p` on the ground plane, each once, in order.
    pub fn near(&self, p: V3, radius: f32) -> Vec<usize> {
        let around = Aabb { min: [p[0] - radius, 0.0, p[2] - radius], max: [p[0] + radius, 0.0, p[2] + radius] };
        let ([x0, z0], [x1, z1]) = self.span(&around);
        let mut out = Vec::new();
        for z in z0..=z1 {
            for x in x0..=x1 {
                out.extend(self.cells[z * self.dims[0] + x].iter().map(|&i| i as usize));
            }
        }
        out.sort_unstable();
        out.dedup();
        out.retain(|&i| {
            let w = &self.walls[i];
            let dx = (w.min[0] - p[0]).max(p[0] - w.max[0]).max(0.0);
            let dz = (w.min[2] - p[2]).max(p[2] - w.max[2]).max(0.0);
            dx * dx + dz * dz <= radius * radius
        });
        out
    }

    /// Distance to the first wall along the ray, or `max` if there is none that close.
    pub fn raycast_walls(&self, o: V3, d: V3, max: f32) -> f32 {
        // clip the ray to the grid's footprint; nothing outside it can be hit
        let (mut t0, mut t1) = (0.0f32, max);
        for (i, axis) in [0, 2].into_iter().enumerate() {
            let lo = self.origin[i];
            let hi = lo + self.dims[i] as f32 * self.cell;
            if d[axis].abs() < 1e-8 {
                if o[axis] < lo || o[axis] > hi {
                    return max;
                }
            } else {
                let (a, b) = ((lo - o[axis]) / d[axis], (hi - o[axis]) / d[axis]);
                t0 = t0.max(a.min(b));
                t1 = t1.min(a.max(b));
            }
        }
        if t0 > t1 {
            return max;
        }

        let start = [o[0] + d[0] * t0, o[2] + d[2] * t0];
        let mut at = [self.index(start[0], 0), self.index(start[1], 1)];
        // t of the next cell boundary on each axis, and how much t one whole cell costs
        let mut next = [f32::INFINITY; 2];
        let mut step = [f32::INFINITY; 2];
        let mut dir = [0isize; 2];
        for (i, axis) in [0, 2].into_iter().enumerate() {
            if d[axis].abs() >= 1e-8 {
                dir[i] = if d[axis] > 0.0 { 1 } else { -1 };
                let edge = self.origin[i] + (at[i] as isize + (dir[i] > 0) as isize) as f32 * self.cell;
                next[i] = (edge - o[axis]) / d[axis];
                step[i] = self.cell / d[axis].abs();
            }
        }

        let mut best = max;
        loop {
            for &w in &self.cells[at[1] * self.dims[0] + at[0]] {
                if let Some(t) = ray_aabb(o, d, &self.walls[w as usize]) {
                    best = best.min(t);
                }
            }
            let i = if next[0] < next[1] { 0 } else { 1 };
            // anything in a later cell is at least this far away
            if best <= next[i] || next[i] >= t1 {
                return best;
            }
            let moved = at[i] as isize + dir[i];
            if moved < 0 || moved >= self.dims[i] as isize {
                return best;
            }
            at[i] = moved as usize;
            next[i] += step[i];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bx(x: f32, z: f32, s: f32) -> Aabb {
        Aabb { min: [x - s, 0.0, z - s], max: [x + s, 2.0, z + s] }
    }

    fn lcg(s: &mut u32) -> f32 {
        *s = s.wrapping_mul(1664525).wrapping_add(1013904223);
        (*s >> 8) as f32 / 16_777_216.0
    }

    #[test]
    fn raycast_matches_checking_every_wall() {
        let mut s = 7;
        let walls: Vec<Aabb> = (0..200).map(|_| bx(lcg(&mut s) * 200.0 - 100.0, lcg(&mut s) * 200.0 - 100.0, 0.5 + lcg(&mut s) * 4.0)).collect();
        let grid = Grid::new(walls.clone(), 4.0);
        for _ in 0..5000 {
            let o = [lcg(&mut s) * 260.0 - 130.0, lcg(&mut s) * 3.0, lcg(&mut s) * 260.0 - 130.0];
            let (a, b, c) = (lcg(&mut s) - 0.5, lcg(&mut s) - 0.5, lcg(&mut s) - 0.5);
            let l = (a * a + b * b + c * c).sqrt();
            let d = [a / l, b / l, c / l];
            let want = walls.iter().filter_map(|w| ray_aabb(o, d, w)).fold(120.0, f32::min);
            let got = grid.raycast(o, d, 120.0);
            assert!((want - got).abs() < 1e-3, "o {o:?} d {d:?}: want {want} got {got}");
        }
    }

    #[test]
    fn straight_up_and_axis_aligned_rays() {
        let grid = Grid::new(vec![bx(0.0, 0.0, 1.0), bx(10.0, 0.0, 1.0)], 4.0);
        assert_eq!(grid.raycast([0.0, 5.0, 0.0], [0.0, 1.0, 0.0], 50.0), 50.0);
        assert_eq!(grid.raycast([0.0, 1.0, 0.0], [0.0, 1.0, 0.0], 50.0), 0.0); // starts inside a box
        assert!((grid.raycast([-5.0, 1.0, 0.0], [1.0, 0.0, 0.0], 50.0) - 4.0).abs() < 1e-4);
        assert!((grid.raycast([20.0, 1.0, 0.0], [-1.0, 0.0, 0.0], 50.0) - 9.0).abs() < 1e-4);
    }

    #[test]
    fn near_returns_each_wall_once_and_respects_radius() {
        let grid = Grid::new(vec![bx(0.0, 0.0, 6.0), bx(30.0, 0.0, 1.0), bx(-30.0, 0.0, 1.0)], 4.0);
        assert_eq!(grid.near([0.0, 0.0, 0.0], 10.0), vec![0]);
        assert_eq!(grid.near([20.0, 0.0, 0.0], 10.0), vec![1]);
        assert_eq!(grid.near([0.0, 0.0, 0.0], 40.0), vec![0, 1, 2]);
    }
}
