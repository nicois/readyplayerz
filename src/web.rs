//! Browser glue: owns the game and renderer, and is driven from JS. Playing online, JS moves the bytes (see
//! `web/net.js`) and the game is a copy of the server's (see `sim::mirror`).

use crate::net::{Frames, Msg, MAX_PLAYERS, VERSION};
use crate::render::Renderer;
use crate::sim::mirror::Mirror;
use crate::sim::{Game, PRIMARIES, STEP, WEAPONS};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{HtmlCanvasElement, WebGl2RenderingContext};

/// Playing in a room on the server.
struct Online {
    mirror: Mirror,
    room: String,
    map: u8,
    started: bool,
    /// who is in, the host first
    players: Vec<(u8, String)>,
    /// found by quick match, and the seconds until it starts by itself (0 when it isn't counting down)
    matched: bool,
    starts_in: u8,
}

#[wasm_bindgen]
pub struct Client {
    game: Game,
    renderer: Renderer,
    acc: f32,
    last: f64,
    paused: bool,
    /// which map, and what it was made from, so ending the game can start a fresh one on the same map
    map: u8,
    seed: u32,
    online: Option<Online>,
    /// the reliable stream's bytes so far, gathered into messages
    frames: Frames,
    /// why the server wouldn't let us in, or the connection was lost
    net_error: String,
    /// our place in line for a room while the server has none free (1 is next), 0 when not waiting
    queued: u16,
    /// guns picked for the loadout, still to be told to the server: one an input
    picks: Vec<u8>,
}

#[wasm_bindgen]
impl Client {
    #[wasm_bindgen(constructor)]
    pub fn new(canvas: HtmlCanvasElement) -> Result<Client, JsValue> {
        std::panic::set_hook(Box::new(|info| web_sys::console::error_1(&info.to_string().into())));
        // the picture is drawn, multisampled, into the renderer's own buffers and only copied onto the canvas,
        // so the canvas needs no multisampling or depth of its own: they would cost a resolve and memory per frame
        let options = js_sys::Object::new();
        for (k, v) in [("antialias", false), ("depth", false), ("stencil", false)] {
            js_sys::Reflect::set(&options, &k.into(), &v.into())?;
        }
        let gl = canvas
            .get_context_with_context_options("webgl2", &options)?
            .ok_or("WebGL2 is not available")?
            .dyn_into::<WebGl2RenderingContext>()?;
        let mut renderer = Renderer::new(gl, canvas.width(), canvas.height())?;
        let seed = (js_sys::Math::random() * 4_000_000_000.0) as u32;
        let map = crate::map::played(0);
        let mut game = Game::on(map, seed);
        game.set_loot(true);
        renderer.upload_world(&game)?;
        Ok(Client { game, renderer, acc: 0.0, last: 0.0, paused: true, map, seed, online: None, frames: Frames::default(), net_error: String::new(), queued: 0, picks: Vec::new() })
    }

    /// The game being played: the room's, online.
    fn game(&self) -> &Game {
        self.online.as_ref().map_or(&self.game, |o| &o.mirror.game)
    }
    fn game_mut(&mut self) -> &mut Game {
        match &mut self.online {
            Some(o) => &mut o.mirror.game,
            None => &mut self.game,
        }
    }

    /// Ends the game: a fresh one on the same map, with the score cleared and the loadout picked so far.
    pub fn end_game(&mut self) {
        let picked = self.game().me().picked;
        self.game = Game::on(self.map, self.seed);
        self.game.set_loot(true);
        for i in picked {
            self.game.pick(i);
        }
        self.acc = 0.0;
    }

    /// A fresh game on map number `map` (0 the town, 1 the suburb), keeping the loadout picked so far. Only the
    /// suburb is played for now, so any number is the suburb (see `map::played`).
    pub fn set_map(&mut self, map: u8) -> Result<(), JsValue> {
        self.map = crate::map::played(map);
        self.end_game();
        Ok(self.renderer.upload_world(&self.game)?)
    }

    /// The map played alone (0 the town, 1 the suburb).
    pub fn map(&self) -> u8 {
        self.map
    }

    /// Dev view: a fresh game on the map made from `seed`, so a picture can be taken in the same place again.
    pub fn set_seed(&mut self, seed: u32) -> Result<(), JsValue> {
        self.seed = seed;
        self.game = Game::on(self.map, seed);
        self.game.set_loot(true);
        Ok(self.renderer.upload_world(&self.game)?)
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.renderer.width = width;
        self.renderer.height = height;
    }

    /// Graphics quality: 0 low, 1 medium, 2 high. JS resizes the canvas to match.
    pub fn set_quality(&mut self, q: u8) {
        self.renderer.quality = q;
    }

    pub fn key(&mut self, code: &str, down: bool) {
        let online = self.online.is_some();
        let g = self.game_mut();
        let i = &mut g.me_mut().input;
        match code {
            "KeyW" | "ArrowUp" => i.w = down,
            "KeyA" | "ArrowLeft" => i.a = down,
            "KeyS" | "ArrowDown" => i.s = down,
            "KeyD" | "ArrowRight" => i.d = down,
            "Space" => i.jump = down,
            "ShiftLeft" | "ShiftRight" => i.sprint = down,
            "KeyR" => i.reload = down,
            "KeyG" => i.throw = down,
            _ if down => {
                // 1 primary, 2 sidearm; the loadout itself is picked in the menu
                if let Some(n) = code.strip_prefix("Digit").and_then(|d| d.parse::<usize>().ok()) {
                    let slot = n.wrapping_sub(1);
                    if online {
                        i.select = Some(slot.min(255) as u8);
                    } else if g.me().alive() {
                        g.select_slot(slot);
                    }
                }
            }
            _ => {}
        }
    }

    /// Mouse wheel: down (`dir` > 0) picks the pistol, up picks the primary, so extra clicks keep the same gun.
    pub fn cycle_weapon(&mut self, dir: i32) {
        let slot = if dir > 0 { 1 } else { 0 };
        if self.online.is_some() {
            self.game_mut().me_mut().input.select = Some(slot as u8);
        } else {
            self.game.select_slot(slot);
        }
    }

    /// The menu is up: keep drawing, stop the simulation. Online the game goes on, with the player standing still.
    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
    }

    /// Puts the player somewhere and facing some way. For looking around and testing.
    pub fn teleport(&mut self, x: f32, y: f32, z: f32, yaw: f32, pitch: f32) {
        let p = self.game.me_mut();
        p.body.pos = [x, y, z];
        p.step_y = 0.0;
        p.climb = None;
        p.yaw = yaw;
        p.pitch = pitch;
    }

    /// Dev view: the bat in hand `u` of the way through swing `n` (odd swings go one way, even the other), as
    /// bloodied as `gore`, held there for as long as the game is paused.
    pub fn pose_swing(&mut self, u: f32, n: u32, gore: f32) {
        self.game.select(crate::sim::BAT);
        let p = self.game.me_mut();
        p.equip_t = 0.0;
        p.swings = n;
        p.gore = if gore.is_finite() { gore.clamp(0.0, 1.0) } else { 0.0 };
        p.cooldown = (1.0 - u) * p.gun().delay;
    }

    /// Dev view: replaces the bots with `n` zombies standing in a row `dist` metres in front of the player.
    pub fn show_zombies(&mut self, n: u32, dist: f32, side: f32) {
        let (p, f) = (self.game.me().body.pos, self.game.me().forward());
        let right = [-f[2], 0.0, f[0]];
        self.game.bots.clear();
        for i in 0..n {
            let off = (i as f32 - (n as f32 - 1.0) / 2.0) * 1.4 + side;
            let at = [p[0] + f[0] * dist + right[0] * off, p[1], p[2] + f[2] * dist + right[2] * off];
            let mut z = crate::sim::Bot::zombie(at, i as f32 * 1.7 + 0.3);
            z.kind = (i % 3) as u8;
            self.game.bots.push(z);
        }
    }

    /// Dev view: `n` other players standing in a row `dist` metres in front of the player, the first facing it and
    /// each next one turned a quarter further round (front, side, back, other side), `turn` more than that; moving
    /// at `run` metres a second where they face (sprinting over 6), each a little further through a step.
    pub fn show_mates(&mut self, n: u32, dist: f32, turn: f32, run: f32) {
        let (p, f, yaw) = (self.game.me().body.pos, self.game.me().forward(), self.game.me().yaw);
        let right = [-f[2], 0.0, f[0]];
        self.game.bots.clear();
        self.game.players.truncate(1);
        for i in 0..n {
            let off = (i as f32 - (n as f32 - 1.0) / 2.0) * 1.6;
            let mut m = crate::sim::Player::new(i as u8 + 1);
            m.body.pos = [p[0] + f[0] * dist + right[0] * off, p[1], p[2] + f[2] * dist + right[2] * off];
            m.body.on_ground = true;
            m.yaw = yaw + std::f32::consts::PI + turn + i as f32 * std::f32::consts::FRAC_PI_2;
            m.weapon = (i as usize * 3) % crate::sim::WEAPON_COUNT;
            m.body.vel = [m.yaw.sin() * run, 0.0, -m.yaw.cos() * run];
            m.sprint_t = if run > 6.0 { 1.0 } else { 0.0 };
            m.gait = i as f32 * 0.9;
            self.game.players.push(m);
        }
        self.game.local = 0;
    }

    /// Dev view: replaces the bots with one zombie frozen `s` of the way through climbing the first ledge on the
    /// ground straight ahead of the player, and looks at it from `side` metres to its right (left if negative) and
    /// `back` behind it. False if there is none.
    pub fn show_climb(&mut self, s: f32, side: f32, back: f32) -> bool {
        let (at, yaw, f) = (self.game.me().body.pos, self.game.me().yaw, self.game.me().forward());
        let found = (4..120).find_map(|k| {
            let d = k as f32 * 0.25;
            crate::sim::Bot::climbing_at([at[0] + f[0] * d, 0.01, at[2] + f[2] * d], yaw, s, &self.game.world).map(|b| (b, d))
        });
        self.game.bots.clear();
        let Some((b, d)) = found else { return false };
        let c = [at[0] + f[0] * d, at[2] + f[2] * d];
        let p = self.game.me_mut();
        p.body.pos = [c[0] - f[2] * side - f[0] * back, p.body.pos[1], c[1] + f[0] * side - f[2] * back];
        p.yaw = yaw - side.atan2(back);
        self.game.bots.push(b);
        true
    }

    /// Dev view: puts the gun somewhere else in the picture, to look at it.
    pub fn set_gun_offset(&mut self, x: f32, y: f32, z: f32, yaw: f32) {
        self.renderer.gun_offset.set(Some([x, y, z]));
        self.renderer.gun_yaw.set(yaw);
    }

    /// Draws only gun `i` on a flat grey of brightness `bg` (0 black to 1 white) from now on, for the loadout's
    /// pictures.
    pub fn showcase(&mut self, i: usize, bg: f32) {
        self.renderer.showcase.set(Some((i, bg)));
    }

    pub fn remove_bots(&mut self) {
        self.game.bots.clear();
    }

    /// Sound events since the last call, flattened as code, a, b triples (see `sim::ev`).
    pub fn take_events(&mut self) -> Vec<f32> {
        std::mem::take(&mut self.game_mut().events).into_iter().flatten().collect()
    }

    /// Picks a primary or a sidearm for the loadout: carried at once before the game starts, otherwise from the
    /// next life. Online the server is told too.
    pub fn pick(&mut self, i: usize) -> bool {
        if self.online.is_some() && i < WEAPONS.len() {
            self.picks.push(i as u8);
        }
        self.game_mut().pick(i)
    }

    /// How many of `weapon_names` are primaries; the sidearms come after them.
    pub fn primary_count(&self) -> usize {
        PRIMARIES
    }

    pub fn weapon_names(&self) -> Vec<String> {
        WEAPONS.iter().map(|w| w.name.to_string()).collect()
    }

    /// What the loadout menu files each weapon under, e.g. "SMG".
    pub fn weapon_classes(&self) -> Vec<String> {
        WEAPONS.iter().map(|w| w.class.to_string()).collect()
    }

    /// Per weapon: damage per shot, rounds per minute, magazine, reload seconds, hip spread, aimed spread,
    /// movement speed multiplier.
    pub fn weapon_stats(&self) -> Vec<f32> {
        WEAPONS
            .iter()
            .flat_map(|w| [w.body_dmg * w.pellets as f32, 60.0 / w.delay, w.mag as f32, w.reload, w.spread, w.ads_spread, w.speed])
            .collect()
    }

    pub fn mouse_move(&mut self, dx: f32, dy: f32) {
        let i = &mut self.game_mut().me_mut().input;
        i.look_dx += dx;
        i.look_dy += dy;
    }

    /// 0 is the primary button (fire), 2 the secondary (aim down sights).
    pub fn mouse_button(&mut self, button: u8, down: bool) {
        let i = &mut self.game_mut().me_mut().input;
        match button {
            0 => i.fire = down,
            2 => i.ads = down,
            _ => {}
        }
    }

    /// Release everything, e.g. when the pointer lock is lost.
    pub fn release_all(&mut self) {
        let i = &mut self.game_mut().me_mut().input;
        *i = crate::sim::Input { select: i.select, pick: i.pick, ..Default::default() };
    }

    pub fn frame(&mut self, now_ms: f64) {
        let dt = if self.last == 0.0 { 0.0 } else { ((now_ms - self.last) / 1000.0).min(0.1) as f32 };
        self.last = now_ms;
        // online the game can't wait for the menu
        let paused = self.paused && self.online.is_none();
        self.acc += if paused { 0.0 } else { dt };
        let mut steps = 0;
        while self.acc >= STEP && steps < 6 {
            match &mut self.online {
                Some(o) => {
                    let p = o.mirror.game.me_mut();
                    if p.input.pick.is_none() && !self.picks.is_empty() {
                        p.input.pick = Some(self.picks.remove(0));
                    }
                    o.mirror.step();
                }
                None => self.game.tick(STEP),
            }
            self.acc -= STEP;
            steps += 1;
        }
        if steps == 6 {
            self.acc = 0.0;
        }
        if let Some(o) = &mut self.online {
            o.mirror.present(self.acc / STEP);
        }
        if !paused {
            self.game_mut().chime(dt);
        }
        self.renderer.render(self.game());
    }

    /// health, ammo, magazine size, reloading, kills, deaths, hit marker, damage flash, respawn countdown,
    /// weapon index, aim amount, scoped, kill marker, x, z, yaw, wave, zombies left, carried primary (the gun count when none),
    /// picked primary, game under way, the circle shots land in as a fraction of half the screen's height,
    /// stamina, winded, carried sidearm, picked sidearm, grenades, down (waiting for the next wave), players
    /// still standing, players in all, reserve
    pub fn hud(&self) -> Vec<f32> {
        let g = self.game();
        let p = g.me();
        vec![
            p.hp.max(0.0),
            p.ammo[p.weapon] as f32,
            p.gun().mag as f32,
            if p.reload_t > 0.0 { 1.0 } else { 0.0 },
            p.kills as f32,
            p.deaths as f32,
            p.hit_marker,
            p.damage_flash,
            if p.alive() { 0.0 } else { p.respawn_t.max(0.0) },
            p.weapon as f32,
            p.ads,
            if p.gun().scope { 1.0 } else { 0.0 },
            p.kill_t,
            p.body.pos[0],
            p.body.pos[2],
            p.yaw,
            g.wave as f32,
            g.zombies_left() as f32,
            p.loadout[0] as f32,
            p.picked[0] as f32,
            if p.in_play { 1.0 } else { 0.0 },
            p.spread() / (p.fov() / 2.0).tan(),
            p.stamina,
            if p.winded { 1.0 } else { 0.0 },
            p.loadout[1] as f32,
            p.picked[1] as f32,
            p.grenades as f32,
            if p.alive() { 0.0 } else { 1.0 },
            g.players.iter().filter(|p| p.alive()).count() as f32,
            g.players.len() as f32,
            p.reserve[p.weapon] as f32,
        ]
    }

    // ---- online ----

    /// The first message on a new connection: to make a room on map `map` (`room` empty), join one, or quick match
    /// (`room` is `net::QUICK`).
    pub fn net_hello(&mut self, room: &str, name: &str, map: u8) -> Vec<u8> {
        self.frames = Frames::default();
        self.net_error.clear();
        self.queued = 0;
        Msg::Hello { version: VERSION, room: room.trim().to_uppercase(), map, name: name.into() }.frame()
    }

    /// Bytes from the reliable stream.
    pub fn net_stream(&mut self, bytes: &[u8]) -> Result<(), JsValue> {
        for m in self.frames.push(bytes) {
            match m {
                Msg::Welcome { id, room, map, seed } => {
                    let mut mirror = Mirror::new(id, map, seed);
                    self.renderer.upload_world(&mirror.game)?;
                    // the loadout carries over, and the server is told it
                    let picked = self.game.me().picked;
                    for w in picked {
                        mirror.game.pick(w);
                    }
                    self.picks = picked.iter().map(|&w| w as u8).collect();
                    self.acc = 0.0;
                    self.queued = 0;
                    self.online = Some(Online { mirror, room, map, started: false, players: Vec::new(), matched: false, starts_in: 0 });
                }
                Msg::Roster { started, players, matched, starts_in } => {
                    if let Some(o) = &mut self.online {
                        (o.started, o.players, o.matched, o.starts_in) = (started, players, matched, starts_in);
                    }
                }
                Msg::Refused { why } => (self.net_error, self.queued) = (why, 0),
                Msg::Queued { place } => self.queued = place,
                _ => {}
            }
        }
        Ok(())
    }

    /// A datagram from the server.
    pub fn net_datagram(&mut self, bytes: &[u8]) {
        if let Some(o) = &mut self.online {
            o.mirror.receive(bytes);
        }
    }

    /// The datagrams to send, each with its length in front as two bytes (little-endian).
    pub fn net_take(&mut self) -> Vec<u8> {
        let Some(o) = &mut self.online else { return Vec::new() };
        let mut out = Vec::new();
        for d in o.mirror.take_outgoing() {
            out.extend_from_slice(&(d.len() as u16).to_le_bytes());
            out.extend_from_slice(&d);
        }
        out
    }

    /// The host letting the waves come, for the stream.
    pub fn net_start(&self) -> Vec<u8> {
        Msg::Start.frame()
    }

    /// The most players in a room.
    pub fn seats(&self) -> usize {
        MAX_PLAYERS
    }

    /// Going by `name` in the room from now on, for the stream.
    pub fn net_name(&self, name: &str) -> Vec<u8> {
        Msg::Name { name: name.into() }.frame()
    }

    /// Out of the room (or the connection gone, with `why`): back to a fresh game alone, on the solo map.
    pub fn net_leave(&mut self, why: &str) -> Result<(), JsValue> {
        self.queued = 0;
        if self.online.take().is_some() {
            self.end_game();
            self.renderer.upload_world(&self.game)?;
        }
        if !why.is_empty() {
            self.net_error = why.into();
        }
        Ok(())
    }

    /// How it is going online, as JSON: whether in a room, its code and map, this player's id, whether the waves
    /// have started, who is in (host first), whether it was found by quick match and the seconds until it starts by
    /// itself, the last error, and the place in line for a room (0 when not waiting).
    pub fn lobby(&self) -> String {
        // names and codes are only ever letters, digits, spaces and a few marks (see `sim::host::clean_name`)
        let q = |s: &str| format!("\"{}\"", s.replace(['"', '\\'], ""));
        match &self.online {
            Some(o) => {
                let players: Vec<String> = o.players.iter().map(|(id, name)| format!("[{id},{}]", q(name))).collect();
                format!("{{\"online\":true,\"room\":{},\"map\":{},\"id\":{},\"started\":{},\"players\":[{}],\"matched\":{},\"startsIn\":{},\"error\":{}}}", q(&o.room), o.map, o.mirror.id, o.started, players.join(","), o.matched, o.starts_in, q(&self.net_error))
            }
            None => format!("{{\"online\":false,\"error\":{},\"queued\":{}}}", q(&self.net_error), self.queued),
        }
    }
}
