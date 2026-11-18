"use strict";
const { invoke } = window.__TAURI__.core;
const $ = (s, r = document) => r.querySelector(s);
const $$ = (s, r = document) => [...r.querySelectorAll(s)];

/* ---------- helpers ---------- */
const esc = (v) => String(v ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));
const icon = (n, fill = false) => `<svg class="i${fill ? " on" : ""}"><use href="#i-${n}"/></svg>`;
const pad = (n) => String(n).padStart(2, "0");
const fmt = (sec) => (isFinite(sec) && sec > 0 ? `${Math.floor(sec / 60)}:${pad(Math.floor(sec % 60))}` : "0:00");
const hash = (s) => [...String(s)].reduce((h, c) => (h * 31 + c.charCodeAt(0)) >>> 0, 7);
const phStyle = (t) => { const h = hash(t) % 360; return `--h1:hsl(${h} 55% 52%);--h2:hsl(${(h + 40) % 360} 50% 26%)`; };
const art = (s) => s.cover
  ? `<img src="${esc(s.cover)}" alt="" loading="lazy" data-t="${esc(s.title)}">`
  : `<span class="ph" style="${phStyle(s.title)}"></span>`;
// A cover that fails to load becomes a gradient placeholder.
document.addEventListener("error", (e) => {
  const im = e.target;
  if (im.tagName !== "IMG" || !im.dataset.t) return;
  const ph = document.createElement("span");
  ph.className = "ph";
  ph.style.cssText = phStyle(im.dataset.t);
  im.replaceWith(ph);
}, true);

let toastTimer;
function toast(msg) {
  const t = $("#toast");
  t.textContent = msg;
  t.classList.add("show");
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => t.classList.remove("show"), 3200);
}

/* ---------- store ---------- */
const KEY = "rustmusic:v1";
const defaults = () => ({ liked: [], history: [], plays: {}, days: [], theme: "auto", eq: { on: false, gains: Array(10).fill(0) }, volume: 0.8 });
let S = (() => { try { return Object.assign(defaults(), JSON.parse(localStorage.getItem(KEY)) || {}); } catch { return defaults(); } })();
const save = () => { try { localStorage.setItem(KEY, JSON.stringify(S)); } catch { /* storage off */ } };
const mini = (s) => ({ id: s.id, title: s.title, artist: s.artist, cover: s.cover });
const dayKey = (d) => `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;

function noteHistory(s) {
  if (s.local) return;
  const m = mini(s);
  if (!S.history.length || S.history[0].s.id !== m.id) S.history.unshift({ s: m, at: Date.now() });
  S.history = S.history.slice(0, 300);
  const p = S.plays[m.id] || (S.plays[m.id] = { s: m, n: 0 });
  p.n++;
  const k = dayKey(new Date());
  if (!S.days.includes(k)) S.days.push(k);
  S.days = S.days.slice(-400);
  save();
}
function streak() {
  const set = new Set(S.days);
  const d = new Date();
  const today = set.has(dayKey(d));
  if (!today) d.setDate(d.getDate() - 1);
  let n = 0;
  while (set.has(dayKey(d))) { n++; d.setDate(d.getDate() - 1); }
  return { n, today };
}
const recent = (n) => { const seen = new Set(); const out = []; for (const h of S.history) if (!seen.has(h.s.id)) { seen.add(h.s.id); out.push(h.s); if (out.length >= n) break; } return out; };
const topPlayed = (n) => Object.values(S.plays).sort((a, b) => b.n - a.n).slice(0, n).map((p) => p.s);
function topArtists(n) {
  const by = new Map();
  for (const p of Object.values(S.plays)) {
    const a = by.get(p.s.artist) || { name: p.s.artist, n: 0, best: p.s, bestN: 0 };
    a.n += p.n;
    if (p.n > a.bestN) { a.best = p.s; a.bestN = p.n; }
    by.set(p.s.artist, a);
  }
  return [...by.values()].sort((a, b) => b.n - a.n).slice(0, n);
}
const isLiked = (id) => S.liked.some((s) => s.id === id);
function toggleLike(s) {
  if (!s) return;
  if (s.local) return toast("File dari Perangkat tidak bisa disukai.");
  if (isLiked(s.id)) S.liked = S.liked.filter((x) => x.id !== s.id);
  else S.liked.unshift(mini(s));
  save();
  refreshUi();
  if (["koleksi", "pustaka", "kamu"].includes(route.name)) rerender();
}

/* ---------- song registry ---------- */
const songs = new Map();
const lists = new Map();
let listSeq = 0;
function listKey(arr) {
  arr.forEach((s) => songs.set(s.id, s));
  const k = "l" + ++listSeq;
  lists.set(k, arr);
  if (lists.size > 80) lists.delete(lists.keys().next().value);
  return k;
}
const deviceSongs = [];

/* ---------- player ---------- */
const streamEl = new Audio();
const localEl = new Audio();
let A = streamEl;
const q = { list: [], i: -1, shuffle: false, repeat: "off" };
let loadTok = 0;
let dragging = false;
const cur = () => q.list[q.i];

async function loadCurrent() {
  const s = cur();
  if (!s) return;
  const tok = ++loadTok;
  const el = s.local ? localEl : streamEl;
  if (A !== el) A.pause();
  A = el;
  playingId = "";
  refreshUi();
  try {
    const slow = setTimeout(() => toast("Memuat lagu, tunggu sebentar..."), 3000);
    let url;
    try { url = s.local ? s.url : await invoke("resolve", { videoId: s.id }); } finally { clearTimeout(slow); }
    if (tok !== loadTok) return;
    A.src = url;
    playingId = s.id;
    A.volume = S.volume;
    if (s.local) ensureEq();
    await A.play();
    if (tok !== loadTok) return;
    noteHistory(s);
    refreshUi();
    prefetchNext();
  } catch (e) {
    if (tok === loadTok) { toast("Gagal putar: " + (e && e.message ? e.message : e)); refreshUi(); }
  }
}
// Siapkan lagu berikutnya diam-diam, supaya pindah lagu langsung jalan.
function prefetchNext() {
  if (q.shuffle) return;
  let j = q.i + 1;
  if (j >= q.list.length) { if (q.repeat === "all") j = 0; else return; }
  const n = q.list[j];
  if (n && !n.local && n.id) invoke("prefetch", { videoId: n.id }).catch(() => {});
}
function playQueue(list, i) {
  q.list = list.slice();
  q.i = i;
  q.list.forEach((s) => songs.set(s.id, s));
  loadCurrent();
}
function step(dir) {
  if (!q.list.length) return;
  if (q.shuffle && q.list.length > 1) {
    let j;
    do { j = Math.floor(Math.random() * q.list.length); } while (j === q.i);
    q.i = j;
  } else {
    let j = q.i + dir;
    if (j >= q.list.length) { if (q.repeat === "all") j = 0; else return stopAtEnd(); }
    if (j < 0) j = dir < 0 && A.currentTime > 3 ? q.i : Math.max(0, j);
    q.i = j;
  }
  loadCurrent();
}
function stopAtEnd() { A.pause(); A.currentTime = 0; refreshUi(); }
function toggle() {
  if (!cur()) return toast("Pilih lagu dulu.");
  if (!A.src) return loadCurrent();
  if (A.paused) A.play().catch((e) => toast("Gagal putar: " + e)); else A.pause();
}
[streamEl, localEl].forEach((el) => {
  el.preload = "auto";
  el.addEventListener("timeupdate", () => { if (el === A) updateTime(); });
  el.addEventListener("durationchange", () => { if (el === A) updateTime(); });
  el.addEventListener("play", () => { if (el === A) refreshUi(); });
  el.addEventListener("pause", () => { if (el === A) refreshUi(); });
  el.addEventListener("ended", () => { if (el !== A) return; if (q.repeat === "one") { A.currentTime = 0; A.play(); } else step(1); });
  el.addEventListener("error", () => { if (el === A && el.src) toast("Audio tidak bisa dimuat."); });
});
const fillRange = (el) => el.style.setProperty("--p", ((el.value - el.min) / (el.max - el.min)) * 100 + "%");
const seeks = () => $$("#seek, #fseek");
function updateTime() {
  const ok = isFinite(A.duration) && A.duration > 0;
  $("#f-now").textContent = fmt(A.currentTime);
  $("#f-all").textContent = fmt(A.duration);
  if (!dragging && ok) seeks().forEach((el) => (el.value = Math.round((A.currentTime / A.duration) * 1000)));
  seeks().forEach(fillRange);
  ensureLyrics();
  syncLyrics();
}
seeks().forEach((el) => {
  el.addEventListener("input", (e) => {
    dragging = true;
    seeks().forEach((o) => { o.value = e.target.value; fillRange(o); });
    if (isFinite(A.duration)) $("#f-now").textContent = fmt((e.target.value / 1000) * A.duration);
  });
  el.addEventListener("change", (e) => {
    if (isFinite(A.duration)) A.currentTime = (e.target.value / 1000) * A.duration;
    dragging = false;
  });
});
let muted = false;
function applyVolume() {
  streamEl.volume = localEl.volume = S.volume;
  streamEl.muted = localEl.muted = muted;
  const v = muted ? 0 : Math.round(S.volume * 100);
  $$("#vol, #fvol").forEach((el) => { el.value = v; fillRange(el); });
  $$("#mute, #f-mute").forEach((b) => { b.innerHTML = icon(v === 0 ? "mute" : "volume"); });
}
$$("#vol, #fvol").forEach((el) => el.addEventListener("input", (e) => { S.volume = e.target.value / 100; muted = S.volume === 0; applyVolume(); save(); }));
$$("#mute, #f-mute").forEach((b) => (b.onclick = () => { muted = !muted; applyVolume(); }));
$$("#toggle, #f-toggle").forEach((b) => (b.onclick = toggle));
$$("#next, #f-next").forEach((b) => (b.onclick = () => step(1)));
$$("#prev, #f-prev").forEach((b) => (b.onclick = () => step(-1)));
$$("#shuffle, #f-shuffle").forEach((b) => (b.onclick = () => { q.shuffle = !q.shuffle; refreshUi(); }));
$$("#repeat, #f-repeat").forEach((b) => (b.onclick = () => { q.repeat = { off: "all", all: "one", one: "off" }[q.repeat]; refreshUi(); }));
$$("#bar-like, #f-like").forEach((b) => (b.onclick = () => toggleLike(cur())));

function refreshUi() {
  const s = cur();
  const playing = !!s && !A.paused && !A.ended;
  const pi = `<svg class="i fill"><use href="#i-${playing ? "pause" : "play"}"/></svg>`;
  $("#toggle").innerHTML = pi;
  $("#f-toggle").innerHTML = pi;
  $("#bar-title").textContent = s ? s.title : "Belum ada lagu";
  $("#bar-artist").textContent = s ? s.artist : "Pilih lagu untuk mulai";
  const c = $("#bar-cover"), ph = $("#bar-ph");
  if (s && s.cover) { c.src = s.cover; c.hidden = false; ph.hidden = true; }
  else { c.hidden = true; ph.hidden = false; ph.style.cssText = phStyle(s ? s.title : "x"); }
  const lk = !!s && isLiked(s.id);
  $$("#bar-like, #f-like").forEach((b) => { b.innerHTML = icon("heart", lk); b.classList.toggle("on", lk); });
  $$("#shuffle, #f-shuffle").forEach((b) => b.classList.toggle("on", q.shuffle));
  $$("#repeat, #f-repeat").forEach((r) => {
    r.classList.toggle("on", q.repeat !== "off");
    r.title = { off: "Ulang: mati", all: "Ulang semua", one: "Ulang satu lagu" }[q.repeat];
    r.innerHTML = icon("repeat") + (q.repeat === "one" ? '<b class="one">1</b>' : "");
  });
  $$(".row[data-id]").forEach((row) => row.classList.toggle("cur", !!s && row.dataset.id === s.id));
  if (!$("#panel").hidden) renderPanel();
  if (!$("#full").hidden) renderFull();
  updateBackdrop();
  if ("mediaSession" in navigator && s) {
    navigator.mediaSession.metadata = new MediaMetadata({ title: s.title, artist: s.artist, artwork: s.cover ? [{ src: s.cover }] : [] });
  }
}
if ("mediaSession" in navigator) {
  const ms = navigator.mediaSession;
  ms.setActionHandler("play", toggle);
  ms.setActionHandler("pause", toggle);
  ms.setActionHandler("nexttrack", () => step(1));
  ms.setActionHandler("previoustrack", () => step(-1));
}

/* ---------- equalizer (Web Audio, local files only) ---------- */
const BANDS = [31, 62, 125, 250, 500, 1000, 2000, 4000, 8000, 16000];
const BAND_LABEL = ["31", "62", "125", "250", "500", "1k", "2k", "4k", "8k", "16k"];
const PRESETS = {
  Datar: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
  Bass: [6, 5, 4, 2, 0, 0, 0, 0, 0, 0],
  Vokal: [-2, -2, -1, 1, 3, 3, 2, 0, -1, -2],
  Treble: [0, 0, 0, 0, 0, 1, 3, 4, 5, 6],
  Rock: [5, 4, 2, 0, -1, 0, 2, 3, 4, 4],
};
let eqCtx, eqNodes = [];
function ensureEq() {
  if (!S.eq.on && !eqCtx) return;
  if (!eqCtx) {
    eqCtx = new (window.AudioContext || window.webkitAudioContext)();
    const src = eqCtx.createMediaElementSource(localEl);
    eqNodes = BANDS.map((f, i) => {
      const b = eqCtx.createBiquadFilter();
      b.type = i === 0 ? "lowshelf" : i === 9 ? "highshelf" : "peaking";
      b.frequency.value = f; b.Q.value = 1.1;
      return b;
    });
    [src, ...eqNodes, eqCtx.destination].reduce((a, b) => (a.connect(b), b));
  }
  applyEq();
  if (eqCtx.state === "suspended") eqCtx.resume();
}
function applyEq() { eqNodes.forEach((n, i) => (n.gain.value = S.eq.on ? S.eq.gains[i] : 0)); }
function eqPath() {
  const pts = S.eq.gains.map((g, i) => [30 + i * (640 / 9), 70 - g * 5.2]);
  return pts.map((p, i) => (i ? "L" : "M") + p[0].toFixed(1) + " " + p[1].toFixed(1)).join(" ");
}

/* ---------- views ---------- */
const MOODS = ["Feel good", "Relax", "Sleep", "Sad", "Romance", "Party", "Fokus", "Olahraga", "Perjalanan"];
const search = { query: "", draft: undefined, results: [], tab: "yt", loading: false, error: "" };
let libTab = "Semua", libMode = "grid", libQ = "";
const K = { q: "", sort: "default", which: "", key: "" };
const SORTS = ["default", "rev", "title", "artist"];
const SORT_LABEL = { default: "Urutan asli", rev: "Urutan terbalik", title: "Judul A-Z", artist: "Artis A-Z" };

// Koleksi bawaan. Sampul dibuat dari gradasi dan satu ikon, seperti Liked Songs di Liner.
const COLLS = {
  liked: { t: "Disukai", sub: "Lagu yang kamu sukai", ic: "heart", g: ["#7a4dff", "#1a1040"], list: () => S.liked, empty: "Tekan ikon hati pada lagu untuk menyimpannya di sini." },
  top: { t: "Teratas saya 50", sub: "Paling sering kamu putar", ic: "trend", g: ["#ff7a3d", "#3a1608"], list: () => topPlayed(50), empty: "Putar beberapa lagu dulu." },
  device: { t: "Di perangkat", sub: "File dari komputer", ic: "phone", g: ["#2ec4b6", "#062b2a"], list: () => deviceSongs, empty: "Belum ada file. Pilih file musik dari komputer." },
};
const genCover = (g, ic) => `<span class="gen" style="--g1:${g[0]};--g2:${g[1]}">${icon(ic, ic === "heart")}</span>`;
const sec = (ic, t) => `<div class="sec">${icon(ic)}<span>${esc(t)}</span></div>`;

function renderSideLib() {
  const ks = ["liked", "top", ...(deviceSongs.length ? ["device"] : [])];
  $("#side-lib").innerHTML = ks.map((k) => `<button class="stile" data-act="go" data-to="koleksi" data-arg="${k}" data-root="1" title="${esc(COLLS[k].t)}" aria-label="${esc(COLLS[k].t)}">${genCover(COLLS[k].g, COLLS[k].ic)}</button>`).join("");
  $("#side-sep").hidden = !ks.length;
}

function row(s, o) {
  const liked = isLiked(s.id);
  return `<div class="row${cur() && cur().id === s.id ? " cur" : ""}" data-act="play" data-list="${o.key}" data-i="${o.i}" data-id="${esc(s.id)}" data-name="${esc((s.title + " " + s.artist).toLowerCase())}" tabindex="0">
    ${o.n ? `<span class="n">${o.n}</span>` : ""}${art(s)}
    <div class="t"><b>${esc(s.title)}</b><small>${esc(s.artist)}</small></div>
    <button class="ibtn x${liked ? " on" : ""}" data-act="like" data-id="${esc(s.id)}" title="Suka">${icon("heart", liked)}</button>
    <button class="ibtn x" data-act="menu" data-id="${esc(s.id)}" title="Lainnya">${icon("more")}</button></div>`;
}
const rowsInner = (arr, key, numbered = false) => arr.map((s, i) => row(s, { key, i, n: numbered ? i + 1 : 0 })).join("");
const rows = (arr, numbered = false) => { const key = listKey(arr); return `<div class="list">${rowsInner(arr, key, numbered)}</div>`; };
function rail(arr) {
  const key = listKey(arr);
  return `<div class="rail">${arr.map((s, i) => `<button class="tile" data-act="play" data-list="${key}" data-i="${i}" data-id="${esc(s.id)}">${art(s)}<b>${esc(s.title)}</b><small>${esc(s.artist)}</small></button>`).join("")}</div>`;
}
const streakCard = () => {
  const { n, today } = streak();
  return `<div class="card streak${today ? " on" : ""}">${icon("flame")}<div><b>${n}</b>hari berturut-turut<br><small>${today ? "Apimu menyala! Putar lagu lagi besok agar tetap menyala." : "Apimu sedang istirahat. Putar satu lagu hari ini agar menyala lagi."}</small></div></div>`;
};
const moodChips = () => `<div class="chips">${MOODS.map((m) => `<button class="chip" data-act="search" data-q="${esc(m)} music">${esc(m)}</button>`).join("")}</div>`;
const item = (ic, title, sub, attrs, off) => `<button class="item${off ? " off" : ""}" ${attrs}>${icon(ic)}<span class="grow">${esc(title)}${sub ? `<small>${esc(sub)}</small>` : ""}</span>${off ? '<small class="muted">Belum tersedia</small>' : icon("chevron")}</button>`;

// Kartu pustaka: kotak (grid) atau daftar, satu bentuk data untuk keduanya.
function libItems(items, round) {
  if (libMode === "list") {
    return `<div class="lrows">${items.map((it) => `<button class="lrow" data-name="${esc(it.name.toLowerCase())}" ${it.attrs}><span class="cv">${it.cover}</span><span class="t"><b>${esc(it.name)}</b><small>${esc(it.sub)}</small></span></button>`).join("")}</div>`;
  }
  return `<div class="grid">${items.map((it) => `<button class="lcard${round ? " round" : ""}" data-name="${esc(it.name.toLowerCase())}" ${it.attrs}><span class="cv">${it.cover}</span><span class="nm">${esc(it.name)}</span><span class="sb">${esc(it.sub)}</span></button>`).join("")}</div>`;
}

function viewList(all) {
  let a = all.slice();
  const f = K.q.trim().toLowerCase();
  if (f) a = a.filter((x) => (x.title + " " + x.artist).toLowerCase().includes(f));
  if (K.sort === "rev") a.reverse();
  else if (K.sort === "title") a.sort((x, y) => x.title.localeCompare(y.title));
  else if (K.sort === "artist") a.sort((x, y) => x.artist.localeCompare(y.artist) || x.title.localeCompare(y.title));
  return a;
}

const views = {
  beranda() {
    const h = new Date().getHours();
    const hi = h < 11 ? "Selamat pagi" : h < 15 ? "Selamat siang" : h < 18 ? "Selamat sore" : "Selamat malam";
    const rec = recent(12), top = topPlayed(12);
    let html = `<h1>${hi}</h1>`;
    if (rec.length) {
      const first = rec.slice(0, 6), key = listKey(rec);
      html += `<div class="picks">${first.map((s, i) => `<button class="pick" data-act="play" data-list="${key}" data-i="${i}" data-id="${esc(s.id)}">${art(s)}<span class="t">${esc(s.title)}</span></button>`).join("")}</div>`;
    } else {
      html += `<div class="card note">Belum ada riwayat. Buka Cari di kiri, putar lagu, dan lagu yang kamu putar muncul di sini.</div>`;
    }
    html += streakCard() + moodChips();
    if (rec.length) html += sec("history", "Lanjutkan") + rail(rec);
    if (top.length) html += sec("trend", "Andalanmu") + rail(top);
    return html;
  },

  cari() {
    const draft = search.draft !== undefined ? search.draft : search.query;
    let html = `<label class="field big">${icon("search")}<input id="q" type="search" placeholder="Cari lagu, album, artis..." autocomplete="off" spellcheck="false" value="${esc(draft)}"></label>`;
    html += `<div class="tabs" style="margin:16px 0 6px"><button class="${search.tab === "yt" ? "on" : ""}" data-act="tab" data-tab="yt">YouTube</button><button class="${search.tab === "dev" ? "on" : ""}" data-act="tab" data-tab="dev">Perangkat</button></div>`;
    if (search.tab === "dev") {
      return html + `<div class="acts"><button class="btn" data-act="pick-files">${icon("plus")}Pilih file musik</button></div>` +
        (deviceSongs.length ? rows(deviceSongs) : `<p class="note">Pilih file audio dari komputer. Daftar ini hilang saat app ditutup. File dari Perangkat bisa diputar dengan ekualiser.</p>`);
    }
    if (search.query) {
      html += `<h2>Hasil untuk “${esc(search.query)}”</h2>`;
      if (search.loading) return html + `<p class="note">Mencari...</p>`;
      if (search.error) return html + `<p class="note">Gagal mencari: ${esc(search.error)}</p>`;
      return html + (search.results.length ? rows(search.results) : `<p class="note">Tidak ada hasil.</p>`);
    }
    const rec = recent(10), top = topPlayed(6), arts = topArtists(10);
    if (!rec.length) html += `<p class="note">Ketik nama lagu di kolom cari lalu tekan Enter.</p>`;
    if (rec.length) html += sec("history", "Terakhir diputar") + rail(rec);
    if (top.length) html += sec("chart", "Paling sering diputar") + rows(top, true);
    if (arts.length) html += sec("users", "Artis favorit") + `<div class="rail">${arts.map((a) => `<button class="tile artist" data-act="search" data-q="${esc(a.name)}">${art(a.best)}<b>${esc(a.name)}</b></button>`).join("")}</div>`;
    return html + sec("search", "Jelajahi semua") + moodChips();
  },

  pustaka() {
    const tabs = ["Semua", "Lagu", "Artis"];
    const head = `<h1>Pustaka</h1><div class="toolbar"><div class="tabs">${tabs.map((t) => `<button class="${libTab === t ? "on" : ""}" data-act="libtab" data-tab="${t}">${t}</button>`).join("")}</div>
      <div class="right"><label class="field">${icon("search")}<input id="lq" placeholder="Cari di pustaka" autocomplete="off" spellcheck="false" value="${esc(libQ)}"></label>
      <button class="glassbtn${libMode === "grid" ? " on" : ""}" data-act="libmode" data-mode="grid" title="Tampilan kotak">${icon("grid", true)}</button>
      <button class="glassbtn${libMode === "list" ? " on" : ""}" data-act="libmode" data-mode="list" title="Tampilan daftar">${icon("list")}</button></div></div>`;
    if (libTab === "Lagu") {
      return head + (S.liked.length ? rows(S.liked) : `<p class="note">Belum ada lagu yang disukai.</p>`);
    }
    if (libTab === "Artis") {
      const a = topArtists(60);
      if (!a.length) return head + `<p class="note">Artis muncul setelah kamu memutar lagu.</p>`;
      return head + libItems(a.map((x) => ({ name: x.name, sub: `${x.n}x diputar`, cover: art(x.best), attrs: `data-act="search" data-q="${esc(x.name)}"` })), true);
    }
    const items = ["liked", "top", "device"].map((k) => ({
      name: COLLS[k].t, sub: `${COLLS[k].list().length} lagu`, cover: genCover(COLLS[k].g, COLLS[k].ic), attrs: `data-act="go" data-to="koleksi" data-arg="${k}"`,
    }));
    items.push({ name: "Riwayat", sub: `${S.history.length} lagu`, cover: genCover(["#5a6372", "#14161a"], "history"), attrs: `data-act="go" data-to="riwayat"` });
    return head + libItems(items, false);
  },

  koleksi(which) {
    const meta = COLLS[which];
    const all = meta.list();
    const arr = viewList(all);
    const key = listKey(arr);
    K.which = which; K.key = key;
    heroCover = all[0] && all[0].cover ? all[0].cover : "";
    const sortTip = `Urut: ${SORT_LABEL[K.sort]} (klik untuk ganti)`;
    return `<div class="hero"><div class="art gen" style="--g1:${meta.g[0]};--g2:${meta.g[1]}">${icon(meta.ic, meta.ic === "heart")}</div>
      <div class="body"><h1>${meta.t}</h1><div class="sub">${meta.sub}</div>
      <div class="metas"><span class="chip meta">${icon("queue")}${all.length} lagu</span></div>
      <div class="bar"><button class="btn" data-act="playall" data-list="${key}">${icon("play", true)}Putar semua</button>
        <button class="btn sq" data-act="shuffleall" data-list="${key}" title="Acak">${icon("shuffle")}</button>
        <div class="right"><label class="field">${icon("search")}<input id="kq" placeholder="Cari di daftar" autocomplete="off" spellcheck="false" value="${esc(K.q)}"></label>
          <button class="btn soft" data-act="queueall" data-list="${key}">${icon("plus")}Tambah ke antrean</button>
          ${which === "device" ? `<button class="btn soft" data-act="pick-files">${icon("plus")}Tambah file</button>` : ""}
          <button class="btn sq" data-act="ksort" title="${sortTip}">${icon("sort")}</button></div></div></div></div>
      <div class="list" id="klist">${rowsInner(arr, key)}</div>
      <p class="note" id="knote">${all.length ? (arr.length ? "" : "Tidak ada yang cocok.") : meta.empty}</p>`;
  },

  kamu() {
    return `<h1>Kamu</h1>` + streakCard() +
      `<div class="card group">${item("history", "Riwayat", "", `data-act="go" data-to="riwayat"`)}${item("chart", "Statistik dan recap", "", `data-act="go" data-to="statistik"`)}</div>
      <div class="card group">${item("users", "Dengar bersama", "", `data-act="go" data-to="bersama"`)}${item("mic", "Kenali lagu", "", `data-act="go" data-to="kenali"`)}${item("eq", "Ekualiser", "", `data-act="go" data-to="ekualiser"`)}</div>
      <div class="card group">${item("login", "Masuk ke YouTube Music", "Sinkronkan lagu disukai, pustaka, dan playlist", "", true)}</div>
      <div class="card group">${item("sliders", "Pengaturan", "", `data-act="go" data-to="pengaturan"`)}${item("info", "Tentang", "", `data-act="go" data-to="tentang"`)}</div>`;
  },

  riwayat() {
    const arr = S.history.map((h) => h.s);
    return `<h1>Riwayat</h1>` + (arr.length ? `<div class="acts"><button class="btn soft" data-act="clear-history">${icon("trash")}Hapus riwayat</button></div>` + rows(arr) : `<p class="note">Belum ada riwayat.</p>`);
  },

  statistik() {
    const total = S.history.length, uniq = Object.keys(S.plays).length, { n } = streak();
    const arts = topArtists(5), top = topPlayed(5);
    const stat = (v, l) => `<div class="card" style="padding:18px 22px;flex:1;min-width:150px"><div style="font-size:30px;font-weight:700">${v}</div><span class="muted">${l}</span></div>`;
    return `<h1>Statistik dan recap</h1><div style="display:flex;gap:12px;flex-wrap:wrap">${stat(total, "lagu diputar")}${stat(uniq, "lagu berbeda")}${stat(n, "hari berturut-turut")}</div>` +
      (arts.length ? `<h2>Artis teratas</h2><div class="card group">${arts.map((a) => `<div class="item"><span class="grow">${esc(a.name)}</span><span class="muted">${a.n}x</span></div>`).join("")}</div><h2>Lagu teratas</h2>${rows(top, true)}` : `<p class="note">Statistik muncul setelah kamu memutar lagu.</p>`);
  },

  ekualiser() {
    const g = S.eq.gains;
    return `<h1>Ekualiser</h1><div class="card eq">
      <svg viewBox="0 0 700 140" width="100%" style="display:block;background:var(--chip);border-radius:14px">
        <g stroke="var(--line)" stroke-width="1"><path d="M0 70H700M0 18H700M0 122H700"/><path d="M${30 + 2 * (640 / 9)} 0V140M${30 + 5 * (640 / 9)} 0V140M${30 + 8 * (640 / 9)} 0V140"/></g>
        <path id="eq-line" d="${eqPath()}" fill="none" stroke="var(--accent)" stroke-width="3" stroke-linecap="round" stroke-linejoin="round"/></svg>
      <label class="radio"><input type="checkbox" id="eq-on" ${S.eq.on ? "checked" : ""}> Aktifkan ekualiser</label>
      <div class="bands">${g.map((v, i) => `<div class="band"><b id="eqv${i}">${v > 0 ? "+" : ""}${v}</b><span class="sl"><input type="range" min="-12" max="12" step="1" value="${v}" data-band="${i}" aria-label="${BAND_LABEL[i]} Hz"></span><span>${BAND_LABEL[i]}</span></div>`).join("")}</div>
      <div class="presets">${Object.keys(PRESETS).map((p) => `<button class="chip" data-act="preset" data-p="${p}">${p}</button>`).join("")}</div>
      <p class="note" style="padding:14px 0 0">Ekualiser bekerja untuk file dari Perangkat. Lagu YouTube diputar langsung oleh WebView dan belum bisa difilter, sampai audio dialihkan ke mesin Rust.</p></div>`;
  },

  bersama() {
    return `<h1>Dengar bersama</h1><div class="card" style="padding:26px;max-width:640px;text-align:center">
      <div style="display:inline-grid;place-items:center;width:88px;height:88px;border-radius:50%;background:var(--soft);color:var(--accent)"><svg class="i" style="width:40px;height:40px"><use href="#i-users"/></svg></div>
      <h2 style="margin:12px 0 4px">Dengar bersama</h2><p class="muted">Putar musik bersama teman secara real time. Buat sesi untuk jadi host, atau gabung dengan kode.</p>
      <p class="note">Belum tersedia di versi Windows.</p>
      <div style="display:grid;gap:10px;opacity:.5;pointer-events:none"><label class="field" style="width:100%"><input placeholder="Nama pengguna" disabled></label><label class="field" style="width:100%"><input placeholder="Kode sesi" disabled></label><span class="btn">Hubungkan</span></div></div>`;
  },

  kenali() {
    return `<h1>Kenali musik</h1><div style="text-align:center;padding:60px 0"><span class="play" style="width:110px;height:110px;margin:0 auto;opacity:.5"><svg class="i" style="width:44px;height:44px"><use href="#i-mic"/></svg></span><p style="margin-top:20px;font-weight:600">Ketuk untuk mengenali</p><p class="note">Belum tersedia di versi Windows.</p></div>`;
  },

  pengaturan() {
    const th = (v, l) => `<button class="${S.theme === v ? "on" : ""}" data-act="theme" data-v="${v}">${l}</button>`;
    return `<h1>Pengaturan</h1>
      <div class="card group"><div class="item">${icon("palette")}<span class="grow">Tampilan<small>Terang, gelap, atau ikut sistem</small></span><div class="seg" style="margin:0">${th("auto", "Sistem")}${th("light", "Terang")}${th("dark", "Gelap")}</div></div>
      ${item("eq", "Ekualiser", "", `data-act="go" data-to="ekualiser"`)}</div>
      <div class="card group">${item("trash", "Hapus riwayat", "Riwayat, statistik, dan apimu", `data-act="clear-history"`)}${item("trash", "Hapus semua data lokal", "Riwayat, lagu disukai, pengaturan", `data-act="clear-all"`)}</div>
      <div class="card group">${item("info", "Tentang", "", `data-act="go" data-to="tentang"`)}</div>`;
  },

  tentang() {
    return `<h1>Tentang</h1><div class="card" style="padding:22px;max-width:640px;line-height:1.6"><b style="font-size:20px">RustMusic 0.1.0</b><p class="muted">Pemutar musik Windows untuk YouTube Music. Gratis, open source (GPL-3.0). Fork dari OpenTune.</p>
      <p>Mesin FLAC lossless ditulis dengan Rust dan memutar pada sample rate serta bit depth asli file.</p></div>`;
  },
};

/* ---------- backdrop: cover art colours the page (Liner style) ---------- */
let heroCover = "";
function updateBackdrop() {
  const s = cur();
  const url = heroCover || (s && s.cover) || "";
  const b = $("#backdrop");
  if (b.dataset.url !== url) {
    b.dataset.url = url;
    if (url) b.style.setProperty("--bd", `url("${url.replace(/["\\]/g, "")}")`);
  }
  b.classList.toggle("on", !!url);
}

/* ---------- router ---------- */
const tabOf = { beranda: "beranda", cari: "cari", pustaka: "pustaka", koleksi: "pustaka" };
let stack = [{ name: "beranda" }];
let route = stack[0];
function go(name, arg, root = false) {
  if (name === "koleksi") { K.q = ""; K.sort = "default"; }
  if (root || tabOf[name] === name) stack = [{ name, arg }];
  else stack.push({ name, arg });
  render();
}
function back() { if (stack.length > 1) { stack.pop(); render(); } }
function afterRender() {
  updateBackdrop();
  const base = tabOf[route.name] || "kamu";
  $$("#side [data-go]").forEach((b) => b.classList.toggle("on", b.dataset.go === base));
  $$(".stile").forEach((b) => b.classList.toggle("on", route.name === "koleksi" && b.dataset.arg === route.arg));
  $("#back").hidden = stack.length < 2;
  $("#view").classList.toggle("has-back", stack.length > 1);
  if (route.name === "pustaka") filterLib();
}
function filterLib() {
  const f = libQ.trim().toLowerCase();
  $$("#view [data-name]").forEach((el) => { el.hidden = !!f && !el.dataset.name.includes(f); });
}
function render() {
  route = stack[stack.length - 1];
  const v = $("#view");
  heroCover = "";
  v.innerHTML = views[route.name](route.arg);
  v.scrollTop = 0;
  v.style.animation = "none"; void v.offsetWidth; v.style.animation = ""; // replay the page fade
  afterRender();
  if (route.name === "cari" && !search.query) { const qi = $("#q"); if (qi) qi.focus(); }
}
function rerender() {
  const v = $("#view"), top = v.scrollTop, ae = document.activeElement;
  const keep = ae && ae.id && v.contains(ae) ? { id: ae.id, a: ae.selectionStart, b: ae.selectionEnd } : null;
  heroCover = "";
  v.innerHTML = views[route.name](route.arg);
  v.scrollTop = top;
  if (keep) {
    const el = document.getElementById(keep.id);
    if (el) { el.focus(); try { el.setSelectionRange(keep.a, keep.b); } catch { /* not a text field */ } }
  }
  afterRender();
}

async function doSearch(query) {
  search.query = query; search.draft = query; search.tab = "yt"; search.loading = true; search.error = ""; search.results = [];
  go("cari", null, true);
  const mine = (doSearch.n = (doSearch.n || 0) + 1);
  try {
    const r = await invoke("search", { query });
    if (mine !== doSearch.n) return;
    search.results = r;
  } catch (e) {
    if (mine !== doSearch.n) return;
    search.error = String(e && e.message ? e.message : e);
  }
  search.loading = false;
  if (route.name === "cari") rerender();
}

/* ---------- queue panel and fullscreen player ---------- */
function queueKey() { lists.set("queue", q.list); q.list.forEach((x) => songs.set(x.id, x)); return "queue"; }
function renderPanel() {
  const s = cur();
  const body = $("#panel-body");
  if (!s) { body.innerHTML = `<p class="note">Belum ada lagu diputar.</p>`; return; }
  const key = queueKey();
  const next = q.list.slice(q.i + 1);
  body.innerHTML = `<h4>Sedang diputar</h4><div class="list">${row(s, { key, i: q.i, n: 0 })}</div>` +
    `<h4>Berikutnya (${next.length})</h4>` +
    (next.length ? `<div class="list">${next.map((x, k) => row(x, { key, i: q.i + 1 + k, n: 0 })).join("")}</div>` : `<p class="note">Tidak ada lagu berikutnya.</p>`);
}
function openPanel(on) { $("#panel").hidden = !on; if (on) renderPanel(); }
let fullId = "";
function renderFull() {
  const s = cur();
  $("#f-title").textContent = s ? s.title : "Belum ada lagu";
  $("#f-artist").textContent = s ? s.artist : "";
  $("#f-cover").innerHTML = s ? art(s) : `<span class="ph" style="${phStyle("x")}"></span>`;
  if (s && lyrShown !== s.id) { lyrShown = s.id; applyLyrics({ state: "load", lines: [], synced: false }); }
  $("#fbg").style.setProperty("--fb", s && s.cover ? `url("${s.cover.replace(/["\\]/g, "")}")` : "none");
  const key = queueKey();
  $("#f-queue").innerHTML = q.list.map((x, i) => `<button class="ql${i === q.i ? " cur" : ""}" data-act="play" data-list="${key}" data-i="${i}" data-id="${esc(x.id)}"><b>${esc(x.title)}</b><small>${esc(x.artist)}</small></button>`).join("");
  if (s && fullId !== s.id) {
    fullId = s.id;
    const c = $("#f-queue .cur");
    if (c) c.scrollIntoView({ block: "center", behavior: "smooth" });
  }
}
function openFull(on) {
  $("#full").hidden = !on;
  if (on) { openPanel(false); fullId = ""; lyrFor = ""; lyrShown = ""; renderFull(); ensureLyrics(); }
}
$("#open-full").onclick = () => openFull(true);
$("#close-full").onclick = () => openFull(false);

/* ---------- lirik (LRCLIB) di layar penuh ---------- */
const lyr = { state: "load", lines: [], synced: false, idx: -1 };
const lyrCache = new Map();
let lyrFor = "", lyrShown = "", playingId = "", fullTab = "lirik";
function parseLrc(txt) {
  const out = [];
  for (const raw of txt.split(/\r?\n/)) {
    const re = /\[(\d+):(\d+(?:\.\d+)?)\]/g;
    const ts = [];
    let m, last = 0;
    while ((m = re.exec(raw))) { ts.push(+m[1] * 60 + parseFloat(m[2])); last = re.lastIndex; }
    if (!ts.length) continue;
    const text = raw.slice(last).trim();
    ts.forEach((t) => out.push({ t, text }));
  }
  return out.sort((a, b) => a.t - b.t);
}
function renderLyrics() {
  const el = $("#f-lyrics");
  if (lyr.state === "load") el.innerHTML = '<p class="lmsg">Memuat lirik...</p>';
  else if (lyr.state === "none") el.innerHTML = '<p class="lmsg">Lirik tidak ditemukan.</p>';
  else el.innerHTML = lyr.lines.map((l, i) => `<p class="ly${lyr.synced ? "" : " plain"}" data-i="${i}"${lyr.synced ? ` data-t="${l.t}"` : ""}>${l.text ? esc(l.text) : lyr.synced ? "♪" : ""}</p>`).join("");
  lyr.idx = -1;
  if (lyr.state === "ok" && !lyr.synced) el.scrollTop = 0;
  syncLyrics(true);
}
function applyLyrics(r) { Object.assign(lyr, r); renderLyrics(); }
function syncLyrics(force) {
  if (!lyr.synced || lyr.state !== "ok" || $("#full").hidden || fullTab !== "lirik") return;
  const t = A.currentTime + 0.25;
  let lo = 0, hi = lyr.lines.length - 1, i = -1;
  while (lo <= hi) { const mid = (lo + hi) >> 1; if (lyr.lines[mid].t <= t) { i = mid; lo = mid + 1; } else hi = mid - 1; }
  if (i === lyr.idx && !force) return;
  lyr.idx = i;
  const ps = $$("#f-lyrics .ly");
  ps.forEach((p, k) => p.classList.toggle("cur", k === i));
  const c = ps[i] || ps[0];
  if (c) c.scrollIntoView({ block: "center", behavior: force ? "auto" : "smooth" });
}
function ensureLyrics() {
  if ($("#full").hidden) return;
  const s = cur();
  if (!s || playingId !== s.id || lyrFor === s.id) return;
  if (!isFinite(A.duration) || A.duration <= 0) return;
  lyrFor = s.id;
  const id = s.id;
  const hit = lyrCache.get(id);
  if (hit) return applyLyrics(hit);
  applyLyrics({ state: "load", lines: [], synced: false });
  invoke("lyrics", { title: s.title, artist: s.artist, duration: A.duration })
    .then((r) => {
      let res = null;
      if (r && r.synced) { const lines = parseLrc(r.synced); if (lines.length) res = { state: "ok", lines, synced: true }; }
      if (!res && r && r.plain) res = { state: "ok", synced: false, lines: r.plain.split(/\r?\n/).map((text) => ({ t: 0, text: text.trim() })) };
      res = res || { state: "none", lines: [], synced: false };
      lyrCache.set(id, res);
      if (cur() && cur().id === id) applyLyrics(res);
    })
    .catch(() => { if (cur() && cur().id === id) applyLyrics({ state: "none", lines: [], synced: false }); });
}
function setFullTab(t) {
  fullTab = t;
  $("#f-lyrics").hidden = t !== "lirik";
  $("#f-queue").hidden = t !== "antrean";
  $("#tab-lirik").classList.toggle("on", t === "lirik");
  $("#tab-antrean").classList.toggle("on", t === "antrean");
  if (t === "lirik") syncLyrics(true);
  else { const c = $("#f-queue .cur"); if (c) c.scrollIntoView({ block: "center", behavior: "auto" }); }
}
$("#tab-lirik").onclick = () => setFullTab("lirik");
$("#tab-antrean").onclick = () => setFullTab("antrean");
$("#f-lyrics").addEventListener("click", (e) => {
  const p = e.target.closest(".ly[data-t]");
  if (!p) return;
  A.currentTime = parseFloat(p.dataset.t);
  syncLyrics(true);
});
$("#open-queue").onclick = () => openPanel($("#panel").hidden);
$("#close-panel").onclick = () => openPanel(false);

/* ---------- menus ---------- */
function closeMenu() { $$(".menu").forEach((m) => m.remove()); }
function openMenu(btn, s) {
  closeMenu();
  const m = document.createElement("div");
  m.className = "menu";
  m.innerHTML = `<button data-m="next">${icon("play")}Putar berikutnya</button><button data-m="queue">${icon("queue")}Tambah ke antrean</button><button data-m="like">${icon("heart", isLiked(s.id))}${isLiked(s.id) ? "Batal suka" : "Suka"}</button>${s.local ? "" : `<button data-m="artist">${icon("search")}Cari artis</button>`}`;
  document.body.append(m);
  const r = btn.getBoundingClientRect();
  m.style.left = Math.max(8, Math.min(r.right - m.offsetWidth, innerWidth - m.offsetWidth - 8)) + "px";
  m.style.top = Math.min(r.bottom + 4, innerHeight - m.offsetHeight - 100) + "px";
  m.addEventListener("click", (e) => {
    const b = e.target.closest("[data-m]");
    if (!b) return;
    closeMenu();
    if (b.dataset.m === "like") toggleLike(s);
    else if (b.dataset.m === "artist") doSearch(s.artist);
    else {
      if (!q.list.length) { playQueue([s], 0); return; }
      q.list.splice(b.dataset.m === "next" ? q.i + 1 : q.list.length, 0, s);
      toast(b.dataset.m === "next" ? "Diputar berikutnya" : "Ditambahkan ke antrean");
      if (!$("#panel").hidden) renderPanel();
    }
  });
}

/* ---------- events ---------- */
let confirming = "";
document.addEventListener("click", (e) => {
  if (!e.target.closest(".menu") && !e.target.closest('[data-act="menu"]')) closeMenu();
  const nav = e.target.closest("[data-go]");
  if (nav) return go(nav.dataset.go, null, true);
  const el = e.target.closest("[data-act]");
  if (!el) return;
  const d = el.dataset;
  switch (d.act) {
    case "play": { const l = lists.get(d.list); if (l) playQueue(l, +d.i); break; }
    case "like": e.stopPropagation(); toggleLike(songs.get(d.id)); break;
    case "menu": e.stopPropagation(); { const s = songs.get(d.id); if (s) openMenu(el, s); } break;
    case "go": go(d.to, d.arg, !!d.root); break;
    case "search": doSearch(d.q); break;
    case "tab": search.tab = d.tab; rerender(); break;
    case "libtab": libTab = d.tab; rerender(); break;
    case "playall": { const l = lists.get(d.list); if (l && l.length) { q.shuffle = false; playQueue(l, 0); } break; }
    case "shuffleall": { const l = lists.get(d.list); if (l && l.length) { q.shuffle = true; playQueue(l, Math.floor(Math.random() * l.length)); } break; }
    case "ksort": K.sort = SORTS[(SORTS.indexOf(K.sort) + 1) % SORTS.length]; toast("Urut: " + SORT_LABEL[K.sort]); rerender(); break;
    case "libmode": libMode = d.mode; rerender(); break;
    case "queueall": {
      const l = lists.get(d.list);
      if (!l || !l.length) break;
      if (!q.list.length) { playQueue(l, 0); break; }
      q.list.push(...l);
      toast("Ditambahkan ke antrean");
      if (!$("#panel").hidden) renderPanel();
      break;
    }
    case "pick-files": $("#files").click(); break;
    case "theme": S.theme = d.v; save(); applyTheme(); rerender(); break;
    case "preset": S.eq.gains = PRESETS[d.p].slice(); save(); applyEq(); rerender(); break;
    case "clear-history":
    case "clear-all":
      if (confirming !== d.act) { confirming = d.act; toast("Klik sekali lagi untuk menghapus."); setTimeout(() => (confirming = ""), 4000); break; }
      confirming = "";
      if (d.act === "clear-all") { S = defaults(); applyTheme(); applyEq(); } else { S.history = []; S.plays = {}; S.days = []; }
      save(); toast("Dihapus."); rerender(); break;
  }
});
document.addEventListener("keydown", (e) => {
  const typing = /^(INPUT|TEXTAREA)$/.test(document.activeElement.tagName) && document.activeElement.type !== "range" && document.activeElement.type !== "checkbox";
  if (e.key === "Escape") { closeMenu(); if (!$("#full").hidden) openFull(false); else openPanel(false); }
  if (typing) return;
  if (e.key === " " && !/^(BUTTON)$/.test(document.activeElement.tagName)) { e.preventDefault(); toggle(); }
  if (e.key === "Enter" && document.activeElement.dataset && document.activeElement.dataset.act) document.activeElement.click();
  if (e.ctrlKey && e.key === "ArrowRight") step(1);
  if (e.ctrlKey && e.key === "ArrowLeft") step(-1);
  if (e.key === "/" || (e.ctrlKey && e.key.toLowerCase() === "k")) {
    e.preventDefault();
    if (route.name !== "cari") go("cari", null, true);
    const qi = $("#q");
    if (qi) { qi.focus(); qi.select(); }
  }
});
document.addEventListener("keydown", (e) => { if (e.target.id === "q" && e.key === "Enter" && e.target.value.trim()) doSearch(e.target.value.trim()); });
$("#back").onclick = back;
$("#view").addEventListener("input", (e) => {
  const t = e.target;
  if (t.id === "q") search.draft = t.value;
  if (t.id === "lq") { libQ = t.value; filterLib(); }
  if (t.id === "kq") {
    K.q = t.value;
    const all = COLLS[K.which].list(), arr = viewList(all);
    lists.set(K.key, arr);
    arr.forEach((x) => songs.set(x.id, x));
    $("#klist").innerHTML = rowsInner(arr, K.key);
    $("#knote").textContent = all.length && !arr.length ? "Tidak ada yang cocok." : "";
  }
  if (t.dataset.band !== undefined) {
    const i = +t.dataset.band; S.eq.gains[i] = +t.value;
    $(`#eqv${i}`).textContent = (t.value > 0 ? "+" : "") + t.value;
    $("#eq-line").setAttribute("d", eqPath()); applyEq(); save();
  }
});
$("#view").addEventListener("change", (e) => {
  if (e.target.id === "eq-on") { S.eq.on = e.target.checked; save(); if (S.eq.on && cur() && cur().local) ensureEq(); else applyEq(); if (S.eq.on && !(cur() && cur().local)) toast("Ekualiser aktif untuk file dari Perangkat."); }
});
$("#files").addEventListener("change", (e) => {
  for (const f of e.target.files) {
    const id = `local:${f.name}:${f.size}`;
    if (deviceSongs.some((s) => s.id === id)) continue;
    deviceSongs.push({ id, title: f.name.replace(/\.[^.]+$/, ""), artist: "Perangkat", cover: "", local: true, url: URL.createObjectURL(f) });
  }
  e.target.value = "";
  deviceSongs.forEach((s) => songs.set(s.id, s));
  renderSideLib();
  if (route.name === "cari" || route.name === "koleksi" || route.name === "pustaka") rerender();
});

function applyTheme() { document.documentElement.dataset.theme = S.theme; }

/* ---------- start ---------- */
applyTheme();
applyVolume();
renderSideLib();
render();
refreshUi();

/* ---------- setup awal: pastikan yt-dlp ada, unduh bila belum ---------- */
async function runSetup() {
  const box = $("#setup"), bar = $("#setup-bar"), msg = $("#setup-msg");
  const retry = $("#setup-retry"), skip = $("#setup-skip");
  const mb = (n) => (n / 1048576).toFixed(1);
  retry.hidden = skip.hidden = true;
  box.classList.remove("err");
  bar.classList.add("indet");
  bar.style.width = "";
  msg.textContent = "Memeriksa yt-dlp...";
  let off = () => {};
  const ev = window.__TAURI__ && window.__TAURI__.event;
  if (ev) {
    off = await ev.listen("setup-progress", (e) => {
      const p = e.payload;
      if (p.stage !== "unduh") return;
      const known = p.total > 0;
      bar.classList.toggle("indet", !known);
      bar.style.width = known ? Math.min(100, (p.done / p.total) * 100) + "%" : "";
      msg.textContent = known
        ? `Mengunduh yt-dlp (sekali saja)... ${mb(p.done)} / ${mb(p.total)} MB`
        : `Mengunduh yt-dlp (sekali saja)... ${mb(p.done)} MB`;
    });
  }
  // Layar setup baru muncul kalau cek lebih dari 350 ms, supaya tidak berkedip bila yt-dlp sudah ada.
  const show = setTimeout(() => { box.hidden = false; }, 350);
  try {
    await invoke("setup");
    clearTimeout(show);
    box.hidden = true;
  } catch (e) {
    clearTimeout(show);
    box.hidden = false;
    box.classList.add("err");
    msg.textContent = "Setup gagal: " + (e && e.message ? e.message : e);
    retry.hidden = skip.hidden = false;
  } finally {
    off();
  }
}
$("#setup-retry").onclick = runSetup;
$("#setup-skip").onclick = () => { $("#setup").hidden = true; };

/* ---------- layar Setup: pasang app + shortcut (hanya bila exe belum terpasang) ---------- */
async function startApp() {
  let need = false;
  try { need = await invoke("install_state"); } catch (e) { need = false; }
  if (!need) return runSetup();
  const box = $("#install"), msg = $("#install-msg"), go = $("#install-go"), skip = $("#install-skip");
  const track = $("#install-track");
  box.hidden = false;
  skip.onclick = () => { box.hidden = true; runSetup(); };
  go.onclick = async () => {
    go.disabled = skip.disabled = true;
    box.classList.remove("err");
    track.hidden = false;
    msg.textContent = "Memasang RustMusic dan membuat shortcut...";
    try {
      await invoke("install_app"); // sukses: app terpasang lalu terbuka sendiri, jendela ini menutup
    } catch (e) {
      track.hidden = true;
      box.classList.add("err");
      msg.textContent = "Setup gagal: " + (e && e.message ? e.message : e);
      go.textContent = "Coba lagi";
      go.disabled = skip.disabled = false;
    }
  };
}
startApp();

/* ---------- top bar gaya Liner: kontrol jendela (jendela tanpa bingkai) ---------- */
(function winControls() {
  const T = window.__TAURI__;
  const box = $("#winctl");
  if (!T || !T.window || !T.window.getCurrentWindow) { box.hidden = true; $("#drag").hidden = true; return; }
  const win = T.window.getCurrentWindow();
  const sync = async () => { try { box.classList.toggle("maxed", await win.isMaximized()); } catch (e) {} };
  $("#w-min").onclick = () => win.minimize();
  $("#w-max").onclick = () => win.toggleMaximize();
  $("#w-close").onclick = () => win.close();
  try { win.onResized(sync); } catch (e) {}
  sync();
})();
