//! Which ground cells are open, and the cheapest way from any of them to the player, so bots can route around walls.

use crate::grid::Grid;
use crate::sim::{
    find_vault, overlaps, Aabb, BOT_CLIMB_PACE, BOT_REACH, LEDGE_LOW, MAX_SLOPE, SQUEEZE, V3,
};

/// Side of one nav cell. A body is 0.8 wide, so standing in the middle of an open cell it clears every blocked neighbour.
const CELL: f32 = 1.0;
const UNREACHED: u32 = u32::MAX;
/// How far from the player to look for an open cell when they are standing on top of something.
const GOAL_SEARCH: i32 = 6;

/// Tallest rise between neighbouring cells a bot can walk or hop up. A stair cell rises about half a metre.
const CLIMB: f32 = 0.65;
/// Furthest it will drop down on its own: off a roof two storeys up, after a player who jumped.
const DROP: f32 = 7.0;
/// Highest a spot can be above someone's feet and still be what they stand on (they may be mid-hop onto it).
const REACH_UP: f32 = 1.0;
/// Higher than a jump goes (see `sim::JUMP`): a body this far above the spot under it is standing on something else.
const PERCH: f32 = 1.5;
/// Body height a standing spot needs clear above it.
const HEADROOM: f32 = 1.8;
/// Narrowest top a bot can stand on: a garden fence, not a post or a rail.
const FOOTING: f32 = 0.11;
/// Narrowest top a bot walks along freely. One narrower, a fence, costs as much as going this many steps further
/// for each step onto it, so a route only goes over one where there is no other way, as on the way up onto a roof,
/// and along one only the step or two it takes to get to where the climb from it goes up: a parapet beside a gap
/// in it is gone round through the gap, and nobody walks the length of a garden fence.
const WALKWAY: f32 = 0.35;
const NARROW_COST: u32 = 30;
/// Tallest ledge a bot pulls itself up onto (see `sim::BOT_REACH`), with room above it to rise; a climb costs as
/// much as walking this many steps further, so stairs win where there are any.
pub const LEDGE: f32 = 2.9;
const LEDGE_COST: u32 = 40;
/// Getting in or out through a window costs as much as walking this many steps further than the way through it, so
/// a door near by is gone round to.
const VAULT_COST: u32 = 50;
/// Path cost out to which the field is kept while following the player: 140 m of walking.
const FOLLOW: u32 = 1400;
/// More than the dearest single step: a diagonal and a climb onto a narrow top.
const BUCKETS: usize = 128;
/// Half a body's width (see `sim::HW`).
const HW: f32 = 0.4;

/// One standable spot: the middle of a cell at some height.
pub struct Nav {
    half: f32,
    n: usize,
    /// heights of the spots in each cell: `spots[start[c]..start[c + 1]]`
    start: Vec<u32>,
    spots: Vec<f32>,
    /// whether each spot is the top of something narrower than `WALKWAY`
    narrow: Vec<bool>,
    /// the bottom and top of everything in each cell: `solid[solid_start[c]..solid_start[c + 1]]`
    solid_start: Vec<u32>,
    solid: Vec<(f32, f32)>,
    /// the height of the ground in each cell, hills and all
    ground: Vec<f32>,
    /// path cost to the goal, 10 per straight step and 14 per diagonal
    cost: Vec<u32>,
    /// the spots the field leads to: each route goes to whichever is nearest
    goals: Vec<usize>,
    /// how far out from the goal the field was worked out, and whether that took in everywhere with a route
    reach: u32,
    complete: bool,
    /// the spots the route search still has to get to before it stops (see `follow`)
    needed: Vec<bool>,
    /// the spots that can step onto each spot: `into[into_start[i]..into_start[i + 1]]`, with what the step costs
    into_start: Vec<u32>,
    into: Vec<u32>,
    into_cost: Vec<u8>,
    /// the ways through a window (see `sim::find_vault`): from a spot by the wall to one on the far side, with what it
    /// costs, in order of where they are from
    through: Vec<(u32, u32, u8)>,
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
        let mut narrow = Vec::new();
        let mut solid_start = Vec::with_capacity(n * n + 1);
        let mut solid = Vec::new();
        let mut ground = Vec::with_capacity(n * n);
        // each wall's bottom and top, and how wide its top is
        let mut spans: Vec<(f32, f32, f32)> = Vec::new();
        for z in 0..n {
            for x in 0..n {
                start.push(spots.len() as u32);
                let (x0, z0) = (-half + x as f32 * CELL, -half + z as f32 * CELL);
                let g = world.ground_at(x0 + CELL / 2.0, z0 + CELL / 2.0);
                ground.push(g);
                let cell = Aabb {
                    min: [x0, -1.0, z0],
                    max: [x0 + CELL, 200.0, z0 + CELL],
                };
                spans.clear();
                world.find(&cell, |w| {
                    if overlaps(&cell, w) {
                        spans.push((
                            w.min[1],
                            w.max[1],
                            (w.max[0] - w.min[0]).min(w.max[2] - w.min[2]),
                        ));
                    }
                    false
                });
                solid_start.push(solid.len() as u32);
                solid.extend(spans.iter().map(|s| (s.0, s.1)));
                // the top of a post or a rail is no place to stand, and a route up onto it would be climbed at for ever
                let mut tops: Vec<f32> = spans
                    .iter()
                    .filter(|s| s.2 >= FOOTING)
                    .map(|s| s.1)
                    .filter(|t| *t > g)
                    .collect();
                tops.push(g);
                tops.sort_by(|a, b| a.partial_cmp(b).unwrap());
                tops.dedup_by(|a, b| (*a - *b).abs() < 0.01);
                for h in tops {
                    if !spans
                        .iter()
                        .any(|(lo, hi, _)| *lo < h + HEADROOM && *hi > h + 0.02)
                    {
                        spots.push(h);
                        let width = spans
                            .iter()
                            .filter(|s| (s.1 - h).abs() < 0.01)
                            .map(|s| s.2)
                            .fold(0.0, f32::max);
                        narrow.push(h != g && width < WALKWAY);
                    }
                }
            }
        }
        start.push(spots.len() as u32);
        solid_start.push(solid.len() as u32);
        let total = spots.len();
        let mut nav = Nav {
            half,
            n,
            start,
            spots,
            narrow,
            solid_start,
            solid,
            ground,
            cost: vec![UNREACHED; total],
            goals: Vec::new(),
            reach: UNREACHED,
            complete: false,
            needed: Vec::new(),
            into_start: Vec::new(),
            into: Vec::new(),
            into_cost: Vec::new(),
            through: Vec::new(),
            buckets: Vec::new(),
        };
        nav.link(world);
        nav
    }

    fn cell_of(&self, x: f32, z: f32) -> Option<(i32, i32)> {
        let (cx, cz) = (
            ((x + self.half) / CELL).floor() as i32,
            ((z + self.half) / CELL).floor() as i32,
        );
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

    /// Whether spot `s` of cell `c` is the ground itself, not something standing on it.
    fn on_ground(&self, c: (i32, i32), s: usize) -> bool {
        self.spots[s] == self.ground[self.at(c)]
    }

    fn cell_at(&self, spot: usize) -> (i32, i32) {
        let i = self.start.partition_point(|s| *s as usize <= spot) - 1;
        ((i % self.n) as i32, (i / self.n) as i32)
    }

    fn center(&self, c: (i32, i32)) -> [f32; 2] {
        [
            -self.half + (c.0 as f32 + 0.5) * CELL,
            -self.half + (c.1 as f32 + 0.5) * CELL,
        ]
    }

    /// Whether a bot can step or hop from height `from` to `to` in the next cell, `run` away. Up onto a hillside it
    /// can go as steep as a body walks anywhere (`sim::MAX_SLOPE`), which on a diagonal is more than a step: walking
    /// up it, or hopping onto it off a rock on it.
    fn can_step(&self, from: f32, to: f32, hill: bool, run: f32) -> bool {
        let up = if hill {
            CLIMB.max(MAX_SLOPE * run)
        } else {
            CLIMB
        };
        to - from <= up && from - to <= DROP
    }

    /// Whether a bot on spot `from` in cell `c` can pull itself up onto a ledge at height `to`: too tall to step
    /// up, within reach, and with room above the cell for the body once it has risen to the ledge's height. The
    /// ledge itself may hang out over the cell, as the eaves of a roof do over a hedge: the body ends up on it. But
    /// nothing may hang over the cell below the ledge's height: that is a ceiling, and the roof on it is not reached
    /// from inside by going up through it.
    fn can_climb(&self, c: (i32, i32), from: usize, to: f32) -> bool {
        let h = self.spots[from];
        let i = self.at(c);
        let solid = &self.solid[self.solid_start[i] as usize..self.solid_start[i + 1] as usize];
        let overhead = |&(lo, hi): &(f32, f32)| {
            (lo > h + 0.02 && lo < to - 0.02) || (lo < to + HEADROOM + 0.05 && hi > to + 0.02)
        };
        to - h > CLIMB && to - h <= LEDGE && !solid.iter().any(overhead)
    }

    /// The spots at `c2` a bot standing on spot `from` in `c` can walk to and climb up onto, if any. A cell can
    /// hold both, as one beside a hedge under the eaves does: the hedge to walk along, the roof to pull up onto.
    fn moves(&self, c: (i32, i32), from: usize, d: (i32, i32)) -> impl Iterator<Item = usize> {
        [self.move_to(c, from, d), self.climb_to(c, from, d)]
            .into_iter()
            .flatten()
    }

    /// What a step from spot `from` onto spot `to` in direction `d` costs: 10 straight, 14 diagonal, and a climb on
    /// top, unless it is only up a hillside, and more for a narrow top.
    fn step_cost(&self, from: usize, to: usize, d: (i32, i32)) -> u32 {
        let up = self.spots[to] - self.spots[from] > CLIMB && !self.on_ground(self.cell_at(to), to);
        let climb = if up { LEDGE_COST } else { 0 };
        let narrow = if self.narrow[to] { NARROW_COST } else { 0 };
        climb + narrow + if d.0 != 0 && d.1 != 0 { 14 } else { 10 }
    }

    /// The lowest spot at `c2` too tall to step up onto that a bot on spot `from` in `c` can pull itself up onto,
    /// onto walls only: a hillside too steep to walk is not climbed either.
    fn climb_to(&self, c: (i32, i32), from: usize, d: (i32, i32)) -> Option<usize> {
        let c2 = (c.0 + d.0, c.1 + d.1);
        self.in_cell(c2)
            .filter(|&s| !self.on_ground(c2, s) && self.can_climb(c, from, self.spots[s]))
            .min_by(|a, b| self.spots[*a].partial_cmp(&self.spots[*b]).unwrap())
    }

    /// Whether a body passing the corner of cell `side`, at height `top`, clears everything in it: nothing there
    /// rises more than a step above its feet, wall or hillside, within its height.
    fn clear(&self, side: (i32, i32), top: f32) -> bool {
        let n = self.n as i32;
        if side.0 < 0 || side.1 < 0 || side.0 >= n || side.1 >= n {
            return false;
        }
        let i = self.at(side);
        let solid = &self.solid[self.solid_start[i] as usize..self.solid_start[i + 1] as usize];
        self.ground[i] <= top + CLIMB
            && !solid
                .iter()
                .any(|(lo, hi)| *hi > top + CLIMB && *lo < top + HEADROOM)
    }

    /// The spot at `c2` a bot standing on spot `from` in `c` can walk to, if any. A diagonal goes past the corners
    /// of the cells beside it, so nobody cuts a corner through a wall; or out on a bare hillside, where the slope
    /// stops a body along each axis on its own, it slides along whichever way the slope lets it.
    fn move_to(&self, c: (i32, i32), from: usize, d: (i32, i32)) -> Option<usize> {
        let h = self.spots[from];
        let c2 = (c.0 + d.0, c.1 + d.1);
        let ground = self.on_ground(c, from);
        let run = if d.0 != 0 && d.1 != 0 {
            std::f32::consts::SQRT_2
        } else {
            1.0
        };
        let to = self
            .in_cell(c2)
            .filter(|&s| self.can_step(h, self.spots[s], self.on_ground(c2, s), run))
            .min_by(|a, b| {
                (self.spots[*a] - h)
                    .abs()
                    .partial_cmp(&(self.spots[*b] - h).abs())
                    .unwrap()
            })?;
        if d.0 != 0 && d.1 != 0 {
            let t = self.spots[to];
            let hill = ground && self.on_ground(c2, to);
            let sides = [(c.0 + d.0, c.1), (c.0, c.1 + d.1)];
            // bare: nothing built in the cells beside, only the ground
            let bare = hill
                && sides.iter().all(|&side| {
                    self.in_cell(side).all(|s| self.on_ground(side, s))
                        && !self.in_cell(side).is_empty()
                });
            let passable = |side: (i32, i32)| {
                let hill = |s: usize| hill && self.on_ground(side, s);
                self.in_cell(side).any(|s| {
                    self.can_step(h, self.spots[s], hill(s), 1.0)
                        && self.can_step(self.spots[s], t, hill(s), 1.0)
                })
            };
            // up a hillside more than a step, it walks it along x and z, each no steeper than a body walks: so by
            // way of a cell beside it, whatever else is or isn't there
            let ok = if hill && t - h > CLIMB {
                sides.iter().any(|&s| passable(s))
            } else {
                sides.iter().all(|&s| self.clear(s, h.max(t)))
                    || bare && sides.iter().any(|&s| passable(s))
            };
            if !ok {
                return None;
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
        let own = self.in_cell(c).min_by(|a, b| {
            (self.spots[*a] - p[1])
                .abs()
                .partial_cmp(&(self.spots[*b] - p[1]).abs())
                .unwrap()
        });
        // more than a jump above anything in its own cell, it is perched on the corner of something in the next,
        // as the canopy of a tree overhangs past the trunk: taken to be on the ground under it, it would stand
        // up there waiting to find the climb onto the fence its route goes up next
        let perched = |s: usize| p[1] - self.spots[s] > PERCH;
        if let Some(s) = own.filter(|&s| {
            self.spots[s] < p[1] + REACH_UP && self.cost[s] != UNREACHED && !perched(s)
        }) {
            return Some(self.along_narrow(p, s));
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

    /// A body on spot `s` that is the top of a fence is on the fence wherever its feet overlap it, and a fence
    /// across the line between two cells is a spot in each. Going along it from one to the other is dear (see
    /// `WALKWAY`), and dearer than it is worth to a body already standing on both, so it is on whichever has the
    /// shorter route: climbed onto from a garden, it is set down just over the middle of the fence, and taken to
    /// be on the far side's spot it would go down off it into the street and round to the garden again.
    fn along_narrow(&self, p: V3, s: usize) -> usize {
        if !self.narrow[s] {
            return s;
        }
        let h = self.spots[s];
        let mut best = s;
        for (x, z) in [
            (p[0] - HW, p[2] - HW),
            (p[0] + HW, p[2] - HW),
            (p[0] - HW, p[2] + HW),
            (p[0] + HW, p[2] + HW),
        ] {
            let Some(c) = self.cell_of(x, z) else {
                continue;
            };
            for k in self.in_cell(c) {
                if self.narrow[k]
                    && (self.spots[k] - h).abs() < 0.01
                    && self.cost[k] < self.cost[best]
                {
                    best = k;
                }
            }
        }
        best
    }

    /// Points the field at `p`, at whatever height it is standing, from everywhere there is a route.
    pub fn set_goal(&mut self, p: V3) {
        self.route_to(&[p], UNREACHED, &[]);
    }

    /// Points the field at the nearest of `ps` (the players) from as far as `FOLLOW` away, the same work on a big
    /// map as on a small one, and on past that as far as it takes to reach the zombies at `need` too: most of the
    /// time they are well inside it, but someone up on the school roof is a 200 m walk from the street, up the one
    /// stair at the far end of it. Does nothing if the players are all still on the same spots and the zombies
    /// all have a route, or none can be had.
    pub fn follow(&mut self, ps: &[V3], need: &[V3]) {
        let need: Vec<usize> = need
            .iter()
            .filter_map(|&p| self.nearest_spot(p, 1))
            .collect();
        self.route_to(ps, FOLLOW, &need);
    }

    /// Points the field at the nearest of `ps` from everywhere within `reach` in path cost, or as far as it takes
    /// to get to all of `need`.
    fn route_to(&mut self, ps: &[V3], reach: u32, need: &[usize]) {
        let mut goals: Vec<usize> = ps
            .iter()
            .filter_map(|&p| self.nearest_spot(p, GOAL_SEARCH))
            .collect();
        goals.sort_unstable();
        goals.dedup();
        let short = !self.complete && need.iter().any(|&s| self.cost[s] == UNREACHED);
        if goals == self.goals && reach == self.reach && !short {
            return;
        }
        self.reach = reach;
        self.cost.fill(UNREACHED);
        self.needed.resize(self.spots.len(), false);
        let mut wanted = 0;
        for &s in need {
            if !self.needed[s] {
                self.needed[s] = true;
                wanted += 1;
            }
        }
        // Dijkstra with a bucket per cost instead of a heap: no step costs more than BUCKETS - 1, so the spots
        // waiting to be done only ever span that many costs, and a ring of buckets holds them in order
        let mut buckets = std::mem::take(&mut self.buckets);
        buckets.resize_with(BUCKETS, Vec::new);
        for &g in &goals {
            self.cost[g] = 0;
            buckets[0].push(g as u32);
        }
        let (mut d, mut waiting) = (0u32, goals.len());
        self.goals = goals;
        // past `reach`, only as long as some of `need` are still to be got to
        while waiting > 0 && (d <= reach || wanted > 0) {
            let b = d as usize % BUCKETS;
            while let Some(i) = buckets[b].pop() {
                waiting -= 1;
                let i = i as usize;
                if self.cost[i] != d {
                    continue;
                }
                if self.needed[i] {
                    self.needed[i] = false;
                    wanted -= 1;
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
        // stopped short: what is still waiting is past the edge of the field, as if never got to
        self.complete = waiting == 0;
        for b in &mut buckets {
            for i in b.drain(..) {
                if self.cost[i as usize] >= d {
                    self.cost[i as usize] = UNREACHED;
                }
            }
        }
        for &s in need {
            self.needed[s] = false;
        }
        self.buckets = buckets;
    }

    /// Where a bot on spot `from` in cell `c` comes down after getting through a window straight ahead of it in
    /// direction `d`, if there is one: the first spot on along the way at the height of the floor it lands on, and
    /// what going that way costs.
    fn vault(
        &self,
        world: &Grid,
        c: (i32, i32),
        from: usize,
        d: (i32, i32),
    ) -> Option<(usize, u32)> {
        let h = self.spots[from];
        // only where there is a wall with a sill and a head in it in the next cell, which is where a body walking on
        // would bump into it, or in the cells either side of that, as it looks along the wall a little way
        let n = self.n as i32;
        let mut sill = false;
        let mut head = false;
        for k in -1..=1 {
            let w = (c.0 + d.0 + k * d.1, c.1 + d.1 + k * d.0);
            if w.0 < 0 || w.1 < 0 || w.0 >= n || w.1 >= n {
                continue;
            }
            let i = self.at(w);
            for &(lo, hi) in
                &self.solid[self.solid_start[i] as usize..self.solid_start[i + 1] as usize]
            {
                sill |= hi > h + LEDGE_LOW && hi <= h + BOT_REACH;
                head |= lo > h + LEDGE_LOW + SQUEEZE - 0.01 && lo < h + BOT_REACH + HEADROOM;
            }
        }
        if !(sill && head) {
            return None;
        }
        let m = self.center(c);
        let to = find_vault(
            [m[0], h, m[1]],
            (d.0 as f32).atan2(-d.1 as f32),
            BOT_REACH,
            BOT_CLIMB_PACE,
            true,
            world,
        )?
        .end();
        let land = (to[1] - world.raycast(to, [0.0, -1.0, 0.0], DROP + 0.5))
            .max(world.ground_at(to[0], to[2]));
        if to[1] - land > DROP {
            return None;
        }
        // past the wall: a cell whose middle is in it has no spot
        let out = (to[0] - m[0]) * d.0 as f32 + (to[2] - m[1]) * d.1 as f32;
        (1..=3).find_map(|k| {
            let c2 = (c.0 + d.0 * k, c.1 + d.1 * k);
            let s = self
                .in_cell(c2)
                .find(|&s| (self.spots[s] - land).abs() < 0.3)?;
            (k as f32 * CELL > out - 0.5).then_some((s, VAULT_COST + 10 * k as u32))
        })
    }

    /// Works out every step a bot can take, once, and files each under the spot it lands on, so a route search
    /// only has to read them off.
    fn link(&mut self, world: &Grid) {
        let n = self.n as i32;
        let mut steps: Vec<(u32, u32, u8)> = Vec::new();
        let mut through = Vec::new();
        for z in 0..n {
            for x in 0..n {
                for j in self.in_cell((x, z)) {
                    for d in NEIGHBOURS {
                        for i in self.moves((x, z), j, d) {
                            steps.push((i as u32, j as u32, self.step_cost(j, i, d) as u8));
                        }
                    }
                    for d in &NEIGHBOURS[..4] {
                        if let Some((i, cost)) = self.vault(world, (x, z), j, *d) {
                            steps.push((i as u32, j as u32, cost as u8));
                            through.push((j as u32, i as u32, cost as u8));
                        }
                    }
                }
            }
        }
        self.through = through;
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

    /// Every spot a bot standing at `p` can get to, as points (x, height, z): the other way round from the field,
    /// which is the spots that can get to the goal.
    #[cfg(test)]
    pub fn reached_from(&self, p: V3) -> Vec<V3> {
        let Some(first) = self.nearest_spot(p, 1) else {
            return Vec::new();
        };
        let mut seen = vec![false; self.spots.len()];
        seen[first] = true;
        let mut queue = vec![first];
        while let Some(s) = queue.pop() {
            let c = self.cell_at(s);
            let through: Vec<usize> = self.through_from(s).map(|(to, _)| to).collect();
            for to in NEIGHBOURS
                .iter()
                .flat_map(|&d| self.moves(c, s, d))
                .chain(through)
            {
                if !seen[to] {
                    seen[to] = true;
                    queue.push(to);
                }
            }
        }
        (0..self.spots.len())
            .filter(|&s| seen[s])
            .map(|s| {
                let m = self.center(self.cell_at(s));
                [m[0], self.spots[s], m[1]]
            })
            .collect()
    }

    /// The spots in the cell under `p` with their path costs, and the bottoms and tops of what is in it.
    #[cfg(test)]
    pub fn describe(&self, p: V3) -> String {
        let Some(c) = self.cell_of(p[0], p[2]) else {
            return "off the map".into();
        };
        let i = self.at(c);
        let spots: Vec<String> = self
            .in_cell(c)
            .map(|s| {
                format!(
                    "{:.2}:{}",
                    self.spots[s],
                    if self.cost[s] == UNREACHED {
                        "-".into()
                    } else {
                        self.cost[s].to_string()
                    }
                )
            })
            .collect();
        let solid: Vec<String> = self.solid
            [self.solid_start[i] as usize..self.solid_start[i + 1] as usize]
            .iter()
            .map(|(lo, hi)| format!("{lo:.2}-{hi:.2}"))
            .collect();
        format!("spots [{}] solid [{}]", spots.join(" "), solid.join(" "))
    }

    /// Middle of the cell `p` is in, if there is somewhere to stand in it.
    pub fn snap(&self, p: V3) -> Option<[f32; 2]> {
        let c = self.cell_of(p[0], p[2])?;
        (!self.in_cell(c).is_empty()).then(|| self.center(c))
    }

    /// True if there is ground to stand on in the cell under `p`.
    pub fn open_at(&self, p: V3) -> bool {
        self.cell_of(p[0], p[2])
            .is_some_and(|c| self.in_cell(c).any(|s| self.on_ground(c, s)))
    }

    /// Ground spots, and how many of them have a route to the goal.
    pub fn coverage(&self) -> (usize, usize) {
        let n = self.n as i32;
        let ground: Vec<usize> = (0..n * n)
            .map(|i| (i % n, i / n))
            .flat_map(|c| self.in_cell(c).filter(move |&s| self.on_ground(c, s)))
            .collect();
        let reached = ground
            .iter()
            .filter(|&&s| self.cost[s] != UNREACHED)
            .count();
        (ground.len(), reached)
    }

    /// The middles of the ground spots with a walk to the goal of no more than `walk` metres, as points.
    pub fn ground_within(&self, walk: f32) -> Vec<V3> {
        let n = self.n as i32;
        let most = (walk * 10.0 / CELL) as u32;
        (0..n * n)
            .map(|i| (i % n, i / n))
            .flat_map(|c| {
                self.in_cell(c)
                    .filter(move |&s| self.on_ground(c, s) && self.cost[s] <= most)
                    .map(move |s| (c, s))
            })
            .map(|(c, s)| {
                let m = self.center(c);
                [m[0], self.spots[s], m[1]]
            })
            .collect()
    }

    /// True if a body can walk across `p` at about the height of its feet: the cell under it has a spot within
    /// a step of them, and not the top of a fence.
    pub fn level_at(&self, p: V3) -> bool {
        self.cell_of(p[0], p[2]).is_some_and(|c| {
            self.in_cell(c)
                .any(|s| (self.spots[s] - p[1]).abs() < CLIMB && !self.narrow[s])
        })
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

    /// The middle of the cell of the spot the route takes a body at `p` to be on: where its next step goes from.
    pub fn from_cell(&self, p: V3) -> Option<[f32; 2]> {
        self.spot_of(p).map(|s| self.center(self.cell_at(s)))
    }

    /// The height of the spot the route takes a body at `p` to be on. A body on the ground beside a hedge that
    /// half fills its cell is on the hedge as far as the route goes, and has to hop up onto it to go on.
    pub fn standing(&self, p: V3) -> Option<f32> {
        self.spot_of(p).map(|s| self.spots[s])
    }

    /// The ways through a window from spot `s`: where to, and what it costs.
    fn through_from(&self, s: usize) -> impl Iterator<Item = (usize, u32)> + '_ {
        let a = self.through.partition_point(|t| (t.0 as usize) < s);
        let b = self.through.partition_point(|t| (t.0 as usize) <= s);
        self.through[a..b]
            .iter()
            .map(|t| (t.1 as usize, t.2 as u32))
    }

    /// Where to step next from `p` on the way to the goal, as a point (x, height, z), or None if `p` is not
    /// on the field (inside a wall, unreachable) or is already at the goal.
    pub fn next(&self, p: V3) -> Option<V3> {
        self.next_step(p).map(|(to, _)| to)
    }

    /// `next`, and whether the way there is through a window.
    pub fn next_step(&self, p: V3) -> Option<(V3, bool)> {
        let s = self.spot_of(p)?;
        let here = self.cost[s];
        if here == UNREACHED || here == 0 {
            return None;
        }
        let c = self.cell_at(s);
        let mut best = (u32::MAX, None);
        let steps = NEIGHBOURS.iter().flat_map(|&d| {
            self.moves(c, s, d)
                .map(move |to| (to, self.step_cost(s, to, d), false))
        });
        for (to, cost, through) in
            steps.chain(self.through_from(s).map(|(to, cost)| (to, cost, true)))
        {
            let total = self.cost[to].saturating_add(cost);
            if self.cost[to] < here && total < best.0 {
                best = (total, Some((to, through)));
            }
        }
        best.1.map(|(to, through)| {
            let m = self.center(self.cell_at(to));
            ([m[0], self.spots[to], m[1]], through)
        })
    }
}

const NEIGHBOURS: [(i32, i32); 8] = [
    (1, 0),
    (-1, 0),
    (0, 1),
    (0, -1),
    (1, 1),
    (1, -1),
    (-1, 1),
    (-1, -1),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn wall(x0: f32, z0: f32, x1: f32, z1: f32) -> Aabb {
        Aabb {
            min: [x0, 0.0, z0],
            max: [x1, 3.0, z1],
        }
    }

    /// The ways through windows on map MAP (default the suburb) nearest the middle, to look at in the game.
    #[test]
    #[ignore]
    fn windows() {
        let map: u8 = std::env::var("MAP")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(1);
        let m = crate::map::make(map, 1);
        let world = Grid::new(m.walls, 4.0).with_ground(m.ground);
        let nav = Nav::new(&world, m.half);
        let at = |s: u32| {
            let c = nav.center(nav.cell_at(s as usize));
            [c[0], nav.spots[s as usize], c[1]]
        };
        let mut ways: Vec<(V3, V3)> = nav.through.iter().map(|t| (at(t.0), at(t.1))).collect();
        ways.sort_by(|a, b| a.0[0].hypot(a.0[2]).total_cmp(&b.0[0].hypot(b.0[2])));
        println!("{} ways through windows", ways.len());
        for (a, b) in ways.iter().take(12) {
            println!("{a:.1?} -> {b:.1?}");
        }
    }

    #[test]
    fn routes_around_a_wall_and_never_through_one() {
        // a wall across the middle with a gap at the +x end
        let nav_world = Grid::new(
            vec![
                wall(-20.0, -0.5, 15.0, 0.5),
                wall(-21.0, -21.0, 21.0, -20.0),
            ],
            4.0,
        );
        let mut nav = Nav::new(&nav_world, 20.0);
        nav.set_goal([0.0, 0.0, 10.0]);
        let mut p = [0.0, 0.0, -10.0];
        for _ in 0..200 {
            let Some(to) = nav.next(p) else {
                break;
            };
            p = [to[0], to[1], to[2]];
            let cell = Aabb {
                min: [p[0] - 0.4, 0.0, p[2] - 0.4],
                max: [p[0] + 0.4, 0.1, p[2] + 0.4],
            };
            assert!(
                !nav_world.find(&cell, |w| overlaps(&cell, w)),
                "stepped into a wall at {p:?}"
            );
        }
        assert!(
            (p[2] - 10.0).abs() < 1.5 && p[0].abs() < 1.5,
            "stopped at {p:?}"
        );
    }

    #[test]
    fn a_goal_on_top_of_a_wall_aims_at_its_foot() {
        let world = Grid::new(vec![wall(-2.0, -2.0, 2.0, 2.0)], 4.0);
        let mut nav = Nav::new(&world, 10.0);
        nav.set_goal([0.0, 1.0, 0.0]);
        assert!(nav.next([0.0, 0.0, 8.0]).is_some());
        // and one on top of it jumps down to the foot, after a player who did
        let down = nav.next([0.0, 3.0, 0.0]).expect("no way down off the wall");
        assert!(
            down[1] < 3.0 || down[0].abs().max(down[2].abs()) > 1.0,
            "stays up on the wall: {down:?}"
        );
    }
}
