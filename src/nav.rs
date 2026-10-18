//! Which ground cells are open, and the cheapest way from any of them to the player, so bots can route around walls.

use crate::grid::Grid;
use crate::sim::{overlaps, Aabb, V3};

/// Side of one nav cell. A body is 0.8 wide, so standing in the middle of an open cell it clears every blocked neighbour.
const CELL: f32 = 1.0;
const UNREACHED: u32 = u32::MAX;
/// How far from the player to look for an open cell when they are standing on top of something.
const GOAL_SEARCH: i32 = 6;

/// Tallest rise between neighbouring cells a bot can walk or hop up. A stair cell rises about half a metre.
const CLIMB: f32 = 0.65;
/// Furthest it will step down on its own.
const DROP: f32 = 1.3;
/// Highest a spot can be above someone's feet and still be what they stand on (they may be mid-hop onto it).
const REACH_UP: f32 = 1.0;
/// Body height a standing spot needs clear above it.
const HEADROOM: f32 = 1.8;
/// Narrowest top a bot can stand on.
const FOOTING: f32 = 0.5;
/// Tallest ledge a bot pulls itself up onto (see `sim::BOT_REACH`), straight on only, with room above it to rise;
/// a climb costs as much as walking this many steps further, so stairs win where there are any.
pub const LEDGE: f32 = 2.9;
const LEDGE_COST: u32 = 40;
/// More than the dearest single step: a diagonal and a climb.
const BUCKETS: usize = 64;

/// One standable spot: the middle of a cell at some height.
pub struct Nav {
    half: f32,
    n: usize,
    /// heights of the spots in each cell: `spots[start[c]..start[c + 1]]`
    start: Vec<u32>,
    spots: Vec<f32>,
    /// how high the open air goes above each spot
    ceil: Vec<f32>,
    /// path cost to the goal, 10 per straight step and 14 per diagonal
    cost: Vec<u32>,
    goal: Option<usize>,
    /// the spots that can step onto each spot: `into[into_start[i]..into_start[i + 1]]`, with what the step costs
    into_start: Vec<u32>,
    into: Vec<u32>,
    into_cost: Vec<u8>,
    /// the route search's queue, kept between searches so it isn't allocated each time
    buckets: Vec<Vec<u32>>,
}

impl Nav {
    /// Every cell gets a spot at the ground and on top of each wall in it, provided a body fits standing there.
    /// So floors, stairs, roofs and crates are all places a bot can go.
    pub fn new(world: &Grid, half: f32) -> Nav {
        let n = (2.0 * half / CELL).ceil() as usize;
        let mut start = Vec::with_capacity(n * n + 1);
        let mut spots = Vec::new();
        let mut ceil = Vec::new();
        // each wall's bottom and top, and whether its top is wide enough to stand on
        let mut spans: Vec<(f32, f32, bool)> = Vec::new();
        for z in 0..n {
            for x in 0..n {
                start.push(spots.len() as u32);
                let (x0, z0) = (-half + x as f32 * CELL, -half + z as f32 * CELL);
                let cell = Aabb { min: [x0, -1.0, z0], max: [x0 + CELL, 200.0, z0 + CELL] };
                spans.clear();
                world.find(&cell, |w| {
                    if overlaps(&cell, w) {
                        spans.push((w.min[1], w.max[1], (w.max[0] - w.min[0]).min(w.max[2] - w.min[2]) >= FOOTING));
                    }
                    false
                });
                // the top of a post, a rail or a fence is no place to stand, and a route up onto it would be
                // climbed at for ever
                let mut tops: Vec<f32> = spans.iter().filter(|s| s.2).map(|s| s.1).filter(|t| *t > 0.0).collect();
                tops.push(0.0);
                tops.sort_by(|a, b| a.partial_cmp(b).unwrap());
                tops.dedup_by(|a, b| (*a - *b).abs() < 0.01);
                for h in tops {
                    if !spans.iter().any(|(lo, hi, _)| *lo < h + HEADROOM && *hi > h + 0.02) {
                        spots.push(h);
                        ceil.push(spans.iter().filter(|s| s.1 > h + 0.02).map(|s| s.0).fold(f32::INFINITY, f32::min));
                    }
                }
            }
        }
        start.push(spots.len() as u32);
        let total = spots.len();
        let mut nav = Nav { half, n, start, spots, ceil, cost: vec![UNREACHED; total], goal: None, into_start: Vec::new(), into: Vec::new(), into_cost: Vec::new(), buckets: Vec::new() };
        nav.link();
        nav
    }

    fn cell_of(&self, x: f32, z: f32) -> Option<(i32, i32)> {
        let (cx, cz) = (((x + self.half) / CELL).floor() as i32, ((z + self.half) / CELL).floor() as i32);
        let n = self.n as i32;
        (cx >= 0 && cz >= 0 && cx < n && cz < n).then_some((cx, cz))
    }

    fn at(&self, c: (i32, i32)) -> usize {
        c.1 as usize * self.n + c.0 as usize
    }

    /// Indices of the spots in a cell (none outside the map).
    fn in_cell(&self, c: (i32, i32)) -> std::ops::Range<usize> {
        let n = self.n as i32;
        if c.0 < 0 || c.1 < 0 || c.0 >= n || c.1 >= n {
            return 0..0;
        }
        let i = self.at(c);
        self.start[i] as usize..self.start[i + 1] as usize
    }

    fn cell_at(&self, spot: usize) -> (i32, i32) {
        let i = self.start.partition_point(|s| *s as usize <= spot) - 1;
        ((i % self.n) as i32, (i / self.n) as i32)
    }

    fn center(&self, c: (i32, i32)) -> [f32; 2] {
        [-self.half + (c.0 as f32 + 0.5) * CELL, -self.half + (c.1 as f32 + 0.5) * CELL]
    }

    fn can_step(&self, from: f32, to: f32) -> bool {
        to - from <= CLIMB && from - to <= DROP
    }

    /// Whether a bot on spot `from` can pull itself up onto a ledge at height `to`: too tall to step up, within
    /// reach, and with the air clear above it for the body to rise beside the face.
    fn can_climb(&self, from: usize, to: f32) -> bool {
        let rise = to - self.spots[from];
        rise > CLIMB && rise <= LEDGE && self.ceil[from] >= to + HEADROOM + 0.05
    }

    /// The spot at `c2` a bot standing on spot `from` in `c` can walk to, if any, or failing that climb up onto
    /// (straight on only). Diagonals need both cells beside them to be passable too, so nobody cuts a corner
    /// through a wall.
    fn move_to(&self, c: (i32, i32), from: usize, d: (i32, i32)) -> Option<usize> {
        let h = self.spots[from];
        let c2 = (c.0 + d.0, c.1 + d.1);
        let best = |cell: (i32, i32), from: f32| self.in_cell(cell).filter(|&s| self.can_step(from, self.spots[s])).min_by(|a, b| (self.spots[*a] - from).abs().partial_cmp(&(self.spots[*b] - from).abs()).unwrap());
        let Some(to) = best(c2, h) else {
            let straight = d.0 == 0 || d.1 == 0;
            return self.in_cell(c2).filter(|&s| straight && self.can_climb(from, self.spots[s])).min_by(|a, b| self.spots[*a].partial_cmp(&self.spots[*b]).unwrap());
        };
        if d.0 != 0 && d.1 != 0 {
            let t = self.spots[to];
            for side in [(c.0 + d.0, c.1), (c.0, c.1 + d.1)] {
                if !self.in_cell(side).any(|s| self.can_step(h, self.spots[s]) && self.can_step(self.spots[s], t)) {
                    return None;
                }
            }
        }
        Some(to)
    }

    /// The spot in or near the cell of `p` that is closest to where `p` stands.
    fn nearest_spot(&self, p: V3, radius: i32) -> Option<usize> {
        let here = self.cell_of(p[0], p[2])?;
        let mut best = (f32::INFINITY, None);
        for dz in -radius..=radius {
            for dx in -radius..=radius {
                for s in self.in_cell((here.0 + dx, here.1 + dz)) {
                    let dy = self.spots[s] - p[1];
                    let score = (dx * dx + dz * dz) as f32 + 4.0 * dy * dy;
                    if score < best.0 {
                        best = (score, Some(s));
                    }
                }
            }
        }
        best.1
    }

    /// The spot a body at `p` is standing on: the one in its cell closest to its feet. A body at the edge of a
    /// cell that something tall half fills is beside it, not on it: its spot is then the nearest one around
    /// with a route, as it is when its own has none.
    fn spot_of(&self, p: V3) -> Option<usize> {
        let c = self.cell_of(p[0], p[2])?;
        let own = self.in_cell(c).min_by(|a, b| (self.spots[*a] - p[1]).abs().partial_cmp(&(self.spots[*b] - p[1]).abs()).unwrap());
        if let Some(s) = own.filter(|&s| self.spots[s] < p[1] + REACH_UP && self.cost[s] != UNREACHED) {
            return Some(s);
        }
        let mut best = (f32::INFINITY, None);
        for dz in -1..=1 {
            for dx in -1..=1 {
                for s in self.in_cell((c.0 + dx, c.1 + dz)) {
                    let dy = self.spots[s] - p[1];
                    if self.cost[s] == UNREACHED || dy >= REACH_UP {
                        continue;
                    }
                    let m = self.center((c.0 + dx, c.1 + dz));
                    let score = (m[0] - p[0]).powi(2) + (m[1] - p[2]).powi(2) + 4.0 * dy * dy;
                    if score < best.0 {
                        best = (score, Some(s));
                    }
                }
            }
        }
        best.1.or(own)
    }

    /// Points the field at `p`, at whatever height it is standing. Does nothing if they are still on the same spot.
    pub fn set_goal(&mut self, p: V3) {
        let goal = self.nearest_spot(p, GOAL_SEARCH);
        if goal == self.goal {
            return;
        }
        self.goal = goal;
        self.cost.fill(UNREACHED);
        let Some(g) = goal else {
            return;
        };
        // Dijkstra with a bucket per cost instead of a heap: no step costs more than BUCKETS - 1, so the spots
        // waiting to be done only ever span that many costs, and a ring of buckets holds them in order
        self.cost[g] = 0;
        let mut buckets = std::mem::take(&mut self.buckets);
        buckets.resize_with(BUCKETS, Vec::new);
        buckets[0].push(g as u32);
        let (mut d, mut waiting) = (0u32, 1usize);
        while waiting > 0 {
            let b = d as usize % BUCKETS;
            while let Some(i) = buckets[b].pop() {
                waiting -= 1;
                let i = i as usize;
                if self.cost[i] != d {
                    continue;
                }
                // walk the field backwards, through the spots that can step onto this one
                for k in self.into_start[i] as usize..self.into_start[i + 1] as usize {
                    let (j, nd) = (self.into[k] as usize, d + self.into_cost[k] as u32);
                    if nd < self.cost[j] {
                        self.cost[j] = nd;
                        buckets[nd as usize % BUCKETS].push(j as u32);
                        waiting += 1;
                    }
                }
            }
            d += 1;
        }
        self.buckets = buckets;
    }

    /// Works out every step a bot can take, once, and files each under the spot it lands on, so a route search
    /// only has to read them off.
    fn link(&mut self) {
        let n = self.n as i32;
        let mut steps: Vec<(u32, u32, u8)> = Vec::new();
        for z in 0..n {
            for x in 0..n {
                for j in self.in_cell((x, z)) {
                    for d in NEIGHBOURS {
                        if let Some(i) = self.move_to((x, z), j, d) {
                            let climb = if self.spots[i] - self.spots[j] > CLIMB { LEDGE_COST } else { 0 };
                            let cost = climb + if d.0 != 0 && d.1 != 0 { 14 } else { 10 };
                            steps.push((i as u32, j as u32, cost as u8));
                        }
                    }
                }
            }
        }
        steps.sort_unstable_by_key(|s| s.0);
        let mut start = vec![0u32; self.spots.len() + 1];
        for s in &steps {
            start[s.0 as usize + 1] += 1;
        }
        for i in 0..self.spots.len() {
            start[i + 1] += start[i];
        }
        self.into_start = start;
        self.into = steps.iter().map(|s| s.1).collect();
        self.into_cost = steps.iter().map(|s| s.2).collect();
    }

    /// Middle of the cell `p` is in, if there is somewhere to stand in it.
    pub fn snap(&self, p: V3) -> Option<[f32; 2]> {
        let c = self.cell_of(p[0], p[2])?;
        (!self.in_cell(c).is_empty()).then(|| self.center(c))
    }

    /// True if there is ground to stand on in the cell under `p`.
    pub fn open_at(&self, p: V3) -> bool {
        self.cell_of(p[0], p[2]).is_some_and(|c| self.in_cell(c).any(|s| self.spots[s] == 0.0))
    }

    /// Ground spots, and how many of them have a route to the goal.
    pub fn coverage(&self) -> (usize, usize) {
        let ground: Vec<usize> = (0..self.spots.len()).filter(|&s| self.spots[s] == 0.0).collect();
        let reached = ground.iter().filter(|&&s| self.cost[s] != UNREACHED).count();
        (ground.len(), reached)
    }

    /// True if `p` stands on a spot that has a route to the goal.
    pub fn reachable(&self, p: V3) -> bool {
        self.spot_of(p).is_some_and(|s| self.cost[s] != UNREACHED)
    }

    /// How far `p` has to walk to the goal, in metres, if it has a route.
    pub fn route_len(&self, p: V3) -> Option<f32> {
        let s = self.spot_of(p)?;
        (self.cost[s] != UNREACHED).then(|| self.cost[s] as f32 * CELL / 10.0)
    }

    /// Where to step next from `p` on the way to the goal, as a point (x, height, z), or None if `p` is not
    /// on the field (inside a wall, unreachable) or is already at the goal.
    pub fn next(&self, p: V3) -> Option<V3> {
        let s = self.spot_of(p)?;
        let here = self.cost[s];
        if here == UNREACHED || here == 0 {
            return None;
        }
        let c = self.cell_at(s);
        let mut best = (here, None);
        for d in NEIGHBOURS {
            if let Some(to) = self.move_to(c, s, d) {
                if self.cost[to] < best.0 {
                    best = (self.cost[to], Some(to));
                }
            }
        }
        best.1.map(|to| {
            let m = self.center(self.cell_at(to));
            [m[0], self.spots[to], m[1]]
        })
    }
}

const NEIGHBOURS: [(i32, i32); 8] = [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)];

#[cfg(test)]
mod tests {
    use super::*;

    fn wall(x0: f32, z0: f32, x1: f32, z1: f32) -> Aabb {
        Aabb { min: [x0, 0.0, z0], max: [x1, 3.0, z1] }
    }

    #[test]
    fn routes_around_a_wall_and_never_through_one() {
        // a wall across the middle with a gap at the +x end
        let nav_world = Grid::new(vec![wall(-20.0, -0.5, 15.0, 0.5), wall(-21.0, -21.0, 21.0, -20.0)], 4.0);
        let mut nav = Nav::new(&nav_world, 20.0);
        nav.set_goal([0.0, 0.0, 10.0]);
        let mut p = [0.0, 0.0, -10.0];
        for _ in 0..200 {
            let Some(to) = nav.next(p) else {
                break;
            };
            p = [to[0], to[1], to[2]];
            let cell = Aabb { min: [p[0] - 0.4, 0.0, p[2] - 0.4], max: [p[0] + 0.4, 0.1, p[2] + 0.4] };
            assert!(!nav_world.find(&cell, |w| overlaps(&cell, w)), "stepped into a wall at {p:?}");
        }
        assert!((p[2] - 10.0).abs() < 1.5 && p[0].abs() < 1.5, "stopped at {p:?}");
    }

    #[test]
    fn a_goal_on_top_of_a_wall_aims_at_its_foot() {
        let world = Grid::new(vec![wall(-2.0, -2.0, 2.0, 2.0)], 4.0);
        let mut nav = Nav::new(&world, 10.0);
        nav.set_goal([0.0, 1.0, 0.0]);
        assert!(nav.next([0.0, 0.0, 8.0]).is_some());
        assert!(nav.next([0.0, 0.0, 0.0]).is_none(), "a cell inside the wall has no route");
    }
}
