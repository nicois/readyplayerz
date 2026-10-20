//! The wire format between the game server and the browsers playing on it. No dependencies, so the server and the
//! wasm client share it.
//!
//! There are two kinds of message. A few go on one reliable stream per player, each with a u16 length in front:
//! joining a room (`Hello`, answered by `Welcome` or `Refused`, after `Queued` while the server has no space for
//! another room), who is in it (`Roster`), a player's new name (`Name`) and starting it (`Start`).
//! Everything else goes as unreliable datagrams, which may be lost or come out of order, so each one stands on its
//! own: a client's last few inputs (`Inputs`), and the server's snapshots of the game, each split into as many
//! datagrams as it takes (see `encode_snapshot`), every one of them a list of records about one thing each.

use crate::sim::{Input, Pose, V3};

/// Bumped whenever the format changes, so an old page is told to reload instead of misreading.
pub const VERSION: u8 = 3;
/// The room asked for in a `Hello` to quick match: never a room's code, which is only letters.
pub const QUICK: &str = "*";
/// Most players in a room.
pub const MAX_PLAYERS: usize = 6;
/// How many of its latest inputs a client sends each time, so a lost datagram or two loses none of them.
pub const RESEND: usize = 8;
/// The most a datagram carries: well under what QUIC fits in one packet on any link, tunnels and proxies on the way
/// included (Fly's take about 70 bytes of every packet).
pub const DATAGRAM: usize = 1000;

/// Writes values little-endian.
#[derive(Default)]
pub struct W(pub Vec<u8>);

impl W {
    pub fn u8(&mut self, v: u8) -> &mut Self {
        self.0.push(v);
        self
    }
    pub fn u16(&mut self, v: u16) -> &mut Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    pub fn u32(&mut self, v: u32) -> &mut Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    pub fn f32(&mut self, v: f32) -> &mut Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    pub fn v3(&mut self, v: V3) -> &mut Self {
        self.f32(v[0]).f32(v[1]).f32(v[2])
    }
    /// A position to the nearest 1/64 m, anywhere within 512 m of the middle of the map.
    pub fn pos(&mut self, v: V3) -> &mut Self {
        for x in v {
            self.0.extend_from_slice(&((x * 64.0).round().clamp(-32768.0, 32767.0) as i16).to_le_bytes());
        }
        self
    }
    /// An angle, to a 65536th of a turn.
    pub fn angle(&mut self, a: f32) -> &mut Self {
        self.u16((a.rem_euclid(std::f32::consts::TAU) / std::f32::consts::TAU * 65536.0) as u32 as u16)
    }
    /// `v` from 0 to `max`, to a 255th of it.
    pub fn unit(&mut self, v: f32, max: f32) -> &mut Self {
        self.u8((v / max * 255.0).round().clamp(0.0, 255.0) as u8)
    }
    /// `v` from -`max` to `max`, to a 127th of it.
    pub fn signed(&mut self, v: f32, max: f32) -> &mut Self {
        self.u8((v / max * 127.0).round().clamp(-127.0, 127.0) as i8 as u8)
    }
    pub fn str(&mut self, s: &str) -> &mut Self {
        let b = &s.as_bytes()[..s.len().min(255)];
        self.u8(b.len() as u8);
        self.0.extend_from_slice(b);
        self
    }
}

/// Reads what `W` wrote; every read is None once the bytes run out.
pub struct R<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> R<'a> {
    pub fn new(b: &'a [u8]) -> R<'a> {
        R { b, at: 0 }
    }
    pub fn done(&self) -> bool {
        self.at >= self.b.len()
    }
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let s = self.b.get(self.at..self.at + n)?;
        self.at += n;
        Some(s)
    }
    pub fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }
    pub fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes(self.take(2)?.try_into().ok()?))
    }
    pub fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    pub fn f32(&mut self) -> Option<f32> {
        Some(f32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    pub fn v3(&mut self) -> Option<V3> {
        Some([self.f32()?, self.f32()?, self.f32()?])
    }
    pub fn pos(&mut self) -> Option<V3> {
        let mut v = [0.0; 3];
        for x in &mut v {
            *x = i16::from_le_bytes(self.take(2)?.try_into().ok()?) as f32 / 64.0;
        }
        Some(v)
    }
    pub fn angle(&mut self) -> Option<f32> {
        Some(self.u16()? as f32 / 65536.0 * std::f32::consts::TAU)
    }
    pub fn unit(&mut self, max: f32) -> Option<f32> {
        Some(self.u8()? as f32 / 255.0 * max)
    }
    pub fn signed(&mut self, max: f32) -> Option<f32> {
        Some(self.u8()? as i8 as f32 / 127.0 * max)
    }
    pub fn str(&mut self) -> Option<String> {
        let n = self.u8()? as usize;
        Some(String::from_utf8_lossy(self.take(n)?).into_owned())
    }
}

// ---- the reliable stream ----

/// A message on a player's reliable stream.
#[derive(Debug, PartialEq, Clone)]
pub enum Msg {
    /// client: to make a room on map `map` (`room` empty), to join room `room`, or (`room` is `QUICK`) to be put in
    /// any quick-match room that hasn't started, or a new one on any map
    Hello { version: u8, room: String, map: u8, name: String },
    /// client, the room's host: let the waves come
    Start,
    /// client: call me `name` from now on
    Name { name: String },
    /// server: you are in, as player `id`, in a game on map `map` made from `seed`
    Welcome { id: u8, room: String, map: u8, seed: u32 },
    /// server: who is in the room, the host first; whether it was found by quick match, and the seconds until it
    /// starts by itself (0 when it isn't counting down)
    Roster { started: bool, players: Vec<(u8, String)>, matched: bool, starts_in: u8 },
    /// server: no way in, and why
    Refused { why: String },
    /// server: every room is taken; you are `place` in line for the next (1 is next)
    Queued { place: u16 },
}

impl Msg {
    /// The message with its length in front, for the stream.
    pub fn frame(&self) -> Vec<u8> {
        let mut w = W::default();
        match self {
            Msg::Hello { version, room, map, name } => w.u8(1).u8(*version).str(room).u8(*map).str(name),
            Msg::Start => w.u8(2),
            Msg::Name { name } => w.u8(3).str(name),
            Msg::Welcome { id, room, map, seed } => w.u8(10).u8(*id).str(room).u8(*map).u32(*seed),
            Msg::Roster { started, players, matched, starts_in } => {
                w.u8(11).u8(*started as u8).u8(players.len() as u8);
                for (id, name) in players {
                    w.u8(*id).str(name);
                }
                w.u8(*matched as u8).u8(*starts_in)
            }
            Msg::Refused { why } => w.u8(12).str(why),
            Msg::Queued { place } => w.u8(13).u16(*place),
        };
        let mut out = (w.0.len() as u16).to_le_bytes().to_vec();
        out.extend_from_slice(&w.0);
        out
    }

    fn read(b: &[u8]) -> Option<Msg> {
        let mut r = R::new(b);
        Some(match r.u8()? {
            1 => Msg::Hello { version: r.u8()?, room: r.str()?, map: r.u8()?, name: r.str()? },
            2 => Msg::Start,
            3 => Msg::Name { name: r.str()? },
            10 => Msg::Welcome { id: r.u8()?, room: r.str()?, map: r.u8()?, seed: r.u32()? },
            11 => {
                let started = r.u8()? != 0;
                let n = r.u8()?;
                let players = (0..n).map(|_| Some((r.u8()?, r.str()?))).collect::<Option<_>>()?;
                Msg::Roster { started, players, matched: r.u8()? != 0, starts_in: r.u8()? }
            }
            12 => Msg::Refused { why: r.str()? },
            13 => Msg::Queued { place: r.u16()? },
            _ => return None,
        })
    }
}

/// Gathers bytes off a stream into whole messages.
#[derive(Default)]
pub struct Frames(Vec<u8>);

impl Frames {
    /// Adds `bytes`, and returns every message now complete. A message that makes no sense is dropped.
    pub fn push(&mut self, bytes: &[u8]) -> Vec<Msg> {
        self.0.extend_from_slice(bytes);
        let mut out = Vec::new();
        while self.0.len() >= 2 {
            let n = u16::from_le_bytes([self.0[0], self.0[1]]) as usize;
            if self.0.len() < 2 + n {
                break;
            }
            out.extend(Msg::read(&self.0[2..2 + n]));
            self.0.drain(..2 + n);
        }
        out
    }
}

// ---- datagrams ----

const INPUTS: u8 = 1;
const SNAPSHOT: u8 = 2;

/// A client's latest inputs, oldest first: the first is number `first`, and so on up.
pub fn encode_inputs(first: u32, inputs: &[Input]) -> Vec<u8> {
    let mut w = W::default();
    w.u8(INPUTS).u32(first).u8(inputs.len() as u8);
    for i in inputs {
        let keys = [i.w, i.a, i.s, i.d, i.jump, i.sprint, i.fire, i.ads, i.reload, i.throw];
        w.u16(keys.iter().enumerate().map(|(k, &on)| (on as u16) << k).sum());
        let [yaw, pitch] = i.view.unwrap_or([0.0; 2]);
        w.f32(yaw).f32(pitch).u8(i.select.unwrap_or(255)).u8(i.pick.unwrap_or(255)).f32(i.seen);
    }
    w.0
}

pub fn decode_inputs(b: &[u8]) -> Option<(u32, Vec<Input>)> {
    let mut r = R::new(b);
    if r.u8()? != INPUTS {
        return None;
    }
    let first = r.u32()?;
    let n = r.u8()? as usize;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let keys = r.u16()?;
        let on = |k: u16| keys & (1 << k) != 0;
        let (yaw, pitch, select, pick, seen) = (r.f32()?, r.f32()?, r.u8()?, r.u8()?, r.f32()?);
        out.push(Input {
            w: on(0),
            a: on(1),
            s: on(2),
            d: on(3),
            jump: on(4),
            sprint: on(5),
            fire: on(6),
            ads: on(7),
            reload: on(8),
            throw: on(9),
            look_dx: 0.0,
            look_dy: 0.0,
            view: Some([yaw, pitch]),
            select: (select != 255).then_some(select),
            pick: (pick != 255).then_some(pick),
            seen,
        });
    }
    Some((first, out))
}

/// The player a snapshot is for, as the server has them, to the full: where the client's own copy of them is put
/// back to before it plays its inputs since again.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Own {
    /// the last of their inputs the server has played
    pub ack: u32,
    pub life: u32,
    pub pos: V3,
    pub vel: V3,
    pub on_ground: bool,
    /// a pull up onto a ledge partway through: see `sim::Climb`
    pub climb: Option<[f32; 14]>,
    pub stamina: f32,
    pub winded: bool,
    pub since_sprint: f32,
    /// facing, for a new life
    pub yaw: f32,
    pub hp: f32,
    pub kills: u32,
    pub deaths: u32,
    pub respawn_t: f32,
}

/// How the game is going.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct World {
    pub wave: u32,
    pub left: u32,
    pub started: bool,
}

/// Another player, as much as is needed to draw them.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Mate {
    pub id: u8,
    pub pos: V3,
    pub yaw: f32,
    pub pitch: f32,
    pub weapon: u8,
    pub alive: bool,
    pub on_ground: bool,
    /// holding a grenade, reloading, aiming down the sights
    pub aiming: bool,
    pub reloading: bool,
    pub ads: bool,
    pub climbing: bool,
    pub hp: f32,
    /// 0 walking, 1 sprinting, eased between the two
    pub sprint: f32,
}

/// A zombie, as much as is needed to draw it.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Zombie {
    pub id: u32,
    pub pos: V3,
    pub face: f32,
    pub kind: u8,
    pub alive: bool,
    pub corpse: bool,
    pub on_ground: bool,
    /// its speed along the ground, for the walk
    pub vel: [f32; 2],
    pub speed: f32,
    pub phase: f32,
    pub hit_t: f32,
    pub swing_t: f32,
    pub dead_t: f32,
    pub head: V3,
    pub pose: Option<Pose>,
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Nade {
    pub id: u32,
    pub pos: V3,
    pub rot: V3,
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Boom {
    pub id: u32,
    pub pos: V3,
    pub t: f32,
}

/// One datagram of a snapshot: the server tick it was taken at, and whatever records it carries.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Snapshot {
    pub tick: u32,
    pub own: Option<Own>,
    pub world: Option<World>,
    pub mates: Vec<Mate>,
    pub zombies: Vec<Zombie>,
    pub nades: Vec<Nade>,
    pub booms: Vec<Boom>,
    pub shots: Vec<crate::sim::Shot>,
}

const OWN: u8 = 1;
const WORLD: u8 = 2;
const MATE: u8 = 3;
const ZOMBIE: u8 = 4;
const NADE: u8 = 5;
const BOOM: u8 = 6;
const SHOT: u8 = 7;

/// A snapshot as datagrams of up to `DATAGRAM` bytes, each standing on its own. The player's own record and how
/// the game is going come first, so they are in the first.
pub fn encode_snapshot(s: &Snapshot) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut w = W::default();
    let start = |w: &mut W| {
        w.u8(SNAPSHOT).u32(s.tick);
    };
    start(&mut w);
    let rec = |out: &mut Vec<Vec<u8>>, w: &mut W, r: W| {
        if w.0.len() + r.0.len() > DATAGRAM {
            out.push(std::mem::take(&mut w.0));
            start(w);
        }
        w.0.extend_from_slice(&r.0);
    };
    if let Some(o) = &s.own {
        let mut r = W::default();
        r.u8(OWN).u32(o.ack).u32(o.life).v3(o.pos).v3(o.vel);
        r.u8(o.on_ground as u8 | (o.winded as u8) << 1 | (o.climb.is_some() as u8) << 2);
        if let Some(c) = &o.climb {
            for &x in c {
                r.f32(x);
            }
        }
        r.f32(o.stamina).f32(o.since_sprint).f32(o.yaw).f32(o.hp).u16(o.kills as u16).u16(o.deaths as u16).f32(o.respawn_t);
        rec(&mut out, &mut w, r);
    }
    if let Some(g) = &s.world {
        let mut r = W::default();
        r.u8(WORLD).u16(g.wave as u16).u16(g.left as u16).u8(g.started as u8);
        rec(&mut out, &mut w, r);
    }
    for m in &s.mates {
        let mut r = W::default();
        let flags = [m.alive, m.on_ground, m.aiming, m.reloading, m.ads, m.climbing];
        r.u8(MATE).u8(m.id).pos(m.pos).angle(m.yaw).signed(m.pitch, 1.6).u8(m.weapon);
        r.u8(flags.iter().enumerate().map(|(k, &on)| (on as u8) << k).sum()).unit(m.hp, 100.0).unit(m.sprint, 1.0);
        rec(&mut out, &mut w, r);
    }
    for z in &s.zombies {
        let mut r = W::default();
        let flags = z.kind | (z.alive as u8) << 2 | (z.corpse as u8) << 3 | (z.on_ground as u8) << 4 | (z.pose.is_some() as u8) << 5;
        r.u8(ZOMBIE).u32(z.id).pos(z.pos).angle(z.face).u8(flags);
        r.signed(z.vel[0], 12.0).signed(z.vel[1], 12.0).unit(z.speed, 8.0).unit(z.phase, 6.3);
        r.unit(z.hit_t, 1.0).unit(z.swing_t, 1.0).unit(z.dead_t, 12.0);
        r.signed(z.head[0], 0.25).signed(z.head[1], 0.25).signed(z.head[2], 0.25);
        if let Some((s, up, ahead, duck)) = z.pose {
            r.unit(s, 1.0).signed(up, 4.0).signed(ahead, 3.0).unit(duck, 1.0);
        }
        rec(&mut out, &mut w, r);
    }
    for n in &s.nades {
        let mut r = W::default();
        r.u8(NADE).u32(n.id).pos(n.pos).angle(n.rot[0]).angle(n.rot[1]).angle(n.rot[2]);
        rec(&mut out, &mut w, r);
    }
    for b in &s.booms {
        let mut r = W::default();
        r.u8(BOOM).u32(b.id).pos(b.pos).unit(b.t, 2.0);
        rec(&mut out, &mut w, r);
    }
    for s in &s.shots {
        let mut r = W::default();
        r.u8(SHOT).u8(s.by).pos(s.a).pos(s.b);
        rec(&mut out, &mut w, r);
    }
    out.push(w.0);
    out
}

/// One datagram of a snapshot, if that is what it is.
pub fn decode_snapshot(b: &[u8]) -> Option<Snapshot> {
    let mut r = R::new(b);
    if r.u8()? != SNAPSHOT {
        return None;
    }
    let mut s = Snapshot { tick: r.u32()?, ..Default::default() };
    while !r.done() {
        match r.u8()? {
            OWN => {
                let (ack, life, pos, vel, flags) = (r.u32()?, r.u32()?, r.v3()?, r.v3()?, r.u8()?);
                let climb = if flags & 4 != 0 {
                    let mut c = [0.0; 14];
                    for x in &mut c {
                        *x = r.f32()?;
                    }
                    Some(c)
                } else {
                    None
                };
                let (stamina, since_sprint, yaw, hp) = (r.f32()?, r.f32()?, r.f32()?, r.f32()?);
                let (kills, deaths, respawn_t) = (r.u16()? as u32, r.u16()? as u32, r.f32()?);
                s.own = Some(Own { ack, life, pos, vel, on_ground: flags & 1 != 0, winded: flags & 2 != 0, climb, stamina, since_sprint, yaw, hp, kills, deaths, respawn_t });
            }
            WORLD => s.world = Some(World { wave: r.u16()? as u32, left: r.u16()? as u32, started: r.u8()? != 0 }),
            MATE => {
                let (id, pos, yaw, pitch, weapon, flags, hp) = (r.u8()?, r.pos()?, r.angle()?, r.signed(1.6)?, r.u8()?, r.u8()?, r.unit(100.0)?);
                let (on, sprint) = (|k: u8| flags & (1 << k) != 0, r.unit(1.0)?);
                s.mates.push(Mate { id, pos, yaw, pitch, weapon, alive: on(0), on_ground: on(1), aiming: on(2), reloading: on(3), ads: on(4), climbing: on(5), hp, sprint });
            }
            ZOMBIE => {
                let (id, pos, face, flags) = (r.u32()?, r.pos()?, r.angle()?, r.u8()?);
                let (vel, speed, phase) = ([r.signed(12.0)?, r.signed(12.0)?], r.unit(8.0)?, r.unit(6.3)?);
                let (hit_t, swing_t, dead_t) = (r.unit(1.0)?, r.unit(1.0)?, r.unit(12.0)?);
                let head = [r.signed(0.25)?, r.signed(0.25)?, r.signed(0.25)?];
                let pose = if flags & 32 != 0 { Some((r.unit(1.0)?, r.signed(4.0)?, r.signed(3.0)?, r.unit(1.0)?)) } else { None };
                let (kind, alive, corpse, on_ground) = (flags & 3, flags & 4 != 0, flags & 8 != 0, flags & 16 != 0);
                s.zombies.push(Zombie { id, pos, face, kind, alive, corpse, on_ground, vel, speed, phase, hit_t, swing_t, dead_t, head, pose });
            }
            NADE => s.nades.push(Nade { id: r.u32()?, pos: r.pos()?, rot: [r.angle()?, r.angle()?, r.angle()?] }),
            BOOM => s.booms.push(Boom { id: r.u32()?, pos: r.pos()?, t: r.unit(2.0)? }),
            SHOT => s.shots.push(crate::sim::Shot { by: r.u8()?, a: r.pos()?, b: r.pos()? }),
            _ => return None,
        }
    }
    Some(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_messages_come_back_the_same_however_the_bytes_are_split() {
        let msgs = vec![
            Msg::Hello { version: VERSION, room: "ABCD".into(), map: 1, name: "Val".into() },
            Msg::Start,
            Msg::Name { name: "Friend".into() },
            Msg::Welcome { id: 3, room: "ABCD".into(), map: 1, seed: 1234567 },
            Msg::Hello { version: VERSION, room: QUICK.into(), map: 0, name: "".into() },
            Msg::Roster { started: true, players: vec![(0, "Val".into()), (3, "Friend".into())], matched: true, starts_in: 27 },
            Msg::Refused { why: "No such room".into() },
            Msg::Queued { place: 300 },
        ];
        let bytes: Vec<u8> = msgs.iter().flat_map(|m| m.frame()).collect();
        let mut f = Frames::default();
        let mut got = Vec::new();
        for chunk in bytes.chunks(3) {
            got.extend(f.push(chunk));
        }
        assert_eq!(got, msgs);
    }

    #[test]
    fn inputs_come_back_the_same() {
        let a = Input { w: true, fire: true, throw: true, view: Some([1.25, -0.3]), select: Some(1), seen: 812.5, ..Default::default() };
        let b = Input { s: true, d: true, ads: true, view: Some([-3.0, 0.7]), pick: Some(9), ..Default::default() };
        let (first, back) = decode_inputs(&encode_inputs(41, &[a, b])).unwrap();
        assert_eq!((first, back), (41, vec![a, b]));
    }

    #[test]
    fn a_big_snapshot_is_split_into_datagrams_that_each_read_on_their_own() {
        let own = Own { ack: 99, life: 2, pos: [1.5, 0.25, -30.125], vel: [0.5, -3.0, 1.0], climb: Some([0.5; 14]), stamina: 0.4, hp: 64.0, kills: 12, ..Default::default() };
        let zombie = |i: u32| Zombie { id: i, pos: [i as f32, 0.0, -2.0 * i as f32], face: 1.0, kind: (i % 3) as u8, alive: i % 2 == 0, speed: 3.0, pose: (i % 5 == 0).then_some((0.5, 1.0, 0.5, 0.25)), ..Default::default() };
        let s = Snapshot {
            tick: 5000,
            own: Some(own.clone()),
            world: Some(World { wave: 3, left: 14, started: true }),
            mates: vec![Mate { id: 2, pos: [10.0, 3.0, 4.0], yaw: 2.0, pitch: -0.2, weapon: 4, alive: true, ads: true, hp: 80.0, ..Default::default() }],
            zombies: (1..=60).map(zombie).collect(),
            nades: vec![Nade { id: 77, pos: [1.0, 1.0, 1.0], rot: [0.1, 0.2, 0.3] }],
            booms: vec![Boom { id: 78, pos: [2.0, 0.1, 2.0], t: 0.5 }],
            shots: vec![crate::sim::Shot { by: 2, a: [0.0, 1.5, 0.0], b: [5.0, 1.0, -20.0] }],
        };
        let parts = encode_snapshot(&s);
        assert!(parts.len() > 1 && parts.iter().all(|p| p.len() <= DATAGRAM), "{:?}", parts.iter().map(Vec::len).collect::<Vec<_>>());
        let back: Vec<Snapshot> = parts.iter().map(|p| decode_snapshot(p).unwrap()).collect();
        assert!(back.iter().all(|b| b.tick == 5000));
        assert_eq!(back[0].own.as_ref(), Some(&own), "the player's own record is exact");
        let zs: Vec<Zombie> = back.iter().flat_map(|b| b.zombies.clone()).collect();
        assert_eq!(zs.len(), 60);
        for (z, was) in zs.iter().zip(&s.zombies) {
            assert_eq!((z.id, z.kind, z.alive, z.pose.is_some()), (was.id, was.kind, was.alive, was.pose.is_some()));
            assert!((0..3).all(|k| (z.pos[k] - was.pos[k]).abs() < 0.01));
        }
        let last = back.last().unwrap();
        assert_eq!((last.nades.len(), last.booms.len(), last.shots.len()), (1, 1, 1));
        assert!((back[0].mates[0].yaw - 2.0).abs() < 0.001 && back[0].mates[0].ads);
    }
}
