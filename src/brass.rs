//! Spent shells and cases thrown clear of a break action or a cylinder as it is reloaded: they tumble down past the
//! gun, bounce on whatever is underfoot and lie there a while. Only the one reloading sees them, so they live in the
//! renderer, not the game.

use crate::grid::Grid;
use crate::sim::V3;

/// Faster than real, as everything in the game falls.
const GRAVITY: f32 = 14.0;
/// How long one lies on the floor before it goes.
pub const LIFE: f32 = 8.0;
/// How many there can be at once: the oldest goes first.
pub const MAX: usize = 36;

pub struct Brass {
    /// which gun it came out of, and whether it is only the case (fired) or a whole round
    pub gun: usize,
    pub spent: bool,
    pub pos: V3,
    pub vel: V3,
    /// its own x, y and z axes in the world: z runs along it, base to mouth
    pub rot: [V3; 3],
    /// how it turns, an axis scaled by radians a second
    pub spin: V3,
    /// how thick it is, to lie on the floor rather than in it
    pub radius: f32,
    pub age: f32,
    pub resting: bool,
}

fn add(a: V3, b: V3, k: f32) -> V3 {
    [a[0] + b[0] * k, a[1] + b[1] * k, a[2] + b[2] * k]
}
fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn norm(a: V3) -> V3 {
    let l = dot(a, a).sqrt().max(1e-9);
    [a[0] / l, a[1] / l, a[2] / l]
}

impl Brass {
    pub fn step(&mut self, dt: f32, world: &Grid) {
        self.age += dt;
        if self.resting {
            return;
        }
        self.vel[1] -= GRAVITY * dt;
        let next = add(self.pos, self.vel, dt);
        // whatever is under it, floor, roof or hillside, from a little above so a fast fall doesn't go through
        let up = 0.3;
        let floor = self.pos[1] + up
            - world.raycast([next[0], self.pos[1] + up, next[2]], [0.0, -1.0, 0.0], 50.0);
        // into a wall: it bounces back off it
        let ahead = [self.vel[0] * dt, 0.0, self.vel[2] * dt];
        let side = dot(ahead, ahead).sqrt();
        if side > 1e-6
            && world.raycast_walls(self.pos, norm(ahead), side + self.radius) < side + self.radius
        {
            self.vel[0] *= -0.3;
            self.vel[2] *= -0.3;
        } else {
            self.pos[0] = next[0];
            self.pos[2] = next[2];
        }
        self.pos[1] = next[1];
        if self.pos[1] < floor + self.radius {
            self.pos[1] = floor + self.radius;
            if self.vel[1] > -1.2 {
                self.settle();
                return;
            }
            // brass rings off the floor: a short hop, most of its speed gone, the spin knocked about
            self.vel = [self.vel[0] * 0.5, -self.vel[1] * 0.3, self.vel[2] * 0.5];
            self.spin = [self.spin[2] * 0.6, self.spin[0] * 0.4, -self.spin[1] * 0.6];
        }
        let w = self.spin;
        for c in &mut self.rot {
            *c = add(*c, cross(w, *c), dt);
        }
        // keep the axes square, or they drift with each step
        let z = norm(self.rot[2]);
        let x = norm(cross(self.rot[1], z));
        self.rot = [x, cross(z, x), z];
    }

    /// Down on its side, rolled along the way it was pointing.
    fn settle(&mut self) {
        let z = self.rot[2];
        let z = if z[0].hypot(z[2]) > 1e-3 {
            norm([z[0], 0.0, z[2]])
        } else {
            [1.0, 0.0, 0.0]
        };
        let y = [0.0, 1.0, 0.0];
        self.rot = [cross(y, z), y, z];
        self.vel = [0.0; 3];
        self.spin = [0.0; 3];
        self.resting = true;
    }

    /// How big it is drawn: it shrinks into the ground at the end of its life.
    pub fn size(&self) -> f32 {
        ((LIFE - self.age) / 0.5).clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::Aabb;

    /// Thrown back and up off a gun held over a crate, a case lands, bounces, and comes to lie on its side on top of
    /// the crate, not in it nor through it.
    #[test]
    fn a_case_comes_to_lie_on_its_side_on_what_it_falls_on() {
        let world = Grid::new(
            vec![Aabb {
                min: [-2.0, 0.0, -2.0],
                max: [2.0, 0.8, 2.0],
            }],
            4.0,
        );
        let mut b = Brass {
            gun: 0,
            spent: true,
            pos: [0.0, 1.5, 0.0],
            vel: [0.4, 1.5, 2.0],
            rot: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            spin: [9.0, 2.0, 0.0],
            radius: 0.01,
            age: 0.0,
            resting: false,
        };
        for _ in 0..240 {
            b.step(1.0 / 60.0, &world);
            assert!(
                b.pos[1] >= 0.8 + 0.01 - 1e-4,
                "sank into the crate: {:?}",
                b.pos
            );
        }
        assert!(b.resting, "still moving at {:?}", b.vel);
        assert!(
            b.rot[2][1].abs() < 1e-3 && b.rot[1][1] > 0.999,
            "not lying on its side: {:?}",
            b.rot
        );
        assert!((b.pos[1] - 0.81).abs() < 1e-3);
    }
}
