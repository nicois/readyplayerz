//! Game simulation. No browser dependencies, so a server can run the same code.

use crate::grid::Grid;
use crate::map::{self, Map};
use crate::nav::Nav;
use std::rc::Rc;

pub type V3 = [f32; 3];

pub const STEP: f32 = 1.0 / 60.0;
const HW: f32 = 0.4; // body half width
const H: f32 = 1.8; // body height
const EYE: f32 = 1.62;
const GRAVITY: f32 = 22.0;
const JUMP: f32 = 7.5;
const WALK: f32 = 6.0;
const SPRINT: f32 = 9.0;
const RANGE: f32 = 120.0;
const EPS: f32 = 0.001;
const SENS: f32 = 0.0022;
/// Half the side of the square arena.
pub const HALF: f32 = 100.0;
const BOT_COUNT: usize = 14;
/// Most zombies alive at once; the rest of a wave trickles in as they die.
const MAX_ZOMBIES: u32 = 24;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// armed soldiers that shoot and respawn
    Soldiers,
    /// waves of melee zombies, no respawns
    Zombies,
}
/// The tallest ledge a walking body climbs without jumping, so stairs and terraces work.
const STEP_UP: f32 = 0.4;
const BOT_SPEED: f32 = 3.2;
/// Bots further than this from the player just glide along their route instead of simulating a body.
const LOD_RADIUS: f32 = 40.0;
/// Side of one cell of the wall grid.
const CELL: f32 = 4.0;
pub const EQUIP_TIME: f32 = 0.35;
pub const WEAPON_COUNT: usize = 4;

/// Things that happened this tick that a client may want to play a sound for.
/// Each event is `[code, a, b]` in `Game::events`.
pub mod ev {
    /// a = weapon
    pub const SHOT: f32 = 1.0;
    /// a = distance, b = pan (-1 left, 1 right)
    pub const ENEMY_SHOT: f32 = 2.0;
    // hit, headshot and kill are ordered so the loudest one in a volley wins
    pub const HIT: f32 = 3.0;
    pub const HEAD: f32 = 4.0;
    pub const KILL: f32 = 5.0;
    /// a = damage
    pub const HURT: f32 = 6.0;
    /// a = weapon
    pub const RELOAD: f32 = 7.0;
    pub const EQUIP: f32 = 8.0;
    /// a = horizontal speed
    pub const STEP: f32 = 9.0;
    pub const JUMP: f32 = 10.0;
    /// a = impact speed
    pub const LAND: f32 = 11.0;
    pub const DEATH: f32 = 12.0;
    /// a = distance, b = pan
    pub const GROWL: f32 = 13.0;
    /// a = wave number
    pub const WAVE: f32 = 14.0;
}

/// Everything that differs between guns. Angles are radians, times are seconds.
pub struct Weapon {
    pub name: &'static str,
    pub mag: u32,
    pub delay: f32,
    pub reload: f32,
    pub pellets: u32,
    /// damage per pellet
    pub body_dmg: f32,
    pub head_dmg: f32,
    /// cone half-angle from the hip and when aiming down sights
    pub spread: f32,
    pub ads_spread: f32,
    pub kick: f32,
    pub auto: bool,
    /// field of view multiplier while aiming
    pub zoom: f32,
    /// sniper scope overlay instead of the gun model while aiming
    pub scope: bool,
    /// movement speed multiplier
    pub speed: f32,
}

pub static WEAPONS: [Weapon; WEAPON_COUNT] = [
    Weapon { name: "Carbine", mag: 30, delay: 0.11, reload: 1.4, pellets: 1, body_dmg: 34.0, head_dmg: 75.0, spread: 0.010, ads_spread: 0.002, kick: 0.006, auto: true, zoom: 0.8, scope: false, speed: 1.0 },
    Weapon { name: "SMG", mag: 35, delay: 0.065, reload: 1.2, pellets: 1, body_dmg: 20.0, head_dmg: 40.0, spread: 0.022, ads_spread: 0.007, kick: 0.004, auto: true, zoom: 0.85, scope: false, speed: 1.08 },
    Weapon { name: "Shotgun", mag: 6, delay: 0.85, reload: 2.4, pellets: 8, body_dmg: 14.0, head_dmg: 28.0, spread: 0.055, ads_spread: 0.038, kick: 0.045, auto: false, zoom: 0.9, scope: false, speed: 1.0 },
    Weapon { name: "Sniper", mag: 5, delay: 1.3, reload: 2.6, pellets: 1, body_dmg: 100.0, head_dmg: 150.0, spread: 0.04, ads_spread: 0.0, kick: 0.03, auto: false, zoom: 0.3, scope: true, speed: 0.92 },
];

#[derive(Clone, Copy, Debug)]
pub struct Aabb {
    pub min: V3,
    pub max: V3,
}

pub fn aabb_at(p: V3, hw: f32, h: f32) -> Aabb {
    Aabb { min: [p[0] - hw, p[1], p[2] - hw], max: [p[0] + hw, p[1] + h, p[2] + hw] }
}

pub(crate) fn overlaps(a: &Aabb, b: &Aabb) -> bool {
    (0..3).all(|i| a.min[i] < b.max[i] && a.max[i] > b.min[i])
}

/// Distance along the ray to the box, if it hits.
pub fn ray_aabb(o: V3, d: V3, b: &Aabb) -> Option<f32> {
    let mut tmin = 0.0f32;
    let mut tmax = f32::INFINITY;
    for i in 0..3 {
        if d[i].abs() < 1e-8 {
            if o[i] < b.min[i] || o[i] > b.max[i] {
                return None;
            }
        } else {
            let inv = 1.0 / d[i];
            let mut t1 = (b.min[i] - o[i]) * inv;
            let mut t2 = (b.max[i] - o[i]) * inv;
            if t1 > t2 {
                std::mem::swap(&mut t1, &mut t2);
            }
            tmin = tmin.max(t1);
            tmax = tmax.min(t2);
            if tmin > tmax {
                return None;
            }
        }
    }
    Some(tmin)
}

pub struct Rng(pub u32);

impl Rng {
    pub fn next(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }
    pub fn f(&mut self) -> f32 {
        (self.next() >> 8) as f32 / 16_777_216.0
    }
    pub fn range(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.f()
    }
}

#[derive(Default, Clone, Copy)]
pub struct Input {
    pub w: bool,
    pub a: bool,
    pub s: bool,
    pub d: bool,
    pub jump: bool,
    pub sprint: bool,
    pub fire: bool,
    pub ads: bool,
    pub reload: bool,
    pub look_dx: f32,
    pub look_dy: f32,
}

#[derive(Default, Clone, Copy)]
pub struct Body {
    pub pos: V3,
    pub vel: V3,
    pub on_ground: bool,
}

pub struct Player {
    pub body: Body,
    pub yaw: f32,
    pub pitch: f32,
    pub hp: f32,
    pub weapon: usize,
    /// rounds left per weapon
    pub ammo: [u32; WEAPON_COUNT],
    pub reload_t: f32,
    pub cooldown: f32,
    /// counts down after switching guns
    pub equip_t: f32,
    /// 0 hip, 1 fully aimed
    pub ads: f32,
    /// the trigger was released since the last shot, so a semi-auto gun may fire again
    pub armed: bool,
    pub kills: u32,
    pub deaths: u32,
    pub respawn_t: f32,
    pub hit_marker: f32,
    pub damage_flash: f32,
    /// 1 right after a kill, fading out
    pub kill_t: f32,
    /// distance walked since the last footstep
    stride: f32,
    /// seconds since last damage, for regeneration
    since_hurt: f32,
}

impl Player {
    pub fn eye(&self) -> V3 {
        [self.body.pos[0], self.body.pos[1] + EYE, self.body.pos[2]]
    }
    pub fn forward(&self) -> V3 {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        [sy * cp, sp, -cy * cp]
    }
    pub fn alive(&self) -> bool {
        self.hp > 0.0
    }
    pub fn gun(&self) -> &'static Weapon {
        &WEAPONS[self.weapon]
    }
}

fn full_ammo() -> [u32; WEAPON_COUNT] {
    std::array::from_fn(|i| WEAPONS[i].mag)
}

pub struct Bot {
    pub body: Body,
    pub hp: f32,
    pub alive: bool,
    pub respawn_t: f32,
    /// time to the next shot (soldiers) or the next swing (zombies)
    pub fire_t: f32,
    pub phase: f32,
    /// zombies: 0 walker, 1 runner, 2 brute
    pub kind: u8,
    pub speed: f32,
    /// visual and hit box size
    pub scale: f32,
    /// 1 as a zombie swings, fading to 0
    pub swing_t: f32,
    pub growl_t: f32,
    /// 1 when just hit, fading to 0: the zombie flinches and slows
    pub hit_t: f32,
    /// a zombie that was shot down without bursting stays as a corpse for `CORPSE_T`; this is how long it has lain there
    pub corpse: bool,
    pub dead_t: f32,
    /// the way it was facing when it fell
    pub face: f32,
}

/// How long a fallen zombie lies there before it fades away.
pub const CORPSE_T: f32 = 6.0;

impl Bot {
    pub(crate) fn soldier(pos: V3, phase: f32) -> Bot {
        Bot { body: Body { pos, ..Body::default() }, hp: 100.0, alive: true, respawn_t: 0.0, fire_t: 1.0, phase, kind: 0, speed: BOT_SPEED, scale: 1.0, swing_t: 0.0, growl_t: 0.0, hit_t: 0.0, corpse: false, dead_t: 0.0, face: 0.0 }
    }
    pub fn body_box(&self) -> Aabb {
        let (p, s) = (self.body.pos, self.scale);
        Aabb { min: [p[0] - 0.32 * s, p[1], p[2] - 0.32 * s], max: [p[0] + 0.32 * s, p[1] + 1.4 * s, p[2] + 0.32 * s] }
    }
    pub fn head_box(&self) -> Aabb {
        let (p, s) = (self.body.pos, self.scale);
        Aabb { min: [p[0] - 0.17 * s, p[1] + 1.4 * s, p[2] - 0.17 * s], max: [p[0] + 0.17 * s, p[1] + H * s, p[2] + 0.17 * s] }
    }
}

/// What a zombie's pieces look like, in the order of the chunks in `burst`: scalp, head, torso, jacket,
/// blood, arms, legs, boots, and a rib bone where the rifle would be.
const ZOMBIE_CHUNKS: [[f32; 3]; 12] = [
    [0.18, 0.14, 0.10],
    [0.66, 0.48, 0.40],
    [0.30, 0.20, 0.18],
    [0.28, 0.10, 0.09],
    [0.45, 0.06, 0.07],
    [0.60, 0.44, 0.36],
    [0.60, 0.44, 0.36],
    [0.20, 0.19, 0.17],
    [0.20, 0.19, 0.17],
    [0.12, 0.10, 0.09],
    [0.12, 0.10, 0.09],
    [0.75, 0.72, 0.62],
];

pub struct Tracer {
    pub a: V3,
    pub b: V3,
    pub life: f32,
    pub enemy: bool,
}

/// A chunk of a dead soldier, or a spark. Purely cosmetic.
pub struct Debris {
    pub pos: V3,
    pub vel: V3,
    pub rot: V3,
    pub spin: V3,
    pub half: V3,
    pub color: [f32; 3],
    pub life: f32,
    pub max_life: f32,
    /// sparks are drawn unlit and fall lighter
    pub glow: bool,
    /// drawn as a smooth lump rather than a box (flesh and blood)
    pub round: bool,
}

const MAX_DEBRIS: usize = 300;

pub struct Game {
    pub world: Rc<Grid>,
    /// one colour per wall of `world`
    pub colors: Vec<[f32; 3]>,
    /// one surface kind per wall, see `map::material`
    pub mats: Vec<u8>,
    pub decals: Vec<(Aabb, [f32; 3])>,
    spawns: Vec<V3>,
    nav: Nav,
    pub debris: Vec<Debris>,
    pub events: Vec<[f32; 3]>,
    pub player: Player,
    pub bots: Vec<Bot>,
    pub mode: Mode,
    /// zombies mode: the current wave (0 before the first), zombies still to arrive, and the timers
    pub wave: u32,
    to_spawn: u32,
    spawn_t: f32,
    wave_t: f32,
    pub tracers: Vec<Tracer>,
    pub input: Input,
    pub time: f32,
    rng: Rng,
}

/// Moves one axis and pushes the body out of any wall it ends up inside. Returns true on contact.
/// A body that hits the side of a ledge no higher than `lift` is put on top of it instead.
fn move_axis(pos: &mut V3, axis: usize, delta: f32, world: &Grid, lift: f32) -> bool {
    let before = *pos;
    pos[axis] += delta;
    let (plus, minus) = if axis == 1 { (H, 0.0) } else { (HW, HW) };
    let mut hit = false;
    // pushing out can move the body back by up to `delta`, so look that far too
    let mut reach = aabb_at(*pos, HW, H);
    reach.min[axis] -= delta.abs();
    reach.max[axis] += delta.abs();
    world.find(&reach, |w| {
        if overlaps(&aabb_at(*pos, HW, H), w) {
            hit = true;
            pos[axis] = if delta > 0.0 { w.min[axis] - plus - EPS } else { w.max[axis] + minus + EPS };
        }
        false
    });
    if hit && axis != 1 && lift > 0.0 {
        let mut up = before;
        up[axis] += delta;
        let mut top = before[1];
        let there = aabb_at(up, HW, H);
        world.find(&there, |w| {
            if overlaps(&there, w) {
                top = top.max(w.max[1]);
            }
            false
        });
        if top - before[1] <= lift {
            up[1] = top + EPS;
            let raised = aabb_at(up, HW, H);
            if !world.find(&raised, |w| overlaps(&raised, w)) {
                *pos = up;
                return false;
            }
        }
    }
    hit
}

/// Shared movement for the player and bots. Returns true if blocked horizontally.
fn step_body(b: &mut Body, target: [f32; 2], jump: bool, dt: f32, world: &Grid) -> bool {
    let lift = if b.on_ground { STEP_UP } else { 0.0 };
    let accel = if b.on_ground { 60.0 } else { 15.0 };
    let dx = target[0] - b.vel[0];
    let dz = target[1] - b.vel[2];
    let len = (dx * dx + dz * dz).sqrt();
    let step = accel * dt;
    if len <= step {
        b.vel[0] = target[0];
        b.vel[2] = target[1];
    } else {
        b.vel[0] += dx / len * step;
        b.vel[2] += dz / len * step;
    }
    if jump && b.on_ground {
        b.vel[1] = JUMP;
    }
    b.vel[1] -= GRAVITY * dt;
    b.on_ground = false;

    let hx = move_axis(&mut b.pos, 0, b.vel[0] * dt, world, lift);
    if hx {
        b.vel[0] = 0.0;
    }
    let hz = move_axis(&mut b.pos, 2, b.vel[2] * dt, world, lift);
    if hz {
        b.vel[2] = 0.0;
    }
    if move_axis(&mut b.pos, 1, b.vel[1] * dt, world, 0.0) {
        if b.vel[1] < 0.0 {
            b.on_ground = true;
        }
        b.vel[1] = 0.0;
    }
    if b.pos[1] <= 0.0 {
        b.pos[1] = 0.0;
        b.vel[1] = 0.0;
        b.on_ground = true;
    }
    hx || hz
}

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn len(a: V3) -> f32 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}
fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn unit(a: V3) -> V3 {
    let l = len(a);
    [a[0] / l, a[1] / l, a[2] / l]
}
fn along(o: V3, d: V3, t: f32) -> V3 {
    [o[0] + d[0] * t, o[1] + d[1] * t, o[2] + d[2] * t]
}

impl Game {
    pub fn new(seed: u32) -> Game {
        Game::with_map(seed, map::build(seed))
    }

    pub fn with_mode(seed: u32, mode: Mode) -> Game {
        let mut g = Game::new(seed);
        g.mode = mode;
        if mode == Mode::Zombies {
            g.bots.clear();
        }
        g
    }

    /// Zombies still to kill this wave: those alive plus those yet to arrive.
    pub fn zombies_left(&self) -> u32 {
        self.bots.iter().filter(|b| b.alive).count() as u32 + self.to_spawn
    }

    fn reset_waves(&mut self) {
        self.bots.clear();
        self.wave = 0;
        self.to_spawn = 0;
        self.wave_t = 3.0;
    }

    fn spawn_zombie(&mut self) {
        let from = [self.player.body.pos];
        let pos = self.pick_spawn(&from, 40.0);
        let (r, w) = (self.rng.f(), self.wave as f32);
        let p_brute = if self.wave >= 3 { (0.03 * w).min(0.15) } else { 0.0 };
        let p_run = (0.1 + 0.04 * w).min(0.4);
        let (kind, hp, speed, scale) = if r < p_brute {
            (2, 320.0, self.rng.range(2.0, 2.4), 1.25)
        } else if r < p_brute + p_run {
            (1, 60.0, self.rng.range(5.0, 5.8), 0.95)
        } else {
            (0, 100.0, self.rng.range(2.5, 3.4), 1.0)
        };
        let phase = self.rng.range(0.0, 6.28);
        let mut z = Bot::soldier(pos, phase);
        z.kind = kind;
        z.hp = hp;
        z.speed = speed;
        z.scale = scale;
        z.fire_t = 0.5;
        z.growl_t = self.rng.range(1.0, 6.0);
        match self.bots.iter().position(|b| !b.alive && (!b.corpse || b.dead_t > CORPSE_T)) {
            Some(i) => self.bots[i] = z,
            None => self.bots.push(z),
        }
    }

    /// A new wave starts a few seconds after the last zombie of the old one dies, and arrives a few at a time.
    fn tick_waves(&mut self, dt: f32) {
        let alive = self.bots.iter().filter(|b| b.alive).count() as u32;
        if self.to_spawn == 0 && alive == 0 {
            self.wave_t -= dt;
            if self.wave_t <= 0.0 {
                self.wave += 1;
                self.to_spawn = 6 + 3 * self.wave;
                self.spawn_t = 0.0;
                self.wave_t = 5.0;
                self.events.push([ev::WAVE, self.wave as f32, 0.0]);
            }
        }
        if self.to_spawn > 0 && alive < MAX_ZOMBIES {
            self.spawn_t -= dt;
            if self.spawn_t <= 0.0 {
                self.spawn_zombie();
                self.to_spawn -= 1;
                self.spawn_t = 0.4;
            }
        }
    }

    /// A game on bare walls with no spawn points, for tests.
    #[cfg(test)]
    fn with_walls(seed: u32, walls: Vec<Aabb>) -> Game {
        let colors = vec![[0.5; 3]; walls.len()];
        let mats = vec![0; walls.len()];
        Game::with_map(seed, Map { walls, colors, mats, decals: Vec::new(), spawns: Vec::new() })
    }

    pub fn with_map(seed: u32, map: Map) -> Game {
        let world = Rc::new(Grid::new(map.walls, CELL));
        let mut nav = Nav::new(&world, HALF);
        // only spawn where a body can actually walk to the middle of the map
        nav.set_goal([0.0, 0.0, 14.0]);
        let spawns = map.spawns.into_iter().filter(|p| nav.reachable(*p)).collect();
        let mut g = Game {
            nav,
            world,
            colors: map.colors,
            mats: map.mats,
            decals: map.decals,
            spawns,
            debris: Vec::new(),
            events: Vec::new(),
            player: Player {
                body: Body::default(),
                yaw: 0.0,
                pitch: 0.0,
                hp: 100.0,
                weapon: 0,
                ammo: full_ammo(),
                reload_t: 0.0,
                cooldown: 0.0,
                equip_t: 0.0,
                ads: 0.0,
                armed: true,
                kills: 0,
                deaths: 0,
                respawn_t: 0.0,
                hit_marker: 0.0,
                damage_flash: 0.0,
                kill_t: 0.0,
                stride: 0.0,
                since_hurt: 99.0,
            },
            bots: Vec::new(),
            mode: Mode::Soldiers,
            wave: 0,
            to_spawn: 0,
            spawn_t: 0.0,
            wave_t: 3.0,
            tracers: Vec::new(),
            input: Input::default(),
            time: 0.0,
            rng: Rng(seed.max(1)),
        };
        g.respawn_player();
        for _ in 0..BOT_COUNT {
            let pos = g.bot_spawn();
            let phase = g.rng.range(0.0, 6.28);
            g.bots.push(Bot::soldier(pos, phase));
        }
        g
    }

    fn respawn_player(&mut self) {
        // the best of a few spawn points: the one furthest from every living bot
        let others: Vec<V3> = self.bots.iter().filter(|b| b.alive).map(|b| b.body.pos).collect();
        let at = self.pick_spawn(&others, f32::INFINITY);
        let p = &mut self.player;
        p.body = Body { pos: at, ..Body::default() };
        // look at the middle of the map
        p.yaw = (-at[0]).atan2(at[2]);
        p.pitch = 0.0;
        p.hp = 100.0;
        p.ammo = full_ammo();
        p.reload_t = 0.0;
        p.equip_t = 0.0;
        p.ads = 0.0;
    }

    /// A spawn point at least `far` from everything in `from`, or failing that the best of a few.
    fn pick_spawn(&mut self, from: &[V3], far: f32) -> V3 {
        let reach = HALF - 2.0;
        let mut best = ([0.0, 0.0, -16.0], -1.0f32);
        for _ in 0..30 {
            let p = if self.spawns.is_empty() {
                // no spawn points (a test arena): anywhere open
                let (x, z) = (self.rng.range(-reach, reach), self.rng.range(-reach, reach));
                let probe = aabb_at([x, 0.0, z], HW, H);
                if self.world.find(&probe, |w| overlaps(&probe, w)) {
                    continue;
                }
                [x, 0.0, z]
            } else {
                self.spawns[(self.rng.next() as usize) % self.spawns.len()]
            };
            let d = from.iter().map(|o| ((o[0] - p[0]).powi(2) + (o[2] - p[2]).powi(2)).sqrt()).fold(f32::INFINITY, f32::min);
            if d >= far.min(30.0) && far.is_finite() {
                return p;
            }
            if d > best.1 {
                best = (p, d);
            }
        }
        best.0
    }

    /// Switches to weapon `i`; the gun has to be raised before it can fire.
    pub fn select(&mut self, i: usize) {
        let p = &mut self.player;
        if i >= WEAPON_COUNT || i == p.weapon {
            return;
        }
        p.weapon = i;
        p.reload_t = 0.0;
        p.equip_t = EQUIP_TIME;
        p.ads = 0.0;
        self.events.push([ev::EQUIP, 0.0, 0.0]);
    }

    fn bot_spawn(&mut self) -> V3 {
        // open ground, not on top of the player
        let from = [self.player.body.pos];
        self.pick_spawn(&from, 30.0)
    }

    fn wall_distance(&self, o: V3, d: V3) -> f32 {
        self.world.raycast(o, d, RANGE)
    }

    fn hurt_player(&mut self, dmg: f32) {
        if !self.player.alive() {
            return;
        }
        self.player.hp -= dmg;
        self.player.damage_flash = 1.0;
        self.player.since_hurt = 0.0;
        self.events.push([ev::HURT, dmg, 0.0]);
        if self.player.hp <= 0.0 {
            self.events.push([ev::DEATH, 0.0, 0.0]);
            self.player.deaths += 1;
            self.player.respawn_t = if self.mode == Mode::Zombies { 4.0 } else { 2.0 };
        }
    }

    /// Nearest thing a shot hits: distance, and the bot (with whether it was the head) if it got there first.
    fn trace_bots(&self, eye: V3, dir: V3) -> (f32, Option<(usize, bool)>) {
        let mut best = self.wall_distance(eye, dir);
        let mut target = None;
        for (i, b) in self.bots.iter().enumerate().filter(|(_, b)| b.alive) {
            if let Some(t) = ray_aabb(eye, dir, &b.head_box()) {
                if t < best {
                    best = t;
                    target = Some((i, true));
                }
            }
            if let Some(t) = ray_aabb(eye, dir, &b.body_box()) {
                if t < best {
                    best = t;
                    target = Some((i, false));
                }
            }
        }
        (best, target)
    }

    /// Blows a soldier apart into the pieces it was drawn from, plus sparks. `dir` is the way the shot was going.
    fn burst(&mut self, at: V3, dir: V3, head: bool, power: f32) {
        let z = self.mode == Mode::Zombies;
        // height of the centre, half extents, colour, extra upward kick
        const CHUNKS: [(f32, V3, [f32; 3], f32); 12] = [
            (1.70, [0.13, 0.08, 0.13], [0.11, 0.12, 0.14], 3.0), // helmet
            (1.55, [0.10, 0.10, 0.10], [0.80, 0.60, 0.46], 1.0), // head
            (1.25, [0.22, 0.12, 0.12], [0.24, 0.26, 0.28], 0.0), // torso
            (1.05, [0.24, 0.10, 0.13], [0.13, 0.14, 0.17], 0.0), // vest
            (1.20, [0.14, 0.10, 0.03], [0.78, 0.14, 0.17], 1.0), // chest plate
            (1.15, [0.05, 0.20, 0.05], [0.24, 0.26, 0.28], 0.5), // arms
            (1.15, [0.05, 0.20, 0.05], [0.24, 0.26, 0.28], 0.5),
            (0.60, [0.07, 0.28, 0.08], [0.15, 0.17, 0.18], 0.0), // legs
            (0.60, [0.07, 0.28, 0.08], [0.15, 0.17, 0.18], 0.0),
            (0.08, [0.09, 0.06, 0.13], [0.10, 0.10, 0.11], 0.0), // boots
            (0.08, [0.09, 0.06, 0.13], [0.10, 0.10, 0.11], 0.0),
            (1.15, [0.03, 0.04, 0.22], [0.10, 0.10, 0.11], 1.5), // rifle
        ];
        let push = [dir[0], 0.0, dir[2]];
        for (i, (h, half, color, lift)) in CHUNKS.iter().enumerate() {
            let k = (4.0 + 3.0 * power) * self.rng.range(0.6, 1.2);
            let pop = if head && i < 2 { 4.0 } else { 0.0 };
            let vel = [
                push[0] * k + self.rng.range(-2.5, 2.5) * power * 0.6,
                (2.5 + lift + pop + self.rng.range(0.0, 3.0)) * (0.8 + 0.3 * power),
                push[2] * k + self.rng.range(-2.5, 2.5) * power * 0.6,
            ];
            let life = self.rng.range(4.0, 5.5);
            let (r, s) = (&mut self.rng, 12.0);
            let rot = [r.range(0.0, 6.28), r.range(0.0, 6.28), r.range(0.0, 6.28)];
            let spin = [r.range(-s, s), r.range(-s, s), r.range(-s, s)];
            let pos = [at[0] + r.range(-0.1, 0.1), at[1] + h, at[2] + r.range(-0.1, 0.1)];
            let color = if z { ZOMBIE_CHUNKS[i] } else { *color };
            self.debris.push(Debris { pos, vel, rot, spin, half: *half, color, life, max_life: life, glow: false, round: z });
        }
        for _ in 0..18 {
            let r = &mut self.rng;
            let a = r.range(0.0, std::f32::consts::TAU);
            let sp = r.range(3.0, 9.0) * power;
            let vel = [a.cos() * sp + dir[0] * 3.0, r.range(1.0, 7.0), a.sin() * sp + dir[2] * 3.0];
            let color = match (z, r.f() < 0.5) {
                (false, true) => [1.0, 0.75, 0.25],
                (false, false) => [1.0, 0.45, 0.15],
                (true, true) => [0.75, 0.05, 0.05],
                (true, false) => [0.5, 0.03, 0.04],
            };
            let life = r.range(0.35, 0.85);
            let pos = [at[0], at[1] + 1.1, at[2]];
            self.debris.push(Debris { pos, vel, rot: [0.0; 3], spin: [0.0; 3], half: [0.035; 3], color, life, max_life: life, glow: true, round: false });
        }
        if self.debris.len() > MAX_DEBRIS {
            let extra = self.debris.len() - MAX_DEBRIS;
            self.debris.drain(..extra);
        }
    }

    /// A spray of blood droplets from a hit.
    fn blood(&mut self, at: V3, dir: V3, n: usize) {
        for _ in 0..n {
            let r = &mut self.rng;
            let vel = [dir[0] * r.range(1.0, 4.0) + r.range(-1.5, 1.5), r.range(0.5, 3.5), dir[2] * r.range(1.0, 4.0) + r.range(-1.5, 1.5)];
            let life = r.range(0.4, 0.9);
            let s = r.range(0.02, 0.04);
            let color = if r.f() < 0.5 { [0.45, 0.03, 0.04] } else { [0.30, 0.02, 0.03] };
            self.debris.push(Debris { pos: at, vel, rot: [0.0; 3], spin: [0.0; 3], half: [s; 3], color, life, max_life: life, glow: false, round: true });
        }
        if self.debris.len() > MAX_DEBRIS {
            let extra = self.debris.len() - MAX_DEBRIS;
            self.debris.drain(..extra);
        }
    }

    fn tick_debris(&mut self, dt: f32, world: &Grid) {
        for d in &mut self.debris {
            d.life -= dt;
            d.vel[1] -= GRAVITY * if d.glow { 0.5 } else { 1.0 } * dt;
            let r = d.half[0].max(d.half[1]).max(d.half[2]);
            let mut grounded = false;
            for axis in 0..3 {
                let old = d.pos[axis];
                d.pos[axis] += d.vel[axis] * dt;
                let floor = axis == 1 && d.pos[1] < r;
                let probe = Aabb { min: [d.pos[0] - r, d.pos[1] - r, d.pos[2] - r], max: [d.pos[0] + r, d.pos[1] + r, d.pos[2] + r] };
                if floor || world.find(&probe, |w| overlaps(&probe, w)) {
                    if floor {
                        d.pos[1] = r.max(old);
                    } else {
                        d.pos[axis] = old;
                    }
                    if axis == 1 && d.vel[1] < 0.0 {
                        grounded = true;
                    }
                    d.vel[axis] *= -0.35;
                }
            }
            if grounded {
                if d.vel[1].abs() < 1.2 {
                    d.vel[1] = 0.0;
                }
                let drag = (1.0 - 6.0 * dt).max(0.0);
                for i in [0, 2] {
                    d.vel[i] *= drag;
                }
                for s in &mut d.spin {
                    *s *= drag;
                }
            }
            for i in 0..3 {
                d.rot[i] += d.spin[i] * dt;
            }
        }
        self.debris.retain(|d| d.life > 0.0);
    }

    fn player_fire(&mut self) {
        let w = self.player.gun();
        let eye = self.player.eye();
        let fwd = self.player.forward();
        let ads = self.player.ads;
        self.player.ammo[self.player.weapon] -= 1;
        self.player.cooldown = w.delay;
        self.player.armed = false;
        self.events.push([ev::SHOT, self.player.weapon as f32, 0.0]);
        let mut outcome = 0.0f32;

        let (sy, cy) = self.player.yaw.sin_cos();
        let right = [cy, 0.0, sy];
        let up = cross(right, fwd);
        let spread = w.spread + (w.ads_spread - w.spread) * ads;
        // the shot is drawn from the muzzle, which sits right/down from the eye until the gun is raised
        let (side, drop) = (0.2 * (1.0 - ads), 0.18 * (1.0 - 0.7 * ads));
        let muzzle = [
            eye[0] + right[0] * side + fwd[0] * 0.6,
            eye[1] - drop + fwd[1] * 0.6,
            eye[2] + right[2] * side + fwd[2] * 0.6,
        ];

        for _ in 0..w.pellets {
            let dir = if spread > 0.0 {
                let r = spread * self.rng.f().sqrt();
                let a = self.rng.range(0.0, std::f32::consts::TAU);
                let (dx, dy) = (r * a.cos(), r * a.sin());
                unit([
                    fwd[0] + right[0] * dx + up[0] * dy,
                    fwd[1] + right[1] * dx + up[1] * dy,
                    fwd[2] + right[2] * dx + up[2] * dy,
                ])
            } else {
                fwd
            };
            let (dist, target) = self.trace_bots(eye, dir);
            if let Some((i, head)) = target {
                if self.mode == Mode::Zombies {
                    self.blood(along(eye, dir, dist), dir, if head { 10 } else { 6 });
                }
                let pp = self.player.body.pos;
                let bot = &mut self.bots[i];
                bot.hp -= if head { w.head_dmg } else { w.body_dmg };
                bot.hit_t = 1.0;
                self.player.hit_marker = 1.0;
                outcome = outcome.max(if head { ev::HEAD } else { ev::HIT });
                if bot.hp <= 0.0 {
                    bot.alive = false;
                    bot.respawn_t = 3.0;
                    let at = bot.body.pos;
                    let power = (w.body_dmg * w.pellets as f32 / 34.0).sqrt().clamp(0.8, 1.8);
                    // zombies always crumple where they stand; harder kills just spray more blood
                    let collapse = self.mode == Mode::Zombies;
                    if collapse {
                        bot.corpse = true;
                        bot.dead_t = 0.0;
                        bot.face = (pp[0] - at[0]).atan2(-(pp[2] - at[2]));
                    }
                    self.player.kills += 1;
                    self.player.kill_t = 1.0;
                    if collapse {
                        self.blood([at[0], at[1] + if head { 1.6 } else { 1.2 }, at[2]], dir, if head || power > 1.3 { 32 } else { 16 });
                    } else {
                        self.burst(at, dir, head, power);
                    }
                    outcome = ev::KILL;
                }
            }
            self.tracers.push(Tracer { a: muzzle, b: along(eye, dir, dist), life: 0.06, enemy: false });
        }

        if outcome > 0.0 {
            self.events.push([outcome, 0.0, 0.0]);
        }

        // recoil kicks the aim up and a little sideways, applied after the shot is traced
        let k = w.kick * (1.0 - 0.5 * ads);
        let kick_y = self.rng.range(-0.4, 0.4) * k;
        self.player.pitch = (self.player.pitch + k).min(1.5);
        self.player.yaw += kick_y;
    }

    pub fn tick(&mut self, dt: f32) {
        self.time += dt;
        let world = self.world.clone();
        self.nav.set_goal(self.player.body.pos);
        self.tick_player(dt, &world);
        if self.mode == Mode::Zombies {
            self.tick_waves(dt);
        }
        self.tick_bots(dt, &world);
        self.tick_debris(dt, &world);
        for t in &mut self.tracers {
            t.life -= dt;
        }
        self.tracers.retain(|t| t.life > 0.0);
    }

    fn tick_player(&mut self, dt: f32, world: &Grid) {
        let inp = self.input;
        self.input.look_dx = 0.0;
        self.input.look_dy = 0.0;
        let p = &mut self.player;
        p.hit_marker = (p.hit_marker - dt * 5.0).max(0.0);
        p.damage_flash = (p.damage_flash - dt * 2.0).max(0.0);
        p.kill_t = (p.kill_t - dt * 2.5).max(0.0);
        p.cooldown = (p.cooldown - dt).max(0.0);

        if !p.alive() {
            p.respawn_t -= dt;
            if p.respawn_t <= 0.0 {
                if self.mode == Mode::Zombies {
                    self.reset_waves();
                }
                self.respawn_player();
            }
            return;
        }

        let w = p.gun();
        // zombie runs are long, so health comes back when nothing has touched you for a while
        p.since_hurt += dt;
        if self.mode == Mode::Zombies && p.since_hurt > 5.0 {
            p.hp = (p.hp + 8.0 * dt).min(100.0);
        }
        p.equip_t = (p.equip_t - dt).max(0.0);
        let aim = if inp.ads && p.reload_t <= 0.0 && p.equip_t <= 0.0 { 1.0 } else { 0.0 };
        p.ads += (aim - p.ads).clamp(-8.0 * dt, 8.0 * dt);

        let sens = SENS * (1.0 + (w.zoom - 1.0) * p.ads);
        p.yaw += inp.look_dx * sens;
        p.pitch = (p.pitch - inp.look_dy * sens).clamp(-1.5, 1.5);

        let (sy, cy) = p.yaw.sin_cos();
        let fwd = (inp.w as i32 - inp.s as i32) as f32;
        let side = (inp.d as i32 - inp.a as i32) as f32;
        let mut wx = sy * fwd + cy * side;
        let mut wz = -cy * fwd + sy * side;
        let l = (wx * wx + wz * wz).sqrt();
        if l > 0.0 {
            wx /= l;
            wz /= l;
        }
        let speed = if inp.sprint { SPRINT } else { WALK } * w.speed * (1.0 - 0.4 * p.ads);
        let (was_ground, fall) = (p.body.on_ground, -p.body.vel[1]);
        step_body(&mut p.body, [wx * speed, wz * speed], inp.jump, dt, world);
        let run = (p.body.vel[0].powi(2) + p.body.vel[2].powi(2)).sqrt();
        if was_ground && !p.body.on_ground && p.body.vel[1] > 0.0 {
            self.events.push([ev::JUMP, 0.0, 0.0]);
        } else if !was_ground && p.body.on_ground && fall > 5.0 {
            self.events.push([ev::LAND, fall, 0.0]);
        }
        if p.body.on_ground && run > 1.5 {
            p.stride += run * dt;
            if p.stride > 2.1 {
                p.stride -= 2.1;
                self.events.push([ev::STEP, run, 0.0]);
            }
        }

        if !inp.fire {
            p.armed = true;
        }
        let ammo = p.ammo[p.weapon];
        if p.reload_t > 0.0 {
            p.reload_t -= dt;
            if p.reload_t <= 0.0 {
                p.ammo[p.weapon] = w.mag;
            }
        } else if (inp.reload && ammo < w.mag) || ammo == 0 {
            p.reload_t = w.reload;
            self.events.push([ev::RELOAD, p.weapon as f32, 0.0]);
        } else if inp.fire && p.cooldown <= 0.0 && p.equip_t <= 0.0 && (w.auto || p.armed) {
            self.player_fire();
        }
    }

    fn tick_bots(&mut self, dt: f32, world: &Grid) {
        let target = self.player.body.pos;
        let target_eye = self.player.eye();
        let player_alive = self.player.alive();
        let time = self.time;
        let mut dmg = 0.0;
        let mut shots = Vec::new();
        let mut heard = Vec::new();
        let (psy, pcy) = self.player.yaw.sin_cos();
        let zombies = self.mode == Mode::Zombies;
        let snap: Vec<(V3, bool)> = self.bots.iter().map(|b| (b.body.pos, b.alive)).collect();

        for i in 0..self.bots.len() {
            if !self.bots[i].alive {
                if zombies {
                    self.bots[i].dead_t += dt;
                    continue;
                }
                self.bots[i].respawn_t -= dt;
                if self.bots[i].respawn_t <= 0.0 {
                    let pos = self.bot_spawn();
                    let b = &mut self.bots[i];
                    b.body = Body { pos, ..Body::default() };
                    b.hp = 100.0;
                    b.alive = true;
                }
                continue;
            }
            let b = &mut self.bots[i];
            let dx = target[0] - b.body.pos[0];
            let dz = target[2] - b.body.pos[2];
            let dist = (dx * dx + dz * dz).sqrt().max(0.001);
            let (nx, nz) = (dx / dist, dz / dist);
            b.hit_t = (b.hit_t - dt * 3.0).max(0.0);
            let strafe = if zombies { 0.0 } else { (time * 1.3 + b.phase).sin() * 2.0 };
            let go = match (player_alive, zombies) {
                (false, _) => 0.0,
                (true, true) if dist > 1.2 || (target[1] - b.body.pos[1]).abs() > 1.6 => b.speed,
                (true, false) if dist > 6.0 => BOT_SPEED,
                _ => 0.0,
            };

            let go = go * (1.0 - 0.8 * b.hit_t);

            if dist > LOD_RADIUS && player_alive {
                // out of sight and out of reach: no physics, no shooting, just follow the route
                if let Some(to) = self.nav.next(b.body.pos) {
                    let (rx, rz) = (to[0] - b.body.pos[0], to[2] - b.body.pos[2]);
                    let l = (rx * rx + rz * rz).sqrt().max(1e-4);
                    let moved = (b.speed * dt).min(l);
                    // it hops up as soon as it starts a rise and comes down only once it arrives, so it is never inside the step
                    let y = if to[1] > b.body.pos[1] || l - moved < 0.3 { to[1] } else { b.body.pos[1] };
                    let at = [b.body.pos[0] + rx / l * moved, y, b.body.pos[2] + rz / l * moved];
                    let probe = aabb_at(at, HW, H);
                    if !world.find(&probe, |w| overlaps(&probe, w)) {
                        b.body.pos = at;
                    } else if let Some(c) = self.nav.snap(b.body.pos) {
                        // squeezed against a wall: drift back to the middle of the cell
                        let (sx, sz) = (c[0] - b.body.pos[0], c[1] - b.body.pos[2]);
                        let m = (b.speed * dt).min((sx * sx + sz * sz).sqrt());
                        let sl = (sx * sx + sz * sz).sqrt().max(1e-4);
                        b.body.pos = [b.body.pos[0] + sx / sl * m, b.body.pos[1], b.body.pos[2] + sz / sl * m];
                    }
                    b.body.vel = [rx / l * b.speed, 0.0, rz / l * b.speed];
                    b.body.on_ground = true;
                    continue;
                }
            }

            // walk straight at the player when nothing at knee height is in the way (low cover gets
            // jumped), otherwise follow the route around it
            let knee = [b.body.pos[0], b.body.pos[1] + 0.6, b.body.pos[2]];
            let to_player = [dx, target[1] + 0.6 - knee[1], dz];
            let reach = len(to_player).max(0.001);
            let straight = world.raycast(knee, [to_player[0] / reach, to_player[1] / reach, to_player[2] / reach], reach) >= reach;
            let tv = match self.nav.next(b.body.pos) {
                Some(to) if !straight => {
                    let (rx, rz) = (to[0] - b.body.pos[0], to[2] - b.body.pos[2]);
                    let l = (rx * rx + rz * rz).sqrt().max(1e-4);
                    [rx / l * go, rz / l * go]
                }
                _ => [nx * go - nz * strafe, nz * go + nx * strafe],
            };
            let mut tv = tv;
            if zombies {
                // push apart from neighbours so a horde doesn't stack into one body
                for (j, (pj, aj)) in snap.iter().enumerate() {
                    let (ox, oz) = (b.body.pos[0] - pj[0], b.body.pos[2] - pj[2]);
                    let d = (ox * ox + oz * oz).sqrt();
                    if j != i && *aj && d < 0.9 && d > 0.001 {
                        tv[0] += ox / d * (0.9 - d) * 4.0;
                        tv[1] += oz / d * (0.9 - d) * 4.0;
                    }
                }
            }
            let blocked = step_body(&mut b.body, tv, false, dt, world);
            if blocked && b.body.on_ground {
                b.body.vel[1] = JUMP;
            }

            if zombies {
                b.swing_t = (b.swing_t - dt * 2.5).max(0.0);
                b.fire_t -= dt;
                b.growl_t -= dt;
                if b.growl_t <= 0.0 {
                    b.growl_t = self.rng.range(4.0, 9.0);
                    if dist < 45.0 {
                        heard.push([ev::GROWL, dist, -(nx * pcy + nz * psy)]);
                    }
                }
                if player_alive && dist < 1.7 && (target[1] - b.body.pos[1]).abs() < 1.6 && b.fire_t <= 0.0 {
                    b.fire_t = 0.9;
                    b.swing_t = 1.0;
                    dmg += if b.kind == 2 { 30.0 } else { 12.0 };
                }
                continue;
            }
            if player_alive && dist < 1.6 {
                dmg += 20.0 * dt;
            }
            b.fire_t -= dt;
            if player_alive && dist < 16.0 && b.fire_t <= 0.0 {
                let eye = [b.body.pos[0], b.body.pos[1] + 1.5, b.body.pos[2]];
                let to = sub(target_eye, eye);
                let d = len(to);
                let dir = [to[0] / d, to[1] / d, to[2] / d];
                b.fire_t = 0.9 + self.rng.f() * 0.6;
                let clear = self.walls_clear(eye, dir, d);
                if clear {
                    let hit = self.rng.f() < 0.3;
                    let end = if hit { target_eye } else { along(target_eye, [self.rng.range(-1.0, 1.0), self.rng.range(-0.5, 0.5), self.rng.range(-1.0, 1.0)], 1.0) };
                    shots.push(Tracer { a: eye, b: end, life: 0.08, enemy: true });
                    // pan by how far to the player's right the shooter is
                    heard.push([ev::ENEMY_SHOT, d, -(dir[0] * pcy + dir[2] * psy)]);
                    if hit {
                        dmg += 8.0;
                    }
                }
            }
        }
        self.tracers.extend(shots);
        self.events.extend(heard);
        if dmg > 0.0 {
            self.hurt_player(dmg);
        }
    }

    fn walls_clear(&self, o: V3, d: V3, dist: f32) -> bool {
        self.world.raycast(o, d, dist) >= dist
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::block;

    /// An empty walled arena.
    fn arena() -> Game {
        Game::with_walls(1, map::perimeter())
    }

    fn idle(g: &mut Game, secs: f32) {
        for _ in 0..(secs / STEP) as usize {
            g.tick(STEP);
        }
    }

    #[test]
    fn ray_hits_and_misses_box() {
        let b = Aabb { min: [-1.0, -1.0, -1.0], max: [1.0, 1.0, 1.0] };
        assert!((ray_aabb([0.0, 0.0, -5.0], [0.0, 0.0, 1.0], &b).unwrap() - 4.0).abs() < 1e-4);
        assert!(ray_aabb([3.0, 0.0, -5.0], [0.0, 0.0, 1.0], &b).is_none());
    }

    #[test]
    fn walls_stop_the_player() {
        let mut g = arena();
        g.bots.clear();
        g.player.yaw = 0.0; // facing -z
        g.input.w = true;
        idle(&mut g, 8.0);
        assert!(g.player.body.pos[2] > -HALF + HW - 0.01, "walked through the wall: {:?}", g.player.body.pos);
    }

    #[test]
    fn bots_spawn_in_the_open_and_away_from_the_player() {
        let mut g = Game::new(3);
        for _ in 0..200 {
            let p = g.bot_spawn();
            let probe = aabb_at(p, HW, H);
            assert!(!g.world.find(&probe, |w| overlaps(&probe, w)), "spawned in a wall: {p:?}");
            let d = ((p[0] - g.player.body.pos[0]).powi(2) + (p[2] - g.player.body.pos[2]).powi(2)).sqrt();
            assert!(d > 29.0, "spawned {d} from the player");
        }
    }

    fn long_wall_arena() -> Game {
        let mut walls = map::perimeter();
        // a wall across the middle, open only at the -x end
        walls.push(block(10.0, 0.0, 60.0, 4.0, 1.0));
        Game::with_walls(1, walls)
    }

    #[test]
    fn bots_find_their_way_around_a_wall() {
        let mut g = long_wall_arena();
        g.bots.truncate(1);
        g.player.body.pos = [20.0, 0.0, 15.0];
        g.bots[0].body.pos = [20.0, 0.0, -15.0];
        let mut closest = f32::MAX;
        for _ in 0..(40.0 / STEP) as usize {
            g.tick(STEP);
            let (b, p) = (g.bots[0].body.pos, g.player.body.pos);
            closest = closest.min(((b[0] - p[0]).powi(2) + (b[2] - p[2]).powi(2)).sqrt());
        }
        assert!(closest < 8.0, "never got round the wall, closest {closest}");
    }

    #[test]
    fn far_bots_glide_without_ever_entering_a_wall() {
        for seed in 1..4 {
            let mut g = Game::new(seed);
            g.player.body.pos = [0.0, 0.0, 16.0];
            for _ in 0..(30.0 / STEP) as usize {
                g.tick(STEP);
                for b in g.bots.iter().filter(|b| b.alive) {
                    let (bb, hb) = (b.body_box(), b.head_box());
                    assert!(
                        !g.world.find(&bb, |w| overlaps(&bb, w)) && !g.world.find(&hb, |w| overlaps(&hb, w)),
                        "seed {seed}: bot inside a wall at {:?}",
                        b.body.pos
                    );
                }
            }
        }
    }

    #[test]
    fn jumping_reaches_a_platform() {
        let mut walls = map::perimeter();
        walls.push(block(-14.0, -14.0, 4.0, 1.0, 4.0));
        let mut g = Game::with_walls(1, walls);
        g.bots.clear();
        // 4x4 platform, 1.0 high, centred at (-14, -14); stand just outside it
        g.player.body.pos = [-14.0, 0.0, -11.0];
        g.player.yaw = 0.0; // facing -z, towards it
        g.input.w = true;
        g.input.jump = true;
        idle(&mut g, 1.5);
        assert!(g.player.body.pos[1] >= 0.99, "did not land on the platform: {:?}", g.player.body.pos);
    }

    #[test]
    fn three_body_shots_kill_a_bot() {
        let mut g = arena();
        g.bots.truncate(1);
        g.player.body.pos = [0.0, 0.0, 16.0];
        g.bots[0].body.pos = [0.0, 0.0, 14.0]; // 2m straight ahead (yaw 0 faces -z)
        g.player.yaw = 0.0;
        g.player.pitch = -0.2; // lands on the torso, below the head box
        let before = g.player.kills;
        for _ in 0..3 {
            g.player.cooldown = 0.0;
            g.player_fire();
        }
        assert_eq!(g.player.kills, before + 1);
        assert!(!g.bots[0].alive);
    }

    fn bot_ahead(g: &mut Game) {
        g.bots.truncate(1);
        g.player.body.pos = [0.0, 0.0, 16.0];
        g.bots[0].body.pos = [0.0, 0.0, 14.0];
        g.player.yaw = 0.0;
        g.player.pitch = -0.2;
    }

    #[test]
    fn sniper_one_shots_and_shotgun_kills_up_close() {
        for gun in [2, 3] {
            let mut g = arena();
            bot_ahead(&mut g);
            g.select(gun);
            g.player.cooldown = 0.0;
            g.player_fire();
            assert!(!g.bots[0].alive, "{} did not kill", WEAPONS[gun].name);
        }
    }

    #[test]
    fn semi_auto_needs_a_fresh_click_and_ammo_is_per_gun() {
        let mut g = arena();
        g.bots.clear();
        g.select(3);
        idle(&mut g, 0.5);
        g.input.fire = true;
        idle(&mut g, 3.0);
        assert_eq!(g.player.ammo[3], WEAPONS[3].mag - 1, "held trigger kept firing a bolt action");
        g.select(0);
        assert_eq!(g.player.ammo[0], WEAPONS[0].mag);
        assert!(g.player.equip_t > 0.0);
    }

    #[test]
    fn cannot_fire_while_raising_a_gun() {
        let mut g = arena();
        g.bots.clear();
        g.select(1);
        g.input.fire = true;
        idle(&mut g, EQUIP_TIME * 0.5);
        assert_eq!(g.player.ammo[1], WEAPONS[1].mag);
    }

    #[test]
    fn a_kill_blows_the_soldier_apart_and_the_pieces_settle_and_vanish() {
        let mut g = arena();
        bot_ahead(&mut g);
        g.select(3);
        g.player.cooldown = 0.0;
        g.player_fire();
        assert!(!g.bots[0].alive);
        assert!(g.debris.len() >= 12, "only {} pieces", g.debris.len());
        assert!(g.player.kill_t > 0.9);
        g.bots.clear();
        for _ in 0..(1.0 / STEP) as usize {
            g.tick(STEP);
            assert!(g.debris.iter().all(|d| d.pos[1] > -0.01), "a piece fell through the floor");
        }
        idle(&mut g, 7.0);
        assert!(g.debris.is_empty());
    }

    #[test]
    fn actions_emit_sound_events() {
        let mut g = arena();
        bot_ahead(&mut g);
        g.player.cooldown = 0.0;
        g.player_fire();
        let codes: Vec<f32> = g.events.iter().map(|e| e[0]).collect();
        assert!(codes.contains(&ev::SHOT) && codes.contains(&ev::HIT), "{codes:?}");
        g.events.clear();
        g.select(1);
        assert_eq!(g.events[0][0], ev::EQUIP);
        g.events.clear();
        g.bots.clear();
        g.player.body.pos = [5.0, 0.0, 16.0]; // clear of the low cover in front of the spawn
        g.input.w = true;
        idle(&mut g, 1.5);
        assert!(g.events.iter().any(|e| e[0] == ev::STEP), "no footsteps while running");
    }

    #[test]
    fn walking_climbs_a_ledge_but_not_a_wall() {
        let mut walls = map::perimeter();
        walls.push(block(0.0, -5.0, 6.0, 0.3, 2.0)); // a kerb
        walls.push(block(10.0, -5.0, 6.0, 1.0, 2.0)); // too tall to step onto
        let mut g = Game::with_walls(1, walls);
        g.bots.clear();
        g.input.w = true;
        g.player.yaw = 0.0;
        g.player.body.pos = [0.0, 0.0, 0.0];
        idle(&mut g, 1.5);
        assert!(g.player.body.pos[2] < -5.0, "the kerb stopped the player: {:?}", g.player.body.pos);
        g.player.body.pos = [10.0, 0.0, 0.0];
        idle(&mut g, 1.5);
        assert!(g.player.body.pos[2] > -3.9, "walked up a 1 m wall: {:?}", g.player.body.pos);
    }

    #[test]
    fn the_player_spawns_on_open_ground_facing_the_middle() {
        for seed in 1..6 {
            let g = Game::new(seed);
            let b = aabb_at(g.player.body.pos, HW, H);
            assert!(!g.world.find(&b, |w| overlaps(&b, w)), "seed {seed}: spawned in a wall");
            let f = g.player.forward();
            let to_mid = [-g.player.body.pos[0], -g.player.body.pos[2]];
            assert!(f[0] * to_mid[0] + f[2] * to_mid[1] > 0.0, "seed {seed}: facing away from the middle");
        }
    }

    fn zombie_game() -> Game {
        let mut g = arena();
        g.mode = Mode::Zombies;
        g.bots.clear();
        g.wave_t = 0.0;
        g
    }

    #[test]
    fn zombies_arrive_in_waves_and_claw_but_never_shoot() {
        let mut g = zombie_game();
        g.player.hp = 1e6;
        for _ in 0..(40.0 / STEP) as usize {
            g.tick(STEP);
            assert!(g.tracers.iter().all(|t| !t.enemy), "a zombie shot at the player");
        }
        assert_eq!(g.wave, 1);
        assert!(g.bots.iter().any(|b| b.alive));
        assert!(g.player.hp < 1e6, "no zombie ever reached the player");
        assert!(g.events.iter().any(|e| e[0] == ev::WAVE));
    }

    #[test]
    fn the_next_wave_starts_after_the_last_zombie_dies_and_is_bigger() {
        let mut g = zombie_game();
        g.player.hp = 1e6;
        idle(&mut g, 0.1);
        let first = g.zombies_left();
        for b in &mut g.bots {
            b.alive = false;
        }
        g.to_spawn = 0;
        assert_eq!(g.zombies_left(), 0);
        idle(&mut g, 5.5);
        assert_eq!(g.wave, 2);
        assert!(g.zombies_left() > first, "wave 2 had {} zombies against {first}", g.zombies_left());
    }

    #[test]
    fn dying_in_zombies_restarts_from_wave_one() {
        let mut g = zombie_game();
        idle(&mut g, 0.5);
        g.wave = 4;
        g.hurt_player(1e9);
        assert!(!g.player.alive());
        idle(&mut g, 4.2);
        assert!(g.player.alive());
        assert_eq!(g.wave, 0);
        assert!(g.bots.is_empty());
    }

    #[test]
    fn a_brute_takes_more_shots_than_a_walker() {
        let mut g = zombie_game();
        bot_ahead_zombie(&mut g, 2);
        let mut shots = 0;
        while g.bots[0].alive && shots < 20 {
            g.player.cooldown = 0.0;
            g.player.ammo[0] = 30;
            g.player_fire();
            shots += 1;
        }
        assert!(shots >= 6, "a brute died in {shots} shots");
    }

    fn bot_ahead_zombie(g: &mut Game, kind: u8) {
        g.player.body.pos = [0.0, 0.0, 16.0];
        g.player.yaw = 0.0;
        g.player.pitch = -0.2;
        let mut z = Bot::soldier([0.0, 0.0, 14.0], 0.0);
        z.kind = kind;
        z.hp = if kind == 2 { 320.0 } else { 100.0 };
        z.scale = if kind == 2 { 1.25 } else { 1.0 };
        g.bots.push(z);
    }
}
