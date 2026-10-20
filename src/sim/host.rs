//! The server's side of a room: the real game, everyone's inputs played into it as they come in, and the
//! snapshots of it each player is sent. Nothing here touches the network itself; the server binary moves the bytes.

use super::*;
use crate::net::{self, Boom, Loot, Mate, Msg, Nade, Own, Snapshot, World, Zombie, MAX_PLAYERS};
use std::collections::BTreeMap;

/// A snapshot goes out every this many ticks.
pub const SNAPSHOT_EVERY: u32 = 2;
/// A player's inputs are kept waiting at most this many before the oldest are played two a tick to catch up, so
/// a client whose clock runs a little fast doesn't fall further and further behind.
const QUEUE: usize = 4;
/// Ticks to wait for an input that was lost on the way before going on without it.
const GAP_WAIT: u32 = 3;
/// A quick-match room starts by itself this many seconds after a second player is in, or after this many once
/// it is full.
pub const MATCH_WAIT: f32 = 30.0;
pub const MATCH_FULL: f32 = 5.0;

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

/// The name player `id` is shown by: letters, digits, spaces and a few marks only, so it is safe to show anywhere,
/// and "Player n" when that leaves nothing.
fn clean_name(name: &str, id: u8) -> String {
    let name: String = name
        .chars()
        .filter(|c| c.is_alphanumeric() || " -_.".contains(*c))
        .take(16)
        .collect();
    if name.trim().is_empty() {
        format!("Player {}", id + 1)
    } else {
        name.trim().to_string()
    }
}

pub struct Room {
    pub game: Game,
    pub map: u8,
    pub seed: u32,
    /// found by quick match: strangers come and go, and it starts by itself (see `MATCH_WAIT`)
    pub matched: bool,
    seats: Vec<Seat>,
    /// the shots fired since the last snapshot
    shots: Vec<Shot>,
    /// ticks until a quick-match room starts by itself, while it is counting down
    countdown: Option<u32>,
}

impl Room {
    /// A room on map `map`, made from `seed`, with nobody in it and the waves held off until the host starts.
    pub fn new(map: u8, seed: u32) -> Room {
        Room::with(Game::on(map, seed), map, seed)
    }

    /// A room on the map of `game`.
    pub(crate) fn with(mut game: Game, map: u8, seed: u32) -> Room {
        game.role = Role::Server;
        game.loot = true;
        game.players.clear();
        game.hold_waves();
        Room {
            game,
            map,
            seed,
            matched: false,
            seats: Vec::new(),
            shots: Vec::new(),
            countdown: None,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.seats.is_empty()
    }

    /// How many are in.
    pub fn players(&self) -> usize {
        self.seats.len()
    }

    /// A quick-match room's countdown after someone comes or goes: it starts once a second player is in, is cut
    /// short when the room fills, and stops if all but one leave.
    fn recount(&mut self) {
        if !self.matched || self.game.waves_on() {
            return;
        }
        let ticks = |s: f32| (s / STEP).round() as u32;
        self.countdown = match self.seats.len() {
            0 | 1 => None,
            MAX_PLAYERS => Some(self.countdown.unwrap_or(u32::MAX).min(ticks(MATCH_FULL))),
            _ => self.countdown.or(Some(ticks(MATCH_WAIT))),
        };
    }

    /// Whole seconds until a quick-match room starts by itself, 0 when it isn't counting down.
    pub fn starts_in(&self) -> u8 {
        self.countdown.map_or(0, |t| {
            t.div_ceil((1.0 / STEP).round() as u32).clamp(1, 255) as u8
        })
    }

    /// Someone coming in, called `name`: their player's id, or None if the room is full.
    pub fn join(&mut self, name: &str) -> Option<u8> {
        if self.seats.len() >= MAX_PLAYERS {
            return None;
        }
        let id = (0..=u8::MAX).find(|id| self.seats.iter().all(|s| s.id != *id))?;
        self.seats.push(Seat {
            id,
            name: clean_name(name, id),
            queue: BTreeMap::new(),
            next: 0,
            waited: 0,
            ack: 0,
        });
        self.game.add_player(id);
        self.recount();
        Some(id)
    }

    /// Player `id` going by `name` from now on.
    pub fn rename(&mut self, id: u8, name: &str) {
        if let Some(s) = self.seats.iter_mut().find(|s| s.id == id) {
            s.name = clean_name(name, id);
        }
    }

    pub fn leave(&mut self, id: u8) {
        self.seats.retain(|s| s.id != id);
        self.game.remove_player(id);
        self.recount();
    }

    /// The host, who starts the game: whoever has been in longest.
    pub fn host(&self) -> Option<u8> {
        self.seats.first().map(|s| s.id)
    }

    /// The host letting the waves come.
    pub fn start(&mut self, by: u8) {
        if self.host() == Some(by) && !self.game.waves_on() {
            self.game.start_waves();
            self.countdown = None;
        }
    }

    /// Who is in, for everyone's lobby.
    pub fn roster(&self) -> Msg {
        let players = self.seats.iter().map(|s| (s.id, s.name.clone())).collect();
        Msg::Roster {
            started: self.game.waves_on(),
            players,
            matched: self.matched,
            starts_in: self.starts_in(),
        }
    }

    /// Inputs from player `id`, the first of them number `first`. Ones already played or already here are
    /// dropped: each comes several times over (see `net::RESEND`).
    pub fn inputs(&mut self, id: u8, first: u32, inputs: &[Input]) {
        let Some(s) = self.seats.iter_mut().find(|s| s.id == id) else {
            return;
        };
        for (k, inp) in inputs.iter().enumerate() {
            let n = first + k as u32;
            if n >= s.next {
                s.queue.entry(n).or_insert(*inp);
            }
        }
        // a client that starts over (a reload of the page keeps the seat) or jumps far ahead is caught up with
        if s.queue.len() > 120 {
            let keep = s
                .queue
                .split_off(&(first + inputs.len() as u32).saturating_sub(QUEUE as u32));
            s.queue = keep;
        }
    }

    /// One step: everyone's next input played, then the world.
    pub fn tick(&mut self) {
        if let Some(t) = &mut self.countdown {
            *t = t.saturating_sub(1);
            if *t == 0 {
                self.countdown = None;
                if let Some(host) = self.host() {
                    self.start(host);
                }
            }
        }
        for k in 0..self.seats.len() {
            let Some(i) = self
                .game
                .players
                .iter()
                .position(|p| p.id == self.seats[k].id)
            else {
                continue;
            };
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
                let Some(inp) = s.queue.remove(&s.next) else {
                    break;
                };
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

    /// The game as each player in the room is to see it now, as datagrams, by player id. What everyone is sent is
    /// encoded once and packed into each player's datagrams with their own record and everyone else's.
    pub fn snapshots(&self) -> Vec<(u8, Vec<Vec<u8>>)> {
        let g = &self.game;
        let shared = net::encode_records(&self.shared_snapshot());
        let (world, rest) = shared
            .split_first()
            .expect("a snapshot always says how the game is going");
        let mates: Vec<(u8, Vec<u8>)> = g
            .players
            .iter()
            .map(|p| {
                (
                    p.id,
                    net::encode_records(&Snapshot {
                        mates: vec![mate(p)],
                        ..Default::default()
                    })
                    .remove(0),
                )
            })
            .collect();
        self.seats
            .iter()
            .map(|seat| {
                let own = self.own(seat).map(|own| {
                    net::encode_records(&Snapshot {
                        own: Some(own),
                        ..Default::default()
                    })
                    .remove(0)
                });
                let others = mates
                    .iter()
                    .filter(|(id, _)| *id != seat.id)
                    .map(|(_, r)| r);
                let records = own
                    .iter()
                    .chain([world])
                    .chain(others)
                    .chain(rest)
                    .map(Vec::as_slice);
                (seat.id, net::pack(g.tick_n, records))
            })
            .collect()
    }

    /// The snapshot player `id` is sent, before it is put in datagrams, to see what it is made of (see
    /// `sim::load`); None if they aren't in.
    #[cfg(test)]
    pub(crate) fn snapshot_of(&self, id: u8) -> Option<Snapshot> {
        let seat = self.seats.iter().find(|s| s.id == id)?;
        let mates = self
            .game
            .players
            .iter()
            .filter(|p| p.id != id)
            .map(mate)
            .collect();
        Some(Snapshot {
            own: self.own(seat),
            mates,
            ..self.shared_snapshot()
        })
    }

    /// What `seat`'s player is sent of themselves, while they are in the game.
    fn own(&self, seat: &Seat) -> Option<Own> {
        self.game
            .players
            .iter()
            .find(|p| p.id == seat.id)
            .map(|p| Own {
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
                kit: p.kit(),
            })
    }

    /// What everyone is sent alike: how the game is going, the zombies, grenades, blasts, shots and loot.
    fn shared_snapshot(&self) -> Snapshot {
        let g = &self.game;
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
        Snapshot {
            tick: g.tick_n,
            own: None,
            world: Some(World {
                wave: g.wave,
                left: g.zombies_left(),
                started: g.waves_on(),
            }),
            mates: Vec::new(),
            zombies,
            nades: g
                .grenades
                .iter()
                .map(|n| Nade {
                    id: n.id,
                    pos: n.pos,
                    rot: n.rot,
                })
                .collect(),
            // a blast is sent for a while, so one lost datagram doesn't lose it
            booms: g
                .blasts
                .iter()
                .filter(|b| b.t < 0.5)
                .map(|b| Boom {
                    id: b.id,
                    pos: b.pos,
                    t: b.t,
                })
                .collect(),
            shots: self.shots.clone(),
            loot: g
                .boxes
                .iter()
                .map(|&pos| Loot { pos, gun: 255 })
                .chain(g.crates.iter().map(|&(pos, gun)| Loot {
                    pos,
                    gun: gun as u8,
                }))
                .collect(),
        }
    }

    /// After the snapshots have gone out: the shots in them are not sent again.
    pub fn sent(&mut self) {
        self.shots.clear();
    }
}

/// What the others in the room are sent of player `p`.
fn mate(p: &Player) -> Mate {
    Mate {
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
        sprint: p.sprint_t,
        kills: p.kills,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn room() -> Room {
        Room::with(Game::with_walls(1, map::perimeter()), 0, 1)
    }

    fn names(room: &Room) -> Vec<(u8, String)> {
        let Msg::Roster { players, .. } = room.roster() else {
            unreachable!()
        };
        players
    }

    #[test]
    fn a_room_seats_six_and_whoever_has_been_in_longest_hosts_it() {
        let mut r = room();
        let ids: Vec<u8> = (0..MAX_PLAYERS).map(|_| r.join("").unwrap()).collect();
        assert_eq!(ids, [0, 1, 2, 3, 4, 5]);
        assert_eq!(r.join("one too many"), None);
        assert_eq!(r.game.players.len(), 6);
        // only the host starts it
        r.start(1);
        assert!(!r.game.waves_on());
        // the host going hands it on, and their seat is the next one taken
        r.leave(0);
        assert_eq!(r.host(), Some(1));
        assert_eq!(r.join(""), Some(0));
        r.start(1);
        assert!(r.game.waves_on());
    }

    #[test]
    fn a_quick_match_room_starts_itself_once_two_are_in_and_soon_after_it_fills() {
        let secs = |r: &mut Room, s: f32| (0..(s / STEP).round() as u32).for_each(|_| r.tick());
        let mut r = room();
        r.matched = true;
        r.join("");
        secs(&mut r, 60.0);
        assert_eq!(r.starts_in(), 0, "alone, it waits");
        r.join("");
        assert_eq!(r.starts_in(), MATCH_WAIT as u8);
        // down to one stops the clock, and a second starts it over
        r.leave(1);
        assert_eq!(r.starts_in(), 0);
        r.join("");
        secs(&mut r, 10.0);
        assert_eq!(r.starts_in(), (MATCH_WAIT - 10.0) as u8);
        // full: a few seconds more
        (2..MAX_PLAYERS).for_each(|_| _ = r.join(""));
        assert_eq!(r.starts_in(), MATCH_FULL as u8);
        secs(&mut r, MATCH_FULL - 0.1);
        assert!(!r.game.waves_on());
        secs(&mut r, 0.2);
        assert!(r.game.waves_on());
        assert_eq!(r.starts_in(), 0);
        // a room made by code only starts when its host says
        let mut r = room();
        (0..MAX_PLAYERS).for_each(|_| _ = r.join(""));
        secs(&mut r, 60.0);
        assert!(!r.game.waves_on());
    }

    /// Packing the shared records once into everyone's datagrams sends each player the very bytes a snapshot made
    /// for them alone would, however many datagrams it takes.
    #[test]
    fn everyones_snapshots_are_what_each_would_be_sent_alone() {
        let mut r = room();
        (0..4).for_each(|_| _ = r.join(""));
        r.start(0);
        for _ in 0..(20.0 / STEP) as u32 {
            r.tick();
        }
        r.game.boxes.push([3.0, 0.0, 4.0]);
        r.game.crates.push(([-5.0, 0.0, 2.0], 4));
        assert!(
            r.game.bots.len() > 4,
            "zombies enough to take more than one datagram"
        );
        let all = r.snapshots();
        assert_eq!(
            all.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            [0, 1, 2, 3]
        );
        for (id, datagrams) in all {
            let seat = r.seats.iter().find(|s| s.id == id).unwrap();
            let alone = Snapshot {
                own: r.own(seat),
                mates: r
                    .game
                    .players
                    .iter()
                    .filter(|p| p.id != id)
                    .map(mate)
                    .collect(),
                ..r.shared_snapshot()
            };
            assert_eq!(datagrams, net::encode_snapshot(&alone), "player {id}");
        }
    }

    #[test]
    fn names_keep_only_what_is_safe_to_show_and_blank_is_player_n() {
        let mut r = room();
        r.join("<b>Val</b> \"x\"");
        r.join("   ");
        r.join("Ä-1_b.c with a much too long name");
        assert_eq!(
            names(&r),
            [
                (0, "bValb x".into()),
                (1, "Player 2".into()),
                (2, "Ä-1_b.c with a m".into())
            ]
        );
        r.rename(1, "Friend");
        r.rename(0, "");
        assert_eq!(
            names(&r)[..2],
            [(0, "Player 1".into()), (1, "Friend".into())]
        );
    }
}
