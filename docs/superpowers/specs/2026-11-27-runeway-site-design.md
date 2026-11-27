# Runeway website: design (Attract Mode)

Owner decisions (2026-11-27): direction **A · Attract Mode** (built on the neon house look); fonts **Press Start 2P** (headings), **VT323** (terminal and HUD text), **Orbitron** (neon labels, short), **IBM Plex Mono** (reading text); a totally retro 1980s arcade throwback; real art only; honest content; mobile-first; host on Cloudflare Pages (proposed `runeway-arcade.pages.dev`, free when checked on 2026-11-27; re-check before the first deploy).

## Intent
A project page for Runeway that feels like walking up to an arcade cabinet: the game is already playing, and everything you need to know is presented like the cabinet's own screens. Visitors are developers who run coding agents; the page shows what Runeway draws, what it reads, how to install it, and its limits.

## Goals / non-goals / anti-goals
- Goals: show the real game (sprites, palette, screenshots) first; real install and usage commands; the cast; what each agent activity looks like; controls; data sources and privacy; honest status.
- Non-goals: a web version of Runeway; accounts, analytics, forms.
- Anti-goals: fake scores, stats, users or testimonials; novelty fonts in body text; motion that ignores reduced-motion; a page wider than a phone.

## Composition
1. Header bar: RUNEWAY wordmark, nav (levels), `MASTER DOCS ↗`, GitHub ↗, the theme chooser beside the menu button; drawer on phones.
2. Hero "cabinet": a bezel around a canvas playing the four demo agents with the real sprites (labelled as the `--demo` cast, pausable, a still frame under reduced motion); headline, lede, PRESS START (install) and SEE THE CAST.
3. LEVEL 1 · What it watches: the real world.svg screenshot in a monitor frame, the sources table.
4. LEVEL 2 · Choose your agent: cast select screen, five runners drawn from the sprite data, arrow-key / tap selection showing tool, description and poses.
5. LEVEL 3 · Move list: each agent activity and what the runner does, with the sprite pose or item beside it.
6. LEVEL 4 · Controls: a keyboard overlay card for the keys, and the flags.
7. LEVEL 5 · Rules of play: read-only, offline, labels only, recent past only.
8. INSERT COIN: install and run commands in a terminal window; requirements; what works today and its limits.
9. Contact band (`<cybercore-contact repo="runeway">`), footer.

## System
- Palette: Runeway's own 16 colours (sky #0a0818 … gold #ffcc40), mapped onto the chooser's eleven variables so the chooser recolours the page; the stylesheet palette is the "Site default" (siteLabel RUNEWAY).
- Type: Press Start 2P for h1/h2 only (short, uppercase, ≥ 16px); VT323 for terminals, HUD and kickers (≥ 20px); Orbitron for button and label caps; IBM Plex Mono 16–17px for reading. Square corners, 1px lines, pixel-step shadows.
- Art: sprites exported from `src/world/sprites.rs` by `cargo run --example export_sprites` to `site/assets/sprites.json`; screenshots copied from `docs/screenshots/`.
- Motion: canvas attract loop, a blinking PRESS START, scanlines at low opacity; all off under `prefers-reduced-motion`, and a pause button on the cabinet.
- Mobile: cabinet becomes full-width screen; levels stack; cast select becomes a 5-up strip of tappable runners; tables become stacked rows; text ≥ 11px (body 16px), taps ≥ 44px.
