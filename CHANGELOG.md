# Changelog

## 2026-10-20

### Cheaper pixels: noise from tables, and sharp screens drawn at 1.5
- The surface patterns (skin, cloth, hair, wood, metal, brick, plaster) read their noise from two small tiling tables, a 256×256 and a 64×64×64, instead of working out four or eight random corners per lookup; a zombie's skin takes about twenty lookups a pixel. They look the same, with the speckles in other places. On an M1 at 1920×993 on High, a frame of the horde bench went from about 10.1 ms to 8.9.
- A sharp (Retina) screen is drawn at most 1.5 pixels to the CSS pixel (was 2), so about half the pixels. On High it went from 34 to about 60 fps in the horde bench.

### Edges smoothed by FXAA instead of multisampling
- Every quality now draws the picture without multisampling and smooths its edges with an FXAA pass on the way to the screen (Medium had 2x MSAA, High 4x, Low none). On an M1 at 1920×993, High's 4x MSAA took about a quarter of each frame; the FXAA pass takes about a millisecond.
- FXAA also smooths edges multisampling never did: the holes in the zombies' clothes and their hairlines. Side by side with 4x MSAA no difference could be seen in play.

### A load bench: what six players against a full horde cost the network and the GPU
- `./bench.sh` plays six players against the most zombies the game lets be up (24, wave 6's mix) on the Suburb, all by script and the same every run, and holds the results to the limits in `bench/limits.txt`; it fails if one is passed, and adds a line of the results to `bench/history.tsv` to compare changes by.
- Network (`src/sim/load.rs`, a `cargo test` on the server's own room, a minute of play): the bytes each player is sent a second, on average and in the worst second, all six together, what each sends back, a snapshot's size and datagrams, and what it is spent on (zombies, mates, own record, shots, loot); and the server's tick times, as CPU time. `./bench.sh net` runs only this.
- Drawing (`?horde` on the page, run by the script in headless Chrome on the real GPU at 1920x993, no vsync): frames a second, frame times, the game code's share, the GPU's time where the browser tells it, and the draw calls and triangles a frame for the picture and for the sun's shadow, which the renderer now counts.
- First results: each player is sent 27 KB/s (162 KB/s out of the server for the room), 853 bytes a snapshot of which 605 are zombies, 940 at most: 60 bytes short of a second datagram. A frame draws 267 calls and 425k triangles, plus 211 calls and 213k triangles for the shadow; 34 to 83 fps on an M1, depending on how busy the machine is.

### Weapon crates are taken with E, into any slot; pickups pop up
- Walking over a weapon crate no longer takes its gun. In reach of one (1.6 m) the HUD says "E Take the AK-47", and what it would leave behind; E takes it, running or not.
- The gun goes into an empty slot if there is one, and otherwise in place of the gun in hand, whatever kind either is: holding the Pistol, a rifle can replace it, for two rifles. The gun left behind is gone.
- An ammo box pops up a little cartoon "ammo" under the aim, rising and fading; a crate pops up the gun's name.
- The take key goes over the network with the other keys (protocol version 5). The controls in Settings and on the About page list E.

### The zombies can't be walked away from
- Runners go 7 to 7.8 m/s (were 5 to 5.8): faster than a walk with any gun, slower than a sprint. A sprint opens a few metres on one, but once it runs out the runner catches up, even sprinting again each time the bar comes back.
- Walkers go 3.5 to 4.5 m/s (were 2.5 to 3.4): still slower than a walk, but they close in on anyone who stops to reload or loot.
- More of each wave are runners: about a quarter on wave one, rising to 60% by wave nine (was 14% rising to 40%). Brutes are unchanged.

### The guns are proportioned like the real ones
- Every gun's trigger moved back 5.3 cm, to 7.4 cm from the back of the grip as on a real gun (it was 12.7), with the trigger guards shortened to match. The rifles' triggers were buried in the magwell; they now hang into the guard where they can be seen.
- Magazines are as deep front to back as their rounds are long: the Carbine's and AK-47's grew from under 4 cm to 6, the Sniper's from 4.6 to 8, and their magwells with them. The SMG's sits right ahead of the guard instead of 6 cm in front of it.
- Shorter where they were long: the Carbine has a 16-inch barrel (89 cm overall, was 94), the AK-47's barrel stands 37 cm out of the receiver (91 cm overall, was 100), the Pistol's slide is a real 18.5 cm (was 22), the Desert Eagle's barrel is six inches (27 cm overall, was 30).
- The Sniper's cheek riser sits on the stock instead of floating over it; the Revolver's trigger guard reaches its grip, and its underlug is slimmer.
- The loadout pictures are made again from the new models.
- Dev: `?bench=i[,hands]` draws gun `i` flat and to scale, from the side and from above, under a centimetre ruler with the bore, the sight line and the muzzle marked (`&half=m` fixes the scale, to compare guns); `?hold=i` puts gun `i` in hand whatever the loadout.

### One map: the Suburb
- The Playground (the ruined town) is no longer played: solo and online both play the Suburb, and the map pickers show only its card. A map saved from before, or asked for by a page from before, is the Suburb too, on the server as well.
- The town's code stays, for the tests built on it and to come back to (`map::played` picks the map a game is made on); its picture is gone.
- The About page, `llms.txt` and the game's structured data describe the one map; the solo screen no longer says to pick where to make your stand.

### Loot online too, and weapon crates that can be found from afar
- Online is played for loot as well: each life starts with only the Pistol, and the guns are found in weapon crates. The death screen says so online too.
- Online, the server spawns the ammo boxes and the crates for the room and sends them in its snapshots (protocol version 3). Pickups run in each player's step, so a client takes a box or crate on the same input the server does, without waiting for it.
- The server's record of a player now carries their guns, rounds and grenades. A client carrying something else after the same input (a crate a teammate got to first) is put right by the difference, so shots fired since still count.
- Crates are placed at least 10 m from every player in the room, not only the first.
- A column of red light rises out of each weapon crate, pulsing and thinning as it rises, so one can be seen from across the map.
- Each crate within 45 m chimes every 2.5 s, panned and quieter with distance, so it can be found by ear.
- The About page and `llms.txt` say online starts each life with the Pistol too, and how the crates can be found.

### Privacy and terms pages
- New `/privacy` page: what the analytics collect (a visitor ID in the browser, game events, the device, a rough location), why, who handles it (PostHog, Cloudflare, Fly.io) and how to have it deleted, by writing to contact@readyplayerz.com.
- New `/terms` page: free and as it is, fair play online (no cheating, attacking the server or offensive names), ownership and Finnish law.
- The main menu's link under the menu items is now a row: About the game · Discord · Privacy · Terms, with Discord in its own blue. The About page links the Discord from the online section, and its footer links all three.
- The About page and `llms.txt` say solo starts each life with the Pistol and finds the other guns in crates.

### A baseball bat for the secondary slot: swung, not fired (`b3de427`)
- A new Melee class in the secondary picker, with the Baseball bat: no magazine, no reloads, no aiming, and 112% movement speed.
- Fire swings it, alternating a flat swing from the right and a backhand chop from the left; held, it keeps swinging, every 0.55 s.
- A swing connects 0.13 s after it starts, reaching 2.4 m across a wide arc, and hits up to three zombies, nearest first: 100 to the head (a walker or runner in one), 60 to the body (a runner in one).
- A hit knocks the zombie off its feet the way the swing goes (a brute barely gives), sprays blood that way, holds the swing still for a moment (longer on a kill) and shakes the view; a swing at a wall in reach thuds against it and throws off splinters.
- The bat is ash with a black-dipped barrel, a burned maker's band and grip tape over the handle and knob, held in both hands; it gathers blood coat by coat as it hits, cleaned each life. Other players are seen holding it on the shoulder.
- New sounds: a whoosh for the swing, a wet thwack for a hit (a bone crack for the head, a deeper crunch for a kill), a wooden clonk off a wall.
- The HUD hides the ammo count with the bat out, and the picker shows only Damage, Fire rate and Mobility for it.
- Playing for loot, weapon crates can hold the bat; picking one up replaces the Pistol, as a Revolver or Desert Eagle does.
- Dev: `?swing=u,n,gore` holds the bat partway through a swing, bloodied as much as `gore`, with the game paused.

### An About page, real favicons and fuller search data (`1f6d2c0`)
- New `/about` page, linked from the main menu (it opens in a new tab, so a game in progress isn't lost): how the waves and ammo work, solo and online co-op, the two maps, the eleven guns, the controls and a short FAQ. It gives search engines text to index, which the game page itself barely has.
- The favicon is now real files (`favicon.svg`, `favicon.ico`, `apple-touch-icon.png`), not an inline data URI, so Google can show it in search results.
- The game's structured data adds the player count (1 to 6), screenshots, the maps, an author and a modified date; the About page carries FAQ data.
- `llms.txt` describes the game for AI search; the sitemap lists `/about` and has modified dates.
- Link previews use `og.jpg` (60 KB) in place of `og.png` (600 KB). The loading screen's title is no longer a second `<h1>`.

### Solo is played for loot: a pistol to start, the other guns in weapon crates
- Alone, each life starts with only the Pistol; the primary slot is empty until a gun is found. Online keeps the picked loadout.
- Each wave tops up weapon crates to two, plus one every third wave, up to five, placed like the ammo boxes: reachable open ground at least 10 m away.
- A crate holds any gun but the Pistol, shown turning over it. Standing or walking over one puts the gun in its slot (a Revolver or Desert Eagle replaces the Pistol), loaded with full reserves and raised; running over one, or one holding a gun already carried, leaves it.
- The death screen says the next life starts with the Pistol instead of naming the loadout.

### Analytics on PostHog (`a2a08f1`)
- The page now reports to PostHog, through our own subdomain `t.readyplayerz.com` (PostHog's managed proxy), so retention, funnels and load problems can be seen.
- Tracked: the game loading (how long, the GPU) or failing to, phones and old browsers turned away, games started and ended (wave, kills, time played, fps), each wave reached, rooms made, joined, lost and waited for, invites copied, loadout and settings changes, and uncaught errors.
- PostHog's script is only fetched once the game is on screen, clicks aren't tracked (in play each one is a shot) and session recording is off, so play isn't slowed.
- Nothing is sent from localhost or with `?dev`, `?demo` or `?showcase`.

### Grenades come from the ammo boxes; the boxes thin to two thirds of a wave (`deb3833`)
- A life starts with two grenades and the ammo boxes are now the only way to get more: walking over one adds a grenade, to a cap of four (was five).
- The once-a-minute grenade auto grant, and the HUD's "next grenade in Xs" countdown, are gone.
- Waves spawn two thirds of their zombies' count in ammo boxes, rounded up (wave one: six, was nine) — the extra fire the grenades provide takes the difference.
- A box is left for later only while the reserves and the grenades are both full; full reserves with room for a grenade still takes it, for the grenade.

### Running changes how you resupply; magazine guns drop their loaded mag on a reload (`074b276`)
- Sprinting cancels a reload in progress — nothing is lost, the magazine comes back as it was — and prevents a new reload from starting, including the automatic one on an empty mag.
- Running over an ammo box no longer collects it; stand or walk over one to pick it up.
- Box-magazine guns (Carbine, SMG, AK-47, Uzi, Pistol, Desert Eagle) now swap the magazine on a reload: unfired rounds left in the old one are dropped, and the new one holds only what the reserve has.
- Tube, break-action, bolt-action and cylinder guns (Shotgun, Double barrel, Sniper, Mosin, Revolver) are unaffected: they keep their loaded rounds and top up.

### build.sh fails with a hint when wasm-bindgen is missing (`715067c`)
- The build script now stops with an explicit error and the `cargo install wasm-bindgen-cli` command (matching the version in `Cargo.toml`) instead of a cryptic failure.

### Ammo boxes: finite per-weapon reserves topped up each wave (`f21bee7`)
- Ammo is finite: each weapon has a reserve capped at 2× its magazine, starting full each life; reloads draw from it, and an empty reserve leaves the gun silent.
- Ammo boxes spawn per wave (6 + 3 per wave number) at reachable ground away from the player; standing or walking over one adds a full magazine to every weapon's reserve (capped), without touching the loaded magazine.
- Boxes stay until collected or the wave resets, and clear when you die.
- The HUD ammo readout shows loaded / reserve; the loadout menu lists each gun's reserve cap and the per-box bonus; pickups play a sound.
- Fixed a render bug: the crate's yellow band was depth-occluded inside the crate body (invisible by construction); the band now overhangs the case so it reads at range.
