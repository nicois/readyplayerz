//! The game server. It serves the page over plain HTTP (for playing on this machine) and runs rooms of up to six
//! players over WebTransport: each room is the real game (`arena::sim::host::Room`), on a thread of its own at 60
//! ticks a second, and each player's session moves the bytes between their browser and their room.
//!
//! Settings come from the environment: `HTTP_PORT` (8080) for the page, `GAME_PORT` (4433, UDP) for the game,
//! `WEB` (`web`) for where the page is. `GAME_BIND` is the address to take the game's UDP on (any, by default; on
//! Fly it has to be `fly-global-services`), and `PUBLIC_HOST` the name players reach the server by. With
//! `TLS_CERT` and `TLS_KEY` (PEM files for the server's domain) it shows browsers a real certificate; without, it
//! makes its own, which browsers only take when told its hash, which the page asks `/net` for. A browser won't take
//! one valid for more than two weeks, so it is made again every week.
//!
//! `MAX_ROOMS` (20) is how many rooms can be open at once. Past that, whoever makes a room waits in line for one,
//! told their place as it changes; joining a room already open never waits.

use arena::net::{self, Frames, Msg};
use arena::sim::host::Room;
use arena::sim::{Input, STEP};
use std::collections::{HashMap, VecDeque};
use std::hash::{BuildHasher, Hasher};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{mpsc as amp, oneshot};
use wtransport::endpoint::IncomingSession;
use wtransport::tls::Sha256DigestFmt;
use wtransport::{Endpoint, Identity, RecvStream, SendStream, ServerConfig};

/// A room with nobody in it is closed after this long, or this much sooner while someone waits for a room.
const EMPTY_FOR: Duration = Duration::from_secs(60);
const EMPTY_FOR_IN_DEMAND: Duration = Duration::from_secs(10);

type Error = Box<dyn std::error::Error + Send + Sync>;

/// What a session tells its room.
enum ToRoom {
    Join { name: String, out: amp::UnboundedSender<Out>, reply: oneshot::Sender<Option<u8>> },
    Inputs(u8, u32, Vec<Input>),
    Start(u8),
    Rename(u8, String),
    Leave(u8),
}

/// What a room has for a session to send.
enum Out {
    Datagram(Vec<u8>),
    Stream(Vec<u8>),
}

/// The rooms open, and the line for new ones.
struct Lobby {
    max: usize,
    rooms: HashMap<String, mpsc::Sender<ToRoom>>,
    /// the tickets of those waiting to make a room, first in line first
    line: VecDeque<u64>,
}

/// Whether a room can be made for someone now, or their place in line for one (1 is next).
#[derive(Debug, PartialEq)]
enum Turn {
    Now,
    Wait(usize),
}

impl Lobby {
    fn new(max: usize) -> Lobby {
        Lobby { max, rooms: HashMap::new(), line: VecDeque::new() }
    }

    /// Ticket `t` asking for a room, at the back of the line the first time it asks: its turn once it is first in
    /// line and there is space, when it leaves the line for its room to be made.
    fn turn(&mut self, t: u64) -> Turn {
        let place = self.line.iter().position(|&w| w == t).unwrap_or_else(|| {
            self.line.push_back(t);
            self.line.len() - 1
        });
        if place == 0 && self.rooms.len() < self.max {
            self.line.pop_front();
            return Turn::Now;
        }
        Turn::Wait(place + 1)
    }

    /// Whether a room that has had nobody in it for `empty` is to close: after a while, sooner while anyone waits.
    fn closes(&self, empty: Duration) -> bool {
        empty > EMPTY_FOR || (empty > EMPTY_FOR_IN_DEMAND && !self.line.is_empty())
    }
}

type Rooms = Arc<Mutex<Lobby>>;

/// A place in line, given up when dropped however the session ends.
struct Ticket(Rooms, u64);

impl Drop for Ticket {
    fn drop(&mut self) {
        self.0.lock().unwrap().line.retain(|&t| t != self.1);
    }
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

/// A random number, from the hasher's random keys.
fn random() -> u64 {
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(Instant::now().elapsed().as_nanos());
    h.finish()
}

/// A room's code: four letters, none that could be read as a digit.
fn new_code(rooms: &HashMap<String, mpsc::Sender<ToRoom>>) -> String {
    const LETTERS: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ";
    loop {
        let r = random();
        let code: String = (0..4).map(|k| LETTERS[((r >> (k * 8)) % LETTERS.len() as u64) as usize] as char).collect();
        if !rooms.contains_key(&code) {
            return code;
        }
    }
}

/// A room's own thread: the game, played to the clock, with the snapshots sent off as they are due.
fn run_room(code: String, mut room: Room, rx: mpsc::Receiver<ToRoom>, rooms: Rooms) {
    let mut outs: HashMap<u8, amp::UnboundedSender<Out>> = HashMap::new();
    let send_all = |outs: &HashMap<u8, amp::UnboundedSender<Out>>, m: &Msg| {
        for tx in outs.values() {
            let _ = tx.send(Out::Stream(m.frame()));
        }
    };
    let (start, mut ticks) = (Instant::now(), 0u64);
    let mut empty_since = Some(Instant::now());
    let mut started = false;
    println!("room {code} open, map {}", room.map);
    loop {
        while let Ok(m) = rx.try_recv() {
            match m {
                ToRoom::Join { name, out, reply } => {
                    let id = room.join(&name);
                    if let Some(id) = id {
                        let _ = out.send(Out::Stream(Msg::Welcome { id, room: code.clone(), map: room.map, seed: room.seed }.frame()));
                        outs.insert(id, out);
                        send_all(&outs, &room.roster());
                        println!("room {code}: {name:?} in as {id}, {} playing", outs.len());
                    }
                    let _ = reply.send(id);
                }
                ToRoom::Inputs(id, first, inputs) => room.inputs(id, first, &inputs),
                ToRoom::Start(id) => room.start(id),
                ToRoom::Rename(id, name) => {
                    room.rename(id, &name);
                    send_all(&outs, &room.roster());
                }
                ToRoom::Leave(id) => {
                    room.leave(id);
                    outs.remove(&id);
                    send_all(&outs, &room.roster());
                    println!("room {code}: {id} left, {} playing", outs.len());
                }
            }
        }
        if room.game.waves_on() != started {
            started = room.game.waves_on();
            send_all(&outs, &room.roster());
        }
        // as many steps as the clock says are due; a long stall is let go rather than caught up all at once
        let due = (start.elapsed().as_secs_f64() / STEP as f64) as u64;
        if due > ticks + 30 {
            ticks = due - 1;
        }
        while ticks < due {
            ticks += 1;
            room.tick();
            if room.snapshot_due() {
                for (id, tx) in &outs {
                    for d in room.snapshot_for(*id) {
                        let _ = tx.send(Out::Datagram(d));
                    }
                }
                room.sent();
            }
        }
        match (room.is_empty(), empty_since) {
            (false, _) => empty_since = None,
            (true, None) => empty_since = Some(Instant::now()),
            (true, Some(t)) if t.elapsed() > EMPTY_FOR_IN_DEMAND => {
                let mut lobby = rooms.lock().unwrap();
                if lobby.closes(t.elapsed()) {
                    lobby.rooms.remove(&code);
                    println!("room {code} closed, {} open", lobby.rooms.len());
                    return;
                }
            }
            _ => {}
        }
        let next = start + Duration::from_secs_f64((ticks + 1) as f64 * STEP as f64);
        std::thread::sleep(next.saturating_duration_since(Instant::now()).min(Duration::from_millis(5)));
    }
}

/// A new room on map `map`, on a thread of its own.
fn open_room(lobby: &mut Lobby, rooms: &Rooms, map: u8) -> mpsc::Sender<ToRoom> {
    let code = new_code(&lobby.rooms);
    let (tx, rx) = mpsc::channel();
    let (map, seed) = (map.min(arena::map::MAPS - 1), random() as u32);
    let (rooms, name) = (rooms.clone(), code.clone());
    // the game is made on its own thread, as it never leaves it
    std::thread::spawn(move || run_room(name, Room::new(map, seed), rx, rooms));
    lobby.rooms.insert(code, tx.clone());
    tx
}

/// A room made on map `map` as soon as there is space for one: until then the session waits in line, told its
/// place whenever that changes. None if they went before their turn came.
async fn make_room(rooms: &Rooms, map: u8, send: &mut SendStream, recv: &mut RecvStream) -> Result<Option<mpsc::Sender<ToRoom>>, Error> {
    let ticket = Ticket(rooms.clone(), random());
    let (mut told, mut buf) = (0, [0u8; 256]);
    loop {
        let place = {
            let mut lobby = rooms.lock().unwrap();
            let Turn::Wait(place) = lobby.turn(ticket.1) else { return Ok(Some(open_room(&mut lobby, rooms, map))) };
            if told == 0 {
                println!("all {} rooms taken, {} waiting", lobby.max, lobby.line.len());
            }
            place
        };
        if place != told {
            send.write_all(&Msg::Queued { place: place.min(u16::MAX as usize) as u16 }.frame()).await?;
            told = place;
        }
        // nothing is said while waiting: anything read is the end of the stream
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_millis(500)) => {}
            n = recv.read(&mut buf) => if !matches!(n, Ok(Some(_))) { return Ok(None) },
        }
    }
}

/// One player's session: their hello, then their inputs to the room and the room's snapshots and news to them,
/// until either side goes.
async fn session(incoming: IncomingSession, rooms: Rooms) -> Result<(), Error> {
    let conn = incoming.await?.accept().await?;
    let (mut send, mut recv) = conn.accept_bi().await?;
    let (mut frames, mut buf) = (Frames::default(), vec![0u8; 8192]);
    let hello = loop {
        let Some(n) = recv.read(&mut buf).await? else { return Ok(()) };
        if let Some(m) = frames.push(&buf[..n]).into_iter().next() {
            break m;
        }
    };
    let refuse = |why: &str| Msg::Refused { why: why.into() }.frame();
    let Msg::Hello { version, room: code, map, name } = hello else { return Ok(()) };
    if version != net::VERSION {
        send.write_all(&refuse("The game has been updated: reload the page.")).await?;
        return Ok(());
    }
    let tx = if code.is_empty() {
        let Some(tx) = make_room(&rooms, map, &mut send, &mut recv).await? else { return Ok(()) };
        tx
    } else {
        let tx = rooms.lock().unwrap().rooms.get(&code.to_uppercase()).cloned();
        let Some(tx) = tx else {
            send.write_all(&refuse("There is no room with that code.")).await?;
            return Ok(());
        };
        tx
    };
    let (out, mut outgoing) = amp::unbounded_channel();
    let (reply, id) = oneshot::channel();
    tx.send(ToRoom::Join { name, out, reply })?;
    let Some(id) = id.await? else {
        send.write_all(&refuse("That room is full.")).await?;
        return Ok(());
    };
    let result: Result<(), Error> = async {
        loop {
            tokio::select! {
                out = outgoing.recv() => match out {
                    // a datagram that can't go (the link is full) is only one of many
                    Some(Out::Datagram(d)) => { let _ = conn.send_datagram(d); }
                    Some(Out::Stream(b)) => send.write_all(&b).await?,
                    None => return Ok(()),
                },
                d = conn.receive_datagram() => {
                    if let Some((first, inputs)) = net::decode_inputs(&d?) {
                        tx.send(ToRoom::Inputs(id, first, inputs))?;
                    }
                }
                n = recv.read(&mut buf) => {
                    let Some(n) = n? else { return Ok(()) };
                    for m in frames.push(&buf[..n]) {
                        match m {
                            Msg::Start => tx.send(ToRoom::Start(id))?,
                            Msg::Name { name } => tx.send(ToRoom::Rename(id, name))?,
                            _ => {}
                        }
                    }
                }
            }
        }
    }
    .await;
    let _ = tx.send(ToRoom::Leave(id));
    result
}

/// The page, over plain HTTP: the files in `web`, and `/net`, which says where the game is and the hash of its
/// certificate (null when it has a real one).
async fn serve_http(port: u16, web: String, game_port: u16, hash: Arc<Mutex<Option<String>>>) -> std::io::Result<()> {
    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port)).await?;
    loop {
        let (mut sock, _) = listener.accept().await?;
        let (web, hash) = (web.clone(), hash.clone());
        tokio::spawn(async move {
            let mut req = Vec::new();
            let mut buf = [0u8; 2048];
            while !req.windows(4).any(|w| w == b"\r\n\r\n") && req.len() < 16384 {
                match sock.read(&mut buf).await {
                    Ok(0) | Err(_) => return,
                    Ok(n) => req.extend_from_slice(&buf[..n]),
                }
            }
            let line = String::from_utf8_lossy(&req).lines().next().unwrap_or("").to_string();
            let path = line.split(' ').nth(1).unwrap_or("/").split('?').next().unwrap_or("/").to_string();
            let (status, kind, body) = if path == "/net" {
                let hash = hash.lock().unwrap().clone().unwrap_or_else(|| "null".into());
                ("200 OK", "application/json", format!("{{\"port\":{game_port},\"hash\":{hash}}}").into_bytes())
            } else {
                let file = if path.ends_with('/') { format!("{path}index.html") } else { path.clone() };
                let kind = match file.rsplit('.').next().unwrap_or("") {
                    "html" => "text/html; charset=utf-8",
                    "js" => "text/javascript",
                    "wasm" => "application/wasm",
                    "webp" => "image/webp",
                    "png" => "image/png",
                    "json" => "application/json",
                    "xml" => "application/xml",
                    "css" => "text/css",
                    _ => "text/plain",
                };
                match (file.contains(".."), tokio::fs::read(format!("{web}{file}")).await) {
                    (false, Ok(b)) => ("200 OK", kind, b),
                    _ => ("404 Not Found", "text/plain", b"not found".to_vec()),
                }
            };
            let head = format!("HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n", body.len());
            let _ = sock.write_all(head.as_bytes()).await;
            let _ = sock.write_all(&body).await;
        });
    }
}

fn config(identity: Identity, port: u16) -> ServerConfig {
    let builder = match std::env::var("GAME_BIND") {
        Ok(host) => {
            use std::net::ToSocketAddrs;
            let addr = (host.as_str(), port).to_socket_addrs().expect("GAME_BIND").find(|a| a.is_ipv4()).expect("an IPv4 address for GAME_BIND");
            ServerConfig::builder().with_bind_address(addr)
        }
        Err(_) => ServerConfig::builder().with_bind_default(port),
    };
    builder.with_identity(identity).keep_alive_interval(Some(Duration::from_secs(3))).max_idle_timeout(Some(Duration::from_secs(15))).unwrap().build()
}

fn self_signed() -> Identity {
    let mut names = vec!["localhost".to_string(), "127.0.0.1".into(), "::1".into()];
    names.extend(std::env::var("PUBLIC_HOST").ok());
    Identity::self_signed(names).expect("a certificate")
}

#[tokio::main]
async fn main() {
    let http_port: u16 = env_or("HTTP_PORT", "8080").parse().expect("HTTP_PORT");
    let game_port: u16 = env_or("GAME_PORT", "4433").parse().expect("GAME_PORT");
    let web = env_or("WEB", "web");
    let real = match (std::env::var("TLS_CERT"), std::env::var("TLS_KEY")) {
        (Ok(cert), Ok(key)) => Some(Identity::load_pemfiles(cert, key).await.expect("TLS_CERT and TLS_KEY")),
        _ => None,
    };
    let own = real.is_none();
    let identity = real.unwrap_or_else(self_signed);
    let digest = |id: &Identity| id.certificate_chain().as_slice()[0].hash().fmt(Sha256DigestFmt::BytesArray);
    let hash = Arc::new(Mutex::new(own.then(|| digest(&identity))));
    let endpoint = Arc::new(Endpoint::server(config(identity, game_port)).expect("the game port"));
    if own {
        let (endpoint, hash) = (endpoint.clone(), hash.clone());
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(7 * 24 * 3600)).await;
                let identity = self_signed();
                let next = digest(&identity);
                if endpoint.reload_config(config(identity, game_port), false).is_ok() {
                    *hash.lock().unwrap() = Some(next);
                }
            }
        });
    }
    let rooms: Rooms = Arc::new(Mutex::new(Lobby::new(env_or("MAX_ROOMS", "20").parse().expect("MAX_ROOMS"))));
    tokio::spawn(async move {
        if let Err(e) = serve_http(http_port, web, game_port, hash).await {
            eprintln!("the page can't be served on port {http_port}: {e}");
        }
    });
    println!("http://localhost:{http_port}  (game on UDP port {game_port})");
    loop {
        let incoming = endpoint.accept().await;
        let rooms = rooms.clone();
        tokio::spawn(async move {
            if let Err(e) = session(incoming, rooms).await {
                eprintln!("a session ended: {e}");
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ticket `t`'s turn, with its room opened if it came.
    fn ask(lobby: &mut Lobby, t: u64) -> Turn {
        let turn = lobby.turn(t);
        if turn == Turn::Now {
            lobby.rooms.insert(format!("R{t}"), mpsc::channel().0);
        }
        turn
    }

    #[test]
    fn rooms_go_in_turn_to_whoever_has_waited_longest() {
        let mut lobby = Lobby::new(1);
        assert_eq!(ask(&mut lobby, 1), Turn::Now);
        assert_eq!(ask(&mut lobby, 2), Turn::Wait(1));
        assert_eq!(ask(&mut lobby, 3), Turn::Wait(2));
        // a room closes: someone new doesn't jump the line, nor does the second in it
        lobby.rooms.clear();
        assert_eq!(ask(&mut lobby, 4), Turn::Wait(3));
        assert_eq!(ask(&mut lobby, 3), Turn::Wait(2));
        assert_eq!(ask(&mut lobby, 2), Turn::Now);
        assert_eq!(ask(&mut lobby, 3), Turn::Wait(1));
        assert_eq!(ask(&mut lobby, 4), Turn::Wait(2));
    }

    #[test]
    fn whoever_gives_up_waiting_lets_the_rest_move_up() {
        let rooms: Rooms = Arc::new(Mutex::new(Lobby::new(1)));
        assert_eq!(ask(&mut rooms.lock().unwrap(), 1), Turn::Now);
        let first = Ticket(rooms.clone(), 2);
        assert_eq!(ask(&mut rooms.lock().unwrap(), first.1), Turn::Wait(1));
        assert_eq!(ask(&mut rooms.lock().unwrap(), 3), Turn::Wait(2));
        drop(first);
        assert_eq!(ask(&mut rooms.lock().unwrap(), 3), Turn::Wait(1));
        rooms.lock().unwrap().rooms.clear();
        assert_eq!(ask(&mut rooms.lock().unwrap(), 3), Turn::Now);
    }

    #[test]
    fn an_empty_room_closes_sooner_while_anyone_waits_for_one() {
        let mut lobby = Lobby::new(1);
        let s = Duration::from_secs;
        assert!(!lobby.closes(s(30)));
        assert!(lobby.closes(s(61)));
        ask(&mut lobby, 1);
        ask(&mut lobby, 2);
        assert!(!lobby.closes(s(5)));
        assert!(lobby.closes(s(11)));
    }
}
