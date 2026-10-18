//! Game simulation. No browser dependencies, so a server can run the same code.

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
const BOT_COUNT: usize = 6;
pub const EQUIP_TIME: f32 = 0.35;
pub const WEAPON_COUNT: usize = 4;

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

fn overlaps(a: &Aabb, b: &Aabb) -> bool {
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

pub struct Rng(u32);

impl Rng {
    fn next(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }
    fn f(&mut self) -> f32 {
        (self.next() >> 8) as f32 / 16_777_216.0
    }
    fn range(&mut self, a: f32, b: f32) -> f32 {
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
    pub fire_t: f32,
    pub phase: f32,
}

impl Bot {
    pub fn body_box(&self) -> Aabb {
        let p = self.body.pos;
        Aabb { min: [p[0] - HW, p[1], p[2] - HW], max: [p[0] + HW, p[1] + 1.4, p[2] + HW] }
    }
    pub fn head_box(&self) -> Aabb {
        let p = self.body.pos;
        Aabb { min: [p[0] - 0.25, p[1] + 1.4, p[2] - 0.25], max: [p[0] + 0.25, p[1] + H, p[2] + 0.25] }
    }
}

pub struct Tracer {
    pub a: V3,
    pub b: V3,
    pub life: f32,
    pub enemy: bool,
}

pub struct Game {
    pub walls: Vec<Aabb>,
    pub player: Player,
    pub bots: Vec<Bot>,
    pub tracers: Vec<Tracer>,
    pub input: Input,
    pub time: f32,
    rng: Rng,
}

fn block(cx: f32, cz: f32, sx: f32, sy: f32, sz: f32) -> Aabb {
    Aabb { min: [cx - sx / 2.0, 0.0, cz - sz / 2.0], max: [cx + sx / 2.0, sy, cz + sz / 2.0] }
}

fn build_walls() -> Vec<Aabb> {
    let mut w = vec![
        // perimeter, arena is 40 x 40
        block(0.0, -20.5, 42.0, 5.0, 1.0),
        block(0.0, 20.5, 42.0, 5.0, 1.0),
        block(-20.5, 0.0, 1.0, 5.0, 42.0),
        block(20.5, 0.0, 1.0, 5.0, 42.0),
        // tall pillars
        block(-8.0, -8.0, 2.0, 4.0, 2.0),
        block(8.0, -8.0, 2.0, 4.0, 2.0),
        block(-8.0, 8.0, 2.0, 4.0, 2.0),
        block(8.0, 8.0, 2.0, 4.0, 2.0),
        // central platform with cover on top
        block(0.0, 0.0, 8.0, 1.0, 8.0),
        block(-2.0, -2.0, 1.5, 2.2, 1.5),
        block(2.0, 2.0, 1.5, 2.2, 1.5),
        // low cover
        block(-14.0, 0.0, 2.0, 1.2, 4.0),
        block(14.0, 0.0, 2.0, 1.2, 4.0),
        block(0.0, -14.0, 4.0, 1.2, 2.0),
        block(0.0, 14.0, 4.0, 1.2, 2.0),
        // stepping platforms
        block(-14.0, -14.0, 4.0, 1.0, 4.0),
        block(14.0, 14.0, 4.0, 1.0, 4.0),
        block(-14.0, 14.0, 3.0, 0.6, 3.0),
        block(14.0, -14.0, 3.0, 0.6, 3.0),
    ];
    w.shrink_to_fit();
    w
}

/// Moves one axis and pushes the body out of any wall it ends up inside. Returns true on contact.
fn move_axis(pos: &mut V3, axis: usize, delta: f32, walls: &[Aabb]) -> bool {
    pos[axis] += delta;
    let (plus, minus) = if axis == 1 { (H, 0.0) } else { (HW, HW) };
    let mut hit = false;
    for w in walls {
        if overlaps(&aabb_at(*pos, HW, H), w) {
            hit = true;
            pos[axis] = if delta > 0.0 { w.min[axis] - plus - EPS } else { w.max[axis] + minus + EPS };
        }
    }
    hit
}

/// Shared movement for the player and bots. Returns true if blocked horizontally.
fn step_body(b: &mut Body, target: [f32; 2], jump: bool, dt: f32, walls: &[Aabb]) -> bool {
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

    let hx = move_axis(&mut b.pos, 0, b.vel[0] * dt, walls);
    if hx {
        b.vel[0] = 0.0;
    }
    let hz = move_axis(&mut b.pos, 2, b.vel[2] * dt, walls);
    if hz {
        b.vel[2] = 0.0;
    }
    if move_axis(&mut b.pos, 1, b.vel[1] * dt, walls) {
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
        let mut g = Game {
            walls: build_walls(),
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
            },
            bots: Vec::new(),
            tracers: Vec::new(),
            input: Input::default(),
            time: 0.0,
            rng: Rng(seed.max(1)),
        };
        g.respawn_player();
        for _ in 0..BOT_COUNT {
            let pos = g.bot_spawn();
            let phase = g.rng.range(0.0, 6.28);
            g.bots.push(Bot {
                body: Body { pos, ..Body::default() },
                hp: 100.0,
                alive: true,
                respawn_t: 0.0,
                fire_t: 1.0,
                phase,
            });
        }
        g
    }

    fn respawn_player(&mut self) {
        let p = &mut self.player;
        p.body = Body { pos: [0.0, 0.0, 16.0], ..Body::default() };
        p.yaw = 0.0;
        p.pitch = 0.0;
        p.hp = 100.0;
        p.ammo = full_ammo();
        p.reload_t = 0.0;
        p.equip_t = 0.0;
        p.ads = 0.0;
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
    }

    fn bot_spawn(&mut self) -> V3 {
        let corner = |r: &mut Rng| if r.f() < 0.5 { -1.0 } else { 1.0 };
        let x = corner(&mut self.rng) * self.rng.range(12.0, 18.0);
        let z = corner(&mut self.rng) * self.rng.range(12.0, 18.0);
        // never spawn on top of a wall
        let probe = aabb_at([x, 0.0, z], HW, H);
        if self.walls.iter().any(|w| overlaps(&probe, w)) {
            return [x, 1.5, z];
        }
        [x, 0.0, z]
    }

    fn wall_distance(&self, o: V3, d: V3) -> f32 {
        self.walls.iter().filter_map(|w| ray_aabb(o, d, w)).fold(RANGE, f32::min)
    }

    fn hurt_player(&mut self, dmg: f32) {
        if !self.player.alive() {
            return;
        }
        self.player.hp -= dmg;
        self.player.damage_flash = 1.0;
        if self.player.hp <= 0.0 {
            self.player.deaths += 1;
            self.player.respawn_t = 2.0;
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

    fn player_fire(&mut self) {
        let w = self.player.gun();
        let eye = self.player.eye();
        let fwd = self.player.forward();
        let ads = self.player.ads;
        self.player.ammo[self.player.weapon] -= 1;
        self.player.cooldown = w.delay;
        self.player.armed = false;

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
                let bot = &mut self.bots[i];
                bot.hp -= if head { w.head_dmg } else { w.body_dmg };
                self.player.hit_marker = 1.0;
                if bot.hp <= 0.0 {
                    bot.alive = false;
                    bot.respawn_t = 3.0;
                    self.player.kills += 1;
                }
            }
            self.tracers.push(Tracer { a: muzzle, b: along(eye, dir, dist), life: 0.06, enemy: false });
        }

        // recoil kicks the aim up and a little sideways, applied after the shot is traced
        let k = w.kick * (1.0 - 0.5 * ads);
        let kick_y = self.rng.range(-0.4, 0.4) * k;
        self.player.pitch = (self.player.pitch + k).min(1.5);
        self.player.yaw += kick_y;
    }

    pub fn tick(&mut self, dt: f32) {
        self.time += dt;
        let walls = self.walls.clone();
        self.tick_player(dt, &walls);
        self.tick_bots(dt, &walls);
        for t in &mut self.tracers {
            t.life -= dt;
        }
        self.tracers.retain(|t| t.life > 0.0);
    }

    fn tick_player(&mut self, dt: f32, walls: &[Aabb]) {
        let inp = self.input;
        self.input.look_dx = 0.0;
        self.input.look_dy = 0.0;
        let p = &mut self.player;
        p.hit_marker = (p.hit_marker - dt * 5.0).max(0.0);
        p.damage_flash = (p.damage_flash - dt * 2.0).max(0.0);
        p.cooldown = (p.cooldown - dt).max(0.0);

        if !p.alive() {
            p.respawn_t -= dt;
            if p.respawn_t <= 0.0 {
                self.respawn_player();
            }
            return;
        }

        let w = p.gun();
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
        step_body(&mut p.body, [wx * speed, wz * speed], inp.jump, dt, walls);

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
        } else if inp.fire && p.cooldown <= 0.0 && p.equip_t <= 0.0 && (w.auto || p.armed) {
            self.player_fire();
        }
    }

    fn tick_bots(&mut self, dt: f32, walls: &[Aabb]) {
        let target = self.player.body.pos;
        let target_eye = self.player.eye();
        let player_alive = self.player.alive();
        let time = self.time;
        let mut dmg = 0.0;
        let mut shots = Vec::new();

        for i in 0..self.bots.len() {
            if !self.bots[i].alive {
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
            let strafe = (time * 1.3 + b.phase).sin() * 2.0;
            let go = if player_alive && dist > 6.0 { 3.2 } else { 0.0 };
            let tv = [nx * go - nz * strafe, nz * go + nx * strafe];
            let blocked = step_body(&mut b.body, tv, false, dt, walls);
            if blocked && b.body.on_ground {
                b.body.vel[1] = JUMP;
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
                let clear = self.walls_clear(walls, eye, dir, d);
                if clear {
                    let hit = self.rng.f() < 0.3;
                    let end = if hit { target_eye } else { along(target_eye, [self.rng.range(-1.0, 1.0), self.rng.range(-0.5, 0.5), self.rng.range(-1.0, 1.0)], 1.0) };
                    shots.push(Tracer { a: eye, b: end, life: 0.08, enemy: true });
                    if hit {
                        dmg += 8.0;
                    }
                }
            }
        }
        self.tracers.extend(shots);
        if dmg > 0.0 {
            self.hurt_player(dmg);
        }
    }

    fn walls_clear(&self, walls: &[Aabb], o: V3, d: V3, dist: f32) -> bool {
        walls.iter().filter_map(|w| ray_aabb(o, d, w)).all(|t| t > dist)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let mut g = Game::new(1);
        g.bots.clear();
        g.player.yaw = 0.0; // facing -z
        g.input.w = true;
        idle(&mut g, 8.0);
        assert!(g.player.body.pos[2] > -20.0 + HW - 0.01, "walked through the wall: {:?}", g.player.body.pos);
    }

    #[test]
    fn jumping_reaches_a_platform() {
        let mut g = Game::new(1);
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
        let mut g = Game::new(1);
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
            let mut g = Game::new(1);
            bot_ahead(&mut g);
            g.select(gun);
            g.player.cooldown = 0.0;
            g.player_fire();
            assert!(!g.bots[0].alive, "{} did not kill", WEAPONS[gun].name);
        }
    }

    #[test]
    fn semi_auto_needs_a_fresh_click_and_ammo_is_per_gun() {
        let mut g = Game::new(1);
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
        let mut g = Game::new(1);
        g.bots.clear();
        g.select(1);
        g.input.fire = true;
        idle(&mut g, EQUIP_TIME * 0.5);
        assert_eq!(g.player.ammo[1], WEAPONS[1].mag);
    }
}
