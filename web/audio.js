// Synthesized sound effects. Everything is built from noise and oscillators, so there are no audio files.
// The sim reports what happened as [code, a, b] triples; `play` turns them into sound.

const SHOT = 1, HIT = 3, HEAD = 4, KILL = 5, HURT = 6, RELOAD = 7, EQUIP = 8, STEP = 9, JUMP = 10, LAND = 11, DEATH = 12, GROWL = 13, WAVE = 14, SNARL = 15;

// the first three formants (Hz) of a man's vowels: oo, uh, aa, aw, and the a of "cat"
const OO = [300, 870, 2240], UH = [640, 1190, 2390], AA = [730, 1090, 2440], AW = [570, 840, 2410], AE = [660, 1720, 2410];

// per gun, in the sim's order: crack (highpass noise: freq, gain, seconds), thump (sine: from, to, seconds, gain),
// tail (lowpass noise), reverb send, and how it works: `action` is what cycles after a shot (pump, bolt), `load`
// how it reloads (mag, shells, bolt, break, cylinder), `quick` scales the bolt's timing, `sonic` adds a supersonic
// snap and an echo
const GUNS = [
  { crack: [2200, 0.50, 0.06], thump: [170, 45, 0.10, 0.9], tail: [900, 200, 0.22, 0.35], wet: 0.25, load: "mag" },
  { crack: [3000, 0.40, 0.045], thump: [230, 70, 0.07, 0.6], tail: [1200, 300, 0.12, 0.2], wet: 0.2, load: "mag" },
  { crack: [1400, 0.90, 0.14], thump: [120, 28, 0.28, 1.1], tail: [700, 100, 0.55, 0.5], wet: 0.35, action: "pump", load: "shells" },
  { crack: [1100, 1.00, 0.16], thump: [90, 24, 0.40, 1.2], tail: [900, 90, 0.95, 0.6], wet: 0.5, action: "bolt", load: "bolt", sonic: true },
  // AK-47: lower and heavier than the carbine
  { crack: [1700, 0.60, 0.07], thump: [140, 38, 0.13, 1.0], tail: [800, 160, 0.30, 0.42], wet: 0.28, load: "mag" },
  // Uzi: a dry, flat 9mm chatter
  { crack: [3400, 0.36, 0.035], thump: [250, 85, 0.05, 0.55], tail: [1300, 350, 0.09, 0.18], wet: 0.18, load: "mag" },
  // double barrel: the biggest boom of the lot
  { crack: [1200, 1.00, 0.16], thump: [105, 24, 0.34, 1.25], tail: [650, 90, 0.65, 0.55], wet: 0.4, load: "break" },
  // Mosin: a rifle crack with a long rolling tail
  { crack: [1300, 0.95, 0.13], thump: [100, 26, 0.34, 1.15], tail: [850, 90, 0.85, 0.55], wet: 0.48, action: "bolt", quick: 0.77, load: "bolt", sonic: true },
  { crack: [2600, 0.45, 0.04], thump: [210, 60, 0.07, 0.6], tail: [1100, 250, 0.15, 0.25], wet: 0.22, load: "mag" },
  // revolver: a sharp heavy bark
  { crack: [1900, 0.85, 0.08], thump: [150, 36, 0.18, 1.05], tail: [900, 130, 0.45, 0.45], wet: 0.38, load: "cylinder" },
  // Desert Eagle: a deep blast
  { crack: [1600, 0.90, 0.09], thump: [125, 30, 0.22, 1.15], tail: [800, 110, 0.5, 0.5], wet: 0.4, load: "mag" },
];

export class Sound {
  /** `reloads` is the reload time in seconds for each weapon. */
  constructor(reloads) {
    this.reloads = reloads;
    this.ctx = null;
    this.timers = new Set();
    this.lastHurt = 0;
    this.groans = []; // when each groan still sounding ends
    this.muted = false;
  }

  // browsers only allow audio after a user gesture, so this is called from clicks and key presses
  start() {
    if (!this.ctx) {
      const C = window.AudioContext || window.webkitAudioContext;
      if (!C) return;
      const ctx = (this.ctx = new C());
      this.master = ctx.createGain();
      this.master.gain.value = this.muted ? 0 : 0.5;
      const comp = ctx.createDynamicsCompressor();
      comp.threshold.value = -14;
      comp.ratio.value = 6;
      this.master.connect(comp);
      comp.connect(ctx.destination);

      this.noiseBuf = ctx.createBuffer(1, ctx.sampleRate * 2, ctx.sampleRate);
      const n = this.noiseBuf.getChannelData(0);
      for (let i = 0; i < n.length; i++) n[i] = Math.random() * 2 - 1;

      // a cheap room: decaying noise as the impulse response
      const len = Math.floor(ctx.sampleRate * 1.4);
      const ir = ctx.createBuffer(2, len, ctx.sampleRate);
      for (let ch = 0; ch < 2; ch++) {
        const d = ir.getChannelData(ch);
        for (let i = 0; i < len; i++) d[i] = (Math.random() * 2 - 1) * Math.pow(1 - i / len, 3);
      }
      this.verb = ctx.createConvolver();
      this.verb.buffer = ir;
      const wet = ctx.createGain();
      wet.gain.value = 0.22;
      this.verb.connect(wet);
      wet.connect(this.master);
    }
    if (this.ctx.state !== "running") this.ctx.resume();
  }

  toggleMute() {
    this.muted = !this.muted;
    if (this.master) this.master.gain.value = this.muted ? 0 : 0.5;
    return this.muted;
  }

  /** Cancels sounds that are still waiting to happen, e.g. a reload that was interrupted. */
  cancel() {
    for (const t of this.timers) clearTimeout(t);
    this.timers.clear();
  }

  after(ms, fn) {
    const t = setTimeout(() => {
      this.timers.delete(t);
      fn();
    }, ms);
    this.timers.add(t);
  }

  // ---- building blocks -------------------------------------------------------------------------

  bus(pan = 0, wet = 0.25) {
    const ctx = this.ctx;
    const g = ctx.createGain();
    let out = g;
    if (pan && ctx.createStereoPanner) {
      const p = ctx.createStereoPanner();
      p.pan.value = Math.max(-1, Math.min(1, pan));
      g.connect(p);
      out = p;
    }
    out.connect(this.master);
    if (wet > 0) {
      const send = ctx.createGain();
      send.gain.value = wet;
      out.connect(send);
      send.connect(this.verb);
    }
    return g;
  }

  noise(dest, t, dur, type, f0, f1, gain, q = 1) {
    const ctx = this.ctx;
    const s = ctx.createBufferSource();
    s.buffer = this.noiseBuf;
    const f = ctx.createBiquadFilter();
    f.type = type;
    f.Q.value = q;
    f.frequency.setValueAtTime(f0, t);
    f.frequency.exponentialRampToValueAtTime(Math.max(f1, 20), t + dur);
    const g = ctx.createGain();
    g.gain.setValueAtTime(gain, t);
    g.gain.exponentialRampToValueAtTime(0.0001, t + dur);
    s.connect(f);
    f.connect(g);
    g.connect(dest);
    s.start(t, Math.random() * 1.5);
    s.stop(t + dur + 0.02);
  }

  tone(dest, t, type, f0, f1, dur, gain) {
    const ctx = this.ctx;
    const o = ctx.createOscillator();
    o.type = type;
    o.frequency.setValueAtTime(f0, t);
    o.frequency.exponentialRampToValueAtTime(Math.max(f1, 20), t + dur);
    const g = ctx.createGain();
    g.gain.setValueAtTime(gain, t);
    g.gain.exponentialRampToValueAtTime(0.0001, t + dur);
    o.connect(g);
    g.connect(dest);
    o.start(t);
    o.stop(t + dur + 0.02);
  }

  // small mechanical sounds, used for reloads and the pump / bolt
  click(gain = 0.5, pitch = 1) {
    const t = this.ctx.currentTime, d = this.bus(0, 0.1);
    this.noise(d, t, 0.025, "bandpass", 3200 * pitch, 1600 * pitch, gain, 2);
    this.tone(d, t, "square", 900 * pitch, 300 * pitch, 0.03, gain * 0.25);
  }

  clack(gain = 0.6, pitch = 1) {
    const t = this.ctx.currentTime, d = this.bus(0, 0.15);
    this.noise(d, t, 0.06, "lowpass", 1400 * pitch, 300, gain);
    this.tone(d, t, "triangle", 240 * pitch, 110 * pitch, 0.07, gain * 0.7);
  }

  // ---- the sounds ------------------------------------------------------------------------------

  shot(w) {
    const c = GUNS[w], t = this.ctx.currentTime, r = 0.92 + Math.random() * 0.16;
    const d = this.bus(0, c.wet);
    this.noise(d, t, c.crack[2], "highpass", c.crack[0] * r, c.crack[0] * 0.5, c.crack[1]);
    this.tone(d, t, "sine", c.thump[0] * r, c.thump[1], c.thump[2], c.thump[3]);
    this.noise(d, t, c.tail[2], "lowpass", c.tail[0], c.tail[1], c.tail[3]);
    if (c.sonic) {
      // supersonic crack and a slap-back echo
      this.noise(d, t, 0.05, "bandpass", 5000, 2500, 0.5, 3);
      this.noise(d, t + 0.16, 0.5, "lowpass", 600, 80, 0.2);
    }
    // the action cycling after the shot, or a cylinder clicking round to the next chamber
    if (c.action === "pump") {
      this.after(380, () => this.clack(0.6, 1.0));
      this.after(560, () => this.clack(0.6, 0.85));
    } else if (c.action === "bolt") {
      const k = c.quick || 1;
      this.after(520 * k, () => { this.click(0.5, 0.8); this.clack(0.6, 1.1); });
      this.after(850 * k, () => this.clack(0.7, 0.9));
    } else if (c.load === "cylinder") {
      this.after(150, () => this.click(0.25, 1.4));
    }
  }

  /**
   * The vocal cords, written out sample by sample: one puff of air per pitch period, each a little late or
   * early and a little louder or softer than the last (a sick, unsteady voice), with breath hissing through
   * on every puff and creaky "fry" where beats get skipped. `o` has f0 / peak / f1 (pitch at the start, a
   * third of the way in, and the end), dur, breath, creak and jitter.
   */
  throat(o) {
    const sr = this.ctx.sampleRate, n = Math.ceil(o.dur * sr);
    const buf = this.ctx.createBuffer(1, n, sr), d = buf.getChannelData(0);
    const pitchAt = x => (x < 0.33 ? o.f0 + (o.peak - o.f0) * (x / 0.33) : o.peak + (o.f1 - o.peak) * ((x - 0.33) / 0.67));
    const wob = 4 + Math.random() * 2;
    let i = 0, prev = 0, peak = 0;
    while (i < n) {
      const x = i / n;
      // the voice breaks into fry more as it runs out of air
      const creaky = Math.random() < o.creak * (0.4 + x);
      const f = pitchAt(x) * (1 + 0.03 * Math.sin(2 * Math.PI * wob * (i / sr)));
      let period = (sr / f) * (1 + o.jitter * (Math.random() * 2 - 1));
      if (creaky) period *= 1.8 + Math.random() * 1.2;
      const len = Math.round(period), open = len * (creaky ? 0.3 : 0.6), rise = open * 0.7;
      const amp = (creaky ? 1.3 : 1) * (1 - 0.35 * Math.random());
      for (let k = 0; k < len && i < n; k++, i++) {
        // the airflow through the cords: a smooth rise, a sharp close, then shut
        const flow = k < rise ? 0.5 - 0.5 * Math.cos((Math.PI * k) / rise) : k < open ? Math.cos(((Math.PI / 2) * (k - rise)) / (open - rise)) : 0;
        const air = amp * flow;
        // the sound is the change in airflow; the breath comes through while the cords are open
        const s = air - prev + o.breath * (0.15 + air) * (Math.random() * 2 - 1) * 0.25;
        prev = air;
        d[i] = s;
        peak = Math.max(peak, Math.abs(s));
      }
    }
    for (let j = 0; j < n; j++) d[j] /= peak || 1;
    return buf;
  }

  /** A voice from `throat`, through a mouth sliding from vowels[0] to vowels[1]; `o.dist` dulls and quiets it. */
  voice(o, pan) {
    const ctx = this.ctx, t = ctx.currentTime, dur = o.dur;
    const far = 1 + o.dist * 0.07;
    const d = this.bus(pan, Math.min(0.55, 0.12 + o.dist * 0.012));
    const src = ctx.createBufferSource();
    src.buffer = this.throat(o);
    const env = ctx.createGain(), loud = (o.gain * 5) / far;
    env.gain.setValueAtTime(0.0001, t);
    env.gain.exponentialRampToValueAtTime(loud, t + Math.min(0.15, dur * 0.2));
    env.gain.setValueAtTime(loud, t + dur * 0.55);
    env.gain.exponentialRampToValueAtTime(0.0001, t + dur);
    // the mouth: four formants, narrow like a real vocal tract, sliding between the two vowels
    const [v0, v1] = o.vowels;
    for (let k = 0; k < 4; k++) {
      const f = ctx.createBiquadFilter(), a = ctx.createGain();
      f.type = "bandpass";
      f.Q.value = [6, 10, 14, 16][k];
      f.frequency.setValueAtTime(k < 3 ? v0[k] : 3300, t);
      f.frequency.linearRampToValueAtTime(k < 3 ? v1[k] : 3300, t + dur * 0.7);
      a.gain.value = [1, 0.8, 0.45, 0.2][k];
      src.connect(f);
      f.connect(a);
      a.connect(env);
    }
    // and further off, duller
    const dull = ctx.createBiquadFilter();
    dull.type = "lowpass";
    dull.frequency.value = 6000 / far;
    env.connect(dull);
    dull.connect(d);
    src.start(t);
  }

  // a groan as it comes: walkers moan, runners shriek and gasp, brutes rumble from deep in the chest
  growl(dist, b) {
    const kind = Math.round(b / 4), pan = b - kind * 4, r = () => Math.random();
    if (this.groans.filter(g => g > this.ctx.currentTime).length >= 5) return; // a crowd blurs into a few voices
    const v = kind === 1
      ? { f0: 200 + r() * 40, peak: 280 + r() * 60, f1: 170, dur: 0.5 + r() * 0.4, vowels: [AA, AE], breath: 2.2, creak: 0.05, jitter: 0.06, gain: 0.85 }
      : kind === 2
        ? { f0: 80 + r() * 10, peak: 95 + r() * 10, f1: 62, dur: 1.5 + r() * 0.7, vowels: [OO, AW], breath: 1.2, creak: 0.35, jitter: 0.08, gain: 1.1 }
        : { f0: 100 + r() * 20, peak: 115 + r() * 25, f1: 78, dur: 1.2 + r() * 0.9, vowels: r() < 0.5 ? [UH, AA] : [OO, UH], breath: 1.4, creak: 0.18, jitter: 0.06, gain: 1.0 };
    v.dist = dist;
    this.groans.push(this.ctx.currentTime + v.dur);
    this.groans = this.groans.slice(-8);
    this.voice(v, pan);
  }

  // the snarl as one swings at you: short, loud and right in your ear
  snarl(kind, pan) {
    const r = Math.random();
    // mostly breath forced through a tight throat: a hissing "hhaaagh"
    const v = kind === 2
      ? { f0: 95, peak: 130, f1: 70, dur: 0.6, vowels: [AA, AW], breath: 3, creak: 0.3, jitter: 0.1, gain: 1.3 }
      : { f0: 150 + r * 40, peak: 220 + r * 50, f1: 120, dur: 0.35 + r * 0.15, vowels: [AE, AA], breath: 3.5, creak: 0.15, jitter: 0.1, gain: 1.2 };
    v.dist = 0;
    this.voice(v, pan * 0.5);
  }

  // a low horn when a wave begins
  wave() {
    const t = this.ctx.currentTime, d = this.bus(0, 0.5);
    this.tone(d, t, "sawtooth", 110, 108, 1.1, 0.18);
    this.tone(d, t + 0.05, "sawtooth", 165, 162, 1.0, 0.1);
    this.tone(d, t + 1.2, "sawtooth", 82, 80, 1.4, 0.2);
    this.noise(d, t, 1.2, "lowpass", 300, 100, 0.2);
  }

  hit() {
    this.tone(this.bus(0, 0), this.ctx.currentTime, "sine", 1700, 1500, 0.045, 0.22);
  }

  headshot() {
    const t = this.ctx.currentTime, d = this.bus(0, 0.1);
    this.tone(d, t, "sine", 2400, 2400, 0.06, 0.25);
    this.tone(d, t + 0.05, "sine", 3300, 3300, 0.09, 0.22);
    this.noise(d, t, 0.03, "highpass", 4000, 3000, 0.2);
  }

  kill() {
    const t = this.ctx.currentTime, d = this.bus(0, 0.3);
    this.tone(d, t, "sine", 110, 28, 0.5, 0.9);
    this.noise(d, t, 0.45, "lowpass", 2000, 150, 0.7);
    this.noise(d, t, 0.08, "highpass", 3000, 2000, 0.4);
    this.tone(d, t + 0.02, "sine", 700, 1400, 0.12, 0.12); // reward ping
    // pieces clattering down and sparks fizzing out
    const dry = this.bus(0, 0.15);
    for (let i = 0; i < 9; i++) {
      this.noise(dry, t + 0.12 + Math.random() * 0.7, 0.03, "bandpass", 2000 + Math.random() * 4000, 1500, 0.1 + Math.random() * 0.15, 3);
    }
    this.noise(dry, t, 0.5, "highpass", 5000, 8000, 0.12);
  }

  hurt() {
    const now = performance.now();
    if (now - this.lastHurt < 120) return;
    this.lastHurt = now;
    const t = this.ctx.currentTime, d = this.bus(0, 0.1);
    this.tone(d, t, "sine", 100, 55, 0.18, 0.5);
    this.noise(d, t, 0.15, "lowpass", 700, 200, 0.4);
  }

  death() {
    this.cancel();
    const t = this.ctx.currentTime, d = this.bus(0, 0.4);
    this.tone(d, t, "sawtooth", 180, 30, 1.2, 0.25);
    this.noise(d, t, 1.0, "lowpass", 600, 60, 0.4);
  }

  reload(w) {
    this.cancel();
    const total = this.reloads[w] * 1000, load = GUNS[w].load;
    if (load === "shells") {
      // shells go in one at a time, then the pump
      for (let i = 0; i < 6; i++) this.after(200 + i * 330, () => { this.click(0.5, 0.9 + Math.random() * 0.2); this.clack(0.35, 1.3); });
      this.after(total - 420, () => this.clack(0.6, 1.0));
      this.after(total - 250, () => this.clack(0.6, 0.85));
    } else if (load === "break") {
      // the lever and the barrels dropping, the empties pulled, two shells in, and the action snapped shut
      this.after(total * 0.05, () => { this.click(0.6, 0.9); this.clack(0.6, 0.8); });
      this.after(total * 0.32, () => this.clack(0.4, 1.2));
      this.after(total * 0.55, () => { this.click(0.4, 1.0); this.clack(0.35, 1.3); });
      this.after(total * 0.66, () => { this.click(0.4, 1.1); this.clack(0.35, 1.25); });
      this.after(total * 0.85, () => { this.clack(0.9, 0.75); this.click(0.6, 0.8); });
    } else if (load === "cylinder") {
      // the cylinder swings out, the empties rattle out, the speedloader goes in, and it clicks shut
      this.after(total * 0.06, () => { this.click(0.5, 1.0); this.clack(0.4, 1.1); });
      this.after(total * 0.28, () => { for (let i = 0; i < 6; i++) this.after(i * 25, () => this.click(0.2, 1.6 + Math.random() * 0.4)); });
      this.after(total * 0.58, () => this.clack(0.5, 1.2));
      this.after(total * 0.86, () => { this.clack(0.7, 1.0); this.click(0.5, 1.2); });
    } else if (load === "bolt") {
      this.after(total * 0.1, () => { this.click(0.5, 0.8); this.clack(0.6, 1.1); });
      this.after(total * 0.4, () => this.clack(0.7, 0.8));
      this.after(total * 0.65, () => { this.clack(0.7, 1.0); this.click(0.5, 1.0); });
      this.after(total - 450, () => this.clack(0.6, 1.1));
      this.after(total - 250, () => this.clack(0.7, 0.9));
    } else {
      this.after(total * 0.3, () => this.clack(0.6, 0.9)); // magazine out
      this.after(total * 0.62, () => { this.clack(0.8, 1.1); this.click(0.5, 1.0); }); // magazine in
      this.after(total * 0.88, () => { this.click(0.6, 0.8); this.after(90, () => this.click(0.6, 1.1)); }); // charging handle
    }
  }

  equip() {
    this.cancel();
    const t = this.ctx.currentTime, d = this.bus(0, 0.1);
    this.noise(d, t, 0.12, "bandpass", 1200, 2400, 0.25, 1.5);
    this.after(150, () => this.click(0.6, 0.9));
  }

  step(speed) {
    const t = this.ctx.currentTime, d = this.bus(0, 0.08), v = Math.min(speed / 6, 1.5), r = 0.85 + Math.random() * 0.3;
    this.noise(d, t, 0.08, "lowpass", 520 * r, 160, 0.1 * v);
    this.tone(d, t, "sine", 90 * r, 55, 0.07, 0.12 * v);
  }

  jump() {
    this.noise(this.bus(0, 0.05), this.ctx.currentTime, 0.14, "bandpass", 500, 1100, 0.08, 1);
  }

  land(speed) {
    const t = this.ctx.currentTime, d = this.bus(0, 0.1), v = Math.min(speed / 12, 1);
    this.tone(d, t, "sine", 80, 35, 0.14, 0.5 * v);
    this.noise(d, t, 0.1, "lowpass", 400, 100, 0.25 * v);
  }

  /** Plays a flat list of [code, a, b] triples from the sim. */
  play(events) {
    if (!this.ctx || this.ctx.state !== "running") return;
    for (let i = 0; i < events.length; i += 3) {
      const a = events[i + 1], b = events[i + 2];
      switch (events[i]) {
        case SHOT: this.shot(a); break;
        case HIT: this.hit(); break;
        case HEAD: this.headshot(); break;
        case KILL: this.kill(); break;
        case HURT: this.hurt(); break;
        case RELOAD: this.reload(a); break;
        case EQUIP: this.equip(); break;
        case STEP: this.step(a); break;
        case JUMP: this.jump(); break;
        case LAND: this.land(a); break;
        case DEATH: this.death(); break;
        case GROWL: this.growl(a, b); break;
        case WAVE: this.wave(); break;
        case SNARL: this.snarl(a, b); break;
      }
    }
  }
}
