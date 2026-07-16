# Social-Clip-Pipeline — Design (2026-07-16)

Status: **APPROVED (mündlich)**, Umsetzung in Phasen.
Repo: `Deadlock-Twitch-Bot`, Crate `tb-social-media` (+ `tb-engagement`, `tb-dashboard-api`).
Branch: `feat/social-clip-pipeline`.

## 1. Kontext / Ist-Zustand

Die komplette Social-Pipeline existiert bereits als portierter Rust-Code, ist aber **dormant**:

- **Clip-Fetch** (`clip/`, Helix `GET /clips`, Tabellen `twitch_clips_social_media`, `clip_fetch_history`) — **AUS**: `tb_bot: clip_fetch deaktiviert (TB_CLIP_FETCHER_ENABLED != 1)` bei jedem Start (zuletzt 2026-07-16 02:59). Es werden aktuell **null Clips gezogen**.
- **Enrichment** (`enrich_pipeline.rs`, `enrichment_worker.rs`, `llm.rs`, `llm_dispatch.rs`): Titel/Hashtags pro Plattform via Ollama (default) / MiniMax / Claude Haiku (consent-gated). Vokabel-Korrektur `vocab.rs` + Tabelle `deadlock_vocab`.
- **Whisper/Voice→Text** (`tb-engagement/src/transcribe.rs`, OpenAI `whisper-1`): existiert, aber im Enrichment-Worker **nicht injiziert** → Stage übersprungen.
- **Approval** (`approval.rs`, `approval_worker.rs`) + Auto-Approve-Settings pro Plattform.
- **Upload** (`upload_worker.rs`, `clip_queue.rs`, Queue `twitch_clips_upload_queue` mit vorhandener Spalte `scheduled_at`, `video_processor.rs` schneidet 9:16) → Uploader `uploaders/{tiktok,youtube,instagram}.rs`, OAuth (`oauth.rs`, `credentials.rs`, `refresh_worker.rs`, Tabelle `social_media_platform_auth`).
- **Analytics/Reports** (`insights_worker.rs`, `report_*`).
- Dashboard: server-gerenderte HTML-Seite unter `/social-media` (Port 8769), Templates `src/templates/dashboard.html`.

**Fazit:** Kein Neubau nötig — anschalten, verdrahten, 4 Lücken schließen.

## 2. Ziel (End-to-End-Flow)

```
Fetch (Helix, bulk, nur Deadlock)  →  Clip-Inbox (status=fetched)
        ↓  [User aktiviert Clip im Dashboard: Ziele + Zeit wählen]
        →  status=activated
  ┌──────────────────────────────────────┬─────────────────────────────┐
  │ EIGENE KANÄLE (TikTok/Shorts/IG)       │ MONTAGE-FORMS (Reichweite)  │
  │ Titel-Gate.decide():                   │ Original-Clip-URL + Credit  │
  │   guter Titel? → behalten              │ + Hero + Typ → HTTP-POST    │
  │   sonst → Whisper → vocab → AI-Titel   │ an formResponse             │
  │ 9:16-Cut → Queue(scheduled_at)         │ pro Form abwählbar          │
  │ → upload_worker postet zur Planzeit    │                             │
  └──────────────────────────────────────┴─────────────────────────────┘
```

**Kernregel:** Teures (Whisper, LLM, Upload) läuft **nur für vom User aktivierte Clips**. Fetch ist bulk & billig.

## 3. Scope / Nicht-Ziele (YAGNI)

- **Kein** generisches Cron/Job-Framework — der vorhandene Interval-Worker + `scheduled_at`-Filter reicht.
- **Kein** React-SPA-Tab — die bestehende `/social-media`-HTML-Seite wird erweitert.
- **Kein** externes „Deadlock-Brain"-AI-Coach-Projekt — „Deadlock-Keywords" = vorhandenes `deadlock_vocab` (Entscheidung A).
- **Kein** eigenes Video-Hosting für Forms — Forms bekommen die öffentliche Twitch-Clip-URL (horizontal, passt).
- Whisper nur selektiv, nicht pauschal.

## 4. Architektur: Wiederverwenden vs. Neu

| | Komponente | Ort |
|---|---|---|
| ♻️ anschalten | Clip-Fetch (`TB_CLIP_FETCHER_ENABLED=1`, Deadlock-Game-Filter) | env + `clip/service.rs` |
| ♻️ verdrahten | Whisper-Transcriber in Enrichment injizieren | `enrichment_worker.rs` |
| ♻️ nutzen | LLM-Titel/Hashtags, `deadlock_vocab`, Approval, Uploader, OAuth, `scheduled_at` | vorhanden |
| 🔨 neu | **Titel-Gate `decide()`** (pure, geloggt) | `enrichment.rs` / neues `title_gate.rs` |
| 🔨 neu | **Forms-Submitter** | neues `forms.rs` + Tabelle |
| 🔨 neu | **Scheduler-Filter + Kadenz-Planer** | `upload_worker.rs` + `settings.rs` |
| 🔨 neu | **Clip-Inbox-UI** (aktivieren/Ziele/Zeit) | `dashboard.html` + `handlers/social_media.rs` |

## 5. Datenmodell (minimal)

Neue Tabelle (Migration `rust/migrations/`):

```sql
CREATE TABLE twitch_clip_form_submissions (
    id            SERIAL PRIMARY KEY,
    clip_id       INTEGER NOT NULL REFERENCES ... ,
    form_key      TEXT NOT NULL,          -- 'vindicta_eclipse' | 'deadlock_high' | 'deadlock_pirate'
    status        TEXT NOT NULL DEFAULT 'pending', -- pending|submitted|failed|skipped
    http_status   INTEGER,
    error         TEXT,
    submitted_at  TIMESTAMPTZ,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (clip_id, form_key)            -- Doppel-Submit-Schutz
);
```

Bestehende Tabellen: `twitch_clips_social_media` bekommt (falls nicht da) einen Status-/Aktivierungs-Marker (`activated_at TIMESTAMPTZ NULL`) und die gewählten Ziele. Prüfen, ob Approval-Tabellen das schon abdecken; sonst minimal ergänzen — **kein** paralleles Status-System.

## 6. Titel-Gate `decide()` (der „Judge" — voll geloggt)

Reine Funktion, seiteneffektfrei, testbar:

```
enum TitleDecision { UseExisting, GenerateFromMetadata, TranscribeThenGenerate }

fn decide(existing_title: &str, has_transcript: bool) -> TitleDecision
```

Heuristik (billig, ohne LLM):
- Titel leer / nur Stream-Titel / generisch („Clip", „!clip", Kanalname, < N sinnvolle Wörter, kein `deadlock_vocab`-Treffer) → **TranscribeThenGenerate** (falls `OPENAI_API_KEY` gesetzt, sonst GenerateFromMetadata).
- Titel enthält Deadlock-Begriff + wirkt beschreibend → **UseExisting** (AI poliert nur Hashtags).
- Sonst → **GenerateFromMetadata**.

**Pflicht (Regel „alle Judge-Entscheidungen loggen"):** JEDE Entscheidung loggt `input(title gekürzt) | verdict | grund`. Auch UseExisting/Skip. Manueller „jetzt transkribieren"-Button erzwingt TranscribeThenGenerate unabhängig vom Gate.

## 7. Whisper-Trigger

- Läuft **nur** bei `TranscribeThenGenerate` und gesetztem `OPENAI_API_KEY`.
- Transcriber (vorhanden in `tb-engagement`) in `enrichment_worker` injizieren (heute `None`).
- Ergebnis-Transkript → `deadlock_vocab`-Keyword-Extraktion → LLM-Prompt für Clickbait-Titel + Tags.
- Kosten-Ceiling: nur aktivierte Clips + nur schlechte Titel → praktisch selten. Kein Batch-Limit nötig (`// ponytail:` Cap dokumentieren, falls Volumen später steigt).

## 8. Forms-Submitter (`forms.rs`)

POST `application/x-www-form-urlencoded` an die `formResponse`-URL. Öffentlich, **kein** Google-Login (keine File-Upload-Felder). Idempotenz via `UNIQUE(clip_id, form_key)`.

**Vindicta Eclipse** — `https://docs.google.com/forms/d/e/1FAIpQLSe6Q0nHYVQSSBaAhSyvOeBzI97f0OB3wIJpYuwZ3ZqRsxmH3Q/formResponse`
| entry | Wert |
|---|---|
| `entry.1290281111` | Clip-URL (Twitch, horizontal) |
| `entry.1554938691` | Credit = Streamer-Name |
| `entry.1264171160` | `"Yes and Vindicta Eclipse has permission to use it"` |
| `entry.1478183538` | AI-Kurzbeschreibung / Titel |
| `entry.1585060458` | Kontakt-E-Mail (Config) |

**Deadlock High** — `https://docs.google.com/forms/d/e/1FAIpQLSeVOlCAmjIVr-GPyoq1D0kp5YjUKDF8U9JglWw-5LsaClV05A/formResponse`
| entry | Wert |
|---|---|
| `entry.652511119` | E-Mail (Config) |
| `entry.1933051763` | Credit |
| `entry.284507193` | Hero (aus Enrichment) |
| `entry.1930240104` | AI-Erklärung „why it's good" |
| `entry.1338784444` | Clip-URL |
| `entry.1950414210` | Typ ∈ `FUNNY|EPIC|CLUTCH|FAIL` (Enrichment-Klassifikation, Default `EPIC`) |
| `entry.1123024881` | `"Yes, and you have my permission to use it on Deadlock HIGH"` |

**deadlock_pirate** (loser Fit, per User-Entscheid: Link ins ID-Feld, Zeit egal) — `https://docs.google.com/forms/d/e/1FAIpQLSdiyrvA_1vLFJf2CribM3fSi4ww-5oRd5IPOUFeTwVwURsUVQ/formResponse`
| entry | Wert |
|---|---|
| `entry.1101495589` (Replay ID) | **Clip-URL** (bewusst zweckentfremdet) |
| `entry.1736002995` (Rank, CHECK) | Default `"Emissary / Archon / Oracle"` |
| `entry.1701310762` (Replay time) | Platzhalter `"0:00"` |
| `entry.292357452` (Hero) | Hero aus Enrichment |
| `entry.344865364` (Description) | AI-Titel |
| `entry.1690403409` (Credit) | Streamer-Name |

> **Caveat (dokumentiert):** deadlock_pirate erwartet eine echte Replay-ID; der URL-Missbrauch kann dazu führen, dass der Kanal-Owner die Einreichung ignoriert oder sperrt. Deshalb pro Clip UND global in Config abschaltbar; Default-Zustand für pirate = **konfigurierbar** (Vorschlag: an, aber leicht deaktivierbar).

**Logging:** jeder Submit-Versuch loggt `clip_id | form_key | http_status | ok/fail`. Auch Skips.

**Consent-Hinweis:** Der Bot beantwortet die Owner-Pflichtfrage automatisch mit „ja/Erlaubnis". Zulässig, weil es die Clips der eigenen Streamer sind — im Dashboard sichtbar machen, dass mit Aktivierung diese Rechteerklärung abgegeben wird.

## 9. Scheduler / Kadenz

- `upload_worker` zieht nur Queue-Zeilen mit `scheduled_at <= now()` (heute wird `scheduled_at` gesetzt, aber nicht als Gate genutzt — Filter ergänzen).
- **Kadenz-Config** (in `social_media_settings`): Slots pro Tag + Uhrzeiten (z. B. `["14:00","18:00","21:00"]`, TZ Europe/Berlin).
- Beim Aktivieren eines Clips: `next_free_slot()` stempelt `scheduled_at` auf den nächsten freien Slot → Bot verteilt automatisch. User kann pro Clip die Zeit überschreiben.
- Forms-Submits folgen demselben `scheduled_at` (oder sofort — Config).

## 10. Clip-Inbox-UI

Erweiterung `dashboard.html` (server-rendered) + JSON-Endpoints (viele existieren: `/social-media/api/clips`, `.../fetch-clips`, `.../mark-uploaded`, `.../batch-upload`):
- Inbox-Liste der `fetched` Clips (Thumbnail, Titel, Länge, Views).
- Pro Clip: „Aktivieren" → Ziel-Checkboxen (TikTok/Shorts/IG + Vindicta/Deadlock High/pirate, alle vorausgewählt) + Zeit-Picker (Default = nächster Kadenz-Slot) + „jetzt transkribieren".
- Neue Endpoints nur, wo keiner existiert (z. B. `POST /social-media/api/clips/:id/activate`).
- **Deutsche user-sichtbare Texte schreibt Claude**, nicht der GPT-Worker (Platzhalter → Claude füllt final).

## 11. Phasen (DAG, mit Definition of Done)

- **P0 — Realitätscheck (Ops, Claude).** Fetcher testweise an (read-only zu Twitch), *ein* Deadlock-Clip landet in DB + Enrichment erzeugt Titel/Hashtags. **KEIN** Posting. DoD: Clip-Zeile + Enrichment-Zeile in DB nachgewiesen, Journal fehlerfrei. Blockiert: nichts. Voraussetzung für alle Code-Phasen (sonst bauen wir auf ungetestetem Pfad).
- **P1 — Titel-Gate + Whisper-Verdrahtung.** `decide()` + Tests, Transcriber injiziert, Logging. DoD: Tests grün; schlechter Titel → Transkript-Pfad, guter Titel → UseExisting, jede Entscheidung geloggt.
- **P2 — Forms-Submitter.** `forms.rs` + Migration + Tests + Logging. DoD: erfolgreicher realer Test-Submit an Vindicta+Deadlock High (verifizierbar), Idempotenz greift, pirate abschaltbar. **Achtung Außenaktion:** der erste echte Submit geht unwiderruflich an einen fremden Kanal — bewusst mit einem realen Clip auslösen, nicht versehentlich in Tests (Tests bauen nur das Payload, POSTen nicht live).
- **P3 — Scheduler/Kadenz.** `scheduled_at`-Filter + `next_free_slot()` + Config + Tests. DoD: aktivierter Clip erhält Slot, Worker postet erst zur Zeit.
- **P4 — Clip-Inbox-UI.** Backend-Endpoints (GPT) + deutsche UI (Claude). DoD: Clip im Dashboard aktivierbar, Ziele/Zeit wählbar, sichtbar in Queue/Form-Tabelle.
- **P5 — Eigene Kanäle scharf.** Nur wenn Plattform-API-Freigaben (TikTok Content Posting / IG Graph / YouTube OAuth) + Tokens vorhanden. DoD: *ein* Clip real auf *einem* eigenen Kanal gepostet + Live-Beweis. **Extern blockiert bis Freigaben stehen** — bis dahin liefern P1–P4 + Forms bereits Wert.

## 12. Risiken & Abhängigkeiten

- **Plattform-API-Freigaben** (P5): unklarer Stand, kann Wochen dauern → Sequenz stellt eigene Kanäle ans Ende.
- **Whisper-Kosten**: durch Aktivierungs- + Titel-Gate gedeckelt.
- **Forms-Blockrisiko** (pirate-Zweckentfremdung; Rate-Limits bei Massen-Submit) → Cooldown/pro-Clip-Toggle.
- **Rechte/Monetarisierung**: Bot gibt Owner-Erklärung ab; im Dashboard transparent machen.
- **Dormanter Code**: P0 zwingt einen echten Durchlauf, bevor erweitert wird (Regel „Feature muss Endzustand erreichen").

## 13. Test-Strategie (TDD, Red→Green)

- `decide()`: Unit-Tests (leer / generisch / vokab-haltig / beschreibend).
- Forms-Payload-Builder: Unit-Tests (korrektes entry-Mapping je Form, pirate-Loose-Fill, Pflicht-MC-Werte exakt).
- `next_free_slot()`: Unit-Tests (Slot-Rollover über Tagesgrenze, TZ).
- Forms-Idempotenz: DB-Integrationstest (`UNIQUE`-Konflikt → skipped, nicht Fehler).
- Enrichment mit/ohne Transcriber: Verhaltenstest (Whisper nur bei TranscribeThenGenerate aufgerufen).

## 14. Delegation

- **Claude:** dieser Spec, P0-Ops, deutsche UI-Texte, Reviews der `changed_files`, Merge→Deploy→Live-Beweis, Changelog/Discord.
- **GPT-Worker** (`gpt-5.6-sol`, effort high): Rust-Impl P1–P4 (forms.rs, decide(), Whisper-Wiring, Scheduler, Endpoints, Migration, Tests). User-sichtbare Strings nur als `"Platzhalter"` + Datei/Zeile.
