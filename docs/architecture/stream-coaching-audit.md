# stream_coaching_audit/ — Architektur & Funktionsreferenz

> Pfad: `bot/stream_coaching_audit/` · Stand: 2026-06-08 · 2 Dateien, ~951 Zeilen
>
> Teil der [Architektur-Doku](README.md). Verwandt: [api.md](api.md) (VOD/HLS), [core.md](core.md) (MiniMax). Produktdoku: [STREAM_COACHING_AUDIT.md](../STREAM_COACHING_AUDIT.md). Internes Admin-Tool (kein nutzersichtbares Feature).

## 1. Zweck & Abgrenzung

`stream_coaching_audit/` erstellt **private, evidenzbasierte Audits** autorisierter Twitch-Aufnahmen — primär ein **Slur-/Verhaltens-Screening** für Partner-Kandidaten. Es lädt einen kurzen Audio-Ausschnitt (Live-HLS oder VOD), transkribiert ihn, findet auffällige Stellen über **lokale Regeln** und optional **MiniMax**, und liefert einen Report mit Zeit-/VOD-Sprunglinks. Findet es etwas, gibt es eine DM (Klartext + Zeit/VOD-Link) — sonst still.

Abgrenzung: Reines **internes Admin-Werkzeug**. Es moderiert nicht automatisch und postet nichts öffentlich; die Slurs werden vor der Weitergabe **redigiert** (Klartext nur im privaten Admin-Excerpt).

## 2. Einordnung & Abhängigkeiten

| Richtung | Beziehung |
|----------|-----------|
| **Wird genutzt von** | Admin-Tooling (manueller Audit + Auto-VOD bei Stream-Ende). |
| **Nutzt** | `api/` (HLS-URL-Auflösung/VOD), Whisper (Transkription), MiniMax (LLM-Findings), `ffmpeg`/streamlink (Audio), `storage/` (Audit-Ablage). |
| **Daten** | `data/stream_coaching_audits/` (private Report-Markdown), transient die Audio-/Transkript-Dateien. |
| **Externe Dienste** | Twitch (HLS/VOD), OpenAI-Whisper (Transkription), MiniMax. |
| **Secret-Namen** | Whisper-/OpenAI-Key, MiniMax-Key. |

## 3. Dateien im Überblick

| Datei | Zeilen | Rolle |
|-------|-------:|-------|
| `service.py` | 931 | Kompletter Audit-Flow: Media holen → transkribieren → Findings → Report. |
| `__init__.py` | 20 | Öffentliche Symbole. |

## 4. Datenfluss / Lebenszyklus

1. **Media holen:** `_acquire_media(source, *, source_kind, live_seconds, workdir)` löst die aktuelle HLS-URL auf (live) bzw. nutzt das VOD und nimmt eine kurze **Audio-only**-Aufnahme in einem temporären Workdir auf (`_AcquiredMedia`).
2. **Transkribieren:** Whisper erzeugt `AuditSegment`s (Text + Zeit).
3. **Findings (zweistufig):** `detect_rule_findings(segments)` liefert hochsichere lokale Treffer **ohne** Transkript-Versand; `_detect_llm_findings_minimax(segments)` schickt Batches (`_segment_batches`, max ~12k Zeichen) an MiniMax für weichere Treffer. Beide werden gemerged.
4. **Redaktion & Report:** Bekannte Slurs werden via `redact_text` maskiert, bevor Evidenz den Transkript-Kontext verlässt; `AuditReport.to_dict` bündelt die Findings mit `_evidence_excerpt` (maskiert) und VOD-Sprunglinks (`_vod_jump_url`, `?t=…`). Der private Admin-Report nutzt `_evidence_excerpt_raw` (unmaskiert).
5. **Zustellung:** Nur bei Funden geht eine DM raus; ohne Funde bleibt es still. Live-Watch + Auto-VOD bei Stream-Ende sind die zwei Auslöser.

## 5. Funktionsreferenz (service.py)

Datentypen: `AuditSegment` (transkribierter Abschnitt), `AuditFinding` (ein Treffer), `AuditReport` (`to_dict`), `_AcquiredMedia`, `AuditError`.

- `redact_text(text) -> str` — bekannte Slurs maskieren, **bevor** Evidenz den transienten Transkript-Kontext verlässt.
- `detect_rule_findings(segments) -> list[AuditFinding]` — hochsichere lokale Findings ohne LLM/Transkript-Versand.
- `_detect_llm_findings_minimax(segments) -> list[AuditFinding]` — weichere Findings via MiniMax; `_segment_batches(segments, *, max_chars=12000)` chunked, `_extract_json_object(raw)` parst.
- `_evidence_excerpt(text, match=None)` — maskierter Beleg-Ausschnitt; `_evidence_excerpt_raw(...)` — **unmaskiert, nur für den privaten Admin-Report**. `_evidence_hash(text)` — stabiler Hash.
- `_vod_jump_url(source_url, seconds) -> str` — Twitch-VOD-Sprunglink (`?t=1h2m3s`); leer ohne VOD.
- `_acquire_media(source, *, source_kind, live_seconds, workdir)` + HLS-Auflösung — kurze Audio-only-Aufnahme. `_collapse_space` als Text-Helfer.

## 6. Datenbank & externe Schnittstellen

- **Daten:** `data/stream_coaching_audits/*.md` (private Reports), transiente Audio-/Transkript-Dateien (nach Lauf entfernt).
- **Extern:** Twitch (HLS live / VOD), Whisper (Transkription), MiniMax (LLM-Findings).

## 7. Stolperfallen / Besonderheiten

- **Privatsphäre by design:** Slurs werden vor Weitergabe maskiert (`redact_text`/`_evidence_excerpt`); nur der private Admin-Excerpt ist unmaskiert. Reports nicht öffentlich teilen.
- **ffmpeg-Falle:** Für Twitch-HLS den System-`/usr/bin/ffmpeg` setzen (`FFMPEG_BIN`); der statische `~/.local`-Build segfaultet mit leerem stderr (siehe Memory).
- **Lokale Regeln zuerst:** `detect_rule_findings` läuft ohne Transkript-Versand — nur die unsicheren Fälle gehen an MiniMax (Kosten + Datensparsamkeit).
- **Internes Tool, kein Changelog:** bewusst ohne Nutzer-Ankündigung/Changelog (siehe Memory) — es ist ein Admin-Screening, kein Produkt-Feature.
- **Nur bei Funden zustellen:** Kein Fund → keine DM. Stille ist das erwartete Ergebnis.
