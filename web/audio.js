// Synthesized sound effects. Everything is built from noise and oscillators, so there are no audio files.
// The sim reports what happened as [code, a, b] triples; `play` turns them into sound.

const SHOT = 1, ENEMY_SHOT = 2, HIT = 3, HEAD = 4, KILL = 5, HURT = 6, RELOAD = 7, EQUIP = 8, STEP = 9, JUMP = 10, LAND = 11, DEATH = 12, GROWL = 13, WAVE = 14;

// per gun: crack (highpass noise: freq, gain, seconds), thump (sine: from, to, seconds, gain), tail (lowpass noise), reverb send
const GUNS = [
  { crack: [2200, 0.50, 0.06], thump: [170, 45, 0.10, 0.9], tail: [900, 200, 0.22, 0.35], wet: 0.25 },
  { crack: [3000, 0.40, 0.045], thump: [230, 70, 0.07, 0.6], tail: [1200, 300, 0.12, 0.2], wet: 0.2 },
  { crack: [1400, 0.90, 0.14], thump: [120, 28, 0.28, 1.1], tail: [700, 100, 0.55, 0.5], wet: 0.35 },
  { crack: [1100, 1.00, 0.16], thump: [90, 24, 0.40, 1.2], tail: [900, 90, 0.95, 0.6], wet: 0.5 },
];

export class Sound {
  /** `reloads` is the reload time in seconds for each weapon. */
  constructor(reloads) {
    this.reloads = reloads;
    this.ctx = null;
    this.timers = new Set();
    this.lastHurt = 0;
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
    if (w === 3) {
      // supersonic crack and a slap-back echo
      this.noise(d, t, 0.05, "bandpass", 5000, 2500, 0.5, 3);
      this.noise(d, t + 0.16, 0.5, "lowpass", 600, 80, 0.2);
    }
    // the action cycling after the shot
    if (w === 2) {
      this.after(380, () => this.clack(0.6, 1.0));
      this.after(560, () => this.clack(0.6, 0.85));
    } else if (w === 3) {
      this.after(520, () => { this.click(0.5, 0.8); this.clack(0.6, 1.1); });
      this.after(850, () => this.clack(0.7, 0.9));
    }
  }

  enemyShot(dist, pan) {
    const t = this.ctx.currentTime, vol = 0.6 / (1 + dist * 0.18), k = 1 / (1 + dist * 0.08);
    const d = this.bus(pan, Math.min(0.5, 0.2 + dist * 0.015));
    this.noise(d, t, 0.06, "bandpass", 1800 * k, 900 * k, vol * 0.9, 0.8);
    this.tone(d, t, "sine", 140, 50, 0.1, vol * 0.8);
    this.noise(d, t, 0.2, "lowpass", 800 * k, 150, vol * 0.4);
  }

  // a low, wavering groan with some breath in it, softer and duller the further away it is
  growl(dist, pan) {
    const ctx = this.ctx, t = ctx.currentTime, vol = 0.9 / (1 + dist * 0.12), dur = 0.9 + Math.random() * 0.5;
    const d = this.bus(pan, Math.min(0.5, 0.15 + dist * 0.01));
    const f0 = 70 + Math.random() * 40;
    const o = ctx.createOscillator();
    o.type = "sawtooth";
    o.frequency.setValueAtTime(f0, t);
    o.frequency.exponentialRampToValueAtTime(f0 * 0.6, t + dur);
    const lfo = ctx.createOscillator(), lg = ctx.createGain();
    lfo.frequency.value = 5 + Math.random() * 3;
    lg.gain.value = f0 * 0.08;
    lfo.connect(lg);
    lg.connect(o.frequency);
    const f = ctx.createBiquadFilter();
    f.type = "lowpass";
    f.frequency.setValueAtTime(500 / (1 + dist * 0.03), t);
    f.frequency.exponentialRampToValueAtTime(220, t + dur);
    f.Q.value = 5;
    const g = ctx.createGain();
    g.gain.setValueAtTime(0.0001, t);
    g.gain.exponentialRampToValueAtTime(0.5 * vol, t + 0.15);
    g.gain.exponentialRampToValueAtTime(0.0001, t + dur);
    o.connect(f);
    f.connect(g);
    g.connect(d);
    o.start(t);
    lfo.start(t);
    o.stop(t + dur + 0.05);
    lfo.stop(t + dur + 0.05);
    this.noise(d, t, dur, "bandpass", 700, 300, 0.25 * vol, 2);
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
    const total = this.reloads[w] * 1000;
    if (w === 2) {
      // shells go in one at a time, then the pump
      for (let i = 0; i < 6; i++) this.after(200 + i * 330, () => { this.click(0.5, 0.9 + Math.random() * 0.2); this.clack(0.35, 1.3); });
      this.after(total - 420, () => this.clack(0.6, 1.0));
      this.after(total - 250, () => this.clack(0.6, 0.85));
    } else if (w === 3) {
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
        case ENEMY_SHOT: this.enemyShot(a, b); break;
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
      }
    }
  }
}
