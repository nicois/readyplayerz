//! Browser glue: owns the game and renderer, and is driven from JS.

use crate::render::Renderer;
use crate::sim::{Game, PRIMARIES, STEP, WEAPONS};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{HtmlCanvasElement, WebGl2RenderingContext};

#[wasm_bindgen]
pub struct Client {
    game: Game,
    renderer: Renderer,
    acc: f32,
    last: f64,
    paused: bool,
    /// what the map was made from, so ending the game can start a fresh one on the same map
    seed: u32,
}

#[wasm_bindgen]
impl Client {
    #[wasm_bindgen(constructor)]
    pub fn new(canvas: HtmlCanvasElement) -> Result<Client, JsValue> {
        std::panic::set_hook(Box::new(|info| web_sys::console::error_1(&info.to_string().into())));
        let gl = canvas
            .get_context("webgl2")?
            .ok_or("WebGL2 is not available")?
            .dyn_into::<WebGl2RenderingContext>()?;
        let mut renderer = Renderer::new(gl, canvas.width(), canvas.height())?;
        let seed = (js_sys::Math::random() * 4_000_000_000.0) as u32;
        let game = Game::new(seed);
        renderer.upload_world(&game)?;
        Ok(Client { game, renderer, acc: 0.0, last: 0.0, paused: true, seed })
    }

    /// Ends the game: a fresh one on the same map, with the score cleared and the loadout picked so far.
    pub fn end_game(&mut self) {
        let picked = self.game.player.picked;
        self.game = Game::new(self.seed);
        for i in picked {
            self.game.pick(i);
        }
        self.acc = 0.0;
    }

    /// Dev view: a fresh game on the map made from `seed`, so a picture can be taken in the same place again.
    pub fn set_seed(&mut self, seed: u32) -> Result<(), JsValue> {
        self.seed = seed;
        self.game = Game::new(seed);
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
        let i = &mut self.game.input;
        match code {
            "KeyW" | "ArrowUp" => i.w = down,
            "KeyA" | "ArrowLeft" => i.a = down,
            "KeyS" | "ArrowDown" => i.s = down,
            "KeyD" | "ArrowRight" => i.d = down,
            "Space" => i.jump = down,
            "ShiftLeft" | "ShiftRight" => i.sprint = down,
            "KeyR" => i.reload = down,
            _ if down => {
                // 1 primary, 2 sidearm; the loadout itself is picked in the menu
                if let Some(n) = code.strip_prefix("Digit").and_then(|d| d.parse::<usize>().ok()) {
                    if self.game.player.alive() {
                        self.game.select_slot(n.wrapping_sub(1));
                    }
                }
            }
            _ => {}
        }
    }

    /// Mouse wheel: down (`dir` > 0) picks the pistol, up picks the primary, so extra clicks keep the same gun.
    pub fn cycle_weapon(&mut self, dir: i32) {
        self.game.select_slot(if dir > 0 { 1 } else { 0 });
    }

    /// The menu is up: keep drawing, stop the simulation.
    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
    }

    /// Puts the player somewhere and facing some way. For looking around and testing.
    pub fn teleport(&mut self, x: f32, y: f32, z: f32, yaw: f32, pitch: f32) {
        self.game.player.body.pos = [x, y, z];
        self.game.player.step_y = 0.0;
        self.game.player.climb = None;
        self.game.player.yaw = yaw;
        self.game.player.pitch = pitch;
    }

    /// Dev view: replaces the bots with `n` zombies standing in a row `dist` metres in front of the player.
    pub fn show_zombies(&mut self, n: u32, dist: f32, side: f32) {
        let (p, f) = (self.game.player.body.pos, self.game.player.forward());
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

    /// Dev view: replaces the bots with one zombie frozen `s` of the way through climbing the first ledge on the
    /// ground straight ahead of the player, and looks at it from `side` metres to its right (left if negative) and
    /// `back` behind it. False if there is none.
    pub fn show_climb(&mut self, s: f32, side: f32, back: f32) -> bool {
        let p = &mut self.game.player;
        let (at, yaw, f) = (p.body.pos, p.yaw, p.forward());
        let found = (4..120).find_map(|k| {
            let d = k as f32 * 0.25;
            crate::sim::Bot::climbing_at([at[0] + f[0] * d, 0.01, at[2] + f[2] * d], yaw, s, &self.game.world).map(|b| (b, d))
        });
        self.game.bots.clear();
        let Some((b, d)) = found else { return false };
        let c = [at[0] + f[0] * d, at[2] + f[2] * d];
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
        std::mem::take(&mut self.game.events).into_iter().flatten().collect()
    }

    /// Picks a primary or a sidearm for the loadout: carried at once before the game starts, otherwise from the
    /// next life.
    pub fn pick(&mut self, i: usize) -> bool {
        self.game.pick(i)
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
        self.game.input.look_dx += dx;
        self.game.input.look_dy += dy;
    }

    /// 0 is the primary button (fire), 2 the secondary (aim down sights).
    pub fn mouse_button(&mut self, button: u8, down: bool) {
        match button {
            0 => self.game.input.fire = down,
            2 => self.game.input.ads = down,
            _ => {}
        }
    }

    /// Release everything, e.g. when the pointer lock is lost.
    pub fn release_all(&mut self) {
        self.game.input = Default::default();
    }

    pub fn frame(&mut self, now_ms: f64) {
        let dt = if self.last == 0.0 { 0.0 } else { ((now_ms - self.last) / 1000.0).min(0.1) as f32 };
        self.last = now_ms;
        self.acc += if self.paused { 0.0 } else { dt };
        let mut steps = 0;
        while self.acc >= STEP && steps < 6 {
            self.game.tick(STEP);
            self.acc -= STEP;
            steps += 1;
        }
        if steps == 6 {
            self.acc = 0.0;
        }
        self.renderer.render(&self.game);
    }

    /// health, ammo, magazine size, reloading, kills, deaths, hit marker, damage flash, respawn countdown,
    /// weapon index, aim amount, scoped, kill marker, x, z, yaw, wave, zombies left, carried primary,
    /// picked primary, game under way, the circle shots land in as a fraction of half the screen's height,
    /// stamina, winded, carried sidearm, picked sidearm
    pub fn hud(&self) -> Vec<f32> {
        let p = &self.game.player;
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
            self.game.wave as f32,
            self.game.zombies_left() as f32,
            p.loadout[0] as f32,
            p.picked[0] as f32,
            if p.in_play { 1.0 } else { 0.0 },
            p.spread() / (p.fov() / 2.0).tan(),
            p.stamina,
            if p.winded { 1.0 } else { 0.0 },
            p.loadout[1] as f32,
            p.picked[1] as f32,
        ]
    }
}
