//! The network load test: a full room of six holding out on the Suburb against the most zombies the game lets be
//! up at once, played on the server's own room for a minute, measuring what it sends each player, what they send
//! it, and how long its ticks take. It fails past the limits in `bench/limits.txt`; `./bench.sh` runs it (and the
//! drawing bench in the browser) and keeps a line of the results in `bench/history.tsv`.
//!
//! `cargo test --release --lib load -- --ignored --nocapture` runs it alone. The bytes are the same every run; the
//! tick times are CPU time, so another program busy on the machine changes them little, and are only held to
//! their limits in a release build, as a debug one is many times slower.

use super::bench::{self, WAVE};
use super::host::{Room, SNAPSHOT_EVERY};
use super::*;
use crate::net::{self, Snapshot, MAX_PLAYERS};

/// The CPU time this thread has taken, in milliseconds: unlike the time on the clock, it doesn't grow while another
/// program has the CPU, so a busy machine changes it little.
fn cpu_ms() -> f64 {
    let mut t = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut t) };
    t.tv_sec as f64 * 1000.0 + t.tv_nsec as f64 / 1e6
}

/// Seconds played before measuring, for the horde to fill up, and seconds measured.
const WARMUP: f32 = 10.0;
const MEASURED: f32 = 60.0;
/// What each datagram costs on the wire beyond its payload: IPv4 and UDP headers, QUIC's short header and
/// datagram frame, and the encryption tag.
const OVERHEAD: usize = 48;

/// `bench/limits.txt` as name and value pairs.
fn limits() -> Vec<(String, f64)> {
    include_str!("../../bench/limits.txt")
        .lines()
        .map(|l| l.split('#').next().unwrap().trim())
        .filter(|l| !l.is_empty())
        .map(|l| {
            let (k, v) = l.split_once(char::is_whitespace).expect("a limit is a name and a value");
            (k.to_string(), v.trim().parse().expect("a limit's value is a number"))
        })
        .collect()
}

#[derive(Default)]
struct Parts {
    own: usize,
    mates: usize,
    zombies: usize,
    shots: usize,
    loot: usize,
    nades: usize,
}

/// The bytes each kind of record of `s` takes, by encoding it alone, less the datagram's own header.
fn parts(s: &Snapshot) -> Parts {
    let size = |part: Snapshot| net::encode_snapshot(&part).iter().map(|d| d.len() - 5).sum::<usize>();
    let base = || Snapshot { tick: s.tick, ..Default::default() };
    Parts {
        own: size(Snapshot { own: s.own.clone(), world: s.world, ..base() }),
        mates: size(Snapshot { mates: s.mates.clone(), ..base() }),
        zombies: size(Snapshot { zombies: s.zombies.clone(), ..base() }),
        shots: size(Snapshot { shots: s.shots.clone(), ..base() }),
        loot: size(Snapshot { loot: s.loot.clone(), ..base() }),
        nades: size(Snapshot { nades: s.nades.clone(), booms: s.booms.clone(), ..base() }),
    }
}

#[test]
#[ignore = "a minute of a full room: run by ./bench.sh, or with --ignored"]
fn load_six_players_against_a_full_horde_stay_within_the_limits() {
    let mut room = Room::new(bench::MAP, bench::SEED);
    let ids: Vec<u8> = (0..MAX_PLAYERS).map(|k| room.join(&format!("Bench {k}")).unwrap()).collect();
    room.start(ids[0]);
    let (warmup, measured) = ((WARMUP / STEP) as u32, (MEASURED / STEP) as u32);
    let per_sec = (1.0 / STEP).round() as u32;
    let snaps_per_sec = (per_sec / SNAPSHOT_EVERY) as f64;

    let (mut snaps, mut bytes, mut wire, mut worst, mut most_datagrams) = (0usize, 0usize, 0usize, 0usize, 0usize);
    let mut sum = Parts::default();
    let (mut zombies_sent, mut alive_seen) = (0usize, 0usize);
    // bytes on the wire to each player in each second, for the worst second
    let mut second = vec![0usize; MAX_PLAYERS];
    let mut worst_second = 0usize;
    let mut ticks_ms = Vec::new();

    for n in 0..warmup + measured {
        room.game.keep_up_load();
        for (k, &id) in ids.iter().enumerate() {
            let inp = bench::input(&room.game, k, n);
            room.inputs(id, n, &[inp]);
        }

        let t0 = cpu_ms();
        room.tick();
        let mut out = Vec::new();
        if room.snapshot_due() {
            out = room.snapshots().into_iter().map(|(_, datagrams)| datagrams).collect();
        }
        let ms = cpu_ms() - t0;
        // what the bytes go on, from one player's snapshot (they differ only in whose own record it is)
        let shown = (!out.is_empty()).then(|| room.snapshot_of(ids[0]).unwrap());
        if !out.is_empty() {
            room.sent();
        }
        if n < warmup {
            continue;
        }
        ticks_ms.push(ms);

        for (k, datagrams) in out.iter().enumerate() {
            let b: usize = datagrams.iter().map(Vec::len).sum();
            snaps += 1;
            bytes += b;
            wire += b + datagrams.len() * OVERHEAD;
            second[k] += b + datagrams.len() * OVERHEAD;
            worst = worst.max(b);
            most_datagrams = most_datagrams.max(datagrams.len());
        }
        if let Some(s) = shown {
            let p = parts(&s);
            zombies_sent += s.zombies.len();
            alive_seen += room.game.bots.iter().filter(|b| b.alive).count();
            for (a, b) in [(&mut sum.own, p.own), (&mut sum.mates, p.mates), (&mut sum.zombies, p.zombies), (&mut sum.shots, p.shots), (&mut sum.loot, p.loot), (&mut sum.nades, p.nades)] {
                *a += b;
            }
        }
        if (n - warmup + 1) % per_sec == 0 {
            worst_second = worst_second.max(*second.iter().max().unwrap());
            second.iter_mut().for_each(|s| *s = 0);
        }
    }
    let kills: u32 = room.game.players.iter().map(|p| p.kills).sum();

    // what each client sends: every tick, its latest inputs over again (see `net::RESEND`)
    let up = net::encode_inputs(0, &[Input::default(); net::RESEND]).len() + OVERHEAD;

    let tick_ms = ticks_ms.iter().sum::<f64>() / ticks_ms.len() as f64;
    // the slowest one in a hundred, and the very slowest
    ticks_ms.sort_by(f64::total_cmp);
    let (tick_p99, tick_max) = (ticks_ms[ticks_ms.len() * 99 / 100], *ticks_ms.last().unwrap());
    let kb = |b: f64| b / 1000.0;
    let per_snap = snaps as f64 / MAX_PLAYERS as f64;
    let results = [
        ("net_down_kbps", kb(wire as f64 / snaps as f64 * snaps_per_sec)),
        ("net_down_peak_kbps", kb(worst_second as f64)),
        ("net_server_out_kbps", kb(wire as f64 / snaps as f64 * snaps_per_sec * MAX_PLAYERS as f64)),
        ("net_up_kbps", kb((up * per_sec as usize) as f64)),
        ("net_snapshot_bytes", bytes as f64 / snaps as f64),
        ("net_snapshot_bytes_max", worst as f64),
        ("net_snapshot_datagrams_max", most_datagrams as f64),
        ("server_tick_ms", tick_ms),
        ("server_tick_ms_p99", tick_p99),
        ("server_tick_ms_max", tick_max),
    ];

    let avg = |b: usize| b as f64 / per_snap;
    println!("\nsix players, {MEASURED} s against wave {WAVE}'s horde ({:.1} zombies up on average, {:.1} sent a snapshot), {kills} killed", alive_seen as f64 / per_snap, zombies_sent as f64 / per_snap);
    println!("a snapshot, {snaps_per_sec} a second, to each player, in bytes: own {:.0}, mates {:.0}, zombies {:.0}, shots {:.0}, loot {:.0}, grenades {:.0}", avg(sum.own), avg(sum.mates), avg(sum.zombies), avg(sum.shots), avg(sum.loot), avg(sum.nades));
    let limits = limits();
    let mut over = Vec::new();
    for (name, value) in results {
        let limit = limits.iter().find(|(k, _)| k == name).map(|l| l.1);
        let timed = name.starts_with("server_tick");
        let held = limit.is_some() && !(timed && cfg!(debug_assertions));
        let mark = match limit {
            Some(l) if held && value > l => {
                over.push(format!("{name} {value:.2} is over its limit of {l}"));
                "OVER"
            }
            Some(_) if !held => "(debug build: not held to its limit)",
            Some(_) => "ok",
            None => "",
        };
        println!("bench {name} {value:.2}  limit {} {mark}", limit.map_or("-".into(), |l| l.to_string()));
    }
    assert!(over.is_empty(), "{}", over.join("\n"));
}
