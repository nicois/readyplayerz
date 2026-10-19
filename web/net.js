// The connection to the game server, over WebTransport: one reliable stream for joining and the lobby, and
// datagrams, which can be lost and never hold anything up, for the game itself. The wasm client makes and reads
// every message; this only moves the bytes.

export class Net {
  /** `onLost(why)` is called once if the connection goes, or can't be made. */
  constructor(client, onLost) {
    this.client = client;
    this.onLost = onLost;
    this.wt = null;
    this.gone = false;
  }

  /**
   * Connects and says hello: to make a room (`room` empty) or join one. `server` is the game server's host (the
   * one the page came from if empty), which says at /net which port the game is on and, if it made its own
   * certificate, the hash to trust it by.
   */
  async open(room, name, server) {
    if (typeof WebTransport === "undefined") throw new Error("This browser can't play online (it has no WebTransport). Try Chrome, Edge or Firefox.");
    const res = await fetch(server ? `https://${server}/net` : "net", { cache: "no-store" }).catch(() => null);
    if (!res?.ok) throw new Error(server ? "The game server isn't answering. Try again in a minute." : "No game server here: start it with ./server.sh");
    const info = await res.json();
    const url = `https://${server || location.hostname}:${info.port}/`, options = {};
    if (info.hash) options.serverCertificateHashes = [{ algorithm: "sha-256", value: new Uint8Array(info.hash) }];
    this.wt = new WebTransport(url, options);
    this.wt.closed.then(() => this.lost("The connection to the server closed."), e => this.lost(`Lost the connection to the server (${e.message || e}).`));
    await this.wt.ready;
    const bidi = await this.wt.createBidirectionalStream();
    this.stream = bidi.writable.getWriter();
    this.datagrams = this.wt.datagrams.writable.getWriter();
    await this.stream.write(this.client.net_hello(room, name));
    this.pump(bidi.readable.getReader(), b => this.client.net_stream(b));
    this.pump(this.wt.datagrams.readable.getReader(), b => this.client.net_datagram(b));
  }

  async pump(reader, take) {
    try {
      for (;;) {
        const { value, done } = await reader.read();
        if (done) return;
        take(value);
      }
    } catch (e) {
      this.lost(`Lost the connection to the server (${e.message || e}).`);
    }
  }

  /** Sends the datagrams the game has for the server: each comes with its length in front. */
  flush() {
    if (!this.datagrams || this.gone) return;
    const b = this.client.net_take();
    for (let i = 0; i + 2 <= b.length;) {
      const n = b[i] | (b[i + 1] << 8);
      this.datagrams.write(b.slice(i + 2, i + 2 + n)).catch(() => {});
      i += 2 + n;
    }
  }

  /** The host letting the waves come. */
  start() {
    this.stream?.write(this.client.net_start()).catch(() => {});
  }

  close() {
    this.gone = true;
    try { this.wt?.close(); } catch {}
  }

  lost(why) {
    if (this.gone) return;
    this.gone = true;
    this.onLost(why);
  }
}
