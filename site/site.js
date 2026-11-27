// Runeway site: everything drawn here comes from assets/sprites.json, which
// `cargo run --example export_sprites` writes from src/world/sprites.rs.
// The attract loop replays the `runeway --demo` script (src/demo.rs).

const reduced = matchMedia("(prefers-reduced-motion: reduce)");
const TPS = 30; // animation ticks per second, as in Runeway

// ---------- Menu ----------
{
  const bar = document.querySelector(".bar");
  const burger = document.querySelector(".burger");
  const setOpen = (open) => {
    bar.classList.toggle("open", open);
    burger.setAttribute("aria-expanded", String(open));
    burger.setAttribute("aria-label", open ? "Close menu" : "Open menu");
  };
  burger.addEventListener("click", () => setOpen(!bar.classList.contains("open")));
  document.querySelectorAll(".nav a").forEach((a) => a.addEventListener("click", () => setOpen(false)));
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && bar.classList.contains("open")) { setOpen(false); burger.focus(); }
  });
}

// ---------- Copy button ----------
document.querySelectorAll(".copy").forEach((btn) => {
  const label = btn.textContent;
  btn.addEventListener("click", async () => {
    try {
      await navigator.clipboard.writeText(btn.dataset.copy);
      btn.textContent = "COPIED ✓";
      btn.classList.add("done");
    } catch {
      btn.textContent = "SELECT IT ABOVE";
    }
    setTimeout(() => { btn.textContent = label; btn.classList.remove("done"); }, 2000);
  });
});

// ---------- Sprites ----------
const data = await fetch("assets/sprites.json").then((r) => r.json());
const PAL = data.palette;
const GREYS = ["#06040c", "#1a1626", "#2c263e", "#787f96"];

/** An offscreen canvas holding one sprite, optionally recoloured. */
const cache = new Map();
function sprite(name, map) {
  const key = name + (map ? JSON.stringify(map) : "");
  if (cache.has(key)) return cache.get(key);
  const s = data.sprites[name];
  const c = document.createElement("canvas");
  c.width = s.w; c.height = s.h;
  const g = c.getContext("2d");
  for (let i = 0; i < s.px.length; i++) {
    const idx = parseInt(s.px[i], 16);
    if (!idx) continue;
    g.fillStyle = map ? map(idx) : PAL[idx];
    g.fillRect(i % s.w, Math.floor(i / s.w), 1, 1);
  }
  const out = { c, w: s.w, h: s.h };
  cache.set(key, out);
  return out;
}
const greyMap = (idx) => {
  const hex = PAL[idx];
  const v = parseInt(hex.slice(1, 3), 16) * 0.3 + parseInt(hex.slice(3, 5), 16) * 0.6 + parseInt(hex.slice(5, 7), 16) * 0.1;
  return GREYS[v < 16 ? 0 : v < 50 ? 1 : v < 110 ? 2 : 3];
};
function blit(g, name, x, y, flip = false, map) {
  const s = sprite(name, map);
  if (!flip) { g.drawImage(s.c, Math.round(x), Math.round(y)); return; }
  g.save(); g.translate(Math.round(x) + s.w, Math.round(y)); g.scale(-1, 1); g.drawImage(s.c, 0, 0); g.restore();
}

const CAST = [
  { key: "rune", name: "RUNE-RUNNER", tool: "CLAUDE CODE", run: [1, 2, 3, 2], text: "A hooded runner with a cyan visor slit and a trailing cloak." },
  { key: "brass", name: "BRASS AUTOMATON", tool: "CODEX", run: [1, 2], text: "A round brass walker whose little chimney puffs as it runs." },
  { key: "drone", name: "HOVER-DRONE", tool: "OPENCODE", run: [1, 2], text: "A lens between two spinning rotors, with a pink thruster." },
  { key: "twin", name: "TWIN-SPIRIT", tool: "GEMINI", run: [1, 2, 3, 2], text: "Two linked orbs, violet and cyan, bobbing along together." },
  { key: "cyber", name: "CYBER-RUNNER", tool: "ANY OTHER AGENT", run: [1, 2], text: "A visored runner whose body is tinted by the agent's name." },
];
const POSES = ["run", "jump", "stumble", "sit", "stand", "cheer"];
const frameName = (who, pose, tick) =>
  pose === "run" ? `${who.key}_run_${who.run[Math.floor(tick / 4) % who.run.length]}` : `${who.key}_${pose}`;

// ---------- Attract mode: the four --demo agents ----------
const FILES = ["main.rs", "lib.rs", "lane.rs", "view.rs", "cli.rs", "rng.rs"];
const file = (s) => FILES[Math.floor(s / 2) % FILES.length];
const r = (kind, detail = "") => ({ kind, detail, status: "running" });
const SCRIPTS = [
  (s) => s < 12 ? r("reading", file(s)) : s < 24 ? r("editing", file(Math.floor(s / 3))) : s < 30 ? r("running", "cargo") : s < 33 ? r("passed", "cargo") : s < 45 ? r("thinking") : r("reading", file(s)),
  (s) => s < 10 ? r("thinking") : s < 18 ? r("running", "make") : s < 21 ? r("failed", "make") : s < 27 ? r("editing", file(s)) : s < 33 ? r("running", "make") : s < 36 ? r("passed", "make") : r("searching", file(s)),
  (s) => s < 8 ? r("searching", "TODO") : s < 35 ? { kind: "waiting", detail: "", status: "waiting" } : r("editing", file(s)),
  (s) => s < 18 ? r("editing", file(s)) : s < 22 ? r("helper", "helper") : s < 40 ? r("thinking") : r("finished"),
];
const LANES = [
  { who: CAST[0], title: "glyphsmith", tool: "claude", pace: 5, backdrop: "server_racks" },
  { who: CAST[1], title: "forge", tool: "codex", pace: 3, backdrop: "server_racks" },
  { who: CAST[2], title: "skyline", tool: "opencode", pace: 7, backdrop: "skyline" },
  { who: CAST[3], title: "twins", tool: "gemini", pace: 4, backdrop: "rune_pillars" },
];
const VERB = { reading: "reading", searching: "searching", editing: "editing", running: "running", failed: "failed", passed: "passed", helper: "started a helper", thinking: "thinking", waiting: "waiting for you", finished: "finished" };

const W = 192, LANE_H = 30, BAND = 8, WORLD_H = 22, HOME = 24;
const moving = (st) => ["run", "collect", "smash", "machine", "cheer"].includes(st);

function newLane(cfg, i) {
  return { ...cfg, i, scroll: 0, state: "run", stateTicks: 0, y: 0, vy: 0, x: 0, frame: 0, effects: [], key: "", companion: 0, ticks: 0 };
}
function setState(l, st) {
  l.state = st; l.stateTicks = 0;
  if (st === "smash" && l.y === 0) l.vy = 3;
}
function enter(l, kind) {
  const rx = l.scroll + l.x;
  const fx = (kind_, x, y, vx = 0, vy = 0) => l.effects.push({ kind: kind_, x, y, vx, vy, age: 0 });
  const map = { thinking: "run", reading: "collect", searching: "collect", editing: "smash", running: "machine", failed: "stumble", passed: "cheer", helper: "cheer", waiting: "gate", finished: "finished" };
  setState(l, map[kind] || "run");
  if (kind === "reading" || kind === "searching") fx("coin", rx + 28, 7);
  if (kind === "editing") fx("brick", rx + 5, 12);
  if (kind === "passed" || kind === "helper") for (let n = 0; n < 5; n++) fx("spark", rx + 4, 10, (n % 5) - 2, 2 + (n % 3));
  if (kind === "helper") l.companion = 300;
}
function sync(l, sec) {
  const a = SCRIPTS[l.i](sec % 60);
  const key = a.kind + ":" + a.detail;
  if (key !== l.key) { l.key = key; enter(l, a.kind); }
  l.act = a;
}
function tick(l) {
  l.ticks++; l.stateTicks++;
  if (l.y > 0 || l.vy > 0) { l.y = Math.max(0, l.y + l.vy); l.vy = l.y === 0 ? 0 : l.vy - 1; }
  if (l.state === "stumble") {
    if (l.stateTicks <= 6) l.x--;
    if (l.stateTicks >= 12) setState(l, "run");
  } else if (l.state === "smash" && l.y === 0 && l.vy === 0 && l.stateTicks > 1) setState(l, "run");
  else if (l.state === "cheer" && l.stateTicks >= 20) setState(l, "run");
  if (l.state !== "stumble" && l.x < 0) l.x++;
  if (moving(l.state)) l.scroll++;
  if (l.companion > 0) l.companion--;
  const rx = l.scroll + l.x;
  for (const e of l.effects) {
    e.age++;
    if (e.kind === "brick" && e.age >= 3) Object.assign(e, { kind: "gem", x: e.x + 1, vy: 2, age: 0 });
    else if (e.kind === "gem") { e.y = Math.max(0, e.y + e.vy); e.vy = e.y === 0 ? 0 : e.vy - 1; }
    else if (e.kind === "spark") { e.x += e.vx; e.y += e.vy; e.vy--; }
  }
  l.effects = l.effects.filter((e) => e.kind === "coin" ? e.age < 150 && rx + 8 < e.x : e.kind === "gem" ? e.age < 45 : e.kind === "spark" ? e.age < 18 && e.y > -4 : true);
}
function pose(l) {
  if (l.state === "smash" && l.y > 0) return "jump";
  return { run: "run", collect: "run", smash: "run", machine: "run", stumble: "stumble", cheer: "cheer", finished: "cheer", gate: "stand" }[l.state] || "run";
}
function drawLane(g, l) {
  const top = l.i * LANE_H, oy = top + BAND, ground = oy + WORLD_H - 4;
  // HUD band (the text is HTML on top of it)
  g.fillStyle = PAL[1]; g.fillRect(0, top, W, BAND);
  g.save(); g.beginPath(); g.rect(0, oy, W, WORLD_H); g.clip();
  g.fillStyle = PAL[0]; g.fillRect(0, oy, W, WORLD_H);
  // stars at a quarter speed
  const far = Math.floor(l.scroll / 4);
  for (let n = 0; n < 9; n++) {
    const sx = ((n * 53 + l.i * 31) % 240) - (far % 240);
    const x = ((sx % W) + W) % W, y = oy + ((n * 7 + l.i * 3) % 9);
    g.fillStyle = (Math.floor(l.ticks / 10) + n) % 5 === 0 ? PAL[13] : PAL[3];
    g.fillRect(x, y, 1, 1);
  }
  // backdrop at half speed
  const strip = data.sprites[l.backdrop];
  for (let x = -((l.scroll / 2) % strip.w); x < W; x += strip.w) blit(g, l.backdrop, Math.floor(x), ground - strip.h);
  // hanging sigil blocks and a critter at full speed
  const chunk0 = Math.floor(l.scroll / 64) - 1;
  for (let c = chunk0; c <= chunk0 + 5; c++) {
    const seed = (c * 2654435761 + l.i * 97) >>> 0;
    if (seed % 3 === 0) blit(g, "sigil_block", c * 64 + (seed % 48) - l.scroll, ground - 12 - 8);
    if (seed % 2 === 0) {
      const walk = Math.floor(l.ticks / 2) % 32, pace = walk < 16 ? walk : 31 - walk;
      blit(g, Math.floor(l.ticks / 6) % 2 ? "critter_2" : "critter_1", c * 64 + 8 + (seed % 16) + pace - l.scroll, ground - 5, walk >= 16);
    }
  }
  // ground
  for (let x = -(l.scroll % 8); x < W; x += 8) blit(g, "ground", x, ground);
  // machine section
  if (l.state === "machine") {
    g.fillStyle = PAL[12]; g.fillRect(HOME - 8, ground - 9, 56, 2);
    const turn = Math.floor(l.ticks / 3) % 2 === 0;
    [HOME - 4, HOME + 18, HOME + 38].forEach((gx, k) => {
      const gy = ground - 7;
      g.fillStyle = PAL[11]; g.fillRect(gx, gy, 6, 6);
      g.fillStyle = PAL[15]; g.fillRect(gx + 2, gy + 2, 2, 2);
      if (turn === (k % 2 === 0)) { g.fillRect(gx + 2, gy, 1, 1); g.fillRect(gx + 2, gy + 5, 1, 1); }
      else { g.fillRect(gx, gy + 2, 1, 1); g.fillRect(gx + 5, gy + 2, 1, 1); }
    });
    [HOME + 10, HOME + 30].forEach((vx, k) => {
      const rise = (l.ticks + k * 7) % 12;
      g.fillStyle = PAL[13]; g.fillRect(vx, ground - 11 - rise, 2, 2);
    });
  }
  if (l.state === "gate") blit(g, "gate", HOME + 16, ground - 12);
  if (l.state === "finished") blit(g, "beacon", HOME + 18, ground - 16);
  for (const e of l.effects) {
    const name = { coin: "glyph_coin", brick: "rune_brick", gem: "data_gem", spark: "glyph_coin" }[e.kind];
    const nm = e.kind === "spark" ? "spark" : name;
    const s = data.sprites[nm];
    blit(g, nm, e.x - l.scroll + HOME, ground - e.y - s.h);
  }
  // runner
  const name = frameName(l.who, pose(l), l.ticks);
  const s = data.sprites[name];
  const rxs = HOME + l.x, rtop = ground - l.y - s.h;
  if (l.companion > 0) {
    const bob = Math.floor(l.ticks / 4) % 2;
    g.fillStyle = PAL[6]; g.fillRect(rxs - 6, rtop + 2 + bob, 4, 4);
    g.fillStyle = PAL[13]; g.fillRect(rxs - 5, rtop + 3 + bob, 1, 1);
  }
  blit(g, name, rxs, rtop);
  if (l.state === "gate" && Math.floor(l.ticks / 8) % 2 === 0) blit(g, "glyph_coin", rxs + 3, rtop - 7);
  if (l.state === "stumble" && l.stateTicks < 4) { g.strokeStyle = PAL[14]; g.lineWidth = 1; g.strokeRect(0.5, oy + 0.5, W - 1, WORLD_H - 1); }
  g.restore();
  // a waiting lane gets a gold edge, as in the terminal
  if (l.act && l.act.status === "waiting") { g.fillStyle = PAL[9]; g.fillRect(0, top, 1, LANE_H); g.fillRect(W - 1, top, 1, LANE_H); }
}
function hudLine(l, sec) {
  const s = sec % 60, tokens = 1500 * l.pace + s * 60 * l.pace;
  const ctx = Math.min(1, 0.05 * l.pace + (0.5 * s) / 60), left = Math.round((1 - ctx) * 5);
  const meter = "▰".repeat(left) + "▱".repeat(5 - left);
  const tok = tokens >= 1000 ? (tokens / 1000).toFixed(1) + "k" : String(tokens);
  const a = l.act || { kind: "thinking", detail: "" };
  const act = VERB[a.kind] + (a.detail && !["helper"].includes(a.kind) ? " " + a.detail : "");
  const cls = a.status === "waiting" ? "wait" : "";
  return `${l.title} <b>${l.tool}</b> <span class="tok">◈ ${tok}</span> ${meter} <span class="${cls}">${act}</span>`;
}
{
  const canvas = document.getElementById("attract");
  const hud = document.getElementById("hud");
  const pauseBtn = document.getElementById("pause");
  const g = canvas.getContext("2d");
  const lanes = LANES.map(newLane);
  hud.innerHTML = lanes.map(() => "<div></div>").join("");
  const rows = [...hud.children];
  let ticks = 0, lastSec = -1, paused = false, visible = true, last = 0, acc = 0;

  const step = () => {
    const sec = Math.floor(ticks / TPS);
    lanes.forEach((l) => sync(l, sec));
    lanes.forEach(tick);
    ticks++;
    if (sec !== lastSec) { lastSec = sec; lanes.forEach((l, k) => { rows[k].innerHTML = hudLine(l, sec); }); }
  };
  const draw = () => lanes.forEach((l) => drawLane(g, l));
  // Start 15 s into the script: something is happening in every lane.
  for (let n = 0; n < 15 * TPS; n++) step();
  draw();

  const fitHud = () => { hud.style.setProperty("--band", ""); rows.forEach((row) => { row.style.fontSize = Math.max(11, (canvas.clientHeight / 120) * BAND * 0.92) + "px"; row.style.lineHeight = (canvas.clientHeight / 120) * BAND + "px"; }); };
  new ResizeObserver(fitHud).observe(canvas);

  const loop = (now) => {
    if (paused || !visible || reduced.matches || document.hidden) { last = now; requestAnimationFrame(loop); return; }
    acc += Math.min(250, now - (last || now)); last = now;
    let stepped = false;
    while (acc >= 1000 / TPS) { step(); acc -= 1000 / TPS; stepped = true; }
    if (stepped) draw();
    requestAnimationFrame(loop);
  };
  requestAnimationFrame(loop);
  new IntersectionObserver((es) => { visible = es[0].isIntersecting; }).observe(canvas);

  const showPause = () => {
    if (reduced.matches) { pauseBtn.textContent = "MOTION OFF"; pauseBtn.disabled = true; return; }
    pauseBtn.disabled = false;
    pauseBtn.textContent = paused ? "▶ PLAY" : "❚❚ PAUSE";
    pauseBtn.setAttribute("aria-pressed", String(paused));
  };
  pauseBtn.addEventListener("click", () => { paused = !paused; showPause(); });
  reduced.addEventListener("change", showPause);
  showPause();
}

// ---------- Cast select ----------
{
  const roster = document.getElementById("roster");
  const player = document.getElementById("player");
  const paint = (cv, name, map) => {
    const s = data.sprites[name];
    const g = cv.getContext("2d");
    g.clearRect(0, 0, cv.width, cv.height);
    g.drawImage(sprite(name, map).c, Math.floor((cv.width - s.w) / 2), cv.height - s.h - 1);
  };
  roster.innerHTML = CAST.map((c, i) => `<button class="pick" type="button" aria-pressed="${i === 0}" data-i="${i}"><span class="p1" aria-hidden="true">1P ▼</span><canvas width="14" height="14" aria-hidden="true"></canvas><span>${c.name.split(/[- ]/)[0]}</span><span class="sr">: ${c.name}, ${c.tool}</span></button>`).join("");
  const picks = [...roster.querySelectorAll(".pick")];
  picks.forEach((b, i) => paint(b.querySelector("canvas"), `${CAST[i].key}_stand`));

  let current = 0, timer = 0, t = 0;
  const show = (i) => {
    current = i;
    picks.forEach((b, k) => b.setAttribute("aria-pressed", String(k === i)));
    const c = CAST[i];
    player.innerHTML = `<div class="big"><canvas width="14" height="14" role="img" aria-label="${c.name} running"></canvas></div>
      <div><h3>${c.name}</h3><p class="tool">PLAYS ${c.tool}</p><p>${c.text}</p>
      <div class="poses">${POSES.map((p) => `<figure><canvas width="14" height="14" aria-hidden="true"></canvas><figcaption>${p}</figcaption></figure>`).join("")}</div></div>`;
    const cvs = player.querySelectorAll(".poses canvas");
    POSES.forEach((p, k) => paint(cvs[k], frameName(c, p, 0)));
    const big = player.querySelector(".big canvas");
    clearInterval(timer);
    paint(big, frameName(c, reduced.matches ? "stand" : "run", 0));
    if (!reduced.matches) timer = setInterval(() => { t += 4; paint(big, frameName(c, "run", t)); }, 1000 / 7.5);
  };
  roster.addEventListener("click", (e) => { const b = e.target.closest(".pick"); if (b) show(+b.dataset.i); });
  roster.addEventListener("keydown", (e) => {
    const d = { ArrowRight: 1, ArrowDown: 1, ArrowLeft: -1, ArrowUp: -1 }[e.key];
    if (!d) return;
    e.preventDefault();
    const i = (current + d + CAST.length) % CAST.length;
    show(i); picks[i].focus();
  });
  show(0);
}

// ---------- Move list ----------
{
  const MOVES = [
    ["thinking", "runs; the level scrolls past", (g) => one(g, "rune_run_1")],
    ["reading or searching", "collects a glyph coin per file", (g) => one(g, "glyph_coin")],
    ["editing", "jumps and breaks a rune brick; a data gem pops out", (g) => { blit(g, "rune_brick", 1, 2); blit(g, "data_gem", 8, 8); }],
    ["running a command", "passes turning gears and venting steam", (g) => { g.fillStyle = PAL[11]; g.fillRect(4, 6, 8, 8); g.fillStyle = PAL[15]; g.fillRect(7, 9, 2, 2); g.fillRect(7, 6, 1, 1); g.fillRect(7, 13, 1, 1); g.fillStyle = PAL[13]; g.fillRect(11, 2, 2, 2); g.fillRect(9, 0, 2, 2); }],
    ["failing a command", "flashes red and stumbles back", (g) => { g.strokeStyle = PAL[14]; g.strokeRect(0.5, 0.5, 15, 15); one(g, "rune_stumble"); }],
    ["passing a command", "gets a shower of sparks", (g) => { blit(g, "spark", 2, 2); blit(g, "spark", 8, 8); }],
    ["starting a helper", "gains a small companion", (g) => { one(g, "rune_stand", 3); g.fillStyle = PAL[6]; g.fillRect(0, 6, 4, 4); g.fillStyle = PAL[13]; g.fillRect(1, 7, 1, 1); }],
    ["waiting for you", "stops at a rune gate, a gold glyph pulsing above", (g) => one(g, "gate")],
    ["idle", "sits down", (g) => one(g, "rune_sit")],
    ["finished", "reaches the beacon tower", (g) => one(g, "beacon")],
    ["gone (error or stopped)", "greys out: lost signal", (g) => one(g, "rune_stand", 0, greyMap)],
  ];
  function one(g, name, dx = 0, map) {
    const s = data.sprites[name];
    blit(g, name, Math.floor((16 - s.w) / 2) + dx, 16 - s.h, false, map);
  }
  const list = document.getElementById("movelist");
  list.innerHTML = MOVES.map(([when, does]) => `<li><canvas width="16" height="16" aria-hidden="true"></canvas><span class="when">${when}</span><span class="does">${does}</span></li>`).join("");
  list.querySelectorAll("canvas").forEach((cv, i) => MOVES[i][2](cv.getContext("2d")));
}
