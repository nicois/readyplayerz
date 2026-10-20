# Changelog

## 2026-10-20

### An About page, real favicons and fuller search data
- New `/about` page, linked from the main menu (it opens in a new tab, so a game in progress isn't lost): how the waves and ammo work, solo and online co-op, the two maps, the eleven guns, the controls and a short FAQ. It gives search engines text to index, which the game page itself barely has.
- The favicon is now real files (`favicon.svg`, `favicon.ico`, `apple-touch-icon.png`), not an inline data URI, so Google can show it in search results.
- The game's structured data adds the player count (1 to 6), screenshots, the maps, an author and a modified date; the About page carries FAQ data.
- `llms.txt` describes the game for AI search; the sitemap lists `/about` and has modified dates.
- Link previews use `og.jpg` (60 KB) in place of `og.png` (600 KB). The loading screen's title is no longer a second `<h1>`.

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
