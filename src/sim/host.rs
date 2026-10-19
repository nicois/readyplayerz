//! The server's side of a room: the real game, everyone's inputs played into it as they come in, and the
//! snapshots of it each player is sent. Nothing here touches the network itself; the server binary moves the bytes.

use super::*;
use crate::net::{self, Boom, Mate, Msg, Nade, Own, Snapshot, World, Zombie, MAX_PLAYERS};
use std::collections::BTreeMap;

/// A snapshot goes out every this many ticks.
pub const SNAPSHOT_EVERY: u32 = 2;
/// A player's inputs are kept waiting at most this many before the oldest are played two a tick to catch up, so
/// a client whose clock runs a little fast doesn't fall further and further behind.
const QUEUE: usize = 4;
/// Ticks to wait for an input that was lost on the way before going on without it.
const GAP_WAIT: u32 = 3;

/// One player's place in a room.
struct Seat {
    id: u8,
    name: String,
    /// inputs come in, by number, waiting to be played
    queue: BTreeMap<u32, Input>,
    /// the number of the next input to play, and how long it has been waited for
    next: u32,
    waited: u32,
    /// the number of the last input played
    ack: u32,
}

pub struct Room {
    pub game: Game,
    pub map: u8,
    pub seed: u32,
    seats: Vec<Seat>,
    /// the shots fired since the last snapshot
    shots: Vec<Shot>,
}

impl Room {
    /// A room on map `map`, made from `seed`, with nobody in it and the waves held off until the host starts.
    pub fn new(map: u8, seed: u32) -> Room {
        Room::with(Game::on(map, seed), map, seed)
    }

    /// A room on the map of `game`.
    pub(crate) fn with(mut game: Game, map: u8, seed: u32) -> Room {
        game.role = Role::Server;
        game.players.clear();
        game.hold_waves();
        Room { game, map, seed, seats: Vec::new(), shots: Vec::new() }
    }

    pub fn is_empty(&self) -> bool {
        self.seats.is_empty()
    }

    /// Someone coming in, called `name`: their player's id, or None if the room is full.
    pub fn join(&mut self, name: &str) -> Option<u8> {
        if self.seats.len() >= MAX_PLAYERS {
            return None;
        }
        let id = (0..=u8::MAX).find(|id| self.seats.iter().all(|s| s.id != *id))?;
        // letters, digits, spaces and a few marks only, so a name is safe to show anywhere
        let name: String = name.chars().filter(|c| c.is_alphanumeric() || " -_.".contains(*c)).take(16).collect();
        let name = if name.trim().is_empty() { format!("Player {}", id + 1) } else { name.trim().to_string() };
        self.seats.push(Seat { id, name, queue: BTreeMap::new(), next: 0, waited: 0, ack: 0 });
        self.game.add_player(id);
        Some(id)
    }

    pub fn leave(&mut self, id: u8) {
        self.seats.retain(|s| s.id != id);
        self.game.remove_player(id);
    }

    /// The host, who starts the game: whoever has been in longest.
    pub fn host(&self) -> Option<u8> {
        self.seats.first().map(|s| s.id)
    }

    /// The host letting the waves come.
    pub fn start(&mut self, by: u8) {
        if self.host() == Some(by) && !self.game.waves_on() {
            self.game.start_waves();
        }
    }

    /// Who is in, for everyone's lobby.
    pub fn roster(&self) -> Msg {
        Msg::Roster { started: self.game.waves_on(), players: self.seats.iter().map(|s| (s.id, s.name.clone())).collect() }
    }

    /// Inputs from player `id`, the first of them number `first`. Ones already played or already here are
    /// dropped: each comes several times over (see `net::RESEND`).
    pub fn inputs(&mut self, id: u8, first: u32, inputs: &[Input]) {
        let Some(s) = self.seats.iter_mut().find(|s| s.id == id) else { return };
        for (k, inp) in inputs.iter().enumerate() {
            let n = first + k as u32;
            if n >= s.next {
                s.queue.entry(n).or_insert(*inp);
            }
        }
        // a client that starts over (a reload of the page keeps the seat) or jumps far ahead is caught up with
        if s.queue.len() > 120 {
            let keep = s.queue.split_off(&(first + inputs.len() as u32).saturating_sub(QUEUE as u32));
            s.queue = keep;
        }
    }

    /// One step: everyone's next input played, then the world.
    pub fn tick(&mut self) {
        for k in 0..self.seats.len() {
            let Some(i) = self.game.players.iter().position(|p| p.id == self.seats[k].id) else { continue };
            let s = &mut self.seats[k];
            // an input lost on the way, and every copy of it, is given up on
            if !s.queue.contains_key(&s.next) && !s.queue.is_empty() {
                s.waited += 1;
                if s.waited > GAP_WAIT {
                    s.next = *s.queue.keys().next().unwrap();
                }
            }
            let plays = if s.queue.len() > QUEUE { 2 } else { 1 };
            for _ in 0..plays {
                let Some(inp) = s.queue.remove(&s.next) else { break };
                s.ack = s.next;
                s.next += 1;
                s.waited = 0;
                self.game.step_player(i, inp, STEP);
            }
        }
        self.game.tick(STEP);
        self.game.events.clear();
        self.shots.append(&mut self.game.shots);
    }

    /// Whether this tick sends snapshots.
    pub fn snapshot_due(&self) -> bool {
        self.game.tick_n % SNAPSHOT_EVERY == 0
    }

    /// The game as player `id` is to see it now, as datagrams.
    pub fn snapshot_for(&self, id: u8) -> Vec<Vec<u8>> {
        let g = &self.game;
        let Some(seat) = self.seats.iter().find(|s| s.id == id) else { return Vec::new() };
        let own = g.players.iter().find(|p| p.id == id).map(|p| Own {
            ack: seat.ack,
            life: p.life,
            pos: p.body.pos,
            vel: p.body.vel,
            on_ground: p.body.on_ground,
            climb: p.climb.as_ref().map(Climb::raw),
            stamina: p.stamina,
            winded: p.winded,
            since_sprint: p.since_sprint,
            yaw: p.yaw,
            hp: p.hp,
            kills: p.kills,
            deaths: p.deaths,
            respawn_t: p.respawn_t,
        });
        let mates = g
            .players
            .iter()
            .filter(|p| p.id != id)
            .map(|p| Mate {
                id: p.id,
                pos: p.body.pos,
                yaw: p.yaw,
                pitch: p.pitch,
                weapon: p.weapon as u8,
                alive: p.alive(),
                on_ground: p.body.on_ground,
                aiming: p.aiming,
                reloading: p.reload_t > 0.0,
                ads: p.ads > 0.5,
                climbing: p.climb.is_some(),
                hp: p.hp.max(0.0),
            })
            .collect();
        // the fallen only while they are still falling: after that a client lets them lie where they were last seen
        let zombies = g
            .bots
            .iter()
            .filter(|b| b.alive || (b.dead_t < 1.5 && (b.corpse || b.dead_t < 0.2)))
            .map(|b| Zombie {
                id: b.id,
                pos: b.body.pos,
                face: b.face,
                kind: b.kind,
                alive: b.alive,
                corpse: b.corpse,
                on_ground: b.body.on_ground,
                vel: [b.body.vel[0], b.body.vel[2]],
                speed: b.speed,
                phase: b.phase,
                hit_t: b.hit_t,
                swing_t: b.swing_t,
                dead_t: b.dead_t,
                head: b.head,
                pose: b.climbing(),
            })
            .collect();
        let snap = Snapshot {
            tick: g.tick_n,
            own,
            world: Some(World { wave: g.wave, left: g.zombies_left(), started: g.waves_on() }),
            mates,
            zombies,
            nades: g.grenades.iter().map(|n| Nade { id: n.id, pos: n.pos, rot: n.rot }).collect(),
            // a blast is sent for a while, so one lost datagram doesn't lose it
            booms: g.blasts.iter().filter(|b| b.t < 0.5).map(|b| Boom { id: b.id, pos: b.pos, t: b.t }).collect(),
            shots: self.shots.clone(),
        };
        net::encode_snapshot(&snap)
    }

    /// After the snapshots have gone out: the shots in them are not sent again.
    pub fn sent(&mut self) {
        self.shots.clear();
    }
}
