// The theme: a slow, synthesized horror loop in A minor, made the same way as the sound effects.
// Eight bars, two per chord, over a low A drone that never moves, so the Bb and F minor grind against it.
// A pulsing bass, a dark pad, a heartbeat kick, detuned bells that change pattern every time round,
// a deep boom at the top of each loop and a metal scrape halfway. It plays louder in the menu and steps
// back while you're in play.

const BPM = 76;
const STEP = 60 / BPM / 4; // a sixteenth note, in seconds
const BAR = 16;
const LOOP = BAR * 8;
// per chord: bass root, then the pad's three notes (MIDI)
const CHORDS = [
  [45, 57, 60, 64], // Am
  [46, 58, 62, 65], // Bb, a half step up
  [41, 53, 56, 60], // Fm
  [40, 52, 56, 59], // E
];
// the bells, in sixteenths: an index into the pad's notes (3 is the octave), -1 rests
const ARPS = [
  [0, -1, -1, -1, 2, -1, -1, -1, 1, -1, -1, -1, -1, -1, -1, -1],
  [3, -1, -1, 2, -1, -1, 1, -1, -1, -1, 0, -1, -1, -1, -1, -1],
  [0, -1, 1, -1, -1, -1, 0, -1, -1, -1, -1, -1, 2, -1, -1, -1],
];
const DRONE = 33; // A1
const MENU_LEVEL = 0.55, PLAY_LEVEL = 0.25;

const hz = m => 440 * Math.pow(2, (m - 69) / 12);

export class Music {
  /** `sound` is the started Sound, whose context, noise and reverb the music shares. */
  constructor(sound) {
    this.sound = sound;
    this.on = true;
    this.playing = false;
    this.level = MENU_LEVEL;
    this.timer = null;
  }

  // called once the audio context exists (after a click), and again whenever it should resume
  start() {
    const s = this.sound, ctx = s.ctx;
    if (!ctx || !this.on || this.playing || document.hidden) return;
    if (!this.out) {
      this.out = ctx.createGain();
      this.out.gain.value = 0;
      this.out.connect(s.master);
      const send = ctx.createGain();
      send.gain.value = 0.6;
      this.out.connect(send);
      send.connect(s.verb);
      // the bells echo on the dotted eighth
      this.echo = ctx.createDelay(1);
      this.echo.delayTime.value = STEP * 3;
      const fb = ctx.createGain();
      fb.gain.value = 0.38;
      const tone = ctx.createBiquadFilter();
      tone.type = "lowpass";
      tone.frequency.value = 2200;
      this.echo.connect(tone);
      tone.connect(fb);
      fb.connect(this.echo);
      tone.connect(this.out);
      this.step = 0;
    }
    this.playing = true;
    this.next = ctx.currentTime + 0.1;
    this.out.gain.cancelScheduledValues(ctx.currentTime);
    this.out.gain.setTargetAtTime(this.level, ctx.currentTime, 0.8);
    this.timer = setInterval(() => this.schedule(), 50);
    this.schedule();
  }

  stop() {
    if (!this.playing) return;
    this.playing = false;
    clearInterval(this.timer);
    const t = this.sound.ctx.currentTime;
    this.out.gain.cancelScheduledValues(t);
    this.out.gain.setTargetAtTime(0, t, 0.15);
  }

  setOn(on) {
    this.on = on;
    if (on) this.start(); else this.stop();
  }

  /** Quieter while playing so the guns and zombies stay in front. */
  setInPlay(inPlay) {
    this.level = inPlay ? PLAY_LEVEL : MENU_LEVEL;
    if (this.playing) this.out.gain.setTargetAtTime(this.level, this.sound.ctx.currentTime, 0.6);
  }

  // queues every sixteenth that starts within the next 0.3s
  schedule() {
    const now = this.sound.ctx.currentTime;
    if (this.next < now) this.next = now + 0.05; // fell behind (e.g. a stalled tab): skip ahead rather than pile up
    while (this.next < now + 0.3) {
      this.note(this.step, this.next);
      this.step++;
      this.next += STEP;
    }
  }

  note(step, t) {
    const i = step % BAR, bar = Math.floor(step / BAR), loop = Math.floor(bar / 8);
    const chord = CHORDS[Math.floor(bar / 2) % 4];
    if (step % LOOP === 0) {
      this.drone(t, STEP * LOOP);
      this.boom(t);
    }
    if (i === 0 && bar % 2 === 0) this.pad(chord.slice(1), t, STEP * BAR * 2);
    if (i % 2 === 0) this.bass(chord[0] - (i % 8 === 0 ? 0 : 12), t, i % 8 === 0);
    // a heartbeat on the first beat of every bar
    if (i === 0) this.kick(t, 0.55);
    if (i === 3) this.kick(t, 0.32);
    // the bells sit out the first time round, then cycle through the patterns
    if (loop > 0) {
      const k = ARPS[(loop - 1) % ARPS.length][i];
      if (k >= 0) this.bell(chord[1 + (k % 3)] + (k === 3 ? 12 : 0), t);
    }
    // something metal dragged across concrete, far off, halfway through
    if (i === 8 && bar % 8 === 3) this.scrape(t);
    // wind rising into the start of each loop
    if (i === 0 && bar % 8 === 6) this.sound.noise(this.out, t, STEP * BAR * 2, "bandpass", 250, 700, 0.06, 0.7);
  }

  // a low saw that slowly opens and closes, held for a whole loop and crossfading into the next
  drone(t, dur) {
    const ctx = this.sound.ctx;
    const f = ctx.createBiquadFilter();
    f.type = "lowpass";
    f.Q.value = 4;
    f.frequency.value = 160;
    const lfo = ctx.createOscillator(), depth = ctx.createGain();
    lfo.frequency.value = 0.09;
    depth.gain.value = 90;
    lfo.connect(depth);
    depth.connect(f.frequency);
    const g = ctx.createGain();
    g.gain.setValueAtTime(0.0001, t);
    g.gain.linearRampToValueAtTime(0.13, t + 3);
    g.gain.setValueAtTime(0.13, t + dur - 1);
    g.gain.linearRampToValueAtTime(0.0001, t + dur + 2);
    f.connect(g);
    g.connect(this.out);
    for (const cents of [-6, 6]) {
      const o = ctx.createOscillator();
      o.type = "sawtooth";
      o.frequency.value = hz(DRONE);
      o.detune.value = cents;
      o.connect(f);
      o.start(t);
      o.stop(t + dur + 2.1);
    }
    lfo.start(t);
    lfo.stop(t + dur + 2.1);
  }

  // a deep, distant impact with a long tail in the room
  boom(t) {
    const d = this.sound.ctx.createGain();
    d.gain.value = 1;
    d.connect(this.out);
    this.sound.tone(d, t, "sine", 70, 24, 2.2, 0.6);
    this.sound.noise(d, t, 1.8, "lowpass", 400, 40, 0.35);
  }

  // a resonant band of noise sliding down, like metal scraped on concrete
  scrape(t) {
    const s = this.sound;
    s.noise(this.out, t, 1.6, "bandpass", 2400, 900, 0.07, 18);
    s.noise(this.out, t + 0.1, 1.4, "bandpass", 3100, 1300, 0.04, 22);
  }

  bass(m, t, accent) {
    const ctx = this.sound.ctx, dur = STEP * 1.8;
    const o = ctx.createOscillator();
    o.type = "sawtooth";
    o.frequency.value = hz(m);
    const f = ctx.createBiquadFilter();
    f.type = "lowpass";
    f.Q.value = 6;
    f.frequency.setValueAtTime(accent ? 900 : 600, t);
    f.frequency.exponentialRampToValueAtTime(120, t + dur);
    const g = ctx.createGain();
    g.gain.setValueAtTime(0.0001, t);
    g.gain.exponentialRampToValueAtTime(accent ? 0.32 : 0.22, t + 0.008);
    g.gain.exponentialRampToValueAtTime(0.0001, t + dur);
    o.connect(f);
    f.connect(g);
    g.connect(this.out);
    o.start(t);
    o.stop(t + dur + 0.02);
  }

  // three detuned saws per note, swelling in and out under a slowly opening filter
  pad(notes, t, dur) {
    const ctx = this.sound.ctx;
    const f = ctx.createBiquadFilter();
    f.type = "lowpass";
    f.Q.value = 1;
    f.frequency.setValueAtTime(350, t);
    f.frequency.linearRampToValueAtTime(1100, t + dur * 0.6);
    f.frequency.linearRampToValueAtTime(400, t + dur);
    const g = ctx.createGain();
    g.gain.setValueAtTime(0.0001, t);
    g.gain.linearRampToValueAtTime(0.06, t + dur * 0.35);
    g.gain.linearRampToValueAtTime(0.045, t + dur * 0.8);
    g.gain.linearRampToValueAtTime(0.0001, t + dur + 0.4);
    f.connect(g);
    g.connect(this.out);
    for (const m of notes) {
      for (const cents of [-9, 0, 9]) {
        const o = ctx.createOscillator();
        o.type = "sawtooth";
        o.frequency.value = hz(m);
        o.detune.value = cents;
        o.connect(f);
        o.start(t);
        o.stop(t + dur + 0.5);
      }
    }
  }

  kick(t, gain) {
    const ctx = this.sound.ctx;
    const o = ctx.createOscillator();
    o.type = "sine";
    o.frequency.setValueAtTime(95, t);
    o.frequency.exponentialRampToValueAtTime(38, t + 0.22);
    const g = ctx.createGain();
    g.gain.setValueAtTime(gain, t);
    g.gain.exponentialRampToValueAtTime(0.0001, t + 0.3);
    o.connect(g);
    g.connect(this.out);
    o.start(t);
    o.stop(t + 0.32);
  }

  bell(m, t) {
    const ctx = this.sound.ctx, dur = 1.4;
    const g = ctx.createGain();
    g.gain.setValueAtTime(0.0001, t);
    g.gain.exponentialRampToValueAtTime(0.09, t + 0.005);
    g.gain.exponentialRampToValueAtTime(0.0001, t + dur);
    g.connect(this.out);
    g.connect(this.echo);
    // two sines a little out of tune with each other, so the note wavers, and a quiet inharmonic overtone
    for (const [mul, amp, cents] of [[1, 0.6, -9], [1, 0.6, 9], [2.76, 0.2, 0]]) {
      const o = ctx.createOscillator(), a = ctx.createGain();
      o.type = "sine";
      o.frequency.value = hz(m) * mul;
      o.detune.value = cents;
      a.gain.value = amp;
      o.connect(a);
      a.connect(g);
      o.start(t);
      o.stop(t + dur + 0.02);
    }
  }
}
