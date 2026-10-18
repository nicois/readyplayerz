//! Which ground cells are open, and the cheapest way from any of them to the player, so bots can route around walls.

use crate::grid::Grid;
use crate::sim::{overlaps, Aabb, V3};
use std::cmp::Reverse;
use std::collections::BinaryHeap;

/// Side of one nav cell. A body is 0.8 wide, so standing in the middle of an open cell it clears every blocked neighbour.
const CELL: f32 = 1.0;
const UNREACHED: u32 = u32::MAX;
/// How far from the player to look for an open cell when they are standing on top of something.
const GOAL_SEARCH: i32 = 6;

pub struct Nav {
    half: f32,
    n: usize,
    open: Vec<bool>,
    /// path cost to the goal, 10 per straight step and 14 per diagonal
    cost: Vec<u32>,
    goal: Option<usize>,
}

impl Nav {
    /// A cell is blocked if any wall stands on any part of it, however low.
    pub fn new(world: &Grid, half: f32) -> Nav {
        let n = (2.0 * half / CELL).ceil() as usize;
        let mut open = vec![true; n * n];
        for z in 0..n {
            for x in 0..n {
                let (x0, z0) = (-half + x as f32 * CELL, -half + z as f32 * CELL);
                let cell = Aabb { min: [x0, 0.0, z0], max: [x0 + CELL, 0.1, z0 + CELL] };
                open[z * n + x] = !world.find(&cell, |w| overlaps(&cell, w));
            }
        }
        Nav { half, n, open, cost: vec![UNREACHED; n * n], goal: None }
    }

    fn cell_of(&self, x: f32, z: f32) -> Option<(i32, i32)> {
        let (cx, cz) = (((x + self.half) / CELL).floor() as i32, ((z + self.half) / CELL).floor() as i32);
        let n = self.n as i32;
        (cx >= 0 && cz >= 0 && cx < n && cz < n).then_some((cx, cz))
    }

    fn at(&self, c: (i32, i32)) -> usize {
        c.1 as usize * self.n + c.0 as usize
    }

    fn is_open(&self, c: (i32, i32)) -> bool {
        let n = self.n as i32;
        c.0 >= 0 && c.1 >= 0 && c.0 < n && c.1 < n && self.open[self.at(c)]
    }

    fn center(&self, c: (i32, i32)) -> [f32; 2] {
        [-self.half + (c.0 as f32 + 0.5) * CELL, -self.half + (c.1 as f32 + 0.5) * CELL]
    }

    /// Points the field at `p`. Does nothing if they are still in the same cell as last time.
    pub fn set_goal(&mut self, p: V3) {
        let Some(here) = self.cell_of(p[0], p[2]) else {
            return;
        };
        // on top of a wall: aim for the nearest open cell around it
        let mut goal = self.is_open(here).then_some(here);
        let mut best = f32::INFINITY;
        if goal.is_none() {
            for dz in -GOAL_SEARCH..=GOAL_SEARCH {
                for dx in -GOAL_SEARCH..=GOAL_SEARCH {
                    let c = (here.0 + dx, here.1 + dz);
                    let d = (dx * dx + dz * dz) as f32;
                    if d < best && self.is_open(c) {
                        best = d;
                        goal = Some(c);
                    }
                }
            }
        }
        let goal = goal.map(|c| self.at(c));
        if goal == self.goal {
            return;
        }
        self.goal = goal;
        self.cost.fill(UNREACHED);
        let Some(g) = goal else {
            return;
        };
        let mut heap = BinaryHeap::new();
        self.cost[g] = 0;
        heap.push(Reverse((0u32, g)));
        while let Some(Reverse((d, i))) = heap.pop() {
            if d > self.cost[i] {
                continue;
            }
            let c = ((i % self.n) as i32, (i / self.n) as i32);
            for (dx, dz) in NEIGHBOURS {
                let to = (c.0 + dx, c.1 + dz);
                if !self.is_open(to) || (dx != 0 && dz != 0 && !(self.is_open((c.0 + dx, c.1)) && self.is_open((c.0, c.1 + dz)))) {
                    continue;
                }
                let nd = d + if dx != 0 && dz != 0 { 14 } else { 10 };
                let j = self.at(to);
                if nd < self.cost[j] {
                    self.cost[j] = nd;
                    heap.push(Reverse((nd, j)));
                }
            }
        }
    }

    /// Centre of the neighbouring cell that is closest to the goal, or None if `p` is not on the field
    /// (inside a wall, unreachable) or is already in the goal cell.
    pub fn next(&self, p: V3) -> Option<[f32; 2]> {
        let c = self.cell_of(p[0], p[2])?;
        let here = self.cost[self.at(c)];
        if !self.is_open(c) || here == UNREACHED || here == 0 {
            return None;
        }
        let mut best = (here, None);
        for (dx, dz) in NEIGHBOURS {
            let to = (c.0 + dx, c.1 + dz);
            if !self.is_open(to) || (dx != 0 && dz != 0 && !(self.is_open((c.0 + dx, c.1)) && self.is_open((c.0, c.1 + dz)))) {
                continue;
            }
            let cost = self.cost[self.at(to)];
            if cost < best.0 {
                best = (cost, Some(to));
            }
        }
        best.1.map(|to| self.center(to))
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
            p = [to[0], 0.0, to[1]];
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
