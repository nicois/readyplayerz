//! Browser glue: owns the game and renderer, and is driven from JS.

use crate::render::Renderer;
use crate::sim::{Game, STEP, WEAPONS, WEAPON_COUNT};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{HtmlCanvasElement, WebGl2RenderingContext};

#[wasm_bindgen]
pub struct Client {
    game: Game,
    renderer: Renderer,
    acc: f32,
    last: f64,
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
        let renderer = Renderer::new(gl, canvas.width(), canvas.height())?;
        let seed = (js_sys::Math::random() * 4_000_000_000.0) as u32;
        Ok(Client { game: Game::new(seed), renderer, acc: 0.0, last: 0.0 })
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.renderer.width = width;
        self.renderer.height = height;
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
                if let Some(n) = code.strip_prefix("Digit").and_then(|d| d.parse::<usize>().ok()) {
                    self.game.select(n.wrapping_sub(1));
                }
            }
            _ => {}
        }
    }

    /// Mouse wheel: next or previous weapon.
    pub fn cycle_weapon(&mut self, dir: i32) {
        let n = WEAPON_COUNT as i32;
        let next = (self.game.player.weapon as i32 + dir).rem_euclid(n);
        self.game.select(next as usize);
    }

    pub fn select_weapon(&mut self, i: usize) {
        self.game.select(i);
    }

    pub fn weapon_names(&self) -> Vec<String> {
        WEAPONS.iter().map(|w| w.name.to_string()).collect()
    }

    /// Per weapon: damage per shot, rounds per minute, magazine, reload seconds.
    pub fn weapon_stats(&self) -> Vec<f32> {
        WEAPONS
            .iter()
            .flat_map(|w| [w.body_dmg * w.pellets as f32, 60.0 / w.delay, w.mag as f32, w.reload])
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
        self.acc += dt;
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
    /// weapon index, aim amount, scoped
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
        ]
    }
}
