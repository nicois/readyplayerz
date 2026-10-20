# Changelog

## 2026-10-20

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
