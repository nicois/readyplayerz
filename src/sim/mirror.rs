//! A client's copy of the server's game. Its own player is played here as soon as a key is pressed, ahead of the
//! server: each step's input is played at once and sent off. When the server says where the player really was
//! after some input, the copy is put back there and every input since is played again on top, so the two only
//! part where the server knew something the client couldn't (a blast throwing them). Everything else (the other
//! players, the zombies, the grenades) is drawn a little in the past, between two snapshots the server sent,
//! so it moves smoothly however the datagrams come in; shots are checked on the server against where the zombies
//! were in that past (see `Game::trace_bots`), so what the player saw on target was on target.

use super::*;
use crate::net::{self, Mate, Nade, Own, World, Zombie};
use std::collections::{HashMap, VecDeque};

/// How far behind the newest snapshot the rest of the game is drawn, in ticks: enough for one or two to be late
/// or lost without anything stopping.
const BEHIND: f32 = 6.0;
/// How many snapshots each thing keeps.
const SAMPLES: usize = 24;

/// Things that can be drawn partway between two snapshots of them.
trait Blend: Clone {
    fn blend(&self, b: &Self, k: f32) -> Self;
}

fn mix(a: f32, b: f32, k: f32) -> f32 {
    a + (b - a) * k
}
fn mix3(a: V3, b: V3, k: f32) -> V3 {
    [mix(a[0], b[0], k), mix(a[1], b[1], k), mix(a[2], b[2], k)]
}
/// The short way round between two angles.
fn turn(a: f32, b: f32, k: f32) -> f32 {
    a + ((b - a + 3.0 * PI).rem_euclid(2.0 * PI) - PI) * k
}

impl Blend for Mate {
    fn blend(&self, b: &Self, k: f32) -> Self {
        let mut m = if k < 0.5 { *self } else { *b };
        m.pos = mix3(self.pos, b.pos, k);
        m.yaw = turn(self.yaw, b.yaw, k);
        m.pitch = mix(self.pitch, b.pitch, k);
        m
    }
}

impl Blend for Zombie {
    fn blend(&self, b: &Self, k: f32) -> Self {
        let mut z = if k < 0.5 { *self } else { *b };
        z.pos = mix3(self.pos, b.pos, k);
        z.face = turn(self.face, b.face, k);
        z.vel = [mix(self.vel[0], b.vel[0], k), mix(self.vel[1], b.vel[1], k)];
        z.head = mix3(self.head, b.head, k);
        (z.hit_t, z.swing_t, z.dead_t) = (mix(self.hit_t, b.hit_t, k), mix(self.swing_t, b.swing_t, k), mix(self.dead_t, b.dead_t, k));
        if let (Some(p), Some(q)) = (self.pose, b.pose) {
            z.pose = Some((mix(p.0, q.0, k), mix(p.1, q.1, k), mix(p.2, q.2, k), mix(p.3, q.3, k)));
        }
        z
    }
}

impl Blend for Nade {
    fn blend(&self, b: &Self, k: f32) -> Self {
        Nade { id: b.id, pos: mix3(self.pos, b.pos, k), rot: [0, 1, 2].map(|i| turn(self.rot[i], b.rot[i], k)) }
    }
}

/// The last few snapshots of one thing, by server tick.
struct Track<T> {
    samples: VecDeque<(u32, T)>,
}

impl<T: Blend> Track<T> {
    fn new() -> Track<T> {
        Track { samples: VecDeque::new() }
    }

    fn push(&mut self, tick: u32, s: T) {
        let at = self.samples.iter().position(|(t, _)| *t >= tick).unwrap_or(self.samples.len());
        if self.samples.get(at).is_some_and(|(t, _)| *t == tick) {
            return;
        }
        self.samples.insert(at, (tick, s));
        if self.samples.len() > SAMPLES {
            self.samples.pop_front();
        }
    }

    fn last_tick(&self) -> u32 {
        self.samples.back().map_or(0, |(t, _)| *t)
    }

    /// How it was at tick `t`: between the snapshots either side, or the nearest one at either end.
    fn at(&self, t: f32) -> Option<T> {
        let (first, last) = (self.samples.front()?, self.samples.back()?);
        if t <= first.0 as f32 {
            return Some(first.1.clone());
        }
        if t >= last.0 as f32 {
            return Some(last.1.clone());
        }
        let k = self.samples.iter().position(|(n, _)| *n as f32 > t)?;
        let ((ta, a), (tb, b)) = (&self.samples[k - 1], &self.samples[k]);
        Some(a.blend(b, (t - *ta as f32) / (*tb - *ta) as f32))
    }
}

/// A zombie as the client has it: its snapshots, and whether its death has been seen yet.
struct Seen {
    track: Track<Zombie>,
    fell: bool,
}

pub struct Mirror {
    pub game: Game,
    /// which player is this one
    pub id: u8,
    /// the number the next input gets, and the ones played here the server hasn't said it has played yet
    seq: u32,
    pending: VecDeque<(u32, Input)>,
    /// steps played here, and the server's tick less that, as best it can be told from when snapshots come in
    clock: f32,
    offset: Option<f32>,
    /// the newest server tick heard of, and the ticks of the newest own and world records used
    newest: u32,
    own_tick: u32,
    world_tick: u32,
    world: World,
    mates: HashMap<u8, Track<Mate>>,
    zombies: HashMap<u32, Seen>,
    nades: HashMap<u32, Track<Nade>>,
    /// the blasts already seen go off, the latest few
    booms: VecDeque<u32>,
    /// datagrams to send
    out: Vec<Vec<u8>>,
}

impl Mirror {
    /// Player `id`'s copy of a game on map `map` made from `seed`.
    pub fn new(id: u8, map: u8, seed: u32) -> Mirror {
        Mirror::with(id, Game::on(map, seed))
    }

    /// Player `id`'s copy of a game on the map of `game`.
    pub(crate) fn with(id: u8, mut game: Game) -> Mirror {
        game.role = Role::Mirror;
        game.hold_waves();
        game.remote_left = Some(0);
        let p = &mut game.players[0];
        p.id = id;
        // not spawned until the server says where
        p.life = 0;
        Mirror {
            game,
            id,
            seq: 0,
            pending: VecDeque::new(),
            clock: 0.0,
            offset: None,
            newest: 0,
            own_tick: 0,
            world_tick: 0,
            world: World::default(),
            mates: HashMap::new(),
            zombies: HashMap::new(),
            nades: HashMap::new(),
            booms: VecDeque::new(),
            out: Vec::new(),
        }
    }

    /// The waves have started.
    pub fn started(&self) -> bool {
        self.world.started
    }

    /// The datagrams to send since the last call.
    pub fn take_outgoing(&mut self) -> Vec<Vec<u8>> {
        std::mem::take(&mut self.out)
    }

    /// The server tick the rest of the game is drawn at, `frac` of a step on from the last one played.
    fn view_tick(&self, frac: f32) -> f32 {
        self.clock + frac + self.offset.unwrap_or(0.0) - BEHIND
    }

    /// One step: the player's input (`Player::input`, as the keys and mouse left it) played here at once and sent
    /// off, with the rest of the game where it is seen.
    pub fn step(&mut self) {
        self.present(0.0);
        let seen = self.view_tick(0.0);
        let i = self.game.local;
        let p = &mut self.game.players[i];
        let mut inp = p.input;
        p.input.look_dx = 0.0;
        p.input.look_dy = 0.0;
        p.input.select = None;
        p.input.pick = None;
        // the view is turned here, and the server told where it ended up
        p.look(inp.look_dx, inp.look_dy);
        inp.view = Some([p.yaw, p.pitch]);
        (inp.look_dx, inp.look_dy, inp.seen) = (0.0, 0.0, seen);
        self.game.step_player(i, inp, STEP);
        self.pending.push_back((self.seq, inp));
        self.seq += 1;
        while self.pending.len() > 180 {
            self.pending.pop_front();
        }
        let from = self.pending.len().saturating_sub(net::RESEND);
        let last: Vec<Input> = self.pending.iter().skip(from).map(|(_, inp)| *inp).collect();
        self.out.push(net::encode_inputs(self.seq - last.len() as u32, &last));
        self.game.tick(STEP);
        self.clock += 1.0;
    }

    /// A datagram from the server.
    pub fn receive(&mut self, bytes: &[u8]) {
        let Some(s) = net::decode_snapshot(bytes) else { return };
        // the server's clock against this one's: quick to follow snapshots coming sooner, slow to believe they come
        // later, as a late one is more often just held up on the way
        let gap = s.tick as f32 - self.clock;
        self.offset = Some(match self.offset {
            Some(o) if (gap - o).abs() < 30.0 => o + (gap - o) * if gap > o { 0.2 } else { 0.02 },
            _ => gap,
        });
        self.newest = self.newest.max(s.tick);
        if let Some(own) = &s.own {
            if s.tick > self.own_tick {
                self.own_tick = s.tick;
                self.reconcile(own);
            }
        }
        if let Some(w) = s.world {
            if s.tick >= self.world_tick {
                self.world_tick = s.tick;
                if w.wave > self.world.wave {
                    self.game.events.push([ev::WAVE, w.wave as f32, 0.0]);
                }
                self.world = w;
                self.game.wave = w.wave;
                self.game.remote_left = Some(w.left);
                self.game.wave_t = if w.started { 0.0 } else { f32::INFINITY };
            }
        }
        for m in s.mates {
            self.mates.entry(m.id).or_insert_with(Track::new).push(s.tick, m);
        }
        for z in s.zombies {
            self.zombies.entry(z.id).or_insert_with(|| Seen { track: Track::new(), fell: false }).track.push(s.tick, z);
        }
        for n in s.nades {
            self.nades.entry(n.id).or_insert_with(Track::new).push(s.tick, n);
        }
        for b in s.booms {
            if !self.booms.contains(&b.id) {
                self.booms.push_back(b.id);
                if self.booms.len() > 64 {
                    self.booms.pop_front();
                }
                self.game.blast_fx(b.id, b.pos);
                if let Some(last) = self.game.blasts.last_mut() {
                    last.t = b.t;
                }
            }
        }
        for shot in s.shots {
            if shot.by != self.id {
                self.mate_shot(shot);
            }
        }
    }

    /// Someone else's shot: a tracer, the sound of it from where they are, and blood if it ended in a zombie.
    fn mate_shot(&mut self, s: Shot) {
        let g = &mut self.game;
        g.tracers.push(Tracer { a: s.a, b: s.b, life: 0.06 });
        let me = g.me();
        let weapon = self.mates.get(&s.by).and_then(|t| t.samples.back()).map_or(0, |(_, m)| m.weapon);
        let (dx, dz) = (s.a[0] - me.body.pos[0], s.a[2] - me.body.pos[2]);
        let d = dx.hypot(dz).max(0.001);
        let (sy, cy) = me.yaw.sin_cos();
        g.events.push([ev::FAR_SHOT, d, weapon as f32 * 4.0 + (dx * cy + dz * sy) / d]);
        let near = |b: &Aabb| (0..3).all(|k| s.b[k] > b.min[k] - 0.1 && s.b[k] < b.max[k] + 0.1);
        if g.bots.iter().any(|b| b.alive && (near(&b.body_box()) || near(&b.head_box()))) {
            g.blood(s.b, unit([s.b[0] - s.a[0], s.b[1] - s.a[1], s.b[2] - s.a[2]]), 6);
        }
    }

    /// The server saying where this player was after input number `own.ack`, and what it decides: put back there,
    /// and the inputs since played again.
    fn reconcile(&mut self, own: &Own) {
        let i = self.game.local;
        if own.life != self.game.players[i].life {
            self.game.players[i].life = own.life.saturating_sub(1);
            self.game.respawn_at(i, own.pos, own.yaw);
        }
        let p = &mut self.game.players[i];
        if p.alive() && own.hp < p.hp - 0.01 {
            self.game.events.push([ev::HURT, p.hp - own.hp, 0.0]);
            (p.damage_flash, p.since_hurt) = (1.0, 0.0);
            if own.hp <= 0.0 {
                self.game.events.push([ev::DEATH, 0.0, 0.0]);
            }
        }
        if own.kills > p.kills {
            self.game.events.push([ev::KILL, 0.0, 0.0]);
            (p.kill_t, p.hit_marker) = (1.0, 1.0);
        }
        (p.hp, p.kills, p.deaths, p.respawn_t) = (own.hp, own.kills, own.deaths, own.respawn_t);

        while self.pending.front().is_some_and(|(n, _)| *n <= own.ack) {
            self.pending.pop_front();
        }
        p.body = Body { pos: own.pos, vel: own.vel, on_ground: own.on_ground };
        p.climb = own.climb.as_ref().map(Climb::from_raw);
        (p.stamina, p.winded, p.since_sprint) = (own.stamina, own.winded, own.since_sprint);
        if !p.alive() {
            return;
        }
        // played again only for where it takes the body: how it looks and sounds has already been, and the view
        // stays where the mouse has it
        let keep = (p.yaw, p.pitch, p.step_y, p.gait, p.stride, p.land_t, p.sprint_t, p.equip_t);
        let (mark, world) = (self.game.events.len(), self.game.world.clone());
        for k in 0..self.pending.len() {
            let inp = self.pending[k].1;
            self.game.move_player(i, inp, STEP, &world);
        }
        self.game.events.truncate(mark);
        let p = &mut self.game.players[i];
        (p.yaw, p.pitch, p.step_y, p.gait, p.stride, p.land_t, p.sprint_t, p.equip_t) = keep;
    }

    /// Puts the rest of the game where it is seen, `frac` of a step on from the last one played: the other
    /// players, the zombies (letting the fallen lie) and the grenades in the air.
    pub fn present(&mut self, frac: f32) {
        let t = self.view_tick(frac);
        let g = &mut self.game;

        let gone: Vec<u8> = self.mates.iter().filter(|(_, tr)| tr.last_tick() + 120 < self.newest).map(|(id, _)| *id).collect();
        for id in gone {
            self.mates.remove(&id);
        }
        g.players.retain(|p| p.id == self.id || self.mates.contains_key(&p.id));
        g.local = g.players.iter().position(|p| p.id == self.id).unwrap_or(0);
        for (id, tr) in &self.mates {
            let Some(m) = tr.at(t) else { continue };
            let i = match g.players.iter().position(|p| p.id == *id) {
                Some(i) => i,
                None => {
                    g.players.push(Player::new(*id));
                    g.players.len() - 1
                }
            };
            let p = &mut g.players[i];
            // how fast they go, for the legs, from where they were a tick ago
            let was = tr.at(t - 1.0).map_or(m.pos, |m| m.pos);
            p.body = Body { pos: m.pos, vel: [(m.pos[0] - was[0]) / STEP, (m.pos[1] - was[1]) / STEP, (m.pos[2] - was[2]) / STEP], on_ground: m.on_ground };
            (p.yaw, p.pitch, p.weapon, p.hp) = (m.yaw, m.pitch, (m.weapon as usize).min(WEAPON_COUNT - 1), if m.alive { m.hp.max(1.0) } else { 0.0 });
            (p.aiming, p.ads) = (m.aiming, if m.ads { 1.0 } else { 0.0 });
            p.reload_t = if m.reloading { 1.0 } else { 0.0 };
        }

        g.bots.clear();
        let newest = self.newest;
        let mut dead = Vec::new();
        let mut fell = Vec::new();
        for (id, seen) in self.zombies.iter_mut() {
            let Some(mut z) = seen.track.at(t) else { continue };
            let last = seen.track.last_tick();
            if !z.alive {
                // lying where it was last seen, for as long as a corpse lies
                z.dead_t += (t - last as f32).max(0.0) * STEP;
                if !seen.fell {
                    seen.fell = true;
                    fell.push(z);
                }
                if z.dead_t > CORPSE_T || !z.corpse && z.dead_t > 0.5 {
                    dead.push(*id);
                }
            } else if last + 60 < newest {
                // alive and not heard of for a second: gone, its place taken by another
                dead.push(*id);
                continue;
            }
            let mut b = Bot::zombie(z.pos, z.phase);
            b.id = z.id;
            b.kind = z.kind;
            b.scale = [1.0, 0.95, 1.25][z.kind.min(2) as usize];
            (b.speed, b.face, b.alive, b.corpse) = (z.speed, z.face, z.alive, z.corpse);
            (b.dead_t, b.hit_t, b.swing_t, b.head, b.pose) = (z.dead_t, z.hit_t, z.swing_t, z.head, z.pose);
            b.body.vel = [z.vel[0], 0.0, z.vel[1]];
            b.body.on_ground = z.on_ground;
            b.hp = if z.alive { 1.0 } else { 0.0 };
            if b.alive || b.corpse {
                g.bots.push(b);
            }
        }
        for id in dead {
            self.zombies.remove(&id);
        }
        // how a death looks: shot down it bleeds, blown apart it is lumps
        for z in fell {
            let s = [1.0, 0.95, 1.25][z.kind.min(2) as usize];
            let at = [z.pos[0], z.pos[1] + 1.1 * s, z.pos[2]];
            let away = [z.face.sin(), -z.face.cos()];
            if z.corpse {
                g.blood(at, [-away[0], 0.0, -away[1]], 18);
            } else {
                g.gibs(at, [-away[0], -away[1]], s);
            }
        }

        let gone: Vec<u32> = self.nades.iter().filter(|(_, tr)| tr.last_tick() + 4 < newest).map(|(id, _)| *id).collect();
        for id in gone {
            self.nades.remove(&id);
        }
        g.grenades = self.nades.values().filter_map(|tr| tr.at(t)).map(|n| Grenade::seen(n.id, n.pos, n.rot)).collect();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::host::Room;

    /// A room on the server and clients of it, with the datagrams between them held up for `lag` ticks each way.
    struct Link {
        room: Room,
        clients: Vec<Mirror>,
        lag: usize,
        up: VecDeque<(usize, u8, Vec<u8>)>,
        down: VecDeque<(usize, usize, Vec<u8>)>,
        now: usize,
    }

    impl Link {
        /// On an empty walled arena.
        fn new(players: usize, lag: usize) -> Link {
            let arena = || Game::with_walls(1, map::perimeter());
            let mut room = Room::with(arena(), 0, 1);
            let clients = (0..players)
                .map(|k| {
                    let id = room.join(&format!("p{k}")).unwrap();
                    Mirror::with(id, arena())
                })
                .collect();
            Link { room, clients, lag, up: VecDeque::new(), down: VecDeque::new(), now: 0 }
        }

        fn step(&mut self) {
            for (k, c) in self.clients.iter_mut().enumerate() {
                c.step();
                for d in c.take_outgoing() {
                    self.up.push_back((self.now + self.lag, k as u8, d));
                }
            }
            while self.up.front().is_some_and(|(t, ..)| *t <= self.now) {
                let (_, k, d) = self.up.pop_front().unwrap();
                let id = self.clients[k as usize].id;
                if let Some((first, inputs)) = net::decode_inputs(&d) {
                    self.room.inputs(id, first, &inputs);
                }
            }
            self.room.tick();
            if self.room.snapshot_due() {
                for (k, c) in self.clients.iter().enumerate() {
                    for d in self.room.snapshot_for(c.id) {
                        self.down.push_back((self.now + self.lag, k, d));
                    }
                }
                self.room.sent();
            }
            while self.down.front().is_some_and(|(t, ..)| *t <= self.now) {
                let (_, k, d) = self.down.pop_front().unwrap();
                self.clients[k].receive(&d);
            }
            self.now += 1;
        }

        fn run(&mut self, secs: f32) {
            for _ in 0..(secs / STEP) as usize {
                self.step();
            }
        }

        fn server_player(&self, k: usize) -> &Player {
            let id = self.clients[k].id;
            self.room.game.players.iter().find(|p| p.id == id).unwrap()
        }
    }

    #[test]
    fn a_client_moves_at_once_and_the_server_agrees_where_it_ends_up() {
        let mut l = Link::new(1, 6);
        l.run(0.5);
        let start = l.clients[0].game.me().body.pos;
        assert!(len([start[0] - l.server_player(0).body.pos[0], 0.0, start[2] - l.server_player(0).body.pos[2]]) < 0.01, "spawned where the server put it");
        let c = &mut l.clients[0];
        c.game.me_mut().input.w = true;
        c.game.me_mut().input.look_dx = 200.0;
        l.step();
        let moved = l.clients[0].game.me().body.pos;
        assert!(moved != start, "it moves on the very first step, before the server has heard");
        l.run(1.0);
        l.clients[0].game.me_mut().input.w = false;
        l.run(1.0);
        let (c, s) = (l.clients[0].game.me(), l.server_player(0));
        let d = len([c.body.pos[0] - s.body.pos[0], c.body.pos[1] - s.body.pos[1], c.body.pos[2] - s.body.pos[2]]);
        assert!(d < 0.001, "client {:?} server {:?}", c.body.pos, s.body.pos);
        assert!((c.yaw - s.yaw).abs() < 1e-5, "the server looks where the client turned");
        assert!(len([s.body.pos[0] - start[0], 0.0, s.body.pos[2] - start[2]]) > 3.0, "it really went somewhere");
    }

    #[test]
    fn a_blast_on_the_server_throws_the_client_too() {
        let mut l = Link::new(1, 5);
        l.run(0.5);
        let at = l.server_player(0).body.pos;
        let world = l.room.game.world.clone();
        l.room.game.explode([at[0] + 4.5, at[1] + 0.1, at[2]], 9, &world);
        let hp = l.server_player(0).hp;
        assert!(hp < 100.0 && hp > 0.0);
        l.run(0.5);
        let c = l.clients[0].game.me();
        assert!(c.body.pos[0] < at[0] - 0.3, "thrown away from it: {:?} from {:?}", c.body.pos, at);
        assert!(c.hp < 100.0 && (c.hp - l.server_player(0).hp).abs() < 1.0, "hurt as the server says: {} {}", c.hp, l.server_player(0).hp);
        assert!(l.clients[0].game.blasts.len() == 1 || l.clients[0].game.debris.len() > 10, "the client saw it go off");
    }

    #[test]
    fn players_see_each_other_and_the_zombies_and_a_lagged_shot_hits_what_was_seen() {
        let mut l = Link::new(2, 8);
        l.run(0.5);
        // a zombie walking across in front of player 0, at the server's pace
        let me = l.server_player(0).body.pos;
        let yaw = l.server_player(0).yaw;
        let f = [yaw.sin(), 0.0, -yaw.cos()];
        let r = [-f[2], 0.0, f[0]];
        let mut z = Bot::zombie([me[0] + f[0] * 8.0 - r[0] * 6.0, me[1], me[2] + f[2] * 8.0 - r[2] * 6.0], 0.0);
        z.id = 4242;
        z.hp = 1.0;
        l.room.game.bots.push(z);
        l.run(0.3);
        assert!(l.clients[0].game.players.len() == 2 && l.clients[1].game.players.len() == 2, "each sees the other");
        let other = l.clients[0].game.players.iter().find(|p| p.id != l.clients[0].id).unwrap().body.pos;
        let real = l.server_player(1).body.pos;
        assert!(len([other[0] - real[0], 0.0, other[2] - real[2]]) < 0.1, "where they are: {other:?} {real:?}");
        // on the client the zombie is where it was a little while back; aim at that and fire
        let c = &mut l.clients[0];
        c.present(0.0);
        let seen = c.game.bots.iter().find(|b| b.id == 4242).expect("the client sees the zombie").body.pos;
        let server_now = l.room.game.bots.iter().find(|b| b.id == 4242).unwrap().body.pos;
        let eye = c.game.me().eye();
        let to = [seen[0] - eye[0], seen[1] + 1.0 - eye[1], seen[2] - eye[2]];
        let p = c.game.me_mut();
        p.yaw = to[0].atan2(-to[2]);
        p.pitch = (to[1] / to[0].hypot(to[2])).atan();
        p.input.fire = true;
        p.input.ads = true;
        assert!(len([seen[0] - server_now[0], 0.0, seen[2] - server_now[2]]) > 0.1, "the zombie has moved on since: {seen:?} {server_now:?}");
        l.run(0.4);
        l.clients[0].game.me_mut().input.fire = false;
        l.run(0.3);
        assert!(!l.room.game.bots.iter().find(|b| b.id == 4242).unwrap().alive, "the shot hit where the client saw it");
        assert_eq!(l.server_player(0).kills, 1);
        assert_eq!(l.clients[0].game.me().kills, 1, "and the client hears of it");
    }

    #[test]
    fn a_thrown_grenade_flies_for_everyone_and_the_waves_start_for_all() {
        let mut l = Link::new(2, 4);
        l.run(0.5);
        l.clients[0].game.me_mut().input.throw = true;
        l.run(0.3);
        l.clients[0].game.me_mut().input.throw = false;
        l.run(0.5);
        assert_eq!(l.room.game.grenades.len(), 1);
        assert_eq!(l.clients[1].game.grenades.len(), 1, "the other player sees it in the air");
        assert_eq!(l.clients[0].game.me().grenades, GRENADES - 1);
        l.run(3.0);
        assert!(l.clients[1].game.blasts.len() + l.clients[1].booms.len() > 0, "and sees it go off");
        assert!(!l.clients[0].started());
        let host = l.room.host().unwrap();
        l.room.start(host);
        l.run(4.0);
        assert!(l.clients.iter().all(|c| c.started() && c.game.wave == 1));
        assert!(l.clients.iter().all(|c| !c.game.bots.is_empty()), "zombies come for both");
    }
}
