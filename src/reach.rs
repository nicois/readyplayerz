//! Tests that wherever a player can climb to, the zombies can follow and get at them.
//!
//! Where a player can get is worked out here on its own, from what a player can do (walk up a slope, jump and pull
//! up onto a ledge, stand on anything a body fits on top of, drop off anything), not from the bots' nav, so a gap in
//! the nav shows up as a place the player reaches and the zombies don't.

use crate::grid::Grid;
use crate::map::{self, Map};
use crate::nav::Nav;
use crate::sim::{overlaps, Aabb, STRIKE_DOWN, STRIKE_UP, V3};
use std::collections::{HashMap, VecDeque};

/// A body's height: a top needs this much clear above it to be stood on.
const BODY: f32 = 1.8;
/// Highest a player gets their feet onto from where they stand: the top of a jump and a pull up from there
/// (see `sim::JUMP`, `sim::GRAVITY` and `sim::LEDGE_REACH`), less a little.
const UP: f32 = 7.5 * 7.5 / 44.0 + 1.6 - 0.05;
/// Steepest hillside a body walks up, rise per metre (see `sim::MAX_SLOPE`).
const SLOPE: f32 = 0.75;

/// One place to stand: a 1 m cell, and a height in it.
#[derive(Clone, Copy)]
struct Spot {
    x: i32,
    z: i32,
    h: f32,
    ground: bool,
    /// whether what it is the top of is under the middle of the cell, so a body put there stays on it
    middle: bool,
}

/// Every place in an arena a body could stand: the ground and the top of anything in each 1 m cell, thin or not,
/// with a body's height clear above it.
struct Places {
    half: f32,
    spots: Vec<Spot>,
    index: HashMap<(i32, i32), Vec<usize>>,
    /// the bottom and top of what is in each cell, so a climb can't go up through a roof overhead nor a step cut
    /// through the corner of a wall
    solid: HashMap<(i32, i32), Vec<(f32, f32)>>,
    ground: HashMap<(i32, i32), f32>,
}

impl Places {
    fn new(world: &Grid, half: f32) -> Places {
        let n = (2.0 * half).ceil() as i32;
        let mut spots = Vec::new();
        let mut index: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
        let mut solid = HashMap::new();
        let mut ground = HashMap::new();
        let mut spans = Vec::new();
        for z in 0..n {
            for x in 0..n {
                let (x0, z0) = (-half + x as f32, -half + z as f32);
                let g = world.ground_at(x0 + 0.5, z0 + 0.5);
                let cell = Aabb { min: [x0, -1.0, z0], max: [x0 + 1.0, 200.0, z0 + 1.0] };
                spans.clear();
                world.find(&cell, |w| {
                    if overlaps(&cell, w) {
                        // something that only nicks a corner of the cell, like the edge of a floor sticking out
                        // past a wall, is in the way but is no place to stand: it would take landing on a hand's
                        // breadth of it. So is the top of a post, which this misses too
                        let along = |a: usize| w.max[a].min(cell.max[a]) - w.min[a].max(cell.min[a]);
                        let middle = w.min[0] <= x0 + 0.5 && w.max[0] >= x0 + 0.5 && w.min[2] <= z0 + 0.5 && w.max[2] >= z0 + 0.5;
                        spans.push((w.min[1], w.max[1], along(0).max(along(2)) >= 0.25, middle));
                    }
                    false
                });
                solid.insert((x, z), spans.iter().map(|s| (s.0, s.1)).collect::<Vec<_>>());
                ground.insert((x, z), g);
                let mut tops: Vec<f32> = spans.iter().filter(|s| s.2).map(|s| s.1).filter(|t| *t > g).chain([g]).collect();
                tops.sort_by(f32::total_cmp);
                tops.dedup_by(|a, b| (*a - *b).abs() < 0.01);
                let here = index.entry((x, z)).or_default();
                for h in tops {
                    if !spans.iter().any(|(lo, hi, _, _)| *lo < h + BODY && *hi > h + 0.02) {
                        here.push(spots.len());
                        let middle = h == g || spans.iter().any(|s| s.3 && (s.1 - h).abs() < 0.01);
                        spots.push(Spot { x, z, h, ground: h == g, middle });
                    }
                }
            }
        }
        Places { half, spots, index, solid, ground }
    }

    fn cell(&self, p: V3) -> (i32, i32) {
        ((p[0] + self.half).floor() as i32, (p[2] + self.half).floor() as i32)
    }

    fn at(&self, i: usize) -> V3 {
        let s = self.spots[i];
        [-self.half + s.x as f32 + 0.5, s.h, -self.half + s.z as f32 + 0.5]
    }

    /// The place in the cell under `p` nearest its height.
    fn under(&self, p: V3) -> Option<usize> {
        self.index.get(&self.cell(p))?.iter().copied().min_by(|a, b| (self.spots[*a].h - p[1]).abs().total_cmp(&(self.spots[*b].h - p[1]).abs()))
    }

    /// Where a player standing on place `i` can get to in one go, into its own cell or one beside it: down any
    /// drop, up a hillside as steep as a body walks, and up onto anything built within a jump and a pull up.
    /// A step on the diagonal goes past the corners of the cells beside: nothing in either may rise more than a
    /// step above the feet within a body's height, or it would cut through the corner of a wall. On a bare hillside
    /// one side being walkable will do, as the slope stops a body along each axis on its own and it slides along
    /// the other. A climb goes up beside the ledge and over, so it doesn't pass the corners.
    fn steps(&self, i: usize) -> impl Iterator<Item = usize> + '_ {
        let s = self.spots[i];
        let walk = move |a: Spot, b: Spot| b.h - a.h <= SLOPE;
        let cell = move |x: i32, z: i32| self.index.get(&(x, z)).into_iter().flatten().map(|&k| self.spots[k]);
        let clear = move |c: (i32, i32), top: f32| {
            self.ground.get(&c).is_some_and(|g| *g <= top + 0.65) && !self.solid[&c].iter().any(|(lo, hi)| *hi > top + 0.65 && *lo < top + BODY)
        };
        (-1..=1).flat_map(move |dz| (-1..=1).map(move |dx| (dx, dz))).flat_map(move |(dx, dz)| {
            self.index.get(&(s.x + dx, s.z + dz)).into_iter().flatten().copied().filter(move |&j| {
                let t = self.spots[j];
                if j == i || t.h - s.h > if t.ground { SLOPE * if dx != 0 && dz != 0 { 2f32.sqrt() } else { 1.0 } } else { UP } {
                    return false;
                }
                // rising to a ledge needs the air clear overhead up to its height, not up through a roof, and room
                // there for the body once it has risen, to go over onto it (see `sim::find_ledge`)
                let rises = t.h - s.h > 0.65 && !t.ground;
                let overhead = |&(lo, hi): &(f32, f32)| (lo >= s.h + BODY - 0.01 && lo < t.h - 0.02) || (lo < t.h + BODY && hi > t.h + 0.02);
                if rises && self.solid[&(s.x, s.z)].iter().any(overhead) {
                    return false;
                }
                if dx == 0 || dz == 0 || rises {
                    return true;
                }
                let sides = [(s.x + dx, s.z), (s.x, s.z + dz)];
                // up a hillside more than a step, along x and z each no steeper than a body walks
                if s.ground && t.ground && t.h - s.h > 0.65 {
                    return sides.iter().any(|&(x, z)| cell(x, z).any(|m| m.ground && walk(s, m) && walk(m, t)));
                }
                if sides.iter().all(|&c| clear(c, s.h.max(t.h))) {
                    return true;
                }
                let bare = s.ground && t.ground && sides.iter().all(|&(x, z)| cell(x, z).count() > 0 && cell(x, z).all(|k| k.ground));
                bare && sides.iter().any(|&(x, z)| cell(x, z).any(|m| walk(s, m) && walk(m, t)))
            })
        })
    }

    /// `places` gathered into patches of places side by side, within a climb of each other.
    fn patches(&self, places: Vec<usize>) -> Vec<Vec<usize>> {
        let mut by_cell: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
        for &i in &places {
            by_cell.entry((self.spots[i].x, self.spots[i].z)).or_default().push(i);
        }
        let mut done = std::collections::HashSet::new();
        let mut patches = Vec::new();
        for &i in &places {
            if !done.insert(i) {
                continue;
            }
            let (mut patch, mut queue) = (Vec::new(), vec![i]);
            while let Some(k) = queue.pop() {
                patch.push(k);
                let s = self.spots[k];
                for dz in -1..=1 {
                    for dx in -1..=1 {
                        for &j in by_cell.get(&(s.x + dx, s.z + dz)).into_iter().flatten() {
                            if (self.spots[j].h - s.h).abs() < UP && done.insert(j) {
                                queue.push(j);
                            }
                        }
                    }
                }
            }
            patches.push(patch);
        }
        patches
    }

    /// Every place a player standing at `start` can get to, each with the one it was first got to from (the start
    /// is its own), breadth first so the way back is a shortest one.
    fn player_reach(&self, start: V3) -> HashMap<usize, usize> {
        let mut from = HashMap::new();
        let Some(first) = self.under(start) else { return from };
        from.insert(first, first);
        let mut queue = VecDeque::from([first]);
        while let Some(i) = queue.pop_front() {
            for j in self.steps(i) {
                if !from.contains_key(&j) {
                    from.insert(j, i);
                    queue.push_back(j);
                }
            }
        }
        from
    }
}

/// What a zombie starting at `start` can get to, and what the player can that no zombie can get at.
struct Gaps {
    places: Places,
    /// the heights the zombies reach in each cell
    zombie: HashMap<(i32, i32), Vec<f32>>,
    player: HashMap<usize, usize>,
}

impl Gaps {
    fn new(map: Map, start: V3) -> Gaps {
        let half = map.half;
        let world = Grid::new(map.walls, 4.0).with_ground(map.ground);
        let start = [start[0], world.ground_at(start[0], start[2]), start[2]];
        let places = Places::new(&world, half);
        let mut zombie: HashMap<(i32, i32), Vec<f32>> = HashMap::new();
        for p in Nav::new(&world, half).reached_from(start) {
            zombie.entry(places.cell(p)).or_default().push(p[1]);
        }
        let player = places.player_reach(start);
        Gaps { places, zombie, player }
    }

    /// Whether a zombie gets to stand on place `i` itself.
    fn zombie_on(&self, i: usize) -> bool {
        let p = self.places.at(i);
        self.zombie.get(&self.places.cell(p)).is_some_and(|hs| hs.iter().any(|h| (h - p[1]).abs() < 0.02))
    }

    /// Whether a zombie can get within striking distance of someone on place `i`: onto a spot up to two cells away,
    /// with their feet in its reach (`sim::STRIKE_UP` and `STRIKE_DOWN`). Striking reaches 1.7 m, and someone on a
    /// parapet or a railing stands at the edge of their cell, nearer the roof than its middle is: at the corner of a
    /// parapet, the nearest cell of roof is two away on the diagonal.
    fn zombie_at(&self, i: usize) -> bool {
        let p = self.places.at(i);
        let c = self.places.cell(p);
        (-2..=2).any(|dz| (-2..=2).any(|dx| self.zombie.get(&(c.0 + dx, c.1 + dz)).is_some_and(|hs| hs.iter().any(|h| (-STRIKE_DOWN..STRIKE_UP).contains(&(p[1] - h))))))
    }

    /// The places the player gets to that no zombie can get at, in patches, the patch with the highest place in it
    /// first; each with its highest place and how many places are in it.
    fn missed(&self) -> Vec<(V3, usize)> {
        let missed = self.player.keys().copied().filter(|&i| !self.zombie_at(i)).collect();
        let top = |p: &Vec<usize>| p.iter().copied().max_by(|a, b| self.places.spots[*a].h.total_cmp(&self.places.spots[*b].h)).unwrap();
        let mut patches: Vec<(V3, usize)> = self.places.patches(missed).iter().map(|p| (self.places.at(top(p)), p.len())).collect();
        patches.sort_by(|a, b| b.0[1].total_cmp(&a.0[1]));
        patches
    }
}

fn show(p: V3) -> String {
    format!("({:.1} {:.2} {:.1})", p[0], p[1], p[2])
}

#[test]
fn the_zombies_can_get_at_the_player_anywhere_on_either_map() {
    let mut report = Vec::new();
    for map in 0..map::MAPS {
        for seed in 1..3 {
            let gaps = Gaps::new(map::make(map, seed), [0.0, 0.0, 14.0]);
            let missed = gaps.missed();
            if missed.is_empty() {
                continue;
            }
            let shown: Vec<String> = missed.iter().take(40).map(|(p, n)| format!("{} x{n}", show(*p))).collect();
            report.push(format!("map {map} seed {seed}: {} patches: {}", missed.len(), shown.join(" ")));
        }
    }
    assert!(report.is_empty(), "places the player can get to and the zombies can't get at:\n{}", report.join("\n"));
}

/// The nav can have a route up somewhere the zombies still don't manage to follow it, as with an eave over a hedge
/// they had to climb onto facing the house. So: for every patch of places the player can get to too high to be
/// struck at from the ground below, put the player there and a few zombies on the ground nearby, and see that one
/// gets to them.
#[test]
fn zombies_get_up_to_the_player_everywhere_up_high() {
    let mut failed = Vec::new();
    let mut through = Vec::new();
    let mut tried = 0;
    for map in 0..map::MAPS {
        let gaps = Gaps::new(map::make(map, 1), [0.0, 0.0, 14.0]);
        let m = map::make(map, 1);
        let world = Grid::new(m.walls, 4.0).with_ground(m.ground);
        let mut nav = Nav::new(&world, m.half);
        let p = &gaps.places;
        let ground = |i: usize| p.index[&(p.spots[i].x, p.spots[i].z)].iter().map(|&k| p.spots[k]).find(|k| k.ground).map_or(0.0, |k| k.h);
        // in order, so the same zombies start in the same places every run
        let mut high: Vec<usize> = gaps.player.keys().copied().filter(|&i| p.spots[i].h - ground(i) >= STRIKE_UP).collect();
        high.sort();
        let mut starts: Vec<usize> = gaps.player.keys().copied().filter(|&i| p.spots[i].ground && gaps.zombie_on(i)).collect();
        starts.sort();
        let starts: Vec<V3> = starts.into_iter().map(|i| p.at(i)).collect();
        let mut g = crate::sim::Game::on(map, 1);
        for patch in p.patches(high) {
            // the highest place in it a body stays put on
            let Some(top) = patch.iter().copied().filter(|&i| p.spots[i].middle).max_by(|a, b| p.spots[*a].h.total_cmp(&p.spots[*b].h)) else { continue };
            let at = p.at(top);
            // a short walk off, as the waves send them: near as the crow flies may be the foot of a bank too steep
            // to climb, a long way round
            nav.set_goal(at);
            let walk = |s: &V3| nav.route_len(*s).unwrap_or(f32::INFINITY);
            let mut near: Vec<V3> = starts.iter().copied().filter(|s| (8.0..20.0).contains(&(s[0] - at[0]).hypot(s[2] - at[2])) && walk(s) < 100.0).collect();
            near.sort_by(|a, b| walk(a).total_cmp(&walk(b)));
            if near.is_empty() {
                continue;
            }
            tried += 1;
            g.bots.clear();
            for (k, s) in [near[0], near[near.len() / 2], near[near.len() - 1]].into_iter().enumerate() {
                let mut z = crate::sim::Bot::zombie(s, k as f32);
                z.hp = 1e6;
                g.bots.push(z);
            }
            g.players[0].body.pos = [at[0], at[1] + 0.05, at[2]];
            g.players[0].body.vel = [0.0; 3];
            g.players[0].climb = None;
            let mut struck = false;
            for _ in 0..(45.0 / crate::sim::STEP) as usize {
                g.players[0].hp = 100.0;
                let before: Vec<V3> = g.bots.iter().map(|b| b.body.pos).collect();
                g.tick(crate::sim::STEP);
                // and none of them gets there up through a floor or a roof
                for (b, a) in g.bots.iter().zip(before) {
                    let (x, z, y) = (b.body.pos[0], b.body.pos[2], b.body.pos[1]);
                    let column = Aabb { min: [x - 0.05, a[1], z - 0.05], max: [x + 0.05, y, z + 0.05] };
                    g.world.find(&column, |w| {
                        if overlaps(&column, w) && w.min[0] < x - 0.05 && w.max[0] > x + 0.05 && w.min[2] < z - 0.05 && w.max[2] > z + 0.05 && a[1] < w.min[1] && y > w.max[1] - 0.05 {
                            through.push(format!("map {map}: a zombie went from {} up through {:.2?}..{:.2?} to {}", show(a), w.min, w.max, show(b.body.pos)));
                        }
                        false
                    });
                }
                // the waves would bring more
                g.bots.truncate(3);
                if g.players[0].hp < 100.0 {
                    struck = true;
                    break;
                }
            }
            if !struck {
                let them: Vec<String> = (0..g.bots.len()).map(|i| g.describe_bot(i)).collect();
                failed.push(format!("map {map}: nobody got to the player at {} ({} places), now at {}:\n    {}", show(at), patch.len(), show(g.players[0].body.pos), them.join("\n    ")));
            }
        }
    }
    assert!(failed.is_empty(), "{} of {tried} places up high the zombies never got to:\n{}", failed.len(), failed.join("\n"));
    through.dedup();
    assert!(through.is_empty(), "{}", through.join("\n"));
}

/// Prints the player's way from the middle to TO=x,height,z on MAP made from SEED, marking the places a zombie
/// stands on too.
#[test]
#[ignore]
fn the_way_to() {
    let map: u8 = std::env::var("MAP").ok().and_then(|s| s.parse().ok()).unwrap_or(1);
    let seed: u32 = std::env::var("SEED").ok().and_then(|s| s.parse().ok()).unwrap_or(1);
    let to: Vec<f32> = std::env::var("TO").unwrap().split(',').map(|s| s.parse().unwrap()).collect();
    let gaps = Gaps::new(map::make(map, seed), [0.0, 0.0, 14.0]);
    let p = &gaps.places;
    let Some(mut i) = gaps.player.keys().copied().find(|&i| {
        let a = p.at(i);
        (a[0] - to[0]).abs() < 0.1 && (a[2] - to[2]).abs() < 0.1 && (a[1] - to[1]).abs() < 0.02
    }) else {
        panic!("the player can't get to {to:?}");
    };
    let mut way = vec![i];
    while gaps.player[&i] != i {
        i = gaps.player[&i];
        way.push(i);
    }
    for &i in way.iter().rev() {
        println!("{}{}{}", show(p.at(i)), if p.spots[i].ground { " ground" } else { "" }, if gaps.zombie_on(i) { "" } else { "   <- no zombie" });
    }
    // how far a zombie at FROM=x,height,z has to walk to get there
    if let Ok(f) = std::env::var("FROM") {
        let f: Vec<f32> = f.split(',').map(|s| s.parse().unwrap()).collect();
        let m = map::make(map, seed);
        let world = Grid::new(m.walls, 4.0).with_ground(m.ground);
        let mut nav = Nav::new(&world, p.half);
        nav.set_goal([to[0], to[1], to[2]]);
        println!("a zombie at {f:?} walks {:?} m", nav.route_len([f[0], f[1], f[2]]));
    }
    // and how the nav sees the step where the zombies are left behind
    if let Some(k) = way.iter().rev().position(|&i| !gaps.zombie_on(i)).filter(|&k| k > 0) {
        let world = Grid::new(map::make(map, seed).walls, 4.0).with_ground(map::make(map, seed).ground);
        let nav = Nav::new(&world, p.half);
        for &i in &way.iter().rev().collect::<Vec<_>>()[k - 1..=k] {
            println!("nav at {}: {}", show(p.at(*i)), nav.describe(p.at(*i)));
        }
    }
}
