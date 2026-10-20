//! Game simulation. No browser dependencies, so a server can run the same code.

use crate::grid::Grid;
use crate::map::{self, Map};
use crate::nav::Nav;
use std::f32::consts::PI;
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
/// Metres between footsteps.
pub const STRIDE: f32 = 2.1;
/// Seconds of sprinting on a full bar of stamina, and seconds for an empty bar to fill again once rested.
const SPRINT_TIME: f32 = 2.5;
const STAMINA_REFILL: f32 = 6.0;
/// Stamina only starts coming back after this long without sprinting.
const STAMINA_REST: f32 = 0.8;
/// Running dry leaves you winded: no sprinting until the bar is back to this.
const WINDED_UNTIL: f32 = 0.35;
/// Hip-fire bloom: each shot adds this much of the full bloom...
const BLOOM_SHOT: f32 = 0.2;
/// ...which spreads shots this many times wider again than the gun's hip spread...
const BLOOM_MAX: f32 = 1.0;
/// ...and settles at this much a second once the trigger has rested this long. Only an automatic held down
/// fires quicker than the rest, so a sniper or a shotgun settles between every shot.
const BLOOM_SETTLE: f32 = 2.5;
const BLOOM_REST: f32 = 0.12;
const RANGE: f32 = 120.0;
const EPS: f32 = 0.001;
const SENS: f32 = 0.0022;
/// Half the side of the square arena.
pub const HALF: f32 = 100.0;
/// Most zombies alive at once; the rest of a wave trickles in as they die.
const MAX_ZOMBIES: u32 = 24;
/// Zombies come in at least this far from the player, as the crow flies...
const SPAWN_NEAR: f32 = 30.0;
/// An ammo box comes in at least this far from every player, as the crow flies.
const BOX_NEAR: f32 = 10.0;
/// No two ammo boxes closer than this, as the crow flies.
const BOX_GAP: f32 = 1.0;
/// ...and, where the map allows, no further than this to walk.
const SPAWN_WALK: f32 = 70.0;
/// A zombie this far off that hasn't got a metre further in `STUCK_T` seconds is caught on something:
/// it comes in again at a spawn point.
const STUCK_FAR: f32 = 20.0;
const STUCK_T: f32 = 5.0;
/// How far off a zombie's groans carry.
const HEAR_GROWL: f32 = 60.0;
/// The tallest ledge a walking body climbs without jumping, so stairs and terraces work.
const STEP_UP: f32 = 0.4;
/// How fast the eye catches up with the body after a step, per second: running up stairs it trails a little and
/// rises steadily instead of jerking up each step.
const STEP_EASE: f32 = 12.0;
/// A landing hurts once it is as hard as a drop from this high, so a jump off a one-storey roof is free...
const FALL_SAFE: f32 = 5.0;
/// ...and costs this much health for every metre of drop past it: a jump off a two-storey roof about 20, the
/// clock tower everything. It goes by how fast the body comes down, so any height anywhere is the same.
const FALL_DMG: f32 = 7.0;
/// Bots further than this from the player just glide along their route instead of simulating a body.
const LOD_RADIUS: f32 = 40.0;
/// Side of one cell of the wall grid.
const CELL: f32 = 4.0;
pub const EQUIP_TIME: f32 = 0.35;
pub const WEAPON_COUNT: usize = 12;
/// The first PRIMARIES weapons are the ones a loadout picks its primary from, the rest its sidearm.
pub const PRIMARIES: usize = 8;
/// The sidearm everyone starts with.
pub const PISTOL: usize = 8;
/// An empty slot of the loadout: playing for loot, a life starts with no primary.
pub const NO_GUN: usize = WEAPON_COUNT;
/// Playing for loot, the weapon crates each wave tops up to: two, and one more every third wave, up to this many.
const MAX_CRATES: usize = 5;
pub const BAT: usize = 11;
/// A swing connects this long after it starts (the wind-up), reaches this far from the eye across a fan this
/// wide each side of the aim, and takes down at most this many at once, nearest first.
pub const STRIKE: f32 = 0.13;
const REACH: f32 = 2.4;
const FAN: f32 = 0.75;
const CLEAVE: usize = 3;
/// A swing that connects holds still this long, so the blow is felt; a kill a little longer.
const HIT_STOP: f32 = 0.06;
const KILL_STOP: f32 = 0.1;
/// A weapon crate chimes every this many seconds, heard this far off, so it can be found by ear.
const CHIME_EVERY: f32 = 2.5;
const HEAR_CHIME: f32 = 45.0;

/// Things that happened this tick that a client may want to play a sound for.
/// Each event is `[code, a, b]` in `Game::events`.
pub mod ev {
    /// a = weapon
    pub const SHOT: f32 = 1.0;
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
    /// a = distance, b = 4 × kind + pan (pan is in -1..1, so the kind is `round(b / 4)`)
    pub const GROWL: f32 = 13.0;
    /// a = wave number
    pub const WAVE: f32 = 14.0;
    /// a zombie swinging at the player: a = kind, b = pan
    pub const SNARL: f32 = 15.0;
    pub const THROW: f32 = 16.0;
    /// a grenade hitting something: a = impact speed, b = distance
    pub const BOUNCE: f32 = 17.0;
    /// a = distance, b = pan
    pub const BLAST: f32 = 18.0;
    /// someone else's shot: a = distance, b = 4 × weapon + pan (as for GROWL)
    pub const FAR_SHOT: f32 = 19.0;
    /// an ammo box goes into the reserves
    pub const PICKUP: f32 = 20.0;
    /// a melee weapon swung: a = which way (see `Player::swings`)
    pub const SWING: f32 = 21.0;
    /// a swing connecting: a = 0 a wall, 1 a zombie, 2 its head, 3 a kill
    pub const SMACK: f32 = 22.0;
    /// a weapon crate calling out: a = distance, b = pan
    pub const CHIME: f32 = 23.0;
}

pub mod bench;
mod grenade;
pub mod host;
#[cfg(test)]
mod load;
pub mod mirror;
pub use grenade::{Blast, Grenade, BLAST_T, GRENADES, MAX_GRENADES, THROW_TIME};

/// Everything that differs between guns. Angles are radians, times are seconds.
pub struct Weapon {
    pub name: &'static str,
    /// what the loadout menu files it under
    pub class: &'static str,
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
    /// on a reload the magazine is swapped, so unfired rounds left in it are dropped
    pub drop_mag: bool,
    /// swung rather than fired: no magazine, no aiming, and `delay` is the time from one swing to the next
    pub melee: bool,
}

pub static WEAPONS: [Weapon; WEAPON_COUNT] = [
    Weapon { name: "Carbine", class: "Assault rifle", mag: 30, delay: 0.11, reload: 1.4, pellets: 1, body_dmg: 34.0, head_dmg: 75.0, spread: 0.06, ads_spread: 0.002, kick: 0.006, auto: true, zoom: 0.8, scope: false, speed: 1.0, drop_mag: true, melee: false },
    Weapon { name: "SMG", class: "SMG", mag: 35, delay: 0.065, reload: 1.2, pellets: 1, body_dmg: 20.0, head_dmg: 40.0, spread: 0.045, ads_spread: 0.007, kick: 0.004, auto: true, zoom: 0.85, scope: false, speed: 1.08, drop_mag: true, melee: false },
    Weapon { name: "Shotgun", class: "Shotgun", mag: 6, delay: 0.85, reload: 2.4, pellets: 8, body_dmg: 14.0, head_dmg: 28.0, spread: 0.07, ads_spread: 0.038, kick: 0.045, auto: false, zoom: 0.9, scope: false, speed: 1.0, drop_mag: false, melee: false },
    Weapon { name: "Sniper", class: "Sniper", mag: 5, delay: 1.3, reload: 2.6, pellets: 1, body_dmg: 100.0, head_dmg: 150.0, spread: 0.12, ads_spread: 0.0, kick: 0.03, auto: false, zoom: 0.3, scope: true, speed: 0.92, drop_mag: false, melee: false },
    // harder hitting than the carbine and harder to hold on target
    Weapon { name: "AK-47", class: "Assault rifle", mag: 30, delay: 0.10, reload: 1.6, pellets: 1, body_dmg: 42.0, head_dmg: 95.0, spread: 0.07, ads_spread: 0.004, kick: 0.010, auto: true, zoom: 0.8, scope: false, speed: 0.97, drop_mag: true, melee: false },
    // a hose: very fast and light, but no stock to steady the sights
    Weapon { name: "Uzi", class: "SMG", mag: 32, delay: 0.055, reload: 1.3, pellets: 1, body_dmg: 17.0, head_dmg: 34.0, spread: 0.05, ads_spread: 0.013, kick: 0.005, auto: true, zoom: 0.88, scope: false, speed: 1.1, drop_mag: true, melee: false },
    // both barrels go almost at once, then it breaks open for two more shells
    Weapon { name: "Double barrel", class: "Shotgun", mag: 2, delay: 0.2, reload: 1.7, pellets: 10, body_dmg: 15.0, head_dmg: 30.0, spread: 0.085, ads_spread: 0.05, kick: 0.06, auto: false, zoom: 0.92, scope: false, speed: 1.02, drop_mag: false, melee: false },
    // iron sights and a quicker bolt than the scoped rifle, and a head shot always kills
    Weapon { name: "Mosin", class: "Sniper", mag: 5, delay: 1.0, reload: 2.2, pellets: 1, body_dmg: 85.0, head_dmg: 220.0, spread: 0.10, ads_spread: 0.0, kick: 0.028, auto: false, zoom: 0.55, scope: false, speed: 0.96, drop_mag: false, melee: false },
    Weapon { name: "Pistol", class: "Pistol", mag: 12, delay: 0.16, reload: 1.1, pellets: 1, body_dmg: 26.0, head_dmg: 60.0, spread: 0.04, ads_spread: 0.004, kick: 0.012, auto: false, zoom: 0.85, scope: false, speed: 1.05, drop_mag: true, melee: false },
    Weapon { name: "Revolver", class: "Pistol", mag: 6, delay: 0.42, reload: 2.0, pellets: 1, body_dmg: 60.0, head_dmg: 135.0, spread: 0.045, ads_spread: 0.003, kick: 0.035, auto: false, zoom: 0.85, scope: false, speed: 1.04, drop_mag: false, melee: false },
    // a hand cannon: every shot throws the aim well up, so quick follow-ups take pulling it back down
    Weapon { name: "Desert Eagle", class: "Pistol", mag: 7, delay: 0.32, reload: 1.5, pellets: 1, body_dmg: 55.0, head_dmg: 125.0, spread: 0.05, ads_spread: 0.004, kick: 0.10, auto: false, zoom: 0.85, scope: false, speed: 1.0, drop_mag: true, melee: false },
    // never runs dry: a runner falls to one blow, a walker to one to the head, and a swing can take down three.
    // Light in the hands, so you move quicker with it out
    Weapon { name: "Baseball bat", class: "Melee", mag: 0, delay: 0.55, reload: 0.0, pellets: 1, body_dmg: 60.0, head_dmg: 100.0, spread: 0.0, ads_spread: 0.0, kick: 0.0, auto: true, zoom: 1.0, scope: false, speed: 1.12, drop_mag: false, melee: true },
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

#[derive(Clone, Copy)]
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

#[derive(Default, Clone, Copy, Debug, PartialEq)]
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
    pub throw: bool,
    pub look_dx: f32,
    pub look_dy: f32,
    /// where the player looks, yaw and pitch, instead of turning by `look_dx` and `look_dy`: from a client over
    /// the network, which turns its own view and says where it ended up
    pub view: Option<[f32; 2]>,
    /// a gun to switch to (see `Game::select`) and one to pick for the loadout (see `Game::pick`), this step
    pub select: Option<u8>,
    pub pick: Option<u8>,
    /// over the network: the server tick the client was showing the zombies at, so shots are checked against
    /// where it saw them (see `Game::history`)
    pub seen: f32,
}

#[derive(Default, Clone, Copy)]
pub struct Body {
    pub pos: V3,
    pub vel: V3,
    pub on_ground: bool,
}

pub struct Player {
    /// who they are over the network; 0 playing alone
    pub id: u8,
    /// the keys held, playing alone; over the network inputs go straight to `Game::step_player`
    pub input: Input,
    /// how many times they have been spawned, so a client sees it has been respawned
    pub life: u32,
    /// for the spread and kick of shots, seeded the same on the server and the client for each life so both
    /// shoot the same way
    rng: Rng,
    pub body: Body,
    pub yaw: f32,
    pub pitch: f32,
    pub hp: f32,
    pub weapon: usize,
    /// the guns carried this life: a primary and a sidearm
    pub loadout: [usize; 2],
    /// the primary and sidearm to carry next life; the loadout only takes them before the first spawn or on a respawn
    pub picked: [usize; 2],
    /// has played this life, which locks the loadout until death
    pub in_play: bool,
    /// rounds left per weapon
    pub ammo: [u32; WEAPON_COUNT],
    /// rounds kept back of the magazine, per weapon, up to twice its size
    pub reserve: [u32; WEAPON_COUNT],
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
    /// metres walked over a left-right pair of steps (0 to 2 × STRIDE), for the view to bob in time with the feet
    pub gait: f32,
    /// 0 walking, 1 sprinting, eased between the two for the view
    pub sprint_t: f32,
    /// actually sprinting this tick: no reloads or box pickups while running
    pub sprinting: bool,
    /// 1 right after a hard landing, fading out
    pub land_t: f32,
    /// how far the eye trails the body after stepping up or down a step, eased away so stairs feel like a slope
    pub step_y: f32,
    /// pulling up onto a ledge, if partway through it
    pub climb: Option<Climb>,
    /// seconds since last damage, for regeneration
    since_hurt: f32,
    /// 1 full, 0 empty: sprinting spends it
    pub stamina: f32,
    /// ran out of stamina and can't sprint until it is back to WINDED_UNTIL
    pub winded: bool,
    /// seconds since the last sprint, for the refill
    since_sprint: f32,
    /// 0 settled, 1 fully bloomed: shots from the hip spread wider the longer the trigger is held
    pub bloom: f32,
    /// seconds since the last shot, for the bloom to settle
    since_shot: f32,
    pub grenades: u32,
    /// counts down while the hand is busy with a throw
    pub throw_t: f32,
    /// the grenade key is held with a grenade ready: the gun is down, and letting go throws it
    pub aiming: bool,
    /// seconds the grenade has been in hand while aiming, for it to be drawn coming up into view
    pub aim_t: f32,
    /// while aiming, where the grenade would fly to the first thing it hits, and the axis it hits that along
    /// (see `Game::grenade_arc`)
    pub arc: Vec<V3>,
    pub arc_axis: usize,
    /// 1 right after a blast close by, fading out: the view shakes
    pub shake: f32,
    /// swings of a melee weapon this life: they go one way and then the other, by whether this is odd
    pub swings: u32,
    /// counts down to the swing in progress connecting, 0 once it has (see `STRIKE`)
    pub strike_t: f32,
    /// counts down while a swing that connected holds still, the time between swings with it (see `HIT_STOP`)
    pub stop_t: f32,
    /// how bloodied the melee weapon is, 0 clean to 1 soaked, from what it has hit this life
    pub gore: f32,
}

impl Player {
    /// Someone not spawned yet (see `Game::respawn_at`).
    pub(crate) fn new(id: u8) -> Player {
        Player {
            id,
            input: Input::default(),
            life: 0,
            rng: Rng(1),
            body: Body::default(),
            yaw: 0.0,
            pitch: 0.0,
            hp: 100.0,
            weapon: 0,
            loadout: [0, PISTOL],
            picked: [0, PISTOL],
            in_play: false,
            ammo: full_ammo(),
            reserve: full_reserve(),
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
            gait: 0.0,
            sprint_t: 0.0,
            sprinting: false,
            land_t: 0.0,
            step_y: 0.0,
            climb: None,
            since_hurt: 99.0,
            stamina: 1.0,
            winded: false,
            since_sprint: 99.0,
            bloom: 0.0,
            since_shot: 99.0,
            grenades: GRENADES,
            throw_t: 0.0,
            aiming: false,
            aim_t: 0.0,
            arc: Vec::new(),
            arc_axis: 1,
            shake: 0.0,
            swings: 0,
            strike_t: 0.0,
            stop_t: 0.0,
            gore: 0.0,
        }
    }

    pub fn eye(&self) -> V3 {
        // through a window, ducked down under its head
        let duck = self.climb.as_ref().map_or(0.0, Climb::duck) * (EYE - 0.7);
        [self.body.pos[0], self.body.pos[1] + EYE + self.step_y - duck, self.body.pos[2]]
    }
    /// Turns the view by a mouse movement, slower while aiming down sights that zoom.
    pub fn look(&mut self, dx: f32, dy: f32) {
        let sens = SENS * (1.0 + (self.gun().zoom - 1.0) * self.ads);
        self.yaw += dx * sens;
        self.pitch = (self.pitch - dy * sens).clamp(-1.5, 1.5);
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
    /// Arms a new life: playing for loot only the pistol, otherwise the picked loadout. Holds the primary if there is one.
    fn arm(&mut self, loot: bool) {
        self.loadout = if loot { [NO_GUN, PISTOL] } else { self.picked };
        self.weapon = if self.loadout[0] == NO_GUN { self.loadout[1] } else { self.loadout[0] };
    }
    /// What they carry, to send over the network.
    pub(crate) fn kit(&self) -> crate::net::Kit {
        crate::net::Kit {
            loadout: self.loadout.map(|w| w as u8),
            weapon: self.weapon as u8,
            grenades: self.grenades as u8,
            ammo: self.ammo.map(|n| n.min(255) as u8),
            reserve: self.reserve.map(|n| n.min(255) as u8),
        }
    }
    /// Carrying `k`, as the server says: a gun put away by it stops reloading.
    pub(crate) fn set_kit(&mut self, k: &crate::net::Kit) {
        let weapon = (k.weapon as usize).min(WEAPON_COUNT - 1);
        if weapon != self.weapon {
            (self.reload_t, self.ads) = (0.0, 0.0);
        }
        self.loadout = k.loadout.map(|w| (w as usize).min(NO_GUN));
        self.weapon = weapon;
        self.grenades = k.grenades as u32;
        self.ammo = k.ammo.map(u32::from);
        self.reserve = k.reserve.map(u32::from);
    }
    /// Vertical field of view, narrowing while aiming down sights.
    pub fn fov(&self) -> f32 {
        1.2 * (1.0 + (self.gun().zoom - 1.0) * self.ads)
    }
    /// How far from the aim a shot can stray, as a sideways offset per metre: from the hip's, opened up by
    /// the bloom, to the aimed, which doesn't bloom.
    pub fn spread(&self) -> f32 {
        let w = self.gun();
        let hip = w.spread * (1.0 + BLOOM_MAX * self.bloom);
        hip + (w.ads_spread - hip) * self.ads
    }
}

fn full_ammo() -> [u32; WEAPON_COUNT] {
    std::array::from_fn(|i| WEAPONS[i].mag)
}

fn full_reserve() -> [u32; WEAPON_COUNT] {
    std::array::from_fn(|i| 2 * WEAPONS[i].mag)
}

pub struct Bot {
    /// never the same for two zombies in a game, as a dead one's place is taken by the next
    pub id: u32,
    /// a zombie seen over the network: how its climb is going (see `Bot::climbing`)
    pub pose: Option<Pose>,
    pub body: Body,
    pub hp: f32,
    pub alive: bool,
    /// time to the next swing
    pub fire_t: f32,
    pub phase: f32,
    /// 0 walker, 1 runner, 2 brute
    pub kind: u8,
    pub speed: f32,
    /// visual and hit box size
    pub scale: f32,
    /// 1 as a zombie swings, fading to 0
    pub swing_t: f32,
    pub growl_t: f32,
    /// 1 when just hit, fading to 0: the zombie flinches and slows
    pub hit_t: f32,
    /// a zombie that was shot down stays as a corpse for `CORPSE_T`; this is how long it has lain there
    pub corpse: bool,
    pub dead_t: f32,
    /// the way it is facing (the way it fell, once dead)
    pub face: f32,
    /// how far its head has jerked from where it would be, as right, up and forward of the way it faces, scaled
    /// with the body; the head's hit box goes with it
    pub head: V3,
    /// where the head is jerking to, how long until it jerks again, and a sideways stagger in metres a second
    twitch: V3,
    twitch_t: f32,
    stagger: f32,
    /// pulling itself up onto a ledge on its route, if partway through it
    climb: Option<Climb>,
    /// where it last stood a metre away from, and how long ago that was: see `STUCK_T`
    anchor: V3,
    stuck_t: f32,
    /// walking straight at the player rather than along the route; only decided on the ground, since mid-hop
    /// it can see over a wall it can't get over
    direct: bool,
    /// how long it has stood at the foot of a climb without finding an edge to get over: see `BALK`
    balk_t: f32,
}

/// How long a fallen zombie lies there before it fades away.
pub const CORPSE_T: f32 = 6.0;

impl Bot {
    /// A walker; `spawn_zombie` makes the others from it.
    pub(crate) fn zombie(pos: V3, phase: f32) -> Bot {
        Bot { id: 0, pose: None, body: Body { pos, ..Body::default() }, hp: 100.0, alive: true, fire_t: 0.5, phase, kind: 0, speed: 3.0, scale: 1.0, swing_t: 0.0, growl_t: 0.0, hit_t: 0.0, corpse: false, dead_t: 0.0, face: 0.0, head: [0.0; 3], twitch: [0.0; 3], twitch_t: 0.5, stagger: 0.0, climb: None, anchor: pos, stuck_t: 0.0, direct: false, balk_t: 0.0 }
    }
    pub fn body_box(&self) -> Aabb {
        let (p, s) = (self.body.pos, self.scale);
        Aabb { min: [p[0] - 0.32 * s, p[1], p[2] - 0.32 * s], max: [p[0] + 0.32 * s, p[1] + 1.4 * s, p[2] + 0.32 * s] }
    }
    /// Dev view: a zombie standing at `at` facing `yaw`, `s` of the way up whatever ledge is in front of it, or
    /// through whatever window, if any.
    pub fn climbing_at(at: V3, yaw: f32, s: f32, world: &Grid) -> Option<Bot> {
        let mut c = find_ledge(at, yaw, BOT_REACH, BOT_CLIMB_PACE, true, world).or_else(|| find_vault(at, yaw, BOT_REACH, BOT_CLIMB_PACE, true, world))?;
        c.t = s * c.time;
        let mut b = Bot::zombie(c.at().0, 0.3);
        b.face = yaw;
        b.climb = Some(c);
        Some(b)
    }

    /// Pulling itself up onto a ledge, and no swinging at anyone: how it is going (see `Climb::pose`).
    pub fn climbing(&self) -> Option<Pose> {
        self.climb.as_ref().map(Climb::pose).or(self.pose)
    }

    pub fn head_box(&self) -> Aabb {
        let (p, s) = (self.body.pos, self.scale);
        let (sy, cy) = self.face.sin_cos();
        let [right, up, fwd] = self.head.map(|v| v * s);
        let c = [p[0] + cy * right + sy * fwd, p[1] + up, p[2] + sy * right - cy * fwd];
        Aabb { min: [c[0] - 0.17 * s, c[1] + 1.4 * s, c[2] - 0.17 * s], max: [c[0] + 0.17 * s, c[1] + H * s, c[2] + 0.17 * s] }
    }
}

pub struct Tracer {
    pub a: V3,
    pub b: V3,
    pub life: f32,
    /// the player who fired it, for others to draw it from the gun they see them holding
    pub by: u8,
}

/// A drop of blood. Purely cosmetic.
pub struct Debris {
    pub pos: V3,
    pub vel: V3,
    pub rot: V3,
    pub spin: V3,
    pub half: V3,
    pub color: [f32; 3],
    pub life: f32,
    /// 0 lit like anything else, 1 glowing: sparks
    pub glow: f32,
}

const MAX_DEBRIS: usize = 300;

/// What a game is run for.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Role {
    /// played alone, everything here
    Solo,
    /// on the server, for everyone in a room: the real game, with nothing only to look at (no blood, no sparks)
    Server,
    /// a client's copy of the server's game: only its own player is played here, ahead of the server, and the
    /// rest is put in from what the server says (see `mirror`)
    Mirror,
}

/// A shot someone on the server fired, for the others to see.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shot {
    pub by: u8,
    pub a: V3,
    pub b: V3,
}

/// How long the server keeps where the zombies were, to check shots against where a client saw them.
const HISTORY: usize = 30;

pub struct Game {
    pub role: Role,
    /// half the side of the map, see `Map::half`
    pub half: f32,
    pub world: Rc<Grid>,
    /// one colour per wall of `world`
    pub colors: Vec<[f32; 3]>,
    /// one surface kind per wall, see `map::material`
    pub mats: Vec<u8>,
    pub decals: Vec<(Aabb, [f32; 3])>,
    /// the ammo boxes on the ground: two thirds of the wave's zombies, rounded up, so far as the map allows
    pub boxes: Vec<V3>,
    /// the weapon crates on the ground and the gun in each, playing for loot only
    pub crates: Vec<(V3, usize)>,
    /// playing for loot: a life starts with only the pistol, and the other guns are found in weapon crates
    pub loot: bool,
    /// until the weapon crates next chime
    chime_t: f32,
    spawns: Vec<V3>,
    nav: Nav,
    pub debris: Vec<Debris>,
    pub grenades: Vec<Grenade>,
    pub blasts: Vec<Blast>,
    pub events: Vec<[f32; 3]>,
    /// everyone playing; one playing alone
    pub players: Vec<Player>,
    /// which of `players` is the one this machine looks through
    pub local: usize,
    /// the zombies, alive and fallen
    pub bots: Vec<Bot>,
    /// the last id given to a zombie, a grenade or a blast
    next_id: u32,
    /// the current wave (0 before the first), zombies still to arrive, and the timers
    pub wave: u32,
    to_spawn: u32,
    spawn_t: f32,
    wave_t: f32,
    pub tracers: Vec<Tracer>,
    /// on the server, the shots fired since the last look, for clients to draw
    pub shots: Vec<Shot>,
    pub time: f32,
    /// steps played
    pub tick_n: u32,
    /// on the server, the zombies' hit boxes (id, body, head) at each of the last `HISTORY` ticks, newest last
    history: std::collections::VecDeque<(u32, Vec<(u32, Aabb, Aabb)>)>,
    /// on a client, how many zombies are left to kill this wave, as the server says
    pub remote_left: Option<u32>,
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
    // a hillside too steep to walk up stops the body like a wall
    if axis != 1 && world.ground.is_some() {
        let (was, now) = (world.ground_at(before[0], before[2]), world.ground_at(pos[0], pos[2]));
        let rise = now - before[1].max(was);
        if rise > 0.0 && rise > (pos[axis] - before[axis]).abs() * MAX_SLOPE + 0.01 {
            pos[axis] = before[axis];
            return true;
        }
    }
    hit
}

/// Steepest rise a body can walk up, per metre along.
pub const MAX_SLOPE: f32 = 0.75;

/// How far above a zombie's feet someone's can be and still be clawed at, by the legs: higher than a garden fence
/// or a car roof, so neither is somewhere to stand out of reach. And how far below for it to strike down at them.
pub const STRIKE_UP: f32 = 2.0;
pub const STRIKE_DOWN: f32 = 1.6;

/// How high above the feet a ledge can be and still be caught: with a jump's 1.28 m, up to about 2.9 m from the
/// ground, onto a container but not a storey (3.3 m).
const LEDGE_REACH: f32 = 1.6;
/// A ledge lower than this is a step or a hop, not a climb.
pub(crate) const LEDGE_LOW: f32 = 0.25;
/// How long a pull up takes: so long and so much more a metre of it. Zombies are slower about it.
const CLIMB_PACE: (f32, f32) = (0.2, 0.15);
pub(crate) const BOT_CLIMB_PACE: (f32, f32) = (0.5, 0.35);
/// Zombies climb from the ground whatever the player can get onto by jumping and pulling up.
pub(crate) const BOT_REACH: f32 = crate::nav::LEDGE + 0.1;
/// The smallest hole a body gets through, on hands and knees: as wide as it is, and as tall as it is folded up. A
/// window is one, a peephole or a slot under a fence is not.
pub(crate) const SQUEEZE: f32 = 0.9;

/// A pull up onto a ledge: the body rises beside the face to the top, then moves in over it. Or through a hole in
/// it, as a window is (see `find_vault`), folded up small, onto the sill and out over it.
#[derive(Clone, Copy)]
pub struct Climb {
    /// where the climb began, and where the body goes up from, at the foot of the face
    start: V3,
    from: V3,
    to: V3,
    t: f32,
    time: f32,
    /// which way the ledge is, and how far ahead of `from` its edge
    yaw: f32,
    edge: f32,
    /// a zombie hauls itself up in heaves (see `CLIMB_GRAB` on); the player's own pull up is one smooth rise
    heave: bool,
    /// through a hole, not up onto a top: `to` is on the far side of it, at the height of the sill
    squeeze: bool,
}

/// How a zombie's climb is going, to draw it by: see `Climb::pose`.
pub type Pose = (f32, f32, f32, f32);

/// How a zombie's climb goes, as fractions of it: a hop to get its hands on the edge, two heaves that bring its
/// hips up level with the top, then a knee onto it and over.
pub const CLIMB_GRAB: f32 = 0.12;
pub const CLIMB_OVER: f32 = 0.62;
/// How far below the top the feet hang with the arms pushed straight down on the edge.
pub const CLIMB_HANG: f32 = 0.85;

fn ease(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

impl Climb {
    /// All of it as numbers, to send over the network (see `net::Own`).
    pub(crate) fn raw(&self) -> [f32; 14] {
        let [s, f, t] = [self.start, self.from, self.to];
        let kind = self.heave as u8 | (self.squeeze as u8) << 1;
        [s[0], s[1], s[2], f[0], f[1], f[2], t[0], t[1], t[2], self.t, self.time, self.yaw, self.edge, kind as f32]
    }
    pub(crate) fn from_raw(r: &[f32; 14]) -> Climb {
        let kind = r[13].round() as u8;
        Climb { start: [r[0], r[1], r[2]], from: [r[3], r[4], r[5]], to: [r[6], r[7], r[8]], t: r[9], time: r[10], yaw: r[11], edge: r[12], heave: kind & 1 != 0, squeeze: kind & 2 != 0 }
    }

    /// Where the body is `t` seconds in, and whether it is done.
    fn at(&self) -> (V3, bool) {
        let s = self.t / self.time;
        let (a, b) = (self.from, self.to);
        let (up, over) = if self.heave {
            // hopping high enough to get its hands onto the edge in front of it
            let grab = (b[1] - 1.85).clamp(a[1], (b[1] - CLIMB_HANG).max(a[1]));
            let hang = (b[1] - CLIMB_HANG).max(grab);
            let y = if s < CLIMB_GRAB {
                a[1] + (grab - a[1]) * ease(s / CLIMB_GRAB)
            } else if s < CLIMB_OVER {
                // two pulls, each stalling as the arms get tired before the next
                let x = (s - CLIMB_GRAB) / (CLIMB_OVER - CLIMB_GRAB);
                grab + (hang - grab) * (x - (4.0 * PI * x).sin() / (4.0 * PI))
            } else {
                hang + (b[1] - hang) * ease((s - CLIMB_OVER) / (1.0 - CLIMB_OVER) / 0.45)
            };
            // the knee goes up onto the top as the hips rise (drawn), and the body moves in only once it is up
            ((y - a[1]) / (b[1] - a[1]).max(1e-4), ease(((s - CLIMB_OVER) / (1.0 - CLIMB_OVER) - 0.4) / 0.6))
        } else {
            // through a hole there is further to go once up: a quick hop onto the sill, then the crawl over it
            let k = if self.squeeze { 0.4 } else { 0.7 };
            (ease(s / k), ease((s - k) / (1.0 - k)))
        };
        // in to the wall with the hop, then over the edge
        let i = ease(s / CLIMB_GRAB);
        let a = [self.start[0] + (a[0] - self.start[0]) * i, a[1], self.start[2] + (a[2] - self.start[2]) * i];
        ([a[0] + (b[0] - a[0]) * over, a[1] + (b[1] - a[1]) * up, a[2] + (b[2] - a[2]) * over], s >= 1.0)
    }

    /// Where the body ends up.
    pub(crate) fn end(&self) -> V3 {
        self.to
    }

    /// How folded up the body is, 0 to 1: going through a hole it ducks as it gets up to it and stands up again only
    /// once it is out the other side.
    pub fn duck(&self) -> f32 {
        let s = self.t / self.time;
        if self.squeeze { ease(s / 0.3) * (1.0 - ease((s - 0.85) / 0.15)) } else { 0.0 }
    }

    /// For drawing a zombie's climb: how far through it is (0 to 1), how high the top is above the feet, how far
    /// ahead its edge is, and how folded up it is.
    pub fn pose(&self) -> Pose {
        // measured along the climb from the foot of the face, so while it hops in the edge is further off
        let (p, _) = self.at();
        let f = [self.yaw.sin(), -self.yaw.cos()];
        let gone = (p[0] - self.from[0]) * f[0] + (p[2] - self.from[2]) * f[1];
        ((self.t / self.time).min(1.0), self.to[1] - p[1], self.edge - gone, self.duck())
    }
}

/// A ledge in front of a body at `pos` facing `yaw` that it can pull itself up onto: the top of anything within a
/// metre or so ahead, between a hop and `reach` above the feet, with room to rise beside it and stand on it.
fn find_ledge(pos: V3, yaw: f32, reach: f32, pace: (f32, f32), heave: bool, world: &Grid) -> Option<Climb> {
    let f = [yaw.sin(), 0.0, -yaw.cos()];
    let free = |p: V3| {
        let b = aabb_at(p, HW, H);
        !world.find(&b, |w| overlaps(&b, w))
    };
    for d in [0.5, 0.7, 0.9, 1.1] {
        let o = [pos[0] + f[0] * d, pos[1] + reach + 0.05, pos[2] + f[2] * d];
        // walls only: a hillside too steep to walk up is not something to pull yourself up either
        let t = world.raycast_walls(o, [0.0, -1.0, 0.0], reach);
        // starting inside something is a wall too tall to reach the top of; no hit is nothing to climb
        if t < 0.02 || t > reach + 0.05 - LEDGE_LOW {
            continue;
        }
        // stand well onto the top if there is room, otherwise just over the edge, on whatever is highest under the
        // feet there (the next step up, on a stair), so long as that is in reach too
        for c in [[o[0] + f[0] * 0.35, o[2] + f[2] * 0.35], [o[0], o[2]]] {
            let column = Aabb { min: [c[0] - HW, pos[1] + 0.05, c[1] - HW], max: [c[0] + HW, pos[1] + reach + 0.05, c[1] + HW] };
            let mut top = 0.0f32;
            world.find(&column, |w| {
                if overlaps(&column, w) {
                    top = top.max(w.max[1]);
                }
                false
            });
            let top = top + EPS;
            // the body has to fit all the way in, risen beside the ledge and over onto it, not just at the ends, or it
            // goes through a wall to the floor behind it, or up through a ceiling to the roof on it
            let rises = ((top - pos[1]) / 0.3).ceil().max(1.0) as usize;
            let clear = (1..=rises).all(|k| free([pos[0], pos[1] + (top - pos[1]) * k as f32 / rises as f32, pos[2]]))
                && (0..=6).all(|k| {
                    let s = k as f32 / 6.0;
                    free([pos[0] + (c[0] - pos[0]) * s, top, pos[2] + (c[1] - pos[2]) * s])
                });
            if top <= pos[1] + reach + 0.05 && clear {
                let time = pace.0 + pace.1 * (top - pos[1]);
                // the face is somewhere between this ray and the last one, which missed it
                let edge = (d - 0.1).max(HW);
                // a zombie hops in to the foot of the wall first, so it goes up against it with its hands on the edge
                // instead of rising straight up from a metre out; just under the top, the face is found exactly
                let face = world.raycast_walls([pos[0], top - 0.15, pos[2]], f, d + 0.1);
                let face = if face <= d + 0.05 { face } else { edge };
                let step = if heave { (face - HW - 0.03).max(0.0) } else { 0.0 };
                let from = [pos[0] + f[0] * step, pos[1], pos[2] + f[2] * step];
                let (from, edge) = if free(from) { (from, face - step) } else { (pos, edge) };
                return Some(Climb { start: pos, from, to: [c[0], top, c[1]], t: 0.0, time, yaw, edge, heave, squeeze: false });
            }
        }
    }
    None
}

/// A hole in front of a body at `pos` facing `yaw` that it can get through, as a window is: a sill above the floor
/// within `reach` of the feet, and over it room for the body folded up (`SQUEEZE`) all the way through, until it is
/// out the other side and can stand. Up to a little over a body's width either side, so it needn't be squarely in
/// front. A peephole or a letterbox is too small for the body to fit through anywhere, and a gap with room to stand
/// in is no hole but a ledge (see `find_ledge`).
pub(crate) fn find_vault(pos: V3, yaw: f32, reach: f32, pace: (f32, f32), heave: bool, world: &Grid) -> Option<Climb> {
    let f = [yaw.sin(), 0.0, -yaw.cos()];
    let fits = |p: V3, h: f32| {
        let b = aabb_at(p, HW, h);
        !world.find(&b, |w| overlaps(&b, w))
    };
    let along = |p: V3, s: f32, y: f32| [p[0] + f[0] * s, y, p[2] + f[2] * s];
    // as far as the body gets walking on before it bumps into the wall
    let near = (1..=12).map(|k| k as f32 * 0.1).find(|&s| !fits(along(pos, s, pos[1] + EPS), H))? - 0.1;
    let floor = (pos[1] + 0.05 - world.raycast([pos[0], pos[1] + 0.05, pos[2]], [0.0, -1.0, 0.0], 3.0)).max(world.ground_at(pos[0], pos[2]));
    // a hole has a head: something just ahead whose underside is over a sill in reach, too low to stand under. A
    // plain wall has none, and so isn't looked along for one
    let wide = aabb_at(along(pos, near + 2.0 * HW, floor), 1.0, reach + pos[1] - floor + H);
    let head = |w: &Aabb| overlaps(&wide, w) && w.min[1] > floor + LEDGE_LOW + SQUEEZE - 0.01 && w.min[1] < pos[1] + reach + H;
    if !world.find(&wide, head) {
        return None;
    }
    for k in [0, -1, 1, -2, 2, -3, 3, -4, 4, -5, 5, -6, 6] {
        let o = k as f32 * 0.1;
        let from = along([pos[0] - f[2] * o, pos[1], pos[2] + f[0] * o], near, pos[1]);
        if !fits(from, SQUEEZE) {
            continue;
        }
        // the sills there could be: the tops of whatever is just ahead, clear of the floor and in reach
        let ahead = aabb_at(along(from, 2.0 * HW, floor), HW, reach + pos[1] - floor);
        let mut sills = Vec::new();
        world.find(&ahead, |w| {
            if overlaps(&ahead, w) && w.max[1] > floor + LEDGE_LOW && w.max[1] <= pos[1] + reach {
                sills.push(w.max[1] + EPS);
            }
            false
        });
        sills.sort_by(f32::total_cmp);
        sills.dedup_by(|a, b| (*a - *b).abs() < 0.01);
        for y in sills {
            // over the sill, folded up, from a little way into the wall (`near` is up to a step short of it), until
            // it can stand up again: out past the wall, or it was never in a hole at all...
            let mut squeezed = false;
            let mut out = None;
            for i in 1..=11 {
                let p = along(from, i as f32 * 0.2, y);
                if !fits(p, SQUEEZE) {
                    break;
                }
                if fits(p, H) {
                    out = squeezed.then_some(p);
                    break;
                }
                squeezed = true;
            }
            let Some(to) = out else { continue };
            // ...having got up to it beside the wall
            let rises = ((y - pos[1]).max(0.0) / 0.3).ceil() as usize;
            if !(1..=rises).all(|i| fits([from[0], pos[1] + (y - pos[1]) * i as f32 / rises as f32, from[2]], SQUEEZE)) {
                continue;
            }
            let time = pace.0 + pace.1 * ((y - pos[1]).max(0.0) + (to[0] - from[0]).hypot(to[2] - from[2]));
            return Some(Climb { start: pos, from, to, t: 0.0, time, yaw, edge: HW + 0.05, heave, squeeze: true });
        }
    }
    None
}

/// A zombie's last resort at the foot of a climb its route goes up, when no edge in front of it can be got over
/// cleanly from where it stands (a garage roof beside the corner of a house, reached from a hedge): it scrambles up
/// anyway, onto the nearest place by the route's next step at that height where a body fits standing on something.
/// The nav has already seen there is room above where it starts, so it only cuts the corner of whatever is in the
/// way on its way over.
fn scramble(pos: V3, to: V3, world: &Grid) -> Option<Climb> {
    let fits = |p: V3| {
        let b = aabb_at(p, HW, H);
        !world.find(&b, |w| overlaps(&b, w))
    };
    // finely: where a garage roof runs out a hand's breadth past the house beside it, that is all there is to stand on
    let mut around: Vec<V3> = (-5..=5).flat_map(|i| (-5..=5).map(move |k| [to[0] + i as f32 * 0.1, to[1] + EPS, to[2] + k as f32 * 0.1])).collect();
    around.sort_by(|a, b| (a[0] - pos[0]).hypot(a[2] - pos[2]).total_cmp(&(b[0] - pos[0]).hypot(b[2] - pos[2])));
    let on = |p: V3, hw: f32| {
        let b = aabb_at([p[0], p[1] - 0.05, p[2]], hw, 0.05);
        world.find(&b, |w| overlaps(&b, w))
    };
    // somewhere with something under the middle of the body if there is one: set down on a sliver of the end of a
    // fence, it only slides off again, and again
    let at = around.iter().copied().find(|&p| fits(p) && on(p, HW / 2.0)).or_else(|| around.iter().copied().find(|&p| fits(p) && on(p, HW)))?;
    let (dx, dz) = (at[0] - pos[0], at[2] - pos[2]);
    let time = BOT_CLIMB_PACE.0 + BOT_CLIMB_PACE.1 * (at[1] - pos[1]);
    Some(Climb { start: pos, from: pos, to: at, t: 0.0, time, yaw: dx.atan2(-dz), edge: dx.hypot(dz).max(HW), heave: true, squeeze: false })
}

/// How many steps of its route a zombie looks along for the furthest it can walk straight to.
const LOOK_AHEAD: usize = 4;

/// Where a zombie at `pos` whose route's next step is `first` heads for: the furthest step of the route a few on
/// that it can walk to in a straight line on the level, so it cuts across a lawn and rounds a corner as someone
/// would, instead of going cell to cell along the route's eight ways. Anything else, a stair, a climb, a drop or
/// a fence top, it takes a step at a time.
fn look_ahead(nav: &Nav, world: &Grid, pos: V3, first: V3) -> V3 {
    let level = |a: V3, b: V3| (a[1] - b[1]).abs() < 0.3;
    // nothing in the way of a body walking from `pos` to `to`, from above a step to the top of the head and across
    // its width, and somewhere to walk all along
    let straight = |to: V3| {
        let d = [to[0] - pos[0], to[1] - pos[1], to[2] - pos[2]];
        let flat = d[0].hypot(d[2]);
        if flat < 0.01 {
            return true;
        }
        let r = len(d);
        let u = [d[0] / r, d[1] / r, d[2] / r];
        let side = [-d[2] / flat * HW * 0.9, d[0] / flat * HW * 0.9];
        let ground = (1..(flat / 0.5) as usize + 1).all(|k| {
            let s = k as f32 * 0.5 / flat;
            nav.level_at([pos[0] + d[0] * s, pos[1] + d[1] * s, pos[2] + d[2] * s])
        });
        ground && [STEP_UP + 0.05, 1.0, 1.7].iter().all(|&y| {
            [-1.0, 0.0, 1.0].iter().all(|&s| world.raycast_walls([pos[0] + side[0] * s, pos[1] + y, pos[2] + side[1] * s], u, r) >= r)
        })
    };
    if !level(pos, first) {
        return first;
    }
    let (mut aim, mut at) = (first, first);
    for _ in 0..LOOK_AHEAD {
        match nav.next(at) {
            Some(n) if level(at, n) && straight(n) => (aim, at) = (n, n),
            _ => break,
        }
    }
    aim
}

/// How long a zombie stands at the foot of a climb its route goes up, finding no edge to get over, before it
/// scrambles up anyway (see `scramble`): not long, as on top of a fence it is soon jostled off again.
const BALK: f32 = 0.5;

/// Shared movement for the player and bots. Returns true if blocked horizontally.
fn step_body(b: &mut Body, target: [f32; 2], jump: bool, dt: f32, world: &Grid) -> bool {
    let grounded = b.on_ground;
    let lift = if grounded { STEP_UP } else { 0.0 };
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
    // walking down a step keeps the feet on it instead of sailing off the edge of each one
    // (and down a hill)
    let ground = world.ground_at(b.pos[0], b.pos[2]);
    if grounded && !b.on_ground && b.vel[1] <= 0.0 {
        let mut down = b.pos;
        if move_axis(&mut down, 1, -STEP_UP, world, 0.0) || down[1] <= ground {
            b.pos = [down[0], down[1].max(ground), down[2]];
            b.vel[1] = 0.0;
            b.on_ground = true;
        }
    }
    if b.pos[1] <= ground {
        b.pos[1] = ground;
        b.vel[1] = 0.0;
        b.on_ground = true;
    }
    // a hillside too steep to walk up is too steep to stand on, or to jump from: the feet slide off it downhill. So
    // a mountainside isn't hopped up a jump at a time, while a short steep bank with a flat top still is. Walking is
    // stopped along x and z each on its own (see `move_axis`), so a slope is walked up on the diagonal as long as
    // it is no steeper than that along either; so it is stood on too
    if b.on_ground && world.ground.is_some() && b.pos[1] <= ground + 0.01 {
        let s = 0.3;
        let gx = (world.ground_at(b.pos[0] + s, b.pos[2]) - world.ground_at(b.pos[0] - s, b.pos[2])) / (2.0 * s);
        let gz = (world.ground_at(b.pos[0], b.pos[2] + s) - world.ground_at(b.pos[0], b.pos[2] - s)) / (2.0 * s);
        let g = gx.hypot(gz);
        if gx.abs().max(gz.abs()) > MAX_FOOTHOLD {
            b.on_ground = false;
            b.vel[0] -= gx / g * SLIDE * dt;
            b.vel[2] -= gz / g * SLIDE * dt;
        }
    }
    hx || hz
}

/// Steepest ground, rise per metre along x or z, a body can stand on and jump from: a little more than it walks up,
/// so a bump on a hill is still hopped over.
const MAX_FOOTHOLD: f32 = MAX_SLOPE * 1.1;
/// How hard the feet slide down a hillside too steep to stand on, in m/s²: more than the 15 a body can push
/// against it with in the air, so it can't hold its place on one either.
const SLIDE: f32 = 25.0;

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

    /// A game on map number `map` (see `map::MAPS`).
    pub fn on(map: u8, seed: u32) -> Game {
        Game::with_map(seed, map::make(map, seed))
    }

    /// Bot `i`'s state for a test to print: where it stands, where its route goes next, and the ledge, if any,
    /// it would find to pull itself up onto towards there.
    #[cfg(test)]
    pub fn describe_bot(&self, i: usize) -> String {
        let b = &self.bots[i];
        let next = self.nav.next(b.body.pos);
        let ledge = next.and_then(|to| {
            let yaw = (to[0] - b.body.pos[0]).atan2(-(to[2] - b.body.pos[2]));
            find_ledge(b.body.pos, yaw, BOT_REACH, BOT_CLIMB_PACE, true, &self.world).map(|c| c.to)
        });
        format!("pos {:.2?} vel {:.1?} ground {} climbing {} direct {} balk {:.2} next {next:.2?} ledge {ledge:.2?}", b.body.pos, b.body.vel, b.body.on_ground, b.climb.is_some(), b.direct, b.balk_t)
    }

    /// Brings `n` zombies of wave `wave` in at once, from the spawn points as the waves would, for a test.
    #[cfg(test)]
    pub(crate) fn send(&mut self, n: u32, wave: u32) {
        self.wave = wave;
        for _ in 0..n {
            self.spawn_zombie();
        }
    }

    /// Zombies still to kill this wave: those alive plus those yet to arrive.
    pub fn zombies_left(&self) -> u32 {
        self.remote_left.unwrap_or_else(|| self.bots.iter().filter(|b| b.alive).count() as u32 + self.to_spawn)
    }

    fn new_id(&mut self) -> u32 {
        self.next_id += 1;
        self.next_id
    }

    /// The player this machine looks through.
    pub fn me(&self) -> &Player {
        &self.players[self.local]
    }
    pub fn me_mut(&mut self) -> &mut Player {
        &mut self.players[self.local]
    }

    /// Where the players still standing are.
    fn standing(&self) -> Vec<V3> {
        self.players.iter().filter(|p| p.alive()).map(|p| p.body.pos).collect()
    }

    /// The nearest player still standing to `at`, if any, and how far off they are along the ground.
    fn nearest_player(&self, at: V3) -> Option<(usize, f32)> {
        let d = |p: &Player| (p.body.pos[0] - at[0]).hypot(p.body.pos[2] - at[2]);
        self.players.iter().enumerate().filter(|(_, p)| p.alive()).map(|(i, p)| (i, d(p))).min_by(|a, b| a.1.total_cmp(&b.1))
    }

    /// Someone joining: a new player with id `id`, spawned away from the zombies. Returns their index.
    pub fn add_player(&mut self, id: u8) -> usize {
        self.players.push(Player::new(id));
        let i = self.players.len() - 1;
        self.respawn_player(i);
        i
    }

    /// Someone leaving.
    pub fn remove_player(&mut self, id: u8) {
        self.players.retain(|p| p.id != id);
        self.local = self.local.min(self.players.len().saturating_sub(1));
    }

    /// Lets the waves come, a few seconds from now; until then a game on the server is a lobby, where the
    /// players wait for everyone to join.
    pub fn start_waves(&mut self) {
        self.reset_waves();
    }

    /// Holds the waves off, see `start_waves`.
    pub fn hold_waves(&mut self) {
        self.wave_t = f32::INFINITY;
    }

    /// The waves have been let come.
    pub fn waves_on(&self) -> bool {
        self.wave_t.is_finite() || self.wave > 0
    }

    fn reset_waves(&mut self) {
        self.bots.clear();
        self.boxes.clear();
        self.crates.clear();
        self.wave = 0;
        self.to_spawn = 0;
        self.wave_t = 3.0;
    }

    /// Where a zombie comes in: a spawn point out of sight range but a short walk away, so the last of a wave
    /// is never a minute's walk off. Failing that the nearest walk of a few. With more than one player, out of
    /// sight of all of them and a short walk from the nearest.
    fn zombie_spawn(&mut self) -> V3 {
        let mut from = self.standing();
        if from.is_empty() {
            from = self.players.iter().map(|p| p.body.pos).collect();
        }
        if self.spawns.is_empty() {
            return self.pick_spawn(&from, SPAWN_NEAR);
        }
        let mut best = (f32::INFINITY, None);
        for _ in 0..40 {
            let s = self.spawns[(self.rng.next() as usize) % self.spawns.len()];
            let d = from.iter().map(|p| (s[0] - p[0]).hypot(s[2] - p[2])).fold(f32::INFINITY, f32::min);
            let Some(walk) = self.nav.route_len(s) else { continue };
            if d < SPAWN_NEAR {
                continue;
            }
            if walk <= SPAWN_WALK {
                return s;
            }
            if walk < best.0 {
                best = (walk, Some(s));
            }
        }
        // none with a route: the players are somewhere a long walk from every spawn point, as up on the school
        // roof, and the field doesn't reach that far. So it comes in on the ground somewhere along the way
        if best.1.is_none() {
            let far = |s: &V3| from.iter().all(|p| (s[0] - p[0]).hypot(s[2] - p[2]) >= SPAWN_NEAR);
            let along: Vec<V3> = self.nav.ground_within(SPAWN_WALK).into_iter().filter(far).collect();
            if !along.is_empty() {
                return along[self.rng.next() as usize % along.len()];
            }
        }
        best.1.unwrap_or_else(|| self.pick_spawn(&from, SPAWN_NEAR))
    }

    fn spawn_zombie(&mut self) {
        let pos = self.zombie_spawn();
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
        let mut z = Bot::zombie(pos, phase);
        z.id = self.new_id();
        z.kind = kind;
        z.hp = hp;
        z.speed = speed;
        z.scale = scale;
        z.growl_t = self.rng.range(1.0, 6.0);
        match self.bots.iter().position(|b| !b.alive && (!b.corpse || b.dead_t > CORPSE_T)) {
            Some(i) => self.bots[i] = z,
            None => self.bots.push(z),
        }
    }

    /// Top up the ammo boxes to two thirds of this wave's zombies, rounded up, so far as the map allows it.
    fn top_up_boxes(&mut self) {
        let target = (2 * self.to_spawn + 2) / 3;
        while self.boxes.len() < target as usize {
            match self.ammo_box_pos() {
                Some(at) => self.boxes.push(at),
                None => break,
            }
        }
    }

    /// Top up the weapon crates, playing for loot: two, and one more every third wave, up to `MAX_CRATES`. Each
    /// holds any gun but the pistol everyone starts with.
    fn top_up_crates(&mut self) {
        let target = (2 + self.wave as usize / 3).min(MAX_CRATES);
        while self.crates.len() < target {
            let Some(at) = self.ammo_box_pos() else { break };
            let mut gun = (self.rng.next() as usize) % (WEAPON_COUNT - 1);
            if gun >= PISTOL {
                gun += 1;
            }
            self.crates.push((at, gun));
        }
    }

    /// Where an ammo box or weapon crate goes: on open ground the player can walk to, clear of the player and of
    /// the other boxes and crates.
    fn ammo_box_pos(&mut self) -> Option<V3> {
        let players: Vec<V3> = self.players.iter().map(|p| p.body.pos).collect();
        for _ in 0..40 {
            let at = if self.spawns.is_empty() {
                // no spawn points (a bare test arena): anywhere open
                let reach = self.half - 2.0;
                let (x, z) = (self.rng.range(-reach, reach), self.rng.range(-reach, reach));
                let probe = aabb_at([x, 0.0, z], HW, H);
                if self.world.find(&probe, |w| overlaps(&probe, w)) {
                    continue;
                }
                [x, 0.0, z]
            } else {
                self.spawns[(self.rng.next() as usize) % self.spawns.len()]
            };
            if !self.nav.reachable(at) {
                continue;
            }
            // online, away from everyone
            if players.iter().any(|p| ((at[0] - p[0]).powi(2) + (at[2] - p[2]).powi(2)).sqrt() < BOX_NEAR) {
                continue;
            }
            if self.boxes.iter().chain(self.crates.iter().map(|(c, _)| c)).any(|b| ((b[0] - at[0]).powi(2) + (b[2] - at[2]).powi(2)) < BOX_GAP * BOX_GAP) {
                continue;
            }
            return Some(at);
        }
        None
    }

    /// A new wave starts a few seconds after the last zombie of the old one dies, and arrives a few at a time.
    fn tick_waves(&mut self, dt: f32) {
        let alive = self.bots.iter().filter(|b| b.alive).count() as u32;
        if self.to_spawn == 0 && alive == 0 {
            self.wave_t -= dt;
            if self.wave_t <= 0.0 {
                self.wave += 1;
                self.to_spawn = 6 + 3 * self.wave;
                // by the server for everyone online: a client has them from its snapshots
                if self.role != Role::Mirror {
                    self.top_up_boxes();
                    if self.loot {
                        self.top_up_crates();
                    }
                }
                self.spawn_t = 0.0;
                self.wave_t = 5.0;
                self.events.push([ev::WAVE, self.wave as f32, 0.0]);
                // anyone who went down last wave is back for this one
                for i in 0..self.players.len() {
                    if !self.players[i].alive() {
                        self.respawn_player(i);
                    }
                }
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

    /// A game on bare walls with no spawn points and no waves coming, for tests.
    #[cfg(test)]
    fn with_walls(seed: u32, walls: Vec<Aabb>) -> Game {
        let colors = vec![[0.5; 3]; walls.len()];
        let mats = vec![0; walls.len()];
        let mut g = Game::with_map(seed, Map { half: HALF, walls, colors, mats, decals: Vec::new(), spawns: Vec::new(), ground: None });
        g.wave_t = f32::INFINITY;
        g
    }

    pub fn with_map(seed: u32, map: Map) -> Game {
        let half = map.half;
        let world = Rc::new(Grid::new(map.walls, CELL).with_ground(map.ground));
        let mut nav = Nav::new(&world, half);
        // only spawn where a body can actually walk to the middle of the map
        nav.set_goal([0.0, 0.0, 14.0]);
        let spawns = map.spawns.into_iter().filter(|p| nav.reachable(*p)).collect();
        let mut g = Game {
            role: Role::Solo,
            half,
            nav,
            world,
            colors: map.colors,
            mats: map.mats,
            decals: map.decals,
            boxes: Vec::new(),
            crates: Vec::new(),
            loot: false,
            chime_t: 0.0,
            spawns,
            debris: Vec::new(),
            grenades: Vec::new(),
            blasts: Vec::new(),
            events: Vec::new(),
            players: Vec::new(),
            local: 0,
            bots: Vec::new(),
            next_id: 0,
            wave: 0,
            to_spawn: 0,
            spawn_t: 0.0,
            wave_t: 3.0,
            tracers: Vec::new(),
            shots: Vec::new(),
            time: 0.0,
            tick_n: 0,
            history: Default::default(),
            remote_left: None,
            rng: Rng(seed.max(1)),
        };
        g.add_player(0);
        g
    }

    fn respawn_player(&mut self, i: usize) {
        // by whoever is still standing, so a team keeps together; alone, the best of a few spawn points: the one
        // furthest from every living bot
        let mates: Vec<V3> = self.players.iter().enumerate().filter(|(k, p)| *k != i && p.alive()).map(|(_, p)| p.body.pos).collect();
        let by_mate = mates.first().and_then(|m| self.spawns.iter().min_by(|a, b| (a[0] - m[0]).hypot(a[2] - m[2]).total_cmp(&(b[0] - m[0]).hypot(b[2] - m[2]))).copied());
        // a step or two to the side of it, where there is room and nobody else is standing
        let by_mate = by_mate.map(|s| {
            let free = |p: V3| {
                let b = aabb_at(p, HW, H);
                !self.world.find(&b, |w| overlaps(&b, w)) && mates.iter().all(|m| (m[0] - p[0]).hypot(m[2] - p[2]) > 1.2)
            };
            (0..24)
                .map(|k| {
                    let (r, a) = (1.5 + (k / 8) as f32, k as f32 * PI / 4.0);
                    let (x, z) = (s[0] + r * a.cos(), s[2] + r * a.sin());
                    [x, s[1].max(self.world.ground_at(x, z)), z]
                })
                .find(|&p| free(p) && (p[1] - s[1]).abs() < 0.5)
                .unwrap_or(s)
        });
        let others: Vec<V3> = self.bots.iter().filter(|b| b.alive).map(|b| b.body.pos).collect();
        let at = by_mate.unwrap_or_else(|| self.pick_spawn(&others, f32::INFINITY));
        // looking at the middle of the map
        self.respawn_at(i, at, (-at[0]).atan2(at[2]));
    }

    /// Player `i` starting a new life at `at`, facing `yaw`.
    pub(crate) fn respawn_at(&mut self, i: usize, at: V3, yaw: f32) {
        let loot = self.loot;
        let p = &mut self.players[i];
        p.life += 1;
        p.rng = Rng((p.id as u32 + 1) * 7919 + p.life * 104_729);
        p.body = Body { pos: at, ..Body::default() };
        p.step_y = 0.0;
        p.climb = None;
        p.yaw = yaw;
        p.pitch = 0.0;
        p.hp = 100.0;
        p.stamina = 1.0;
        p.winded = false;
        p.bloom = 0.0;
        p.ammo = full_ammo();
        p.grenades = GRENADES;
        p.throw_t = 0.0;
        p.aiming = false;
        p.aim_t = 0.0;
        p.arc.clear();
        p.shake = 0.0;
        p.swings = 0;
        p.strike_t = 0.0;
        p.stop_t = 0.0;
        p.gore = 0.0;
        p.reserve = full_reserve();
        p.arm(loot);
        p.reload_t = 0.0;
        p.equip_t = 0.0;
        p.ads = 0.0;
    }

    /// Picks a gun for the loadout: a primary for slot 0 or a sidearm for slot 1, whichever it is. Before the first
    /// spawn is played it is carried straight away; otherwise from the next life, so a life is always played with
    /// the guns it started with.
    pub fn pick(&mut self, i: usize) -> bool {
        self.pick_for(self.local, i)
    }

    fn pick_for(&mut self, pi: usize, i: usize) -> bool {
        if i >= WEAPON_COUNT {
            return false;
        }
        let slot = if i < PRIMARIES { 0 } else { 1 };
        let loot = self.loot;
        let p = &mut self.players[pi];
        p.picked[slot] = i;
        // playing for loot the pick waits for a game that isn't
        if p.alive() && !p.in_play && !loot {
            let held = p.weapon == p.loadout[slot];
            p.loadout[slot] = i;
            if held {
                p.weapon = i;
                p.reload_t = 0.0;
                p.ads = 0.0;
            }
        }
        true
    }

    /// Plays for loot or not (see `Game::loot`): from the next life, or straight away before the first is played.
    /// The weapon crates within earshot chiming, now and then, from where each lies as the local player hears it.
    /// Called once a frame, `dt` seconds on, by whoever plays the sound: it changes nothing in the game.
    pub fn chime(&mut self, dt: f32) {
        self.chime_t -= dt;
        if self.chime_t > 0.0 {
            return;
        }
        self.chime_t = CHIME_EVERY;
        let Some(p) = self.players.get(self.local) else { return };
        let (ear, (sy, cy)) = (p.body.pos, p.yaw.sin_cos());
        for &(at, _) in &self.crates {
            let (ex, ez) = (ear[0] - at[0], ear[2] - at[2]);
            let d = ex.hypot(ez).max(0.001);
            if d < HEAR_CHIME {
                self.events.push([ev::CHIME, d, -(ex * cy + ez * sy) / d]);
            }
        }
    }

    pub fn set_loot(&mut self, on: bool) {
        self.loot = on;
        let p = &mut self.players[self.local];
        if p.alive() && !p.in_play {
            p.arm(on);
        }
    }

    /// Switches to the primary (slot 0) or the sidearm (slot 1).
    pub fn select_slot(&mut self, slot: usize) {
        self.select_slot_for(self.local, slot);
    }

    fn select_slot_for(&mut self, pi: usize, slot: usize) {
        if let Some(&w) = self.players[pi].loadout.get(slot) {
            self.select_for(pi, w);
        }
    }

    /// A spawn point at least `far` from everything in `from`, or failing that the best of a few.
    fn pick_spawn(&mut self, from: &[V3], far: f32) -> V3 {
        let reach = self.half - 2.0;
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
        self.select_for(self.local, i);
    }

    fn select_for(&mut self, pi: usize, i: usize) {
        let p = &mut self.players[pi];
        if i >= WEAPON_COUNT || i == p.weapon {
            return;
        }
        p.weapon = i;
        p.reload_t = 0.0;
        p.strike_t = 0.0;
        p.equip_t = EQUIP_TIME;
        p.ads = 0.0;
        self.events.push([ev::EQUIP, 0.0, 0.0]);
    }

    fn wall_distance(&self, o: V3, d: V3) -> f32 {
        self.world.raycast(o, d, RANGE)
    }

    fn hurt_player(&mut self, i: usize, dmg: f32) {
        // on a client the server says how hurt everyone is
        if !self.players[i].alive() || self.role == Role::Mirror {
            return;
        }
        let p = &mut self.players[i];
        p.hp -= dmg;
        p.damage_flash = 1.0;
        p.since_hurt = 0.0;
        self.events.push([ev::HURT, dmg, 0.0]);
        if p.hp <= 0.0 {
            p.deaths += 1;
            p.respawn_t = 4.0;
            self.events.push([ev::DEATH, 0.0, 0.0]);
        }
    }

    /// Nearest thing a shot hits: distance, and the bot (with whether it was the head) if it got there first.
    /// Checked against where the zombies were at tick `seen`, if the server still knows, which is where the one
    /// shooting saw them.
    fn trace_bots(&self, eye: V3, dir: V3, seen: Option<u32>) -> (f32, Option<(usize, bool)>) {
        let mut best = self.wall_distance(eye, dir);
        let mut target = None;
        let then = seen.and_then(|t| self.history.iter().find(|(n, _)| *n == t));
        let boxes: Vec<(usize, Aabb, Aabb)> = match then {
            Some((_, was)) => was.iter().filter_map(|&(id, body, head)| self.bots.iter().position(|b| b.id == id && b.alive).map(|i| (i, body, head))).collect(),
            None => self.bots.iter().enumerate().filter(|(_, b)| b.alive).map(|(i, b)| (i, b.body_box(), b.head_box())).collect(),
        };
        for (i, body, head) in boxes {
            if let Some(t) = ray_aabb(eye, dir, &head) {
                if t < best {
                    best = t;
                    target = Some((i, true));
                }
            }
            if let Some(t) = ray_aabb(eye, dir, &body) {
                if t < best {
                    best = t;
                    target = Some((i, false));
                }
            }
        }
        (best, target)
    }

    /// The server keeping where the zombies are now, for shots fired at them later (see `trace_bots`).
    fn remember_bots(&mut self) {
        let now = self.bots.iter().filter(|b| b.alive).map(|b| (b.id, b.body_box(), b.head_box())).collect();
        self.history.push_back((self.tick_n, now));
        while self.history.len() > HISTORY {
            self.history.pop_front();
        }
    }

    /// A spray of blood droplets from a hit.
    fn blood(&mut self, at: V3, dir: V3, n: usize) {
        if self.role == Role::Server {
            return;
        }
        for _ in 0..n {
            let r = &mut self.rng;
            let vel = [dir[0] * r.range(1.0, 4.0) + r.range(-1.5, 1.5), r.range(0.5, 3.5), dir[2] * r.range(1.0, 4.0) + r.range(-1.5, 1.5)];
            let life = r.range(0.4, 0.9);
            let s = r.range(0.02, 0.04);
            let color = if r.f() < 0.5 { [0.45, 0.03, 0.04] } else { [0.30, 0.02, 0.03] };
            self.debris.push(Debris { pos: at, vel, rot: [0.0; 3], spin: [0.0; 3], half: [s; 3], color, life, glow: 0.0 });
        }
        self.trim_debris();
    }

    fn trim_debris(&mut self) {
        if self.debris.len() > MAX_DEBRIS {
            let extra = self.debris.len() - MAX_DEBRIS;
            self.debris.drain(..extra);
        }
    }

    fn tick_debris(&mut self, dt: f32, world: &Grid) {
        for d in &mut self.debris {
            d.life -= dt;
            d.vel[1] -= GRAVITY * dt;
            let r = d.half[0].max(d.half[1]).max(d.half[2]);
            let mut grounded = false;
            for axis in 0..3 {
                let old = d.pos[axis];
                d.pos[axis] += d.vel[axis] * dt;
                let ground = world.ground_at(d.pos[0], d.pos[2]);
                let floor = axis == 1 && d.pos[1] < ground + r;
                let probe = Aabb { min: [d.pos[0] - r, d.pos[1] - r, d.pos[2] - r], max: [d.pos[0] + r, d.pos[1] + r, d.pos[2] + r] };
                if floor || world.find(&probe, |w| overlaps(&probe, w)) {
                    if floor {
                        d.pos[1] = (ground + r).max(old);
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

    /// Player `pi` firing, at the zombies as they were at tick `seen` if it is a client's shot on the server. On
    /// a client the shot only shows: the server says what it did.
    fn player_fire(&mut self, pi: usize, seen: Option<u32>) {
        let p = &mut self.players[pi];
        let w = p.gun();
        let (eye, fwd, ads, yaw) = (p.eye(), p.forward(), p.ads, p.yaw);
        p.ammo[p.weapon] -= 1;
        p.cooldown = w.delay;
        p.armed = false;
        let spread = p.spread();
        p.bloom = (p.bloom + BLOOM_SHOT).min(1.0);
        p.since_shot = 0.0;
        self.events.push([ev::SHOT, p.weapon as f32, 0.0]);
        let mut outcome = 0.0f32;

        let (sy, cy) = yaw.sin_cos();
        let right = [cy, 0.0, sy];
        let up = cross(right, fwd);
        // the shot is drawn from the muzzle, which sits right/down from the eye until the gun is raised
        let (side, drop) = (0.2 * (1.0 - ads), 0.18 * (1.0 - 0.7 * ads));
        let muzzle = [
            eye[0] + right[0] * side + fwd[0] * 0.6,
            eye[1] - drop + fwd[1] * 0.6,
            eye[2] + right[2] * side + fwd[2] * 0.6,
        ];

        for _ in 0..w.pellets {
            let dir = if spread > 0.0 {
                let r = &mut self.players[pi].rng;
                let (r, a) = (spread * r.f().sqrt(), r.range(0.0, std::f32::consts::TAU));
                let (dx, dy) = (r * a.cos(), r * a.sin());
                unit([
                    fwd[0] + right[0] * dx + up[0] * dy,
                    fwd[1] + right[1] * dx + up[1] * dy,
                    fwd[2] + right[2] * dx + up[2] * dy,
                ])
            } else {
                fwd
            };
            let (dist, target) = self.trace_bots(eye, dir, seen);
            if let Some((i, head)) = target {
                self.blood(along(eye, dir, dist), dir, if head { 10 } else { 6 });
                let pp = self.players[pi].body.pos;
                self.players[pi].hit_marker = 1.0;
                outcome = outcome.max(if head { ev::HEAD } else { ev::HIT });
                let bot = &mut self.bots[i];
                if self.role == Role::Mirror {
                    // flinching at once, as it will on the server
                    bot.hit_t = 1.0;
                    self.tracers.push(Tracer { a: muzzle, b: along(eye, dir, dist), life: 0.06, by: self.players[pi].id });
                    continue;
                }
                bot.hp -= if head { w.head_dmg } else { w.body_dmg };
                bot.hit_t = 1.0;
                if bot.hp <= 0.0 {
                    bot.alive = false;
                    let at = bot.body.pos;
                    let power = (w.body_dmg * w.pellets as f32 / 34.0).sqrt().clamp(0.8, 1.8);
                    // zombies always crumple where they stand; harder kills just spray more blood
                    bot.corpse = true;
                    bot.dead_t = 0.0;
                    bot.face = (pp[0] - at[0]).atan2(-(pp[2] - at[2]));
                    self.players[pi].kills += 1;
                    self.players[pi].kill_t = 1.0;
                    self.blood([at[0], at[1] + if head { 1.6 } else { 1.2 }, at[2]], dir, if head || power > 1.3 { 32 } else { 16 });
                    outcome = ev::KILL;
                }
            }
            let (a, b) = (muzzle, along(eye, dir, dist));
            self.tracers.push(Tracer { a, b, life: 0.06, by: self.players[pi].id });
            if self.role == Role::Server {
                self.shots.push(Shot { by: self.players[pi].id, a, b });
            }
        }

        if outcome > 0.0 {
            self.events.push([outcome, 0.0, 0.0]);
        }

        // recoil kicks the aim up and a little sideways, applied after the shot is traced
        let p = &mut self.players[pi];
        let k = w.kick * (1.0 - 0.5 * ads);
        let kick_y = p.rng.range(-0.4, 0.4) * k;
        p.pitch = (p.pitch + k).min(1.5);
        p.yaw += kick_y;
    }

    /// Player `pi` swinging a melee weapon: it winds up and connects `STRIKE` later (see `strike`).
    fn swing(&mut self, pi: usize) {
        let p = &mut self.players[pi];
        p.cooldown = p.gun().delay;
        p.armed = false;
        p.strike_t = STRIKE;
        p.swings += 1;
        self.events.push([ev::SWING, (p.swings % 2) as f32, 0.0]);
    }

    /// The swing of player `pi` connecting, at the zombies as they were at tick `seen` on the server. The first
    /// `CLEAVE` zombies within reach across the fan it sweeps are struck, nearest first, in the head if it went
    /// through it, and knocked off their feet the way the swing goes; with no one there, a wall within reach takes
    /// the blow. Either way the swing stops dead a moment. On a client it only shows: the server says what it did.
    fn strike(&mut self, pi: usize, seen: Option<u32>) {
        let p = &self.players[pi];
        let w = p.gun();
        let (eye, fwd, from) = (p.eye(), p.forward(), p.body.pos);
        let (sy, cy) = p.yaw.sin_cos();
        let right = [cy, 0.0, sy];
        let up = cross(right, fwd);
        // flat across from the right, or every other swing back the other way and down from high on the left
        let back = p.swings.is_multiple_of(2);
        let going = if back { right } else { [-right[0], 0.0, -right[2]] };
        let mut struck: Vec<(usize, f32, bool)> = Vec::new();
        let mut wall = RANGE;
        const RAYS: usize = 9;
        for k in 0..RAYS {
            let s = k as f32 / (RAYS - 1) as f32 * 2.0 - 1.0;
            let (dx, dy) = (s * FAN.tan(), if back { -0.3 * s } else { 0.0 });
            let dir = unit([fwd[0] + right[0] * dx + up[0] * dy, fwd[1] + right[1] * dx + up[1] * dy, fwd[2] + right[2] * dx + up[2] * dy]);
            match self.trace_bots(eye, dir, seen) {
                (dist, Some((i, head))) if dist <= REACH => match struck.iter_mut().find(|s| s.0 == i) {
                    Some(s) => (s.1, s.2) = (s.1.min(dist), s.2 || head),
                    None => struck.push((i, dist, head)),
                },
                (dist, None) => wall = wall.min(dist),
                _ => {}
            }
        }
        struck.sort_by(|a, b| a.1.total_cmp(&b.1));
        struck.truncate(CLEAVE);

        let mirror = self.role == Role::Mirror;
        let (mut outcome, mut killed) = (0.0f32, false);
        for &(i, _, head) in &struck {
            let b = &self.bots[i];
            let (at, s) = (b.body.pos, b.scale);
            let (ax, az) = (at[0] - from[0], at[2] - from[2]);
            let l = ax.hypot(az).max(1e-3);
            // knocked away from the swing and along the way it goes
            let (px, pz) = (ax / l + 0.7 * going[0], az / l + 0.7 * going[2]);
            let pl = px.hypot(pz).max(1e-3);
            let push = [px / pl, pz / pl];
            let hit_at = [at[0] - ax / l * 0.25 * s, at[1] + if head { 1.6 } else { 1.15 } * s, at[2] - az / l * 0.25 * s];
            self.blood(hit_at, [push[0], 0.0, push[1]], if head { 16 } else { 10 });
            outcome = outcome.max(if head { ev::HEAD } else { ev::HIT });
            let b = &mut self.bots[i];
            b.hit_t = 1.0;
            if mirror {
                continue;
            }
            b.hp -= if head { w.head_dmg } else { w.body_dmg };
            let dead = b.hp <= 0.0;
            // a brute hardly gives
            let heavy = if b.kind == 2 { 0.35 } else { 1.0 };
            // stopped in its tracks and sent the other way, however hard it was coming on
            let k = if dead { 7.5 } else { 5.0 } * heavy;
            b.climb = None;
            b.body.on_ground = false;
            b.body.vel = [push[0] * k, b.body.vel[1].max(if dead { 3.5 } else { 2.2 } * heavy), push[1] * k];
            if dead {
                // down on its back, facing the one who swung
                b.alive = false;
                b.corpse = true;
                b.dead_t = 0.0;
                b.face = (-push[0]).atan2(push[1]);
                killed = true;
                self.players[pi].kills += 1;
                self.players[pi].kill_t = 1.0;
                self.blood(hit_at, [push[0], 0.4, push[1]], 24);
                outcome = ev::KILL;
            }
        }

        let p = &mut self.players[pi];
        if !struck.is_empty() {
            p.hit_marker = 1.0;
            p.stop_t = if killed { KILL_STOP } else { HIT_STOP };
            p.shake = p.shake.max(if killed { 0.5 } else { 0.35 });
            p.gore = (p.gore + 0.15 * struck.len() as f32 + if killed { 0.1 } else { 0.0 }).min(1.0);
            let smack = if killed { 3.0 } else if outcome == ev::HEAD { 2.0 } else { 1.0 };
            self.events.push([ev::SMACK, smack, 0.0]);
            self.events.push([outcome, 0.0, 0.0]);
        } else if wall <= REACH {
            p.stop_t = HIT_STOP;
            p.shake = p.shake.max(0.25);
            self.events.push([ev::SMACK, 0.0, 0.0]);
            // splinters and dust off what it hit
            if self.role != Role::Server {
                let at = along(eye, fwd, (wall - 0.05).max(0.0));
                for _ in 0..8 {
                    let r = &mut self.rng;
                    let vel = [going[0] * r.range(0.5, 2.5) - fwd[0] * r.range(0.5, 2.0), r.range(0.5, 2.5), going[2] * r.range(0.5, 2.5) - fwd[2] * r.range(0.5, 2.0)];
                    let spin = [r.range(-10.0, 10.0), r.range(-10.0, 10.0), r.range(-10.0, 10.0)];
                    let half = [r.range(0.006, 0.015), r.range(0.004, 0.01), r.range(0.006, 0.02)];
                    let g = r.range(0.35, 0.55);
                    self.debris.push(Debris { pos: at, vel, rot: [0.0; 3], spin, half, color: [g, g * 0.9, g * 0.8], life: r.range(0.4, 0.9), glow: 0.0 });
                }
                self.trim_debris();
            }
        }
    }

    /// One step of the game. Playing alone that is the player too, with the keys in `Player::input`; otherwise
    /// each player is stepped on their own with `step_player` first, as their inputs come in.
    pub fn tick(&mut self, dt: f32) {
        self.time += dt;
        self.tick_n += 1;
        let world = self.world.clone();
        if self.role == Role::Solo {
            let p = &mut self.players[0];
            let inp = p.input;
            (p.input.look_dx, p.input.look_dy) = (0.0, 0.0);
            self.step_player(0, inp, dt);
        }
        if self.role != Role::Mirror {
            let goals = self.standing();
            if !goals.is_empty() {
                let zombies: Vec<V3> = self.bots.iter().filter(|b| b.alive).map(|b| b.body.pos).collect();
                self.nav.follow(&goals, &zombies);
            }
            self.tick_respawns(dt);
            self.tick_waves(dt);
        }
        self.tick_grenades(dt, &world);
        if self.role != Role::Mirror {
            self.tick_bots(dt, &world);
        }
        if self.role == Role::Server {
            self.remember_bots();
        }
        self.tick_debris(dt, &world);
        for t in &mut self.tracers {
            t.life -= dt;
        }
        self.tracers.retain(|t| t.life > 0.0);
    }

    /// Those who are down come back: all together once everyone is down (from wave one again), otherwise
    /// when the next wave starts (see `tick_waves`).
    fn tick_respawns(&mut self, dt: f32) {
        for p in self.players.iter_mut().filter(|p| !p.alive()) {
            p.respawn_t -= dt;
        }
        if !self.players.is_empty() && self.players.iter().all(|p| !p.alive() && p.respawn_t <= 0.0) {
            let lobby = !self.waves_on();
            self.reset_waves();
            if lobby {
                self.hold_waves();
            }
            for i in 0..self.players.len() {
                self.respawn_player(i);
            }
        }
    }

    /// One step of player `i` with input `inp`.
    pub fn step_player(&mut self, i: usize, inp: Input, dt: f32) {
        let world = self.world.clone();
        self.tick_player(i, inp, dt, &world);
    }

    fn tick_player(&mut self, pi: usize, inp: Input, dt: f32, world: &Grid) {
        if let Some(i) = inp.pick {
            self.pick_for(pi, i as usize);
        }
        if let Some(s) = inp.select {
            if self.players[pi].alive() {
                self.select_slot_for(pi, s as usize);
            }
        }
        let p = &mut self.players[pi];
        p.hit_marker = (p.hit_marker - dt * 5.0).max(0.0);
        p.damage_flash = (p.damage_flash - dt * 2.0).max(0.0);
        p.kill_t = (p.kill_t - dt * 2.5).max(0.0);
        p.shake = (p.shake - dt * 1.8).max(0.0);
        p.throw_t = (p.throw_t - dt).max(0.0);
        if p.stop_t > 0.0 {
            p.stop_t = (p.stop_t - dt).max(0.0);
        } else {
            p.cooldown = (p.cooldown - dt).max(0.0);
        }
        p.since_shot += dt;
        if p.since_shot > BLOOM_REST {
            p.bloom = (p.bloom - dt * BLOOM_SETTLE).max(0.0);
        }

        if !p.alive() {
            return;
        }
        p.in_play = true;

        let w = p.gun();
        // zombie runs are long, so health comes back when nothing has touched you for a while
        p.since_hurt += dt;
        if p.since_hurt > 5.0 && self.role != Role::Mirror {
            p.hp = (p.hp + 8.0 * dt).min(100.0);
        }
        p.equip_t = (p.equip_t - dt).max(0.0);
        let aim = if inp.ads && !w.melee && p.reload_t <= 0.0 && p.equip_t <= 0.0 { 1.0 } else { 0.0 };
        p.ads += (aim - p.ads).clamp(-8.0 * dt, 8.0 * dt);

        let crash = self.move_player(pi, inp, dt, world);

        let p = &mut self.players[pi];
        if !inp.fire {
            p.armed = true;
        }
        let on_grenade = self.tick_throw(pi, dt, inp.throw);
        let seen = (self.role == Role::Server).then(|| inp.seen.round().max(0.0) as u32);
        let p = &mut self.players[pi];
        if p.strike_t > 0.0 {
            p.strike_t -= dt;
            if p.strike_t <= 0.0 {
                p.strike_t = 0.0;
                self.strike(pi, seen);
            }
        }
        let p = &mut self.players[pi];
        let ammo = p.ammo[p.weapon];
        if on_grenade {
            // the hand is on the grenade: no reloading or shooting
        } else if p.reload_t > 0.0 {
            if p.sprinting {
                // running interrupts a reload, and nothing is lost
                p.reload_t = 0.0;
            } else {
                p.reload_t -= dt;
                if p.reload_t <= 0.0 {
                    if w.drop_mag {
                        // the magazine is swapped in, and what was in the old one is dropped
                        let n = w.mag.min(p.reserve[p.weapon]);
                        p.ammo[p.weapon] = n;
                        p.reserve[p.weapon] -= n;
                    } else {
                        // rounds come from the reserve, and only as many as it has
                        let n = (w.mag - p.ammo[p.weapon]).min(p.reserve[p.weapon]);
                        p.ammo[p.weapon] += n;
                        p.reserve[p.weapon] -= n;
                    }
                }
            }
        } else if !p.sprinting && (inp.reload || ammo == 0) && ammo < w.mag && p.reserve[p.weapon] > 0 {
            p.reload_t = w.reload;
            self.events.push([ev::RELOAD, p.weapon as f32, 0.0]);
        } else if inp.fire && (ammo > 0 || w.melee) && p.cooldown <= 0.0 && p.equip_t <= 0.0 && (w.auto || p.armed) {
            if w.melee {
                self.swing(pi);
            } else {
                self.player_fire(pi, seen);
            }
        }
        if crash > 0.0 {
            self.hurt_player(pi, crash);
        }
        // with the step, so a client playing its player ahead picks up on the same input the server does
        self.tick_boxes(pi);
        self.tick_crates(pi);
    }

    /// Walk over an ammo box standing or walking and a magazine of its rounds goes into the reserves, so far as
    /// they are not full, and one grenade, so far as there is room; running over one leaves it be.
    fn tick_boxes(&mut self, pi: usize) {
        if !self.players[pi].alive() || self.players[pi].sprinting {
            return;
        }
        let full = full_reserve();
        if self.players[pi].reserve == full && self.players[pi].grenades == MAX_GRENADES {
            return;
        }
        let body = aabb_at(self.players[pi].body.pos, HW, H);
        let mut kept = Vec::new();
        for at in std::mem::take(&mut self.boxes) {
            if !overlaps(&body, &aabb_at(at, 0.35, 0.7)) {
                kept.push(at);
            } else {
                for i in 0..WEAPON_COUNT {
                    self.players[pi].reserve[i] = (self.players[pi].reserve[i] + WEAPONS[i].mag).min(full[i]);
                }
                self.players[pi].grenades = (self.players[pi].grenades + 1).min(MAX_GRENADES);
                self.events.push([ev::PICKUP, 0.0, 0.0]);
            }
        }
        self.boxes = kept;
    }

    /// Walk over a weapon crate standing or walking and its gun takes the place of the one carried in its slot,
    /// loaded, with full reserves, and raised. Running over one, or over one with a gun already carried, leaves it be.
    fn tick_crates(&mut self, pi: usize) {
        let p = &self.players[pi];
        if !p.alive() || p.sprinting {
            return;
        }
        let body = aabb_at(p.body.pos, HW, H);
        let Some(k) = self.crates.iter().position(|&(at, gun)| !p.loadout.contains(&gun) && overlaps(&body, &aabb_at(at, 0.45, 0.5))) else {
            return;
        };
        let (_, gun) = self.crates.remove(k);
        let p = &mut self.players[pi];
        p.loadout[if gun < PRIMARIES { 0 } else { 1 }] = gun;
        p.ammo[gun] = WEAPONS[gun].mag;
        p.reserve[gun] = 2 * WEAPONS[gun].mag;
        self.events.push([ev::PICKUP, 0.0, 0.0]);
        self.select_for(pi, gun);
    }

    /// Where player `i` looks and how they move this step: turning, running, jumping, climbing and stamina, all
    /// of it down to `inp` and what they were doing, so a client can play it again from where the server says
    /// they were (see `mirror`). Returns the damage of a hard landing.
    pub(crate) fn move_player(&mut self, pi: usize, inp: Input, dt: f32, world: &Grid) -> f32 {
        let p = &mut self.players[pi];
        let w = p.gun();
        match inp.view {
            Some([yaw, pitch]) => (p.yaw, p.pitch) = (yaw, pitch.clamp(-1.5, 1.5)),
            None => p.look(inp.look_dx, inp.look_dy),
        }

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
        // sprinting while moving spends stamina; once rested it comes back
        let sprinting = inp.sprint && l > 0.0 && !p.winded && p.stamina > 0.0;
        if sprinting {
            p.stamina = (p.stamina - dt / SPRINT_TIME).max(0.0);
            p.since_sprint = 0.0;
            p.winded = p.stamina == 0.0;
        } else {
            p.since_sprint += dt;
            if p.since_sprint > STAMINA_REST {
                p.stamina = (p.stamina + dt / STAMINA_REFILL).min(1.0);
            }
            p.winded &= p.stamina < WINDED_UNTIL;
        }
        p.sprint_t += (if sprinting { 1.0 } else { 0.0 } - p.sprint_t) * (dt * 6.0).min(1.0);
        p.sprinting = sprinting;
        p.land_t = (p.land_t - dt * 3.0).max(0.0);
        let speed = if sprinting { SPRINT } else { WALK } * w.speed * (1.0 - 0.4 * p.ads);
        let (was_ground, fall, was_y) = (p.body.on_ground, -p.body.vel[1], p.body.pos[1]);
        // space in the air near a ledge pulls the player up onto it, or near a window through it, the gun lowered
        // while the hands are busy
        if p.climb.is_none() && !p.body.on_ground && inp.jump {
            p.climb = find_ledge(p.body.pos, p.yaw, LEDGE_REACH, CLIMB_PACE, false, world)
                .or_else(|| find_vault(p.body.pos, p.yaw, LEDGE_REACH, CLIMB_PACE, false, world));
            if p.climb.is_some() {
                self.events.push([ev::JUMP, 0.0, 0.0]);
            }
        }
        if let Some(c) = &mut p.climb {
            c.t += dt;
            let (at, done) = c.at();
            p.body.pos = at;
            p.body.vel = [0.0; 3];
            p.body.on_ground = done;
            p.equip_t = p.equip_t.max(EQUIP_TIME * 0.7);
            if done {
                p.climb = None;
            }
        } else {
            step_body(&mut p.body, [wx * speed, wz * speed], inp.jump, dt, world);
        }
        // a step up or down moves the body at once; the eye stays put and catches up
        p.step_y *= (-STEP_EASE * dt).exp();
        if was_ground && p.body.on_ground {
            p.step_y = (p.step_y - (p.body.pos[1] - was_y)).clamp(-0.8, 0.8);
        }
        let run = (p.body.vel[0].powi(2) + p.body.vel[2].powi(2)).sqrt();
        let mut crash = 0.0;
        if was_ground && !p.body.on_ground && p.body.vel[1] > 0.0 {
            self.events.push([ev::JUMP, 0.0, 0.0]);
        } else if !was_ground && p.body.on_ground && fall > 5.0 {
            self.events.push([ev::LAND, fall, 0.0]);
            p.land_t = p.land_t.max((fall / 12.0).min(1.0));
            let drop = fall * fall / (2.0 * GRAVITY);
            crash = (drop - FALL_SAFE).max(0.0) * FALL_DMG;
        }
        if p.body.on_ground && run > 1.5 {
            p.stride += run * dt;
            p.gait = (p.gait + run * dt) % (2.0 * STRIDE);
            if p.stride > STRIDE {
                p.stride -= STRIDE;
                self.events.push([ev::STEP, run, 0.0]);
            }
        }
        crash
    }

    /// The zombies, each after whoever is nearest. Sounds are heard by the player this machine looks through.
    fn tick_bots(&mut self, dt: f32, world: &Grid) {
        let mut dmg = vec![0.0; self.players.len()];
        let mut heard = Vec::new();
        let mut stuck = Vec::new();
        let (ear, (psy, pcy)) = match self.players.get(self.local) {
            Some(p) => (p.body.pos, p.yaw.sin_cos()),
            None => ([0.0; 3], (0.0, 1.0)),
        };
        let snap: Vec<(V3, bool)> = self.bots.iter().map(|b| (b.body.pos, b.alive)).collect();

        for i in 0..self.bots.len() {
            let (victim, target, player_alive) = match self.nearest_player(self.bots[i].body.pos) {
                Some((k, _)) => (k, self.players[k].body.pos, true),
                None => (0, self.players.first().map_or([0.0; 3], |p| p.body.pos), false),
            };
            if !self.bots[i].alive {
                // one shot off a wall or out of a hop drops to the ground
                let b = &mut self.bots[i];
                b.dead_t += dt;
                b.climb = None;
                if !b.body.on_ground {
                    step_body(&mut b.body, [0.0, 0.0], false, dt, world);
                }
                continue;
            }
            let b = &mut self.bots[i];
            let dx = target[0] - b.body.pos[0];
            let dz = target[2] - b.body.pos[2];
            let dist = (dx * dx + dz * dz).sqrt().max(0.001);
            let (nx, nz) = (dx / dist, dz / dist);
            b.hit_t = (b.hit_t - dt * 3.0).max(0.0);
            // they groan as they come, runners most often, and more often still once they're close
            let (ex, ez) = (ear[0] - b.body.pos[0], ear[2] - b.body.pos[2]);
            let heard_at = ex.hypot(ez).max(0.001);
            let pan = -(ex * pcy + ez * psy) / heard_at;
            b.growl_t -= dt;
            if b.growl_t <= 0.0 {
                let (lo, hi) = [(2.5, 6.0), (1.5, 4.0), (3.0, 7.0)][b.kind as usize];
                b.growl_t = self.rng.range(lo, hi) * if dist < 10.0 { 0.6 } else { 1.0 };
                if heard_at < HEAR_GROWL {
                    heard.push([ev::GROWL, heard_at, b.kind as f32 * 4.0 + pan]);
                }
            }
            // it faces where it is going, and swings round to the player once it is close enough to strike
            let to_player = dx.atan2(-dz);
            b.face = if let Some(c) = &b.climb {
                c.yaw
            } else if b.body.vel[0].hypot(b.body.vel[2]) > 0.4 {
                let going = b.body.vel[0].atan2(-b.body.vel[2]);
                let turn = (to_player - going + 3.0 * PI).rem_euclid(2.0 * PI) - PI;
                going + turn * ((4.0 - dist) / 2.0).clamp(0.0, 1.0)
            } else {
                to_player
            };
            // the head jerks about at random, so it is hard to hold a bead on, and now and then the whole body
            // staggers aside
            b.twitch_t -= dt;
            if b.twitch_t <= 0.0 {
                let ((lo, hi), side) = [((0.3, 1.4), 0.12), ((0.2, 0.8), 0.1), ((0.6, 2.0), 0.07)][b.kind as usize];
                b.twitch_t = self.rng.range(lo, hi);
                b.twitch = if self.rng.f() < 0.3 {
                    [0.0; 3]
                } else {
                    [self.rng.range(-side, side), -self.rng.range(0.0, 0.08), self.rng.range(-0.03, 0.07)]
                };
                if self.rng.f() < 0.35 {
                    b.stagger = self.rng.range(-1.4, 1.4);
                }
            }
            let jerk = 1.0 - (-22.0 * dt).exp();
            for a in 0..3 {
                b.head[a] += (b.twitch[a] - b.head[a]) * jerk;
            }
            b.stagger *= (-2.5 * dt).exp();
            let (ax, az) = (b.body.pos[0] - b.anchor[0], b.body.pos[2] - b.anchor[2]);
            if ax * ax + az * az > 1.0 || dist < STUCK_FAR || !player_alive {
                b.anchor = b.body.pos;
                b.stuck_t = 0.0;
            } else {
                b.stuck_t += dt;
                if b.stuck_t > STUCK_T {
                    stuck.push(i);
                }
            }
            // it walks at the player until it is close enough to strike, flinching when hit
            let in_reach = (-STRIKE_DOWN..STRIKE_UP).contains(&(target[1] - b.body.pos[1]));
            let go = if player_alive && (dist > 1.2 || !in_reach) { b.speed } else { 0.0 };
            let go = go * (1.0 - 0.8 * b.hit_t);

            if dist > LOD_RADIUS && player_alive {
                // out of sight and out of reach: no physics, no shooting, just follow the route
                b.climb = None;
                if let Some((to, through)) = self.nav.next_step(b.body.pos) {
                    let (rx, rz) = (to[0] - b.body.pos[0], to[2] - b.body.pos[2]);
                    let l = (rx * rx + rz * rz).sqrt().max(1e-4);
                    let moved = (b.speed * dt).min(l);
                    // it hops up as soon as it starts a rise and comes down only once it arrives, so it is never inside the step
                    let y = if to[1] > b.body.pos[1] || l - moved < 0.3 { to[1] } else { b.body.pos[1] };
                    let at = [b.body.pos[0] + rx / l * moved, y, b.body.pos[2] + rz / l * moved];
                    // a step that grazes a corner slides along one axis instead, and one under an overhang on the
                    // way down off a ledge drops at once: pushed back to the middle, it would try the same step forever.
                    // Through a window, it goes through
                    let clear = |at: V3| {
                        let probe = aabb_at(at, HW, H);
                        through || !world.find(&probe, |w| overlaps(&probe, w))
                    };
                    let tries = [at, [at[0], to[1], at[2]], [at[0], y, b.body.pos[2]], [b.body.pos[0], y, at[2]]];
                    if let Some(at) = tries.into_iter().find(|&t| clear(t)) {
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

            // walk straight at the player when nothing from knee height to the top of the head is in the way
            // across the body's width (lower cover gets jumped), otherwise follow the route around it: a post or
            // a tree trunk the middle of the body would miss still snags a shoulder. Someone higher up than a hop is
            // followed by the route too: seen over the edge of a flight or a roof, they would only be clawed at
            // from below. So is someone the route leads away from: seen over a parapet from the top of a stair,
            // a straight line would step back down it, out of sight again, and up and down it would go for ever
            let step = if b.climb.is_none() { self.nav.next_step(b.body.pos) } else { None };
            let (next, through) = (step.map(|s| s.0), step.is_some_and(|s| s.1));
            if b.body.on_ground {
                let to_player = [dx, target[1] - b.body.pos[1], dz];
                let reach = len(to_player).max(0.001);
                let d = [to_player[0] / reach, to_player[1] / reach, to_player[2] / reach];
                let side = [-d[2] / d[0].hypot(d[2]).max(1e-4) * HW * 0.9, d[0] / d[0].hypot(d[2]).max(1e-4) * HW * 0.9];
                let along = next.map_or(true, |to| (to[0] - b.body.pos[0]) * dx + (to[2] - b.body.pos[2]) * dz > 0.0);
                b.direct = target[1] - b.body.pos[1] < 1.1
                    && (along || dist < 2.0)
                    && [0.6, 1.2, 1.7].iter().all(|&y| {
                        [-1.0, 0.0, 1.0].iter().all(|&s| {
                            let o = [b.body.pos[0] + side[0] * s, b.body.pos[1] + y, b.body.pos[2] + side[1] * s];
                            world.raycast(o, d, reach) >= reach
                        })
                    });
            }
            if let Some(c) = &mut b.climb {
                c.t += dt;
                let (at, done) = c.at();
                b.body.pos = at;
                b.body.vel = [0.0; 3];
                b.body.on_ground = done;
                if done {
                    b.climb = None;
                }
            } else {
                // a climb goes from the spot the route has it on, and the route only knows metre cells: on a fence
                // across two of them it may be at the far end of the wrong one, and heading straight for the top of
                // the climb from there takes it in under a tree's canopy, so it goes along into the right one first
                let climb_to = next.filter(|to| !b.direct && !through && to[1] - b.body.pos[1] > 0.65);
                let near = |to: &V3| (to[0] - b.body.pos[0]).hypot(to[2] - b.body.pos[2]) < 1.5;
                let from = climb_to.filter(|to| !near(to)).and_then(|_| self.nav.from_cell(b.body.pos));
                let off = from.map(|c| [c[0] + (b.body.pos[0] - c[0]).clamp(-0.35, 0.35), c[1] + (b.body.pos[2] - c[1]).clamp(-0.35, 0.35)]);
                let off = off.filter(|o| (o[0] - b.body.pos[0]).hypot(o[1] - b.body.pos[2]) > 0.05);
                let mut tv = match next {
                    Some(to) if !b.direct => {
                        let to = match off {
                            Some(o) => [o[0], b.body.pos[1], o[1]],
                            None if b.body.on_ground && !through => look_ahead(&self.nav, world, b.body.pos, to),
                            None => to,
                        };
                        let (rx, rz) = (to[0] - b.body.pos[0], to[2] - b.body.pos[2]);
                        let l = (rx * rx + rz * rz).sqrt().max(1e-4);
                        [rx / l * go, rz / l * go]
                    }
                    // out in the open it lurches from side to side; on a route it keeps to the route, which may
                    // be a narrow stair
                    _ => {
                        let st = if dist > 2.5 && go > 0.0 { b.stagger } else { 0.0 };
                        [nx * go - nz * st, nz * go + nx * st]
                    }
                };
                // push apart from neighbours so a horde doesn't stack into one body; only those level with it: one
                // up on a fence isn't shouldered off it by the crowd at its foot
                for (j, (pj, aj)) in snap.iter().enumerate() {
                    let (ox, oz) = (b.body.pos[0] - pj[0], b.body.pos[2] - pj[2]);
                    let d = (ox * ox + oz * oz).sqrt();
                    if j != i && *aj && d < 0.9 && d > 0.001 && (b.body.pos[1] - pj[1]).abs() < 1.2 {
                        tv[0] += ox / d * (0.9 - d) * 4.0;
                        tv[1] += oz / d * (0.9 - d) * 4.0;
                    }
                }
                // up on something (a fence, a hedge) with the climb from it not yet found, it keeps still while it
                // looks: walking on at the edge would take it off what it is on and back to the ground
                let foot = climb_to.filter(|to| near(to) || off.is_none() && (to[0] - b.body.pos[0]).hypot(to[2] - b.body.pos[2]) < 2.2);
                if foot.is_some() && b.balk_t > 0.0 && b.body.pos[1] > world.ground_at(b.body.pos[0], b.body.pos[2]) + 0.3 {
                    tv = [0.0; 2];
                }
                let blocked = step_body(&mut b.body, tv, false, dt, world);
                // a route up a ledge too tall to step or hop: once at its foot, it pulls itself up. The route only
                // knows metre cells, so where the edge can't be got over facing the way it goes (a narrow eave seen
                // end on from a hedge under it), it tries the other ways round for an edge up to the same height. Or
                // up onto what the route takes it to be on already: it may be beside the hedge the climb starts from
                // a route through a window: once at the wall, in through it
                if let Some(to) = next.filter(|_| through && !b.direct && b.body.on_ground) {
                    let yaw = (to[0] - b.body.pos[0]).atan2(-(to[2] - b.body.pos[2]));
                    b.climb = find_vault(b.body.pos, yaw, BOT_REACH, BOT_CLIMB_PACE, true, world);
                }
                if let Some(to) = foot {
                    let yaw = (to[0] - b.body.pos[0]).atan2(-(to[2] - b.body.pos[2]));
                    let on = self.nav.standing(b.body.pos).filter(|h| h - b.body.pos[1] > LEDGE_LOW);
                    let wanted = |c: &Climb| (c.to[1] - to[1]).abs() < 0.15 || on.is_some_and(|h| (c.to[1] - h).abs() < 0.15);
                    b.climb = find_ledge(b.body.pos, yaw, BOT_REACH, BOT_CLIMB_PACE, true, world).or_else(|| {
                        (1..8).find_map(|k| find_ledge(b.body.pos, yaw + k as f32 * PI / 4.0, BOT_REACH, BOT_CLIMB_PACE, true, world).filter(wanted))
                    });
                    b.balk_t = if b.climb.is_some() { 0.0 } else { b.balk_t + dt };
                    if b.balk_t > BALK && b.body.on_ground && to[1] - b.body.pos[1] <= BOT_REACH {
                        b.climb = scramble(b.body.pos, to, world);
                        b.balk_t = 0.0;
                    }
                } else {
                    // only slowly forgotten: a route can flicker between the climb and a step along the foot of it
                    b.balk_t = (b.balk_t - dt * 0.25).max(0.0);
                }
                if b.climb.is_none() && blocked && b.body.on_ground && !through {
                    b.body.vel[1] = JUMP;
                }
            }

            b.swing_t = (b.swing_t - dt * 2.5).max(0.0);
            b.fire_t -= dt;
            if player_alive && b.climb.is_none() && dist < 1.7 && in_reach && b.fire_t <= 0.0 {
                b.fire_t = 0.9;
                b.swing_t = 1.0;
                dmg[victim] += if b.kind == 2 { 30.0 } else { 12.0 };
                heard.push([ev::SNARL, b.kind as f32, pan]);
            }
        }
        self.events.extend(heard);
        for i in stuck {
            let at = self.zombie_spawn();
            let b = &mut self.bots[i];
            b.body = Body { pos: at, ..Body::default() };
            b.climb = None;
            b.anchor = at;
            b.stuck_t = 0.0;
        }
        for (i, dmg) in dmg.into_iter().enumerate() {
            if dmg > 0.0 {
                self.hurt_player(i, dmg);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::block;

    /// An empty walled arena.
    pub(super) fn arena() -> Game {
        Game::with_walls(1, map::perimeter())
    }

    pub(super) fn idle(g: &mut Game, secs: f32) {
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
        g.players[0].yaw = 0.0; // facing -z
        g.players[0].input.w = true;
        idle(&mut g, 8.0);
        assert!(g.players[0].body.pos[2] > -HALF + HW - 0.01, "walked through the wall: {:?}", g.players[0].body.pos);
    }

    #[test]
    fn long_falls_hurt_and_the_clock_tower_kills() {
        // dropped from the top of a jump off: a one-storey roof, a two-storey one, the clock tower's balcony
        let peak = JUMP * JUMP / (2.0 * GRAVITY);
        for (from, lo, hi) in [(3.3 + peak, 100.0, 100.0), (6.6 + peak, 70.0, 85.0), (21.9 + peak, f32::MIN, 0.0)] {
            let mut g = arena();
            g.bots.clear();
            g.players[0].body.pos = [0.0, from, 0.0];
            g.players[0].body.on_ground = false;
            idle(&mut g, 3.0);
            let hp = g.players[0].hp;
            assert!((lo..=hi).contains(&hp), "a fall from {from} left {hp} health");
        }
    }

    #[test]
    fn zombies_spawn_in_the_open_and_away_from_the_player() {
        let mut g = Game::new(3);
        for _ in 0..200 {
            g.spawn_zombie();
            let p = g.bots.last().unwrap().body.pos;
            let probe = aabb_at(p, HW, H);
            assert!(!g.world.find(&probe, |w| overlaps(&probe, w)), "spawned in a wall: {p:?}");
            let d = ((p[0] - g.players[0].body.pos[0]).powi(2) + (p[2] - g.players[0].body.pos[2]).powi(2)).sqrt();
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
        g.players[0].body.pos = [20.0, 0.0, 15.0];
        g.bots.push(Bot::zombie([20.0, 0.0, -15.0], 0.0));
        let mut closest = f32::MAX;
        // once it gets there it claws the player down, and the restart clears it away
        for _ in 0..(40.0 / STEP) as usize {
            g.tick(STEP);
            let Some(z) = g.bots.first() else { break };
            let (b, p) = (z.body.pos, g.players[0].body.pos);
            closest = closest.min(((b[0] - p[0]).powi(2) + (b[2] - p[2]).powi(2)).sqrt());
        }
        assert!(closest < 8.0, "never got round the wall, closest {closest}");
    }

    #[test]
    fn far_bots_glide_without_ever_entering_a_wall() {
        for seed in 1..4 {
            let mut g = Game::new(seed);
            g.players[0].body.pos = [0.0, 0.0, 16.0];
            g.players[0].hp = 1e6;
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
        g.players[0].body.pos = [-14.0, 0.0, -11.0];
        g.players[0].yaw = 0.0; // facing -z, towards it
        g.players[0].input.w = true;
        g.players[0].input.jump = true;
        idle(&mut g, 1.5);
        assert!(g.players[0].body.pos[1] >= 0.99, "did not land on the platform: {:?}", g.players[0].body.pos);
    }

    #[test]
    fn three_body_shots_kill_a_walker() {
        let mut g = arena();
        bot_ahead(&mut g);
        let before = g.players[0].kills;
        for _ in 0..3 {
            g.players[0].cooldown = 0.0;
            g.player_fire(0, None);
        }
        assert_eq!(g.players[0].kills, before + 1);
        assert!(!g.bots[0].alive);
    }

    /// A walker 2 m straight ahead (yaw 0 faces -z), aimed at the torso, below the head box.
    fn bot_ahead(g: &mut Game) {
        bot_ahead_zombie(g, 0);
    }

    #[test]
    fn sniper_one_shots_and_shotgun_kills_up_close() {
        for gun in [2, 3] {
            let mut g = arena();
            bot_ahead(&mut g);
            g.select(gun);
            g.players[0].cooldown = 0.0;
            g.player_fire(0, None);
            assert!(!g.bots[0].alive, "{} did not kill", WEAPONS[gun].name);
        }
    }

    #[test]
    fn semi_auto_needs_a_fresh_click_and_ammo_is_per_gun() {
        let mut g = arena();
        g.bots.clear();
        g.select(3);
        idle(&mut g, 0.5);
        g.players[0].input.fire = true;
        idle(&mut g, 3.0);
        assert_eq!(g.players[0].ammo[3], WEAPONS[3].mag - 1, "held trigger kept firing a bolt action");
        g.select(0);
        assert_eq!(g.players[0].ammo[0], WEAPONS[0].mag);
        assert!(g.players[0].equip_t > 0.0);
    }

    #[test]
    fn cannot_fire_while_raising_a_gun() {
        let mut g = arena();
        g.bots.clear();
        g.select(1);
        g.players[0].input.fire = true;
        idle(&mut g, EQUIP_TIME * 0.5);
        assert_eq!(g.players[0].ammo[1], WEAPONS[1].mag);
    }

    #[test]
    fn a_kill_drops_the_zombie_in_a_spray_of_blood_that_settles_and_vanishes() {
        let mut g = arena();
        bot_ahead(&mut g);
        g.select(3);
        g.players[0].cooldown = 0.0;
        g.player_fire(0, None);
        assert!(!g.bots[0].alive && g.bots[0].corpse);
        assert!(g.debris.len() >= 16, "only {} drops", g.debris.len());
        assert!(g.players[0].kill_t > 0.9);
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
        g.players[0].cooldown = 0.0;
        g.player_fire(0, None);
        let codes: Vec<f32> = g.events.iter().map(|e| e[0]).collect();
        assert!(codes.contains(&ev::SHOT) && codes.contains(&ev::HIT), "{codes:?}");
        g.events.clear();
        g.select(1);
        assert_eq!(g.events[0][0], ev::EQUIP);
        g.events.clear();
        g.bots.clear();
        g.players[0].body.pos = [5.0, 0.0, 16.0]; // clear of the low cover in front of the spawn
        g.players[0].input.w = true;
        idle(&mut g, 1.5);
        assert!(g.events.iter().any(|e| e[0] == ev::STEP), "no footsteps while running");
    }

    #[test]
    fn zombies_groan_as_they_come_and_snarl_when_they_strike() {
        for kind in 0..3 {
            let mut g = arena();
            bot_ahead_zombie(&mut g, kind);
            g.events.clear();
            idle(&mut g, 8.0);
            let growl = g.events.iter().find(|e| e[0] == ev::GROWL).unwrap_or_else(|| panic!("kind {kind} never groaned"));
            assert_eq!((growl[2] / 4.0).round() as u8, kind);
            assert!((growl[2] - kind as f32 * 4.0).abs() <= 1.0, "pan out of range: {}", growl[2]);
            assert!(g.events.iter().any(|e| e[0] == ev::SNARL && e[1] == kind as f32), "kind {kind} struck in silence");
        }
    }

    #[test]
    fn walking_climbs_a_ledge_but_not_a_wall() {
        let mut walls = map::perimeter();
        walls.push(block(0.0, -5.0, 6.0, 0.3, 2.0)); // a kerb
        walls.push(block(10.0, -5.0, 6.0, 1.0, 2.0)); // too tall to step onto
        let mut g = Game::with_walls(1, walls);
        g.bots.clear();
        g.players[0].input.w = true;
        g.players[0].yaw = 0.0;
        g.players[0].body.pos = [0.0, 0.0, 0.0];
        idle(&mut g, 1.5);
        assert!(g.players[0].body.pos[2] < -5.0, "the kerb stopped the player: {:?}", g.players[0].body.pos);
        g.players[0].body.pos = [10.0, 0.0, 0.0];
        idle(&mut g, 1.5);
        assert!(g.players[0].body.pos[2] > -3.9, "walked up a 1 m wall: {:?}", g.players[0].body.pos);
    }

    #[test]
    fn the_player_spawns_on_open_ground_facing_the_middle() {
        for (map, seed) in (0..map::MAPS).flat_map(|m| (1..6).map(move |s| (m, s))) {
            let g = Game::on(map, seed);
            let b = aabb_at(g.players[0].body.pos, HW, H);
            assert!(!g.world.find(&b, |w| overlaps(&b, w)), "map {map} seed {seed}: spawned in a wall");
            let f = g.players[0].forward();
            let to_mid = [-g.players[0].body.pos[0], -g.players[0].body.pos[2]];
            assert!(f[0] * to_mid[0] + f[2] * to_mid[1] > 0.0, "map {map} seed {seed}: facing away from the middle");
        }
    }

    #[test]
    fn a_hill_is_walked_up_with_the_feet_on_it_and_a_cliff_stops_the_walk() {
        // flat to x = 5, a slope rising 0.4 a metre to x = 20, then a cliff rising 3 a metre
        let t = crate::terrain::Terrain::new(HALF, |x, _| if x < 5.0 { 0.0 } else if x < 20.0 { (x - 5.0) * 0.4 } else { 6.0 + (x - 20.0) * 3.0 });
        let walls = map::perimeter();
        let n = walls.len();
        let mut g = Game::with_map(1, Map { half: HALF, walls, colors: vec![[0.5; 3]; n], mats: vec![0; n], decals: Vec::new(), spawns: Vec::new(), ground: Some(t) });
        g.bots.clear();
        g.wave_t = f32::INFINITY;
        g.players[0].body.pos = [0.0, 0.0, 0.0];
        g.players[0].yaw = std::f32::consts::FRAC_PI_2;
        g.players[0].input.w = true;
        let mut lowest = 0.0f32;
        for _ in 0..(8.0 / STEP) as usize {
            g.tick(STEP);
            let p = g.players[0].body.pos;
            lowest = lowest.min(p[1] - g.world.ground_at(p[0], p[2]));
        }
        let p = g.players[0].body.pos;
        assert!((17.5..20.5).contains(&p[0]), "walked to {p:?}, not up the slope to the foot of the cliff");
        assert!((p[1] - g.world.ground_at(p[0], p[2])).abs() < 0.05, "the feet are off the ground at {p:?}");
        assert!(lowest > -0.01, "the feet sank {lowest} into the slope");
        // and a shot along the slope hits the cliff, while one back down over it doesn't
        assert!(g.world.raycast([10.0, 5.0, 0.0], [1.0, 0.0, 0.0], 50.0) < 11.0);
        assert_eq!(g.world.raycast([10.0, 5.0, 0.0], [-1.0, 0.0, 0.0], 50.0), 50.0);
    }

    /// A game on bare ground shaped by `f`, the player at the middle facing +x and walking with jump held down.
    fn hopping_on(f: impl Fn(f32, f32) -> f32) -> Game {
        let t = crate::terrain::Terrain::new(HALF, f);
        let walls = map::perimeter();
        let n = walls.len();
        let mut g = Game::with_map(1, Map { half: HALF, walls, colors: vec![[0.5; 3]; n], mats: vec![0; n], decals: Vec::new(), spawns: Vec::new(), ground: Some(t) });
        g.bots.clear();
        g.wave_t = f32::INFINITY;
        g.players[0].hp = 1e6;
        g.players[0].body.pos = [0.0, 0.0, 0.0];
        g.players[0].yaw = std::f32::consts::FRAC_PI_2;
        g.players[0].input.w = true;
        g.players[0].input.jump = true;
        g
    }

    #[test]
    fn a_mountainside_too_steep_to_walk_is_not_hopped_up_either() {
        // flat to x = 4, then rising 1.2 a metre: steeper than a body walks (from a point of the terrain's grid,
        // so the first square of it is as steep as the rest)
        let mut g = hopping_on(|x, _| (x - 4.0).max(0.0) * 1.2);
        let mut top = 0.0f32;
        for _ in 0..(10.0 / STEP) as usize {
            g.tick(STEP);
            top = top.max(g.players[0].body.pos[1]);
        }
        assert!(top < 1.5, "hopped {top} m up the mountainside, to {:?}", g.players[0].body.pos);
    }

    #[test]
    fn a_short_steep_rise_can_still_be_jumped_up() {
        // a bank a metre high, rising over 0.8 m, flat on top
        let mut g = hopping_on(|x, _| ((x - 5.0) / 0.8).clamp(0.0, 1.0));
        for i in 0..(3.0 / STEP) as usize {
            g.players[0].input.jump = i < (2.0 / STEP) as usize;
            g.tick(STEP);
        }
        let p = g.players[0].body.pos;
        assert!(p[0] > 7.0 && (p[1] - 1.0).abs() < 0.1, "didn't get up the bank: at {p:?}");
    }

    fn zombie_game() -> Game {
        let mut g = arena();
        g.wave_t = 0.0;
        g
    }

    /// On real maps, with the player standing still and each zombie dropped as it gets to them, every wave
    /// finishes: no zombie is caught on the map, or spawns so far off that it takes a minute to turn up.
    #[test]
    fn every_zombie_of_a_wave_turns_up() {
        for (map, seed) in (0..map::MAPS).flat_map(|m| [1, 7, 42].map(move |s| (m, s))) {
            let mut g = Game::on(map, seed);
            g.wave_t = 0.0;
            let at = g.players[0].body.pos;
            let mut born: Vec<Option<f32>> = Vec::new();
            while g.time < 150.0 {
                g.players[0].hp = 100.0;
                g.players[0].body.pos = at;
                g.tick(STEP);
                born.resize(g.bots.len(), None);
                for (i, b) in g.bots.iter_mut().enumerate() {
                    if !b.alive {
                        born[i] = None;
                        continue;
                    }
                    let t0 = *born[i].get_or_insert(g.time);
                    assert!(g.time - t0 < 45.0, "map {map} seed {seed}: a zombie still hasn't come after 45s, at {:?} (the player is at {at:?})", b.body.pos);
                    let (dx, dz) = (b.body.pos[0] - at[0], b.body.pos[2] - at[2]);
                    if dx * dx + dz * dz < 2.5 * 2.5 {
                        b.alive = false;
                    }
                }
            }
            assert!(g.wave >= 3, "map {map} seed {seed}: only got to wave {} in 150s", g.wave);
        }
    }

    #[test]
    fn zombies_arrive_in_waves_and_claw() {
        let mut g = zombie_game();
        for _ in 0..(40.0 / STEP) as usize {
            g.players[0].hp = 1e6;
            g.tick(STEP);
        }
        assert_eq!(g.wave, 1);
        assert!(g.bots.iter().any(|b| b.alive));
        assert!(g.events.iter().any(|e| e[0] == ev::SNARL), "no zombie ever reached the player");
        assert!(g.events.iter().any(|e| e[0] == ev::WAVE));
    }

    #[test]
    fn the_next_wave_starts_after_the_last_zombie_dies_and_is_bigger() {
        let mut g = zombie_game();
        g.players[0].hp = 1e6;
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
        g.hurt_player(0, 1e9);
        assert!(!g.players[0].alive());
        idle(&mut g, 4.2);
        assert!(g.players[0].alive());
        assert_eq!(g.wave, 0);
        assert!(g.bots.is_empty());
    }

    #[test]
    fn a_pick_is_carried_at_once_before_playing_and_from_the_next_life_after() {
        let mut g = arena();
        g.bots.clear();
        assert!(g.pick(2));
        assert_eq!((g.players[0].weapon, g.players[0].loadout), (2, [2, PISTOL]));
        // a sidearm goes in the second slot, and the primary stays in hand
        assert!(g.pick(PISTOL + 1));
        assert_eq!((g.players[0].weapon, g.players[0].loadout), (2, [2, PISTOL + 1]));
        assert!(!g.pick(WEAPON_COUNT));
        idle(&mut g, 0.1);
        // picked while alive: carried from the next life
        assert!(g.pick(1));
        assert!(g.pick(PISTOL));
        assert_eq!(g.players[0].loadout, [2, PISTOL + 1], "changed guns mid-life");
        g.select_slot(1);
        assert_eq!(g.players[0].weapon, PISTOL + 1);
        g.hurt_player(0, 1e9);
        assert!(g.pick(3));
        assert_eq!(g.players[0].loadout[0], 2, "the pick should wait for the respawn");
        idle(&mut g, 4.2);
        assert!(g.players[0].alive());
        assert_eq!((g.players[0].weapon, g.players[0].loadout), (3, [3, PISTOL]));
    }

    #[test]
    fn primaries_and_sidearms_are_where_the_loadout_expects_them() {
        assert!(WEAPONS[..PRIMARIES].iter().all(|w| w.class != "Pistol" && !w.melee));
        assert!(WEAPONS[PRIMARIES..].iter().all(|w| w.class == "Pistol" || w.melee));
        assert_eq!(WEAPONS[PISTOL].name, "Pistol");
        assert!(WEAPONS[BAT].melee && WEAPONS[BAT].mag == 0);
        // a Mosin head shot does most of a brute; the double barrel empties in two
        assert!(WEAPONS[7].head_dmg >= 200.0);
        assert_eq!(WEAPONS[6].mag, 2);
    }

    #[test]
    fn a_brute_takes_more_shots_than_a_walker() {
        let mut g = zombie_game();
        bot_ahead_zombie(&mut g, 2);
        let mut shots = 0;
        while g.bots[0].alive && shots < 20 {
            g.players[0].cooldown = 0.0;
            g.players[0].ammo[0] = 30;
            g.player_fire(0, None);
            shots += 1;
        }
        assert!(shots >= 6, "a brute died in {shots} shots");
    }

    #[test]
    fn sprint_runs_out_leaves_you_winded_and_comes_back() {
        let mut g = arena();
        g.wave_t = f32::INFINITY;
        g.players[0].input.w = true;
        g.players[0].input.sprint = true;
        idle(&mut g, SPRINT_TIME / 2.0);
        let s = g.players[0].stamina;
        assert!((0.4..0.6).contains(&s), "half the sprint left {s}");
        idle(&mut g, SPRINT_TIME / 2.0 + 0.5);
        assert_eq!(g.players[0].stamina, 0.0);
        assert!(g.players[0].winded);
        // still holding sprint: walking pace, and the bar refills
        let z = g.players[0].body.pos[2];
        idle(&mut g, 1.0);
        assert!(z - g.players[0].body.pos[2] < WALK * 1.05, "sprinted while winded");
        g.players[0].input = Input::default();
        idle(&mut g, WINDED_UNTIL * STAMINA_REFILL);
        assert!(!g.players[0].winded, "still winded on {}", g.players[0].stamina);
        idle(&mut g, STAMINA_REFILL);
        assert_eq!(g.players[0].stamina, 1.0);
    }

    #[test]
    fn hip_fire_blooms_while_held_and_settles_after() {
        let mut g = arena();
        g.wave_t = f32::INFINITY;
        let hip = g.players[0].gun().spread;
        g.players[0].input.fire = true;
        idle(&mut g, 0.7);
        assert!(g.players[0].spread() > hip * (1.0 + BLOOM_MAX * 0.95), "held carbine only spread {}", g.players[0].spread());
        g.players[0].input.fire = false;
        idle(&mut g, BLOOM_REST + 1.0 / BLOOM_SETTLE);
        assert_eq!(g.players[0].spread(), hip, "never settled");

        // aimed, a held trigger stays tight
        g.players[0].input.ads = true;
        idle(&mut g, 0.5);
        g.players[0].input.fire = true;
        idle(&mut g, 0.7);
        assert!((g.players[0].spread() - g.players[0].gun().ads_spread).abs() < 1e-5, "aimed spread {}", g.players[0].spread());

        // a sniper has settled again by the time it can fire
        let mut g = arena();
        g.wave_t = f32::INFINITY;
        g.pick(3);
        g.players[0].input.fire = true;
        idle(&mut g, 0.05);
        g.players[0].input.fire = false;
        idle(&mut g, WEAPONS[3].delay);
        assert_eq!(g.players[0].spread(), WEAPONS[3].spread);
    }

    /// The arena with one box `h` high across the way, its face 0.5 m in front of a player facing it.
    fn ledge_ahead(h: f32) -> Game {
        let mut walls = map::perimeter();
        walls.push(block(0.0, -3.5, 4.0, h, 2.0));
        let mut g = Game::with_walls(1, walls);
        g.bots.clear();
        g.players[0].body.pos = [0.0, 0.0, -1.6];
        g.players[0].yaw = 0.0;
        g
    }

    #[test]
    fn space_in_the_air_pulls_the_player_up_onto_a_ledge() {
        let mut g = ledge_ahead(2.0);
        g.players[0].input.jump = true;
        idle(&mut g, 0.1);
        g.players[0].input.jump = false;
        idle(&mut g, 1.5);
        let p = g.players[0].body.pos;
        assert!((p[1] - 2.0).abs() < 0.05 && p[2] < -2.5, "ended at {p:?}, not on top of the box");
        assert!(g.players[0].body.on_ground);
    }

    #[test]
    fn a_ledge_out_of_reach_or_without_space_is_not_climbed() {
        // a storey is too high to reach even at the top of a jump
        let mut g = ledge_ahead(3.3);
        g.players[0].input.jump = true;
        idle(&mut g, 2.0);
        assert!(g.players[0].body.pos[1] < 1.5, "climbed a 3.3 m wall: {:?}", g.players[0].body.pos);
        // walking into a crate without jumping only bumps into it
        let mut g = ledge_ahead(1.0);
        g.players[0].input.w = true;
        idle(&mut g, 1.5);
        assert!(g.players[0].body.pos[1] < 0.01, "got onto the crate without jumping: {:?}", g.players[0].body.pos);
    }

    #[test]
    fn a_ledge_is_caught_falling_past_it() {
        // short of a roof after a jump: falling with its edge at head height
        let mut g = ledge_ahead(3.5);
        g.players[0].body.pos[1] = 2.2;
        g.players[0].body.vel[1] = -4.0;
        g.players[0].body.on_ground = false;
        g.players[0].input.jump = true;
        idle(&mut g, 0.1);
        g.players[0].input.jump = false;
        idle(&mut g, 1.5);
        let p = g.players[0].body.pos;
        assert!((p[1] - 3.5).abs() < 0.05, "fell to {p:?} instead of pulling up");
        assert_eq!(g.players[0].hp, 100.0);
    }

    #[test]
    fn a_ledge_is_not_climbed_through_a_wall() {
        // a tall wall with an upper floor behind it, its top in reach of a player high up beside the wall
        let mut walls = map::perimeter();
        walls.push(block(0.0, -2.3, 4.0, 7.0, 0.4));
        walls.push(Aabb { min: [-2.0, 3.0, -6.0], max: [2.0, 3.3, -2.5] });
        let mut g = Game::with_walls(1, walls);
        g.bots.clear();
        g.players[0].body.pos = [0.0, 2.0, -1.6];
        g.players[0].body.on_ground = false;
        g.players[0].yaw = 0.0;
        g.players[0].input.jump = true;
        idle(&mut g, 1.5);
        assert!(g.players[0].body.pos[2] > -2.1, "went through the wall to {:?}", g.players[0].body.pos);
    }

    /// A wall 0.4 thick across the way at z = -3.5, with a hole in it `w` wide and `tall` from a sill `sill` up.
    fn hole_in(w: f32, sill: f32, tall: f32) -> Vec<Aabb> {
        let (z0, z1) = (-3.7, -3.3);
        let mut walls = map::perimeter();
        walls.push(Aabb { min: [-4.0, 0.0, z0], max: [-w / 2.0, 3.5, z1] });
        walls.push(Aabb { min: [w / 2.0, 0.0, z0], max: [4.0, 3.5, z1] });
        walls.push(Aabb { min: [-w / 2.0, 0.0, z0], max: [w / 2.0, sill, z1] });
        walls.push(Aabb { min: [-w / 2.0, sill + tall, z0], max: [w / 2.0, 5.0, z1] });
        walls
    }

    /// Walking at a window with space held gets the player out through it, off to one side of it too, and down on
    /// the far side; a hole too small for a body anywhere is only jumped at.
    #[test]
    fn a_window_is_climbed_through_but_not_a_peephole() {
        let through = |w: f32, sill: f32, tall: f32, x: f32| {
            let mut g = Game::with_walls(1, hole_in(w, sill, tall));
            g.bots.clear();
            g.players[0].body.pos = [x, 0.0, -2.0];
            g.players[0].yaw = 0.0;
            g.players[0].input.w = true;
            g.players[0].input.jump = true;
            let mut low = f32::INFINITY;
            for _ in 0..(2.5 / STEP) as usize {
                g.tick(STEP);
                low = low.min(g.players[0].eye()[1] - g.players[0].body.pos[1]);
            }
            let p = g.players[0].body.pos;
            assert!(p[2] > -3.3 || p[2] < -3.7 - HW, "left inside the wall at {p:?}");
            (p[2] < -3.7, low)
        };
        for (w, sill, tall, x) in [(1.4, 0.9, 1.3, 0.0), (1.6, 1.0, 1.3, 0.3), (1.4, 1.0, 1.0, 0.0), (1.4, 1.5, 1.2, -0.2)] {
            let (out, low) = through(w, sill, tall, x);
            assert!(out, "didn't get through a {w} by {tall} window {sill} up");
            assert!(low < tall - 0.2, "the eye didn't duck under the head of a {tall} tall window: {low}");
        }
        for (w, sill, tall) in [(0.5, 1.0, 0.5), (0.7, 1.0, 1.3), (1.4, 1.0, 0.7), (1.4, 3.1, 1.3)] {
            assert!(!through(w, sill, tall, 0.0).0, "got through a {w} by {tall} hole {sill} up");
        }
    }

    /// A room with no door, only a window, a player inside it and a zombie outside: the zombie climbs in through the
    /// window, and never through a hole too small for it.
    #[test]
    fn zombies_climb_in_through_a_window_but_not_a_peephole() {
        let room = |w: f32, tall: f32| {
            let mut walls = hole_in(w, 1.0, tall);
            walls.push(Aabb { min: [-4.0, 0.0, -10.0], max: [-3.6, 3.5, -3.3] });
            walls.push(Aabb { min: [3.6, 0.0, -10.0], max: [4.0, 3.5, -3.3] });
            walls.push(Aabb { min: [-4.0, 0.0, -10.4], max: [4.0, 3.5, -10.0] });
            let mut g = Game::with_walls(1, walls);
            g.bots.clear();
            g.players[0].hp = 1e6;
            g.players[0].body.pos = [0.0, 0.0, -7.0];
            g.bots.push(Bot::zombie([3.0, 0.0, 6.0], 0.0));
            let mut inside = false;
            for _ in 0..(25.0 / STEP) as usize {
                g.tick(STEP);
                let z = g.bots[0].body.pos;
                inside |= z[2] < -4.1 && z[0].abs() < 3.2;
                if inside && g.bots[0].climb.is_none() && g.bots[0].body.on_ground {
                    break;
                }
            }
            inside
        };
        assert!(room(1.4, 1.3), "the zombie never got in through the window");
        assert!(!room(0.6, 0.6), "the zombie got in through a peephole");
    }

    /// A container with no stairs, the player camped on top and a zombie on the ground 8 m off.
    fn camped_container() -> Game {
        let mut walls = map::perimeter();
        walls.push(block(0.0, 0.0, 12.0, 2.7, 2.5));
        let mut g = Game::with_walls(1, walls);
        g.bots.clear();
        g.players[0].hp = 1e6;
        g.players[0].body.pos = [0.0, 2.7, 0.0];
        g.bots.push(Bot::zombie([0.0, 0.0, 8.0], 0.0));
        g
    }

    #[test]
    fn zombies_climb_up_after_a_player_on_a_container() {
        let mut g = camped_container();
        g.tick(STEP);
        let hp = g.players[0].hp;
        let mut climbed = false;
        for _ in 0..(15.0 / STEP) as usize {
            g.tick(STEP);
            climbed |= g.bots[0].climb.is_some();
            if g.players[0].hp < hp {
                assert!(climbed, "bitten from below");
                return;
            }
        }
        panic!("the zombie never got up to the player: it is at {:?}", g.bots[0].body.pos);
    }

    #[test]
    fn a_zombie_shot_off_a_wall_falls_down() {
        let mut g = camped_container();
        for _ in 0..(15.0 / STEP) as usize {
            g.tick(STEP);
            let b = &g.bots[0];
            if b.climb.is_some() && b.body.pos[1] > 1.0 {
                break;
            }
        }
        assert!(g.bots[0].body.pos[1] > 1.0, "never caught it partway up");
        g.bots[0].alive = false;
        idle(&mut g, 1.5);
        let y = g.bots[0].body.pos[1];
        assert!(y < 0.01, "the body hangs on the wall at {y}");
    }

    #[test]
    fn the_head_hit_box_goes_where_the_head_jerks() {
        let mut z = Bot::zombie([0.0, 0.0, 0.0], 0.0);
        let mid = |b: Aabb| [(b.min[0] + b.max[0]) / 2.0, b.min[1], (b.min[2] + b.max[2]) / 2.0];
        z.head = [0.1, -0.05, 0.04];
        // facing -z its right is +x and forward -z; facing +x its right is +z
        let c = mid(z.head_box());
        assert!((c[0] - 0.1).abs() < 1e-4 && (c[1] - 1.35).abs() < 1e-4 && (c[2] + 0.04).abs() < 1e-4, "{c:?}");
        z.face = std::f32::consts::FRAC_PI_2;
        let c = mid(z.head_box());
        assert!((c[0] - 0.04).abs() < 1e-4 && (c[2] - 0.1).abs() < 1e-4, "{c:?}");
    }

    #[test]
    fn zombies_jerk_their_heads_about() {
        let mut g = arena();
        g.bots.clear();
        g.players[0].hp = 1e6;
        bot_ahead_zombie(&mut g, 0);
        g.bots[0].body.pos = [0.0, 0.0, -10.0];
        let (mut far, mut moves) = (0.0f32, 0);
        let mut last = [0.0; 3];
        for _ in 0..(8.0 / STEP) as usize {
            g.tick(STEP);
            let h = g.bots[0].head;
            far = far.max(h[0].abs());
            moves += (len([h[0] - last[0], h[1] - last[1], h[2] - last[2]]) > 0.01) as usize;
            last = h;
        }
        assert!(far > 0.06, "the head only ever moved {far} aside");
        assert!(moves > 10, "the head jerked only {moves} times");
    }

    fn bot_ahead_zombie(g: &mut Game, kind: u8) {
        g.players[0].body.pos = [0.0, 0.0, 16.0];
        g.players[0].yaw = 0.0;
        g.players[0].pitch = -0.2;
        let mut z = Bot::zombie([0.0, 0.0, 14.0], 0.0);
        z.kind = kind;
        z.hp = if kind == 2 { 320.0 } else { 100.0 };
        z.scale = if kind == 2 { 1.25 } else { 1.0 };
        g.bots.push(z);
    }

    /// The bat in hand, ready to swing.
    fn bat_out(g: &mut Game) {
        g.pick(BAT);
        g.select_slot(1);
        g.players[0].equip_t = 0.0;
        g.players[0].pitch = 0.0;
    }

    /// A swing takes its wind-up to connect, then knocks a walker back off its feet with blood flying, and holds
    /// the swing still a moment; it never runs dry, and the bat never reloads.
    #[test]
    fn a_bat_swing_connects_after_its_wind_up_and_knocks_a_zombie_back() {
        let mut g = arena();
        bot_ahead(&mut g);
        bat_out(&mut g);
        g.players[0].pitch = -0.2;
        g.players[0].input.fire = true;
        g.tick(STEP);
        assert!(g.events.iter().any(|e| e[0] == ev::SWING));
        assert_eq!(g.bots[0].hp, 100.0, "struck before the wind-up was done");
        g.players[0].input.fire = false;
        idle(&mut g, STRIKE + STEP);
        let b = &g.bots[0];
        assert_eq!(b.hp, 100.0 - WEAPONS[BAT].body_dmg, "a body blow");
        assert!(b.alive && b.body.vel[2] < -2.0 && b.body.vel[1] > 0.0, "not knocked back: {:?}", b.body.vel);
        assert!(g.players[0].stop_t > 0.0 && g.players[0].gore > 0.0);
        assert_eq!(g.players[0].reload_t, 0.0);
    }

    /// Level with the eye a swing goes through a walker's head and drops it in one; held down, it swings again.
    #[test]
    fn a_bat_to_the_head_drops_a_walker_and_held_it_keeps_swinging() {
        let mut g = arena();
        bot_ahead(&mut g);
        bat_out(&mut g);
        g.players[0].input.fire = true;
        idle(&mut g, STRIKE + 2.0 * STEP);
        assert!(!g.bots[0].alive, "a head blow did not kill");
        assert_eq!(g.players[0].kills, 1);
        idle(&mut g, 1.0);
        assert!(g.players[0].swings >= 2, "held down, it should swing on");
    }

    /// One swing fells up to three of a crowd in front, nearest first, and nobody behind the player.
    #[test]
    fn a_bat_swing_cleaves_three_of_a_crowd_but_not_the_one_behind() {
        let mut g = arena();
        g.players[0].body.pos = [0.0, 0.0, 16.0];
        g.players[0].yaw = 0.0;
        for at in [[-0.9, 14.6], [0.0, 14.4], [0.9, 14.6], [0.0, 13.0], [0.0, 17.5]] {
            g.bots.push(Bot::zombie([at[0], 0.0, at[1]], 0.0));
        }
        bat_out(&mut g);
        g.players[0].input.fire = true;
        idle(&mut g, STRIKE + 2.0 * STEP);
        let hit: Vec<bool> = g.bots.iter().map(|b| b.hp < 100.0).collect();
        assert_eq!(hit, [true, true, true, false, false]);
    }

    /// A swing at nothing but air comes to nothing; at a wall within reach it thuds against it.
    #[test]
    fn a_bat_swung_at_air_misses_and_at_a_wall_thuds() {
        let mut g = arena();
        bat_out(&mut g);
        g.players[0].input.fire = true;
        idle(&mut g, STRIKE + 2.0 * STEP);
        assert!(!g.events.iter().any(|e| e[0] == ev::SMACK), "hit something in the open");
        let wall = HALF - HW - 1.0;
        g.players[0].body.pos = [0.0, 0.0, -wall];
        g.players[0].yaw = 0.0;
        g.players[0].cooldown = 0.0;
        g.players[0].armed = true;
        idle(&mut g, STRIKE + 2.0 * STEP);
        assert!(g.events.iter().any(|e| e[0] == ev::SMACK && e[1] == 0.0), "no thud against the wall");
    }

    /// A new life starts with the magazine full and the reserve at twice the magazine.
    #[test]
    fn a_new_life_starts_with_full_mags_and_full_reserves() {
        let g = arena();
        assert_eq!(g.players[0].ammo, full_ammo());
        assert_eq!(g.players[0].reserve, full_reserve());
        for i in 0..WEAPON_COUNT {
            assert_eq!(g.players[0].reserve[i], 2 * WEAPONS[i].mag);
        }
    }

    /// A reload moves rounds from the reserve into the magazine, and only as many as the reserve has.
    #[test]
    fn a_reload_moves_rounds_from_the_reserve_and_only_as_far_as_it_reaches() {
        let mut g = arena();
        g.bots.clear();
        g.wave_t = f32::INFINITY;
        g.players[0].ammo[0] = 0;
        g.players[0].reserve[0] = 5;
        g.players[0].input.reload = true;
        idle(&mut g, WEAPONS[0].reload + 0.3);
        assert_eq!(g.players[0].ammo[0], 5, "the magazine takes what the reserve has");
        assert_eq!(g.players[0].reserve[0], 0, "the reserve pays for it");
    }

    /// With an empty reserve a reload never starts and an empty magazine fires no shot.
    #[test]
    fn an_empty_reserve_leaves_the_gun_silent_and_unreloading() {
        let mut g = arena();
        g.bots.clear();
        g.wave_t = f32::INFINITY;
        g.players[0].ammo[0] = 0;
        g.players[0].reserve[0] = 0;
        g.players[0].input.reload = true;
        g.players[0].input.fire = true;
        idle(&mut g, 2.0);
        assert_eq!(g.players[0].reload_t, 0.0, "no reload with no reserve");
        assert_eq!(g.players[0].ammo[0], 0, "no shot underflows the magazine");
        assert!(!g.events.iter().any(|e| e[0] == ev::RELOAD));
        assert!(!g.events.iter().any(|e| e[0] == ev::SHOT));
    }

    /// Running cancels a reload in progress, and the loaded magazine comes back as it was.
    #[test]
    fn running_cancels_a_reload_in_progress() {
        let mut g = arena();
        g.bots.clear();
        g.wave_t = f32::INFINITY;
        g.players[0].ammo[0] = 20;
        g.players[0].input.reload = true;
        idle(&mut g, 0.2);
        assert!(g.players[0].reload_t > 0.0, "the reload had started");
        g.players[0].input.reload = false;
        g.players[0].input.sprint = true;
        g.players[0].input.w = true;
        idle(&mut g, 0.3);
        assert_eq!(g.players[0].reload_t, 0.0, "running cancels the reload");
        assert_eq!(g.players[0].ammo[0], 20, "the loaded magazine comes back");
        assert_eq!(g.players[0].reserve[0], 2 * WEAPONS[0].mag, "nothing was taken from the reserve");
    }

    /// A reload does not start while running.
    #[test]
    fn running_forbids_starting_a_reload() {
        let mut g = arena();
        g.bots.clear();
        g.wave_t = f32::INFINITY;
        g.players[0].ammo[0] = 20;
        g.players[0].input.reload = true;
        g.players[0].input.sprint = true;
        g.players[0].input.w = true;
        idle(&mut g, 0.5);
        assert_eq!(g.players[0].reload_t, 0.0, "no reload starts while running");
        assert_eq!(g.players[0].reserve[0], 2 * WEAPONS[0].mag, "the reserve is untouched");
    }

    /// Running over an ammo box leaves it; a standing player still collects it.
    #[test]
    fn running_over_a_box_leaves_it_behind() {
        let mut g = arena();
        g.bots.clear();
        g.wave_t = f32::INFINITY;
        g.players[0].reserve[0] = WEAPONS[0].mag;
        let p = g.players[0].body.pos;
        g.boxes.push([p[0], p[1], p[2]]);
        g.players[0].input.sprint = true;
        g.players[0].input.w = true;
        idle(&mut g, 0.2);
        assert_eq!(g.boxes.len(), 1, "the box is not picked up while running");
        assert_eq!(g.players[0].reserve[0], WEAPONS[0].mag, "no rounds came out of it");
        assert!(!g.events.iter().any(|e| e[0] == ev::PICKUP));
        g.players[0].input = Input::default();
        let q = g.players[0].body.pos;
        g.boxes.push([q[0], q[1], q[2]]);
        idle(&mut g, 0.1);
        assert!(g.events.iter().any(|e| e[0] == ev::PICKUP), "a standing player still collects a box");
    }

    /// Reloading a magazine gun swaps the magazine: rounds left in the old one are dropped.
    #[test]
    fn reloading_a_magazine_gun_drops_the_loaded_magazine() {
        let mut g = arena();
        g.bots.clear();
        g.wave_t = f32::INFINITY;
        g.players[0].ammo[0] = 20;
        g.players[0].reserve[0] = 5;
        g.players[0].input.reload = true;
        idle(&mut g, WEAPONS[0].reload + 0.3);
        assert_eq!(g.players[0].ammo[0], 5, "the new magazine holds what the reserve has");
        assert_eq!(g.players[0].reserve[0], 0, "the reserve pays for it");
    }

    /// Tube-fed, bolt and cylinder guns keep the rounds they loaded: their reload only tops up.
    #[test]
    fn reloading_shotguns_and_revolvers_keeps_the_loaded_rounds() {
        let mut g = arena();
        g.bots.clear();
        g.wave_t = f32::INFINITY;
        g.select(2);
        g.players[0].ammo[2] = 3;
        g.players[0].reserve[2] = 5;
        g.players[0].input.reload = true;
        idle(&mut g, WEAPONS[2].reload + 0.3);
        assert_eq!(g.players[0].ammo[2], 6, "the loaded shells stay and the tube tops up");
        assert_eq!(g.players[0].reserve[2], 2, "the reserve pays only for the top-up");
        g.select(9);
        g.players[0].ammo[9] = 4;
        g.players[0].reserve[9] = 2;
        idle(&mut g, WEAPONS[9].reload + 0.3);
        assert_eq!(g.players[0].ammo[9], 6, "the loaded chambers stay and top up");
        assert_eq!(g.players[0].reserve[9], 0, "the reserve pays for the top-up");
    }

    /// Each wave tops the ammo boxes up to two thirds of its zombies, rounded up, on top of the ones left behind.
    #[test]
    fn each_wave_tops_up_the_boxes_to_two_thirds_of_the_zombies() {
        let mut g = zombie_game();
        g.players[0].hp = 1e6;
        idle(&mut g, 0.1);
        assert_eq!(g.wave, 1);
        assert_eq!(g.boxes.len(), 6, "wave one: two thirds of the nine zombies, rounded up");
        g.boxes.pop();
        for b in &mut g.bots {
            b.alive = false;
        }
        g.to_spawn = 0;
        idle(&mut g, 5.5);
        assert_eq!(g.wave, 2);
        assert_eq!(g.boxes.len(), 8, "wave two tops up to two thirds of its twelve");
    }

    /// Every box of a wave lands in bounds, within walking distance of the player, and clear of the others.
    #[test]
    fn boxes_land_where_the_player_can_reach_them() {
        let mut g = zombie_game();
        g.players[0].hp = 1e6;
        idle(&mut g, 0.1);
        assert_eq!(g.boxes.len(), 6);
        let p = g.players[0].body.pos;
        for at in &g.boxes {
            assert!(at[0].abs() <= HALF - 2.0 && at[2].abs() <= HALF - 2.0, "box out of bounds: {at:?}");
            assert!(g.nav.reachable(*at), "box the player cannot walk to: {at:?}");
            let d = ((at[0] - p[0]).powi(2) + (at[2] - p[2]).powi(2)).sqrt();
            assert!(d >= BOX_NEAR, "box only {d:.1} m from the player: {at:?}");
        }
        for i in 0..g.boxes.len() {
            for j in (i + 1)..g.boxes.len() {
                let d = ((g.boxes[i][0] - g.boxes[j][0]).powi(2) + (g.boxes[i][2] - g.boxes[j][2]).powi(2)).sqrt();
                assert!(d >= BOX_GAP, "boxes only {d:.2} m apart: {:?} {:?}", g.boxes[i], g.boxes[j]);
            }
        }
    }

    /// On the made maps, a wave's boxes all spawn where the player can reach them.
    #[test]
    fn boxes_on_the_made_maps_are_all_reachable() {
        for (map, seed) in (0..map::MAPS).flat_map(|m| [1, 7].map(move |s| (m, s))) {
            let mut g = Game::on(map, seed);
            g.wave_t = 0.0;
            g.players[0].hp = 1e6;
            idle(&mut g, 0.1);
            assert_eq!(g.boxes.len(), 6, "map {map} seed {seed}: the wave's boxes are missing");
            for at in &g.boxes {
                assert!(g.nav.reachable(*at), "map {map} seed {seed}: box the player cannot reach: {at:?}");
            }
        }
    }

    /// Running over a box adds a magazine to the reserves, capped at twice the magazine, and leaves the loaded one alone.
    #[test]
    fn a_box_adds_a_magazine_to_the_reserves_capped_at_double() {
        let mut g = arena();
        g.bots.clear();
        g.wave_t = f32::INFINITY;
        g.players[0].ammo[0] = 15;
        g.players[0].reserve[0] = 20;
        let p = g.players[0].body.pos;
        g.boxes.push([p[0] + 0.5, p[1], p[2]]);
        idle(&mut g, 0.1);
        assert!(g.boxes.is_empty(), "the box was run over");
        assert_eq!(g.players[0].ammo[0], 15, "the loaded magazine is left alone");
        assert_eq!(g.players[0].reserve[0], 20 + WEAPONS[0].mag, "one magazine added");
        assert!(g.events.iter().any(|e| e[0] == ev::PICKUP));
        g.boxes.push([p[0] + 0.5, p[1], p[2]]);
        idle(&mut g, 0.1);
        assert_eq!(g.players[0].reserve[0], 2 * WEAPONS[0].mag, "capped at twice the magazine");
        assert!(g.boxes.is_empty());
    }

    /// A box is not taken while the reserves and the grenades are both full, even with an empty magazine: full
    /// means both. With full reserves but room for a grenade, it is taken for the grenade.
    #[test]
    fn a_box_stays_only_when_the_reserves_and_grenades_are_full() {
        let mut g = arena();
        g.bots.clear();
        g.wave_t = f32::INFINITY;
        g.players[0].ammo[0] = 0;
        g.players[0].reserve[0] = 2 * WEAPONS[0].mag;
        g.players[0].grenades = MAX_GRENADES;
        let p = g.players[0].body.pos;
        g.boxes.push([p[0] + 0.5, p[1], p[2]]);
        idle(&mut g, 0.1);
        assert_eq!(g.boxes.len(), 1, "both full: the box is left for later");
        assert!(!g.events.iter().any(|e| e[0] == ev::PICKUP));
        g.players[0].grenades = 1;
        idle(&mut g, 0.1);
        assert!(g.boxes.is_empty(), "room for a grenade: the box is taken");
        assert_eq!(g.players[0].grenades, 2, "the grenade comes out of it");
        assert_eq!(g.players[0].reserve[0], 2 * WEAPONS[0].mag, "the full reserves are left alone");
    }

    /// Running over a box adds one grenade at a time, up to the limit, and no further.
    #[test]
    fn a_box_adds_one_grenade_up_to_the_limit() {
        let mut g = arena();
        g.bots.clear();
        g.wave_t = f32::INFINITY;
        let p = g.players[0].body.pos;
        g.players[0].grenades = 1;
        for _ in 0..MAX_GRENADES {
            g.boxes.push([p[0] + 0.5, p[1], p[2]]);
            idle(&mut g, 0.1);
        }
        assert_eq!(g.players[0].grenades, MAX_GRENADES, "one per box, up to the limit");
        g.boxes.push([p[0] + 0.5, p[1], p[2]]);
        idle(&mut g, 0.1);
        assert_eq!(g.players[0].grenades, MAX_GRENADES, "no more than can be carried");
    }

    /// A box just out of reach is left alone, and a dead player takes nothing.
    #[test]
    fn a_box_just_out_of_reach_stays_and_a_dead_player_takes_nothing() {
        let mut g = arena();
        g.bots.clear();
        g.wave_t = f32::INFINITY;
        let p = g.players[0].body.pos;
        g.players[0].reserve[0] = 0;
        g.boxes.push([p[0] + 1.2, p[1], p[2]]);
        idle(&mut g, 0.1);
        assert_eq!(g.boxes.len(), 1, "a box 1.2 m off is not run over");
        g.boxes.push([p[0] - 0.6, p[1], p[2]]);
        g.hurt_player(0, 1e9);
        idle(&mut g, 0.1);
        assert_eq!(g.boxes.len(), 2, "a dead player takes nothing");
    }

    /// Dying resets the boxes with the waves, and the new life starts with full reserves.
    #[test]
    fn dying_clears_the_boxes_with_the_waves() {
        let mut g = zombie_game();
        idle(&mut g, 0.1);
        assert!(!g.boxes.is_empty());
        g.hurt_player(0, 1e9);
        assert!(!g.players[0].alive());
        idle(&mut g, 4.2);
        assert!(g.players[0].alive());
        assert_eq!(g.wave, 0);
        assert!(g.boxes.is_empty(), "the boxes of the old life are gone");
        assert_eq!(g.players[0].reserve, full_reserve(), "the new life starts with full reserves");
    }

    /// Playing for loot a life starts with only the pistol, and picking a loadout doesn't change that.
    #[test]
    fn a_loot_life_starts_with_only_the_pistol() {
        let mut g = arena();
        g.set_loot(true);
        assert_eq!((g.players[0].weapon, g.players[0].loadout), (PISTOL, [NO_GUN, PISTOL]));
        g.pick(2);
        assert_eq!(g.players[0].loadout, [NO_GUN, PISTOL], "the pick doesn't arm a loot life");
        g.select_slot(0);
        assert_eq!(g.players[0].weapon, PISTOL, "nothing to switch to in the empty slot");
        g.hurt_player(0, 1e9);
        idle(&mut g, 4.2);
        assert!(g.players[0].alive());
        assert_eq!(g.players[0].loadout, [NO_GUN, PISTOL], "the next life starts with the pistol again");
    }

    /// Walking over a weapon crate takes its gun into its slot, loaded and raised; running over one, or over one
    /// with a gun already carried, leaves it.
    #[test]
    fn a_weapon_crate_puts_its_gun_in_its_slot() {
        let mut g = arena();
        g.bots.clear();
        g.wave_t = f32::INFINITY;
        g.set_loot(true);
        let p = g.players[0].body.pos;
        g.crates.push(([p[0] + 0.5, p[1], p[2]], 4));
        g.players[0].input.sprint = true;
        g.players[0].input.w = true;
        idle(&mut g, 0.2);
        assert_eq!(g.crates.len(), 1, "not taken while running");
        g.players[0].input = Input::default();
        let p = g.players[0].body.pos;
        g.crates[0].0 = [p[0] + 0.5, p[1], p[2]];
        idle(&mut g, 0.1);
        assert!(g.crates.is_empty(), "the crate was walked over");
        assert_eq!((g.players[0].weapon, g.players[0].loadout), (4, [4, PISTOL]), "the AK-47 is the primary, raised");
        assert_eq!((g.players[0].ammo[4], g.players[0].reserve[4]), (WEAPONS[4].mag, 2 * WEAPONS[4].mag));
        assert!(g.events.iter().any(|e| e[0] == ev::PICKUP));
        // a sidearm takes the pistol's place
        g.crates.push(([p[0] + 0.5, p[1], p[2]], 9));
        idle(&mut g, 0.1);
        assert_eq!(g.players[0].loadout, [4, 9], "the Revolver replaces the pistol");
        // the gun already carried stays in its crate
        g.crates.push(([p[0] + 0.5, p[1], p[2]], 4));
        idle(&mut g, 0.1);
        assert_eq!(g.crates.len(), 1, "an AK-47 already carried is left");
    }

    /// Each wave tops up the weapon crates playing for loot, and only then; none holds the pistol.
    #[test]
    fn waves_bring_weapon_crates_only_playing_for_loot() {
        let mut g = zombie_game();
        g.players[0].hp = 1e6;
        idle(&mut g, 0.1);
        assert!(g.crates.is_empty(), "no crates with a loadout");
        let mut g = zombie_game();
        g.set_loot(true);
        g.players[0].hp = 1e6;
        idle(&mut g, 0.1);
        assert_eq!(g.wave, 1);
        assert_eq!(g.crates.len(), 2, "wave one brings two crates");
        let p = g.players[0].body.pos;
        for &(at, gun) in &g.crates {
            assert!(gun < WEAPON_COUNT && gun != PISTOL, "crate with gun {gun}");
            assert!(g.nav.reachable(at), "crate the player cannot walk to: {at:?}");
            assert!(((at[0] - p[0]).powi(2) + (at[2] - p[2]).powi(2)).sqrt() >= BOX_NEAR);
            for b in &g.boxes {
                assert!(((b[0] - at[0]).powi(2) + (b[2] - at[2]).powi(2)).sqrt() >= BOX_GAP, "crate on an ammo box");
            }
        }
    }
}
