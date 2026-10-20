//! Grenades: carrying them, aiming and throwing them, their flight, and what they do when they go off. A blast
//! hurts and throws whatever it can see, and walls and hills shelter what is behind them; it breaks nothing.

use super::*;

/// Grenades each run starts with, and the most that can be carried: the rest come from the ammo boxes.
pub const GRENADES: u32 = 2;
pub const MAX_GRENADES: u32 = 4;
const FUSE: f32 = 2.5;
/// steps of flight between the points of the aiming arc
const ARC_EVERY: usize = 2;
const THROW_SPEED: f32 = 18.0;
/// a grenade falls about as it would for real, slower than bodies, which fall fast so jumps are snappy: it flies
/// flatter and further, out to about THROW_SPEED² / GRENADE_GRAVITY
const GRENADE_GRAVITY: f32 = 11.0;
/// how long the hand is busy throwing, with the gun lowered
pub const THROW_TIME: f32 = 0.6;
/// a blast does `BLAST_DMG` out to `BLAST_CORE` and falls off to nothing at `BLAST_RADIUS`
const BLAST_RADIUS: f32 = 7.0;
const BLAST_CORE: f32 = 1.5;
const BLAST_DMG: f32 = 400.0;
/// the thrower's own grenade hurts them this much less, so it takes a near miss rather than one in the same room
const SELF_BLAST: f32 = 0.35;
/// a zombie killed this close to a blast, or by this much more than it had left, is blown apart
const GIB_NEAR: f32 = 3.5;
const GIB_OVER: f32 = 150.0;
/// how long the fireball and its smoke last
pub const BLAST_T: f32 = 1.6;

/// A thrown grenade, bouncing about until its fuse runs out.
#[derive(Clone)]
pub struct Grenade {
    pub id: u32,
    /// who threw it, for the kills
    pub by: u8,
    pub pos: V3,
    pub vel: V3,
    pub rot: V3,
    spin: V3,
    fuse: f32,
}

impl Grenade {
    /// One seen over the network, only to be drawn.
    pub(crate) fn seen(id: u32, pos: V3, rot: V3) -> Grenade {
        Grenade { id, by: 0, pos, vel: [0.0; 3], rot, spin: [0.0; 3], fuse: FUSE }
    }
}

/// Where a grenade went off and how long ago, for the fireball to be drawn.
pub struct Blast {
    pub id: u32,
    pub pos: V3,
    pub t: f32,
}

/// One step of a grenade's flight: it falls, bounces off walls, the ground and living zombies, and rolls to a stop.
/// Returns how hard it hit something, and the axis (0 x, 1 up, 2 z) it hit along, if it did.
fn fly(n: &mut Grenade, dt: f32, world: &Grid, bots: &[Bot]) -> (f32, Option<usize>) {
    const R: f32 = 0.07;
    n.vel[1] -= GRENADE_GRAVITY * dt;
    let mut grounded = false;
    let mut knock = 0.0f32;
    let mut hit = None;
    for axis in 0..3 {
        let old = n.pos[axis];
        n.pos[axis] += n.vel[axis] * dt;
        let ground = world.ground_at(n.pos[0], n.pos[2]);
        let floor = axis == 1 && n.pos[1] < ground + R;
        let probe = Aabb { min: [n.pos[0] - R, n.pos[1] - R, n.pos[2] - R], max: [n.pos[0] + R, n.pos[1] + R, n.pos[2] + R] };
        let body = axis != 1 && bots.iter().any(|b| b.alive && overlaps(&probe, &b.body_box()));
        if floor || body || world.find(&probe, |w| overlaps(&probe, w)) {
            if floor {
                n.pos[1] = (ground + R).max(old);
            } else {
                n.pos[axis] = old;
            }
            if axis == 1 && n.vel[1] < 0.0 {
                grounded = true;
            }
            if n.vel[axis].abs() >= knock {
                knock = n.vel[axis].abs();
                hit = Some(axis);
            }
            n.vel[axis] *= -0.35;
        }
    }
    if grounded {
        if n.vel[1].abs() < 1.2 {
            n.vel[1] = 0.0;
        }
        let drag = (1.0 - 4.0 * dt).max(0.0);
        for i in [0, 2] {
            n.vel[i] *= drag;
        }
        for s in &mut n.spin {
            *s *= drag;
        }
    }
    for i in 0..3 {
        n.rot[i] += n.spin[i] * dt;
    }
    (knock, hit)
}

/// How much of a blast at `at` gets to a body standing at `pos` that is `h` tall: none behind a wall or a hill,
/// and from all of it at the core to none at `BLAST_RADIUS`. Also the way it is pushed, flat, and how far off it is.
fn blast_on(at: V3, pos: V3, h: f32, world: &Grid) -> Option<(f32, [f32; 2], f32)> {
    let (dx, dz) = (pos[0] - at[0], pos[2] - at[2]);
    // to the nearest part of the body, from its feet to its head
    let y = (at[1]).clamp(pos[1], pos[1] + h);
    let d = len([dx, y - at[1], dz]);
    if d >= BLAST_RADIUS {
        return None;
    }
    // seen from just above where it went off, at the waist, the chest or the head
    let o = [at[0], at[1] + 0.25, at[2]];
    let seen = [0.3, 0.55, 0.9].iter().any(|&k| {
        let to = [pos[0] - o[0], pos[1] + h * k - o[1], pos[2] - o[2]];
        let l = len(to);
        l < 0.05 || world.raycast(o, unit(to), l) >= l - 0.05
    });
    if !seen {
        return None;
    }
    let share = ((BLAST_RADIUS - d) / (BLAST_RADIUS - BLAST_CORE)).clamp(0.0, 1.0);
    let flat = dx.hypot(dz).max(1e-3);
    Some((share, [dx / flat, dz / flat], d))
}

impl Game {
    /// The grenade hand, each tick the player is alive: holding the grenade key takes one out and shows where it
    /// would go, letting go throws it. Climbing takes both hands, so it puts the grenade away again. Returns
    /// whether the hand is on a grenade, so the gun can't be reloaded or fired.
    pub(super) fn tick_throw(&mut self, pi: usize, dt: f32, held: bool) -> bool {
        let p = &mut self.players[pi];
        let ready = p.throw_t <= 0.0 && p.grenades > 0 && p.climb.is_none();
        let throw = p.aiming && !held && ready;
        p.aiming = held && ready;
        if p.aiming {
            p.equip_t = p.equip_t.max(EQUIP_TIME * 0.7);
            p.reload_t = 0.0;
            p.aim_t += dt;
        } else {
            p.aim_t = 0.0;
        }
        // only the one throwing sees the arc, so the server needn't work it out
        let (arc, axis) = if p.aiming && self.role != Role::Server { self.grenade_arc(pi) } else { (Vec::new(), 1) };
        let p = &mut self.players[pi];
        (p.arc, p.arc_axis) = (arc, axis);
        if throw {
            self.throw_grenade(pi);
        }
        throw || self.players[pi].aiming
    }

    /// A grenade leaving the hand right now: from the right hand, or from the eye if that would start it inside a
    /// wall, the way the player is looking and a little up. How they move doesn't carry it on, so where it lands is
    /// down to where they look alone, not to running or strafing. Thrown from the side, it is aimed in across the
    /// line of sight so it comes down on it: what is in the middle of the view is what it hits, and the arc is seen
    /// curving in rather than end on.
    fn grenade_in_hand(&self, pi: usize) -> Grenade {
        const SIDE: f32 = 0.22;
        let p = &self.players[pi];
        let eye = p.eye();
        let f = p.forward();
        let dir = unit([f[0], f[1] + 0.15, f[2]]);
        let (sy, cy) = p.yaw.sin_cos();
        let hand = [eye[0] + cy * SIDE + f[0] * 0.3, eye[1] - 0.15, eye[2] + sy * SIDE + f[2] * 0.3];
        let off = [hand[0] - eye[0], hand[1] - eye[1], hand[2] - eye[2]];
        let free = self.world.raycast(eye, unit(off), len(off) + 0.1) > len(off) + 0.05;
        let vel = [dir[0] * THROW_SPEED, dir[1] * THROW_SPEED, dir[2] * THROW_SPEED];
        let mut n = Grenade { id: 0, by: p.id, pos: if free { hand } else { eye }, vel, rot: [0.0; 3], spin: [0.0; 3], fuse: FUSE };
        if free {
            let (_, _, t) = self.flight(&n);
            let k = SIDE / t.max(0.1);
            n.vel[0] -= cy * k;
            n.vel[2] -= sy * k;
        }
        n
    }

    /// Where a grenade thrown now would fly, up to the first thing it hits (see `flight`), and the axis it hits along.
    fn grenade_arc(&self, pi: usize) -> (Vec<V3>, usize) {
        let (arc, axis, _) = self.flight(&self.grenade_in_hand(pi));
        (arc, axis)
    }

    /// Where grenade `n` flies up to the first thing it hits (or to where it is when it goes off, if it hits
    /// nothing), with zombies where they are now: one point per `ARC_EVERY` steps and the end, the axis it hits
    /// along, and how long it takes. It is flown by the same steps as a real one, so it hits where this says.
    fn flight(&self, n: &Grenade) -> (Vec<V3>, usize, f32) {
        let mut n = n.clone();
        let mut arc = vec![n.pos];
        for k in 1.. {
            n.fuse -= STEP;
            if n.fuse <= 0.0 {
                break;
            }
            let (from, vel) = (n.pos, n.vel);
            if let (_, Some(axis)) = fly(&mut n, STEP, &self.world, &self.bots) {
                // a step is a good part of a metre, and the hit puts it back to where it was before it: the arc
                // ends on the surface itself, where the step would have met it (if it was a zombie, near enough)
                let d = unit(vel);
                let reach = len(vel) * STEP + 0.3;
                let mut t = self.world.raycast(from, d, reach);
                // flat ground is no wall and no hill: it is height 0
                if self.world.ground.is_none() && d[1] < 0.0 {
                    t = t.min(-from[1] / d[1]);
                }
                arc.push(if t < reach { along(from, d, t) } else { n.pos });
                return (arc, axis, k as f32 * STEP);
            }
            if k % ARC_EVERY == 0 {
                arc.push(n.pos);
            }
        }
        arc.push(n.pos);
        (arc, 1, FUSE)
    }

    /// Player `pi` letting go of the grenade. On a client it flies only once the server says it does.
    fn throw_grenade(&mut self, pi: usize) {
        if self.role != Role::Mirror {
            let mut n = self.grenade_in_hand(pi);
            n.id = self.new_id();
            n.spin = [self.rng.range(-12.0, 12.0), self.rng.range(-6.0, 6.0), self.rng.range(-12.0, 12.0)];
            self.grenades.push(n);
        }
        let p = &mut self.players[pi];
        p.grenades -= 1;
        p.throw_t = THROW_TIME;
        p.equip_t = p.equip_t.max(THROW_TIME);
        self.events.push([ev::THROW, 0.0, 0.0]);
    }

    /// Grenades fly, bounce off walls, the ground and zombies, roll to a stop and go off, and the fireballs of the
    /// ones that went off burn out.
    pub(super) fn tick_grenades(&mut self, dt: f32, world: &Grid) {
        for b in &mut self.blasts {
            b.t += dt;
        }
        self.blasts.retain(|b| b.t < BLAST_T);
        // on a client, the server says where they are
        if self.role == Role::Mirror {
            return;
        }
        let me = self.players.get(self.local).map_or([0.0; 3], |p| p.body.pos);
        let mut gone = Vec::new();
        for n in &mut self.grenades {
            n.fuse -= dt;
            if n.fuse <= 0.0 {
                gone.push((n.pos, n.by));
                continue;
            }
            let (knock, _) = fly(n, dt, world, &self.bots);
            if knock > 2.0 {
                let d = (n.pos[0] - me[0]).hypot(n.pos[2] - me[2]);
                self.events.push([ev::BOUNCE, knock, d]);
            }
        }
        self.grenades.retain(|n| n.fuse > 0.0);
        for (at, by) in gone {
            self.explode(at, by, world);
        }
    }

    /// How a blast at `at` looks, sounds and feels to the player this machine looks through: a fireball, sparks
    /// and a shake. A client sees it this way when the server says one went off.
    pub(crate) fn blast_fx(&mut self, id: u32, at: V3) {
        self.blasts.push(Blast { id, pos: at, t: 0.0 });
        if self.role == Role::Server {
            return;
        }
        let p = &mut self.players[self.local];
        let me = p.body.pos;
        let (dx, dz) = (at[0] - me[0], at[2] - me[2]);
        let d = dx.hypot(dz).max(0.001);
        let (sy, cy) = p.yaw.sin_cos();
        let pan = (dx * cy + dz * sy) / d;
        p.shake = p.shake.max(((25.0 - d) / 20.0).clamp(0.0, 1.0));
        self.events.push([ev::BLAST, len([dx, at[1] - me[1], dz]), pan]);
        // sparks flying out of it
        for _ in 0..36 {
            let r = &mut self.rng;
            let (a, up) = (r.range(0.0, std::f32::consts::TAU), r.range(0.1, 1.0));
            let speed = r.range(6.0, 16.0);
            let flat = (1.0 - up * up).sqrt();
            let vel = [a.cos() * flat * speed, up * speed, a.sin() * flat * speed];
            let s = r.range(0.02, 0.05);
            let color = if r.f() < 0.5 { [1.0, 0.75, 0.3] } else { [1.0, 0.5, 0.15] };
            let life = r.range(0.3, 0.8);
            self.debris.push(Debris { pos: [at[0], at[1] + 0.2, at[2]], vel, rot: [0.0; 3], spin: [0.0; 3], half: [s; 3], color, life, glow: 1.0 });
        }
        self.trim_debris();
    }

    /// A grenade player `by` threw going off: zombies near it are hurt, thrown and blown apart, players too if they
    /// are close.
    pub(super) fn explode(&mut self, at: V3, by: u8, world: &Grid) {
        let id = self.new_id();
        self.blast_fx(id, at);
        let mut killed = 0;
        for i in 0..self.bots.len() {
            let b = &self.bots[i];
            if !b.alive && !b.corpse {
                continue;
            }
            let Some((share, away, d)) = blast_on(at, b.body.pos, 1.7 * b.scale, world) else { continue };
            let push = share * if b.kind == 2 { 0.6 } else { 1.0 };
            let b = &mut self.bots[i];
            b.climb = None;
            b.body.on_ground = false;
            b.body.vel[0] += away[0] * 14.0 * push;
            b.body.vel[1] = b.body.vel[1].max(3.0 + 6.0 * push);
            b.body.vel[2] += away[1] * 14.0 * push;
            if !b.alive {
                continue;
            }
            let dmg = BLAST_DMG * share;
            b.hp -= dmg;
            b.hit_t = 1.0;
            if b.hp > 0.0 {
                continue;
            }
            b.alive = false;
            b.dead_t = 0.0;
            killed += 1;
            // thrown down facing away from it, or blown to pieces
            b.face = (-away[0]).atan2(away[1]);
            let gib = d < GIB_NEAR || -b.hp > GIB_OVER;
            b.corpse = !gib;
            let (pos, scale) = (b.body.pos, b.scale);
            if gib {
                self.gibs([pos[0], pos[1] + 1.0 * scale, pos[2]], away, scale);
            } else {
                self.blood([pos[0], pos[1] + 1.2 * scale, pos[2]], [away[0], 0.0, away[1]], 16);
            }
        }
        if let Some(p) = self.players.iter_mut().find(|p| p.id == by).filter(|_| killed > 0) {
            p.kills += killed;
            p.kill_t = 1.0;
            p.hit_marker = 1.0;
            self.events.push([ev::KILL, 0.0, 0.0]);
        }

        // anyone close by, the one who threw it or not, is hurt by it as by their own
        for i in 0..self.players.len() {
            let p = &mut self.players[i];
            if !p.alive() {
                continue;
            }
            if let Some((share, away, _)) = blast_on(at, p.body.pos, H, world) {
                p.climb = None;
                p.body.on_ground = false;
                p.body.vel[0] += away[0] * 8.0 * share;
                p.body.vel[1] = p.body.vel[1].max(4.0 * share);
                p.body.vel[2] += away[1] * 8.0 * share;
                self.hurt_player(i, BLAST_DMG * SELF_BLAST * share);
            }
        }
    }

    /// A zombie blown apart: lumps of it, in its skin and clothes, and a lot of blood.
    pub(crate) fn gibs(&mut self, at: V3, away: [f32; 2], scale: f32) {
        if self.role == Role::Server {
            return;
        }
        const FLESH: [[f32; 3]; 4] = [[0.42, 0.47, 0.36], [0.33, 0.30, 0.27], [0.25, 0.27, 0.32], [0.45, 0.05, 0.05]];
        for k in 0..18 {
            let r = &mut self.rng;
            let speed = r.range(3.0, 10.0);
            let vel = [away[0] * speed + r.range(-4.0, 4.0), r.range(3.0, 9.0), away[1] * speed + r.range(-4.0, 4.0)];
            let half = [r.range(0.05, 0.13) * scale, r.range(0.05, 0.11) * scale, r.range(0.05, 0.13) * scale];
            let spin = [r.range(-14.0, 14.0), r.range(-14.0, 14.0), r.range(-14.0, 14.0)];
            let pos = [at[0] + r.range(-0.3, 0.3), at[1] + r.range(-0.6, 0.5), at[2] + r.range(-0.3, 0.3)];
            let life = r.range(3.0, 5.0);
            self.debris.push(Debris { pos, vel, rot: [0.0; 3], spin, half, color: FLESH[k % FLESH.len()], life, glow: 0.0 });
        }
        self.blood(at, [away[0], 0.0, away[1]], 40);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::tests::{arena, idle};

    fn press_throw(g: &mut Game, secs: f32) {
        g.players[0].input.throw = true;
        idle(g, secs);
        g.players[0].input.throw = false;
        idle(g, STEP);
    }

    #[test]
    fn a_run_starts_with_two_grenades_and_throwing_runs_them_out() {
        let mut g = arena();
        assert_eq!(g.players[0].grenades, GRENADES);
        // held down, it is aimed; let go, it is thrown
        g.players[0].input.throw = true;
        idle(&mut g, 1.5);
        assert!(g.players[0].aiming && g.players[0].arc.len() > 10 && g.grenades.is_empty());
        assert_eq!(g.players[0].grenades, GRENADES);
        g.players[0].input.throw = false;
        idle(&mut g, STEP);
        assert_eq!((g.players[0].grenades, g.grenades.len()), (1, 1));
        assert!(!g.players[0].aiming && g.players[0].arc.is_empty());
        idle(&mut g, THROW_TIME);
        press_throw(&mut g, STEP);
        assert_eq!(g.players[0].grenades, 0);
        press_throw(&mut g, STEP);
        assert_eq!(g.players[0].grenades, 0, "none left to throw");
        g.respawn_player(0);
        assert_eq!(g.players[0].grenades, GRENADES, "a new run starts with two again");
    }

    #[test]
    fn a_thrown_grenade_flies_bounces_and_goes_off_once_its_fuse_is_out() {
        let mut g = arena();
        g.players[0].body.pos = [0.0, 0.0, 0.0];
        g.players[0].yaw = 0.0;
        g.players[0].pitch = 0.1;
        press_throw(&mut g, STEP);
        idle(&mut g, FUSE - 0.2);
        let n = &g.grenades[0];
        assert!(n.pos[2] < -10.0 && n.pos[1] < 0.2, "it flies off ahead and comes down: {:?}", n.pos);
        assert!(g.blasts.is_empty());
        idle(&mut g, 0.3);
        assert!(g.grenades.is_empty() && g.blasts.len() == 1);
        assert!(g.events.iter().any(|e| e[0] == ev::BLAST) && g.events.iter().any(|e| e[0] == ev::BOUNCE));
        assert_eq!(g.players[0].hp, 100.0, "too far off to be hurt");
    }

    #[test]
    fn the_aiming_arc_ends_where_the_grenade_first_hits_something() {
        let mut walls = map::perimeter();
        walls.push(Aabb { min: [-3.0, 0.0, -9.0], max: [3.0, 6.0, -8.5] });
        let mut g = Game::with_walls(1, walls);
        g.players[0].body.pos = [0.0, 0.0, 0.0];
        g.players[0].yaw = 0.0;
        g.players[0].pitch = 0.3;
        g.players[0].input.throw = true;
        idle(&mut g, 0.3);
        let (arc, axis) = (g.players[0].arc.clone(), g.players[0].arc_axis);
        let end = *arc.last().unwrap();
        assert_eq!(axis, 2, "it hits the wall face on");
        assert!((end[2] + 8.5).abs() < 0.02 && end[1] > 0.5, "it ends on the wall: {end:?}");
        assert!(end[0].abs() < 0.03, "thrown from the right hand, it still comes down in line with the view: {end:?}");
        assert!(arc[3][0] > 0.1, "it starts out off to the right");
        // the grenade really gets there, within a step and its size, on its way to bouncing off
        g.players[0].input.throw = false;
        let mut nearest = f32::INFINITY;
        for _ in 0..60 {
            g.tick(STEP);
            if let Some(n) = g.grenades.first() {
                nearest = nearest.min(len([n.pos[0] - end[0], n.pos[1] - end[1], n.pos[2] - end[2]]));
            }
        }
        assert!(nearest < 0.35, "it came within {nearest} of where the arc ends");
        // thrown down at the ground, it ends there, lying flat
        let mut g = arena();
        g.players[0].body.pos = [0.0, 0.0, 0.0];
        g.players[0].pitch = -0.5;
        g.players[0].input.throw = true;
        idle(&mut g, 0.1);
        let end = *g.players[0].arc.last().unwrap();
        assert!(g.players[0].arc_axis == 1 && end[1].abs() < 0.02, "{end:?}");
    }

    #[test]
    fn a_blast_kills_near_it_blows_apart_up_close_and_a_wall_shelters() {
        let mut walls = map::perimeter();
        walls.push(Aabb { min: [3.0, 0.0, -5.0], max: [3.4, 3.0, 5.0] });
        let mut g = Game::with_walls(1, walls);
        g.players[0].body.pos = [-40.0, 0.0, 40.0];
        let walker = |at: V3| Bot::zombie(at, 0.0);
        let mut brute = walker([0.0, 0.0, -4.0]);
        (brute.kind, brute.hp, brute.scale) = (2, 320.0, 1.25);
        g.bots = vec![walker([0.0, 0.0, 1.5]), walker([-4.5, 0.0, 0.0]), walker([5.0, 0.0, 0.0]), brute];
        let world = g.world.clone();
        g.explode([0.0, 0.07, 0.0], 0, &world);
        let b = &g.bots;
        assert!(!b[0].alive && !b[0].corpse, "blown apart up close");
        assert!(!b[1].alive && b[1].corpse, "killed further off, and left lying");
        assert!(b[2].alive && b[2].hp == 100.0, "behind the wall, untouched");
        assert!(b[3].alive && b[3].hp < 320.0 && b[3].body.vel[2] < -3.0, "a brute takes it, and is thrown back");
        assert!(b[1].body.vel[0] < -3.0 && b[1].body.vel[1] > 0.0, "the body is thrown away from it");
        assert_eq!(g.players[0].kills, 2);
        assert!(g.debris.len() > 50);
        idle(&mut g, 2.0);
        assert!(g.blasts.is_empty());
        assert!(g.bots[1].body.on_ground);
    }

    #[test]
    fn your_own_grenade_kills_you_underfoot_and_only_scratches_you_further_off() {
        let mut g = arena();
        g.players[0].body.pos = [0.0, 0.0, 0.0];
        let world = g.world.clone();
        g.explode([6.3, 0.07, 0.0], 0, &world);
        assert!(g.players[0].alive() && g.players[0].hp > 75.0 && g.players[0].hp < 100.0, "hp {}", g.players[0].hp);
        assert!(g.players[0].shake > 0.5);
        g.explode([0.3, 0.07, 0.0], 0, &world);
        assert!(!g.players[0].alive());
    }
}
