//! The game the benches play: a full team on the Suburb against the most zombies the game lets be up at once,
//! each player shooting the nearest in bursts and throwing a grenade now and then, nobody going down and nobody
//! running out, so the load stays the same all through. The network bench (`load.rs`) plays it on the server's
//! room; the drawing bench (`?horde` on the page) plays it alone, with the others stepped on this machine.

use super::*;

/// The map and seed played, the same every run.
pub const MAP: u8 = 1;
pub const SEED: u32 = 7;
/// The wave the zombies are spawned as, for its mix of walkers, runners and brutes.
pub const WAVE: u32 = 6;

impl Game {
    /// Keeps the load up, before each tick: once the waves have started, the horde is wave `WAVE`'s and is
    /// topped up for good to the most the game lets be up, and everyone's health and reserves are kept full.
    pub fn keep_up_load(&mut self) {
        if self.wave > 0 {
            self.wave = WAVE;
            self.to_spawn = self.to_spawn.max(1000);
        }
        for p in &mut self.players {
            p.hp = 100.0;
            p.reserve.iter_mut().for_each(|r| *r = (*r).max(30));
        }
    }
}

/// Player `k`'s input on tick `n`: shooting at the nearest zombie in bursts, side-stepping as it does, and with
/// none near walking round in a wide circle; a grenade every eight seconds, each player at its own time.
pub fn input(g: &Game, k: usize, n: u32) -> Input {
    let p = &g.players[k];
    let eye = p.eye();
    let near = g
        .bots
        .iter()
        .filter(|b| b.alive)
        .map(|b| (b, (b.body.pos[0] - eye[0]).hypot(b.body.pos[2] - eye[2])))
        .filter(|(_, d)| *d < 40.0)
        .min_by(|a, b| a.1.total_cmp(&b.1));
    let secs = n as f32 * STEP;
    let mut inp = Input { seen: g.tick_n.saturating_sub(6) as f32, ..Default::default() };
    match near {
        Some((b, d)) => {
            let (dx, dz) = (b.body.pos[0] - eye[0], b.body.pos[2] - eye[2]);
            let dy = b.body.pos[1] + 1.2 - eye[1];
            inp.view = Some([dx.atan2(-dz), dy.atan2(d)]);
            // pulled and let go every few ticks, as a semi-automatic only fires again once it is let go
            inp.fire = (n / 20 + k as u32) % 3 != 0 && n % 6 < 3;
            let left = (secs / 2.0 + k as f32) as u32 % 2 == 0;
            (inp.a, inp.d) = (left, !left);
            inp.s = d < 4.0;
        }
        None => {
            inp.view = Some([secs * 0.3 + k as f32, 0.0]);
            inp.w = true;
            inp.sprint = (secs as u32 + k as u32) % 6 < 2;
        }
    }
    inp.throw = n % 480 == k as u32 * 80;
    inp
}

/// The drawing bench's game, played alone: `players` in all, the first looked through, the waves under way.
pub fn solo(players: usize) -> Game {
    let mut g = Game::on(MAP, SEED);
    g.set_loot(true);
    for id in 1..players {
        g.add_player(id as u8);
    }
    g.start_waves();
    g
}

/// One tick of the drawing bench's game, tick `n` of it: everyone's input played, the first's through the game's
/// own step as when playing alone, the others' here.
pub fn step_solo(g: &mut Game, n: u32) {
    g.keep_up_load();
    for k in 1..g.players.len() {
        let inp = input(g, k, n);
        g.step_player(k, inp, STEP);
    }
    g.players[0].input = input(g, 0, n);
    g.tick(STEP);
}
