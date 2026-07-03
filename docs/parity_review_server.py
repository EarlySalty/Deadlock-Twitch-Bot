#!/usr/bin/env python3
"""Lokaler Review-Server fuer das Python->Rust Paritaets-Audit.

Zweck: pro Finding ein Kommentar-/Entscheidungsfeld mit ECHTER Auto-Speicherung
auf Platte, damit Claude die Notizen einfach aus der JSON-Datei lesen kann.

Start:  python3 parity_review_server.py
Oeffnen: http://127.0.0.1:8791/
Kommentare landen in: parity_review_comments.json (neben diesem Skript).

Keine externen Abhaengigkeiten (nur Python-stdlib). Bindet nur an 127.0.0.1.
"""
from __future__ import annotations
import json
import os
import tempfile
from datetime import datetime
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

HERE = os.path.dirname(os.path.abspath(__file__))
DATA_FILE = os.path.join(HERE, "parity_review_data.json")
COMMENTS_FILE = os.path.join(HERE, "parity_review_comments.json")
PORT = 8791

VALID_VERDICTS = {"", "fixen", "spaeter", "ignorieren", "rust_ok"}


def load_data() -> dict:
    with open(DATA_FILE, encoding="utf-8") as f:
        return json.load(f)


def load_comments() -> dict:
    if not os.path.exists(COMMENTS_FILE):
        return {}
    try:
        with open(COMMENTS_FILE, encoding="utf-8") as f:
            return json.load(f)
    except (json.JSONDecodeError, OSError):
        return {}


def save_comments(data: dict) -> None:
    """Atomar schreiben, damit ein Absturz die Datei nie halb hinterlaesst."""
    fd, tmp = tempfile.mkstemp(dir=HERE, prefix=".parity_comments_", suffix=".tmp")
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as f:
            json.dump(data, f, ensure_ascii=False, indent=2, sort_keys=True)
        os.replace(tmp, COMMENTS_FILE)
    finally:
        if os.path.exists(tmp):
            os.remove(tmp)


def _now() -> str:
    return datetime.now().isoformat(timespec="seconds")


def _clean_entry(raw: dict) -> dict | None:
    """Einen eingehenden Eintrag validieren/normalisieren. None = nichts speichern."""
    verdict = (raw.get("verdict") or "").strip()
    if verdict not in VALID_VERDICTS:
        verdict = ""
    comment = (raw.get("comment") or "").strip()
    if not verdict and not comment:
        return None  # leerer Eintrag -> loeschen
    return {"verdict": verdict, "comment": comment, "ts": _now()}


HTML_SHELL = r"""<!DOCTYPE html>
<html lang="de">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Py→Rust Paritaet — Entscheidungen</title>
<style>
  :root{
    --bg:#0f1115; --panel:#171a21; --panel2:#1d222b; --line:#2a2f3a;
    --fg:#e7e9ee; --muted:#9aa4b2; --accent:#6ea8fe;
    --fixen:#3fb950; --spaeter:#d29922; --ignorieren:#6e7681; --rustok:#58a6ff;
  }
  *{box-sizing:border-box}
  body{margin:0;background:var(--bg);color:var(--fg);
    font:15px/1.5 system-ui,-apple-system,Segoe UI,Roboto,sans-serif}
  header{position:sticky;top:0;z-index:10;background:rgba(15,17,21,.96);
    backdrop-filter:blur(6px);border-bottom:1px solid var(--line);
    padding:12px 20px}
  h1{font-size:18px;margin:0 0 8px}
  .bar{display:flex;flex-wrap:wrap;gap:10px;align-items:center}
  .bar input[type=search]{flex:1;min-width:220px;background:var(--panel2);
    border:1px solid var(--line);color:var(--fg);border-radius:8px;padding:8px 12px}
  .bar label{color:var(--muted);display:flex;gap:6px;align-items:center;cursor:pointer}
  .progress{color:var(--muted);font-size:13px;white-space:nowrap}
  .progress b{color:var(--fg)}
  main{max-width:960px;margin:0 auto;padding:20px}
  .theme{margin:22px 0 8px}
  .theme h2{font-size:15px;color:var(--accent);margin:0 0 2px;
    border-left:3px solid var(--accent);padding-left:10px}
  .theme .cnt{color:var(--muted);font-size:12px;padding-left:13px}
  .card{background:var(--panel);border:1px solid var(--line);
    border-left:4px solid var(--line);border-radius:10px;padding:12px 14px;margin:10px 0}
  .card.v-fixen{border-left-color:var(--fixen)}
  .card.v-spaeter{border-left-color:var(--spaeter)}
  .card.v-ignorieren{border-left-color:var(--ignorieren)}
  .card.v-rust_ok{border-left-color:var(--rustok)}
  .chead{display:flex;flex-wrap:wrap;gap:8px;align-items:center;margin-bottom:4px}
  .fid{font-weight:700;font-family:ui-monospace,Menlo,Consolas,monospace;font-size:13px}
  .badge{font-size:11px;padding:2px 8px;border-radius:999px;border:1px solid var(--line);
    color:var(--muted);background:var(--panel2)}
  .badge.ruling{color:#e7e9ee;border-color:var(--rustok)}
  .summary{margin:2px 0}
  .risk{color:var(--muted);font-size:13px;margin:2px 0 8px}
  .risk::before{content:"⚠ ";color:var(--spaeter)}
  .rulingnote{color:#c9d3e0;font-size:13px;background:var(--panel2);
    border-radius:8px;padding:6px 10px;margin:4px 0}
  .chips{display:flex;flex-wrap:wrap;gap:6px;margin:6px 0}
  .chip{cursor:pointer;font-size:12px;padding:4px 10px;border-radius:999px;
    border:1px solid var(--line);background:var(--panel2);color:var(--muted);user-select:none}
  .chip:hover{border-color:var(--muted)}
  .chip.on{color:#0f1115;font-weight:700;border-color:transparent}
  .chip.on[data-v=fixen]{background:var(--fixen)}
  .chip.on[data-v=spaeter]{background:var(--spaeter)}
  .chip.on[data-v=ignorieren]{background:var(--ignorieren);color:#fff}
  .chip.on[data-v=rust_ok]{background:var(--rustok)}
  textarea{width:100%;min-height:96px;resize:vertical;background:var(--panel2);
    border:1px solid var(--line);color:var(--fg);border-radius:8px;padding:10px 12px;
    font:inherit;line-height:1.55;margin-top:2px;overflow:hidden}
  textarea:focus{outline:none;border-color:var(--accent)}
  .status{font-size:11px;color:var(--muted);min-height:14px;margin-top:3px}
  .status.ok{color:var(--fixen)}
  .status.err{color:#f85149}
  details{background:var(--panel);border:1px solid var(--line);border-radius:10px;
    margin:14px 0;padding:0 14px}
  details>summary{cursor:pointer;padding:12px 0;font-weight:600;color:var(--muted)}
  details .card{background:var(--panel2)}
  .mini{font-size:13px}
  .hidden{display:none !important}
  footer{color:var(--muted);text-align:center;padding:30px 0;font-size:12px}
</style>
</head>
<body>
<header>
  <h1>Python→Rust Paritaet — <span id="openCount"></span> offene Entscheidungen</h1>
  <div class="bar">
    <input id="q" type="search" placeholder="Suchen (ID oder Text)…" autocomplete="off">
    <label><input type="checkbox" id="onlyOpen"> nur unentschiedene</label>
    <span class="progress"><b id="decided">0</b> / <span id="total">0</span> entschieden · <span id="saveglobal">bereit</span></span>
  </div>
</header>
<main id="app"></main>
<footer>Auto-Speicherung nach <code>parity_review_comments.json</code>. Einfach zumachen — nichts geht verloren.</footer>

<script>
window.__DATA__ = __DATA_JSON__;
window.__COMMENTS__ = __COMMENTS_JSON__;
</script>
<script>
const DATA = window.__DATA__;
const COMMENTS = window.__COMMENTS__ || {};
const LS_KEY = "parity_comments_v1";
const VLABEL = {fixen:"fixen", spaeter:"später", ignorieren:"ignorieren", rust_ok:"Rust ok"};

// --- lokale Spiegelung (Offline-Backup, falls Server mal aus ist) ---
function lsLoad(){ try{ return JSON.parse(localStorage.getItem(LS_KEY)||"{}"); }catch(e){ return {}; } }
function lsSave(map){ try{ localStorage.setItem(LS_KEY, JSON.stringify(map)); }catch(e){} }

// Merge: Server ist Wahrheit; localStorage fuellt nur Luecken (Recovery nach Server-Neustart)
const state = {};
const ls = lsLoad();
const missingOnServer = {};
new Set([...Object.keys(COMMENTS), ...Object.keys(ls)]).forEach(id=>{
  if (COMMENTS[id]) { state[id] = COMMENTS[id]; }
  else if (ls[id] && (ls[id].verdict || ls[id].comment)) { state[id] = ls[id]; missingOnServer[id] = ls[id]; }
});

function entry(id){ return state[id] || {verdict:"", comment:""}; }

async function post(path, body){
  const r = await fetch(path, {method:"POST", headers:{"Content-Type":"application/json"}, body:JSON.stringify(body)});
  if(!r.ok) throw new Error("HTTP "+r.status);
  return r.json();
}

const timers = {};
function scheduleSave(id, statusEl){
  clearTimeout(timers[id]);
  statusEl.textContent = "…"; statusEl.className = "status";
  timers[id] = setTimeout(()=>flush(id, statusEl), 450);
}
async function flush(id, statusEl){
  const e = entry(id);
  const payload = {id, verdict:e.verdict||"", comment:e.comment||""};
  lsSave(collect());
  try{
    const res = await post("/save", payload);
    const ts = res.ts ? (" " + res.ts.replace("T"," ").slice(11,19)) : "";
    statusEl.textContent = (payload.verdict||payload.comment) ? ("gespeichert ✓"+ts) : "geleert";
    statusEl.className = "status ok";
    setGlobal("gespeichert ✓"+ts);
  }catch(err){
    statusEl.textContent = "OFFLINE gespeichert (nur Browser) — Server aus?";
    statusEl.className = "status err";
    setGlobal("⚠ Server nicht erreichbar (lokal gesichert)");
  }
  updateProgress();
}
function collect(){
  const out = {};
  Object.keys(state).forEach(id=>{ const e=state[id]; if(e.verdict||e.comment) out[id]=e; });
  return out;
}
let gt;
function setGlobal(msg){ const g=document.getElementById("saveglobal"); g.textContent=msg;
  clearTimeout(gt); gt=setTimeout(()=>{g.textContent="bereit";}, 2500); }

function autogrow(ta){ ta.style.height="auto"; ta.style.height=(ta.scrollHeight+4)+"px"; }

function card(id, {closed=false}={}){
  const sum = (DATA.summaries[id]||"").toString();
  const risk = (DATA.risk[id]||"").toString();
  const ruling = DATA.rulings[id];
  const e = entry(id);
  const el = document.createElement("div");
  el.className = "card" + (e.verdict?(" v-"+e.verdict):"");
  el.dataset.id = id;
  el.dataset.hay = (id+" "+sum+" "+risk).toLowerCase();

  const head = document.createElement("div"); head.className="chead";
  const fid = document.createElement("span"); fid.className="fid"; fid.textContent=id; head.appendChild(fid);
  if(risk){ const b=document.createElement("span"); b.className="badge"; b.textContent="Risiko"; b.title=risk; head.appendChild(b); }
  if(ruling){ const b=document.createElement("span"); b.className="badge ruling"; b.textContent=ruling.verdict; head.appendChild(b); }
  el.appendChild(head);

  const s = document.createElement("div"); s.className="summary"+(closed?" mini":""); s.textContent=sum; el.appendChild(s);
  if(risk && !closed){ const r=document.createElement("div"); r.className="risk"; r.textContent=risk; el.appendChild(r); }
  if(ruling){ const rn=document.createElement("div"); rn.className="rulingnote"; rn.textContent="Ruling: "+ruling.note; el.appendChild(rn); }

  // Verdict-Chips
  const chips = document.createElement("div"); chips.className="chips";
  const status = document.createElement("div"); status.className="status";
  ["fixen","spaeter","ignorieren","rust_ok"].forEach(v=>{
    const c=document.createElement("span"); c.className="chip"+(e.verdict===v?" on":""); c.dataset.v=v; c.textContent=VLABEL[v];
    c.onclick=()=>{
      const cur = entry(id);
      const nv = cur.verdict===v ? "" : v;
      state[id] = {verdict:nv, comment:cur.comment||""};
      chips.querySelectorAll(".chip").forEach(x=>x.classList.toggle("on", x.dataset.v===nv));
      el.className = "card" + (nv?(" v-"+nv):"");
      scheduleSave(id, status);
    };
    chips.appendChild(c);
  });
  el.appendChild(chips);

  const ta = document.createElement("textarea");
  ta.placeholder = "Notiz / Begründung (optional)…";
  ta.value = e.comment||"";
  ta.oninput = ()=>{
    const cur = entry(id);
    state[id] = {verdict:cur.verdict||"", comment:ta.value};
    autogrow(ta);
    scheduleSave(id, status);
  };
  el.appendChild(ta);
  el.appendChild(status);
  return el;
}

function themeSection(title, ids){
  const wrap = document.createElement("section"); wrap.className="theme";
  const h=document.createElement("h2"); h.textContent=title; wrap.appendChild(h);
  const c=document.createElement("div"); c.className="cnt"; c.textContent=ids.length+" Findings"; wrap.appendChild(c);
  ids.slice().sort().forEach(id=> wrap.appendChild(card(id)));
  return wrap;
}

function collapsed(title, ids){
  const d=document.createElement("details");
  const s=document.createElement("summary"); s.textContent=title+" ("+ids.length+")"; d.appendChild(s);
  ids.slice().sort().forEach(id=> d.appendChild(card(id, {closed:true})));
  return d;
}

function render(){
  const app=document.getElementById("app");
  app.innerHTML="";
  const themes = DATA.keep_themes;
  Object.keys(themes).forEach(t=> app.appendChild(themeSection(t, themes[t])));
  // RAID-RECRUIT-015: Rust-korrekt-Ruling, nicht in Themen -> in "schon korrekt"
  const alreadyIds = DATA.already.concat(DATA.rulings["RAID-RECRUIT-015"] ? ["RAID-RECRUIT-015"] : []);
  app.appendChild(collapsed("✅ Gefixt — nur zur Nachvollziehbarkeit", DATA.fixed));
  app.appendChild(collapsed("✔ Schon korrekt / kein Handlungsbedarf", alreadyIds));
  const openTotal = Object.values(themes).reduce((a,b)=>a+b.length,0);
  document.getElementById("openCount").textContent = openTotal;
  document.getElementById("total").textContent = openTotal;
  updateProgress();
  // Auto-Hoehe: sichtbare Felder sofort, eingeklappte beim Aufklappen
  document.querySelectorAll("main textarea").forEach(autogrow);
  document.querySelectorAll("details").forEach(d=>{
    d.addEventListener("toggle", ()=>{ if(d.open) d.querySelectorAll("textarea").forEach(autogrow); });
  });
}

function updateProgress(){
  const themes = DATA.keep_themes;
  const openIds = Object.values(themes).flat();
  const decided = openIds.filter(id=> entry(id).verdict).length;
  document.getElementById("decided").textContent = decided;
}

// Filter
function applyFilter(){
  const q=(document.getElementById("q").value||"").toLowerCase().trim();
  const onlyOpen=document.getElementById("onlyOpen").checked;
  document.querySelectorAll("main .theme .card").forEach(c=>{
    const id=c.dataset.id;
    const matchQ = !q || c.dataset.hay.includes(q);
    const matchOpen = !onlyOpen || !entry(id).verdict;
    c.classList.toggle("hidden", !(matchQ && matchOpen));
  });
  // leere Themen ausblenden
  document.querySelectorAll("main .theme").forEach(sec=>{
    const anyVisible=[...sec.querySelectorAll(".card")].some(c=>!c.classList.contains("hidden"));
    sec.classList.toggle("hidden", !anyVisible);
  });
}
document.getElementById("q").addEventListener("input", applyFilter);
document.getElementById("onlyOpen").addEventListener("change", applyFilter);

render();

// Recovery: was nur im Browser lag (Server war aus) jetzt nachziehen
if(Object.keys(missingOnServer).length){
  post("/sync", missingOnServer).then(()=>setGlobal("Browser-Backup mit Server synchronisiert")).catch(()=>{});
}
</script>
</body>
</html>
"""


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *a):  # leise
        pass

    def _send(self, code, body, ctype="application/json"):
        if isinstance(body, (dict, list)):
            body = json.dumps(body, ensure_ascii=False)
        data = body.encode("utf-8")
        self.send_response(code)
        self.send_header("Content-Type", ctype + "; charset=utf-8")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        if self.path in ("/", "/index.html"):
            try:
                data = load_data()
            except Exception as exc:  # noqa: BLE001
                self._send(500, f"<h1>Daten fehlen</h1><pre>{exc}</pre>", "text/html")
                return
            comments = load_comments()
            data_js = json.dumps(data, ensure_ascii=False).replace("</", "<\\/")
            comments_js = json.dumps(comments, ensure_ascii=False).replace("</", "<\\/")
            html = (HTML_SHELL
                    .replace("__DATA_JSON__", data_js, 1)
                    .replace("__COMMENTS_JSON__", comments_js, 1))
            self._send(200, html, "text/html")
        elif self.path == "/comments":
            self._send(200, load_comments())
        else:
            self._send(404, {"error": "not found"})

    def _read_json(self):
        length = int(self.headers.get("Content-Length", 0))
        raw = self.rfile.read(length) if length else b"{}"
        return json.loads(raw.decode("utf-8") or "{}")

    def do_POST(self):
        try:
            payload = self._read_json()
        except (json.JSONDecodeError, ValueError):
            self._send(400, {"error": "bad json"})
            return

        if self.path == "/save":
            fid = (payload.get("id") or "").strip()
            if not fid:
                self._send(400, {"error": "missing id"})
                return
            comments = load_comments()
            entry = _clean_entry(payload)
            if entry is None:
                comments.pop(fid, None)
                save_comments(comments)
                self._send(200, {"ok": True, "cleared": True})
            else:
                comments[fid] = entry
                save_comments(comments)
                self._send(200, {"ok": True, "ts": entry["ts"]})
        elif self.path == "/sync":
            # Bulk-Merge: nur Eintraege setzen, die noch fehlen (Recovery aus Browser-Backup)
            comments = load_comments()
            changed = 0
            for fid, raw in payload.items():
                if fid in comments:
                    continue
                entry = _clean_entry(raw if isinstance(raw, dict) else {})
                if entry is not None:
                    comments[fid] = entry
                    changed += 1
            if changed:
                save_comments(comments)
            self._send(200, {"ok": True, "added": changed})
        else:
            self._send(404, {"error": "not found"})


def main():
    if not os.path.exists(DATA_FILE):
        raise SystemExit(f"Datendatei fehlt: {DATA_FILE}")
    server = ThreadingHTTPServer(("127.0.0.1", PORT), Handler)
    print(f"Paritaets-Review laeuft auf http://127.0.0.1:{PORT}/")
    print(f"Kommentare -> {COMMENTS_FILE}")
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass


if __name__ == "__main__":
    main()
