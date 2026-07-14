import { useEffect, useRef, useState } from "react";
import type { FormEvent } from "react";
import { ArrowLeft, BellRing, BookOpen, ExternalLink, Send } from "lucide-react";
import { DoormanBadge, KeyRackBackdrop } from "@/components/faq/LobbyArt";

/*
 * Der Empfang — die Seite hinter /twitch/faq.
 *
 * Sie fragt denselben Endpoint wie das kleine Chat-Widget der Website
 * (`self-explainer`): ein LLM, das strikt auf einen Bot-Steckbrief geerdet ist,
 * rate-limitiert und gegen Prompt-Injection gehaertet. Kennt es die Antwort
 * nicht, verweigert es sie — hier wird also NICHT im Frontend nachgeholfen,
 * kein Fallback-Text, kein Raten. Ein Portier, der nichts weiss, nennt die
 * naechste Tuer; er erfindet keine Zimmernummer.
 *
 * WELCHER HANDLER BEDIENT DAS?
 * `rust/crates/tb-dashboard-api/src/handlers/self_explainer.rs` — der Rust-Dienst
 * tb-dashboard (:8769) ist live. Er liefert `{answer, parts, grounded, sources}`;
 * `sources` wird dort aus dem Grounding befuellt und mitgesendet.
 * NICHT `bot/dashboard/routes_self_explainer.py`: das ist die ausgemusterte
 * Python-Vorlage des Ports, sie ist nirgends mehr eingebunden und laeuft nicht.
 * Wer den Antwort-Vertrag dieser Seite pruefen will, liest den Rust-Handler.
 */

const ASK_URL = "/twitch/api/v2/self-explainer/ask";
const DISCORD_URL = "https://discord.gg/z5TfVHuQq2";
const REQUEST_TIMEOUT_MS = 30_000;
const MAX_QUESTION_LENGTH = 500;

const SUGGESTIONS = [
  "Was macht der Bot eigentlich?",
  "Wie verbinde ich meinen Kanal?",
  "Wie richte ich das Stream-Overlay ein?",
  "Was kostet das, und was ist gratis?",
  "Welche Daten sammelt ihr über meinen Chat?",
];

interface AskResponse {
  answer?: string;
  parts?: string[];
  sources?: string[];
}

interface Entry {
  id: number;
  role: "guest" | "concierge";
  text: string;
  sources?: string[];
}

export function FaqDoormanPage() {
  const [question, setQuestion] = useState("");
  const [entries, setEntries] = useState<Entry[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [ringing, setRinging] = useState(false);

  const nextId = useRef(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const transcriptEnd = useRef<HTMLDivElement>(null);

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  useEffect(() => {
    transcriptEnd.current?.scrollIntoView({ behavior: "smooth", block: "end" });
  }, [entries, loading]);

  function addEntry(role: Entry["role"], text: string, sources?: string[]) {
    nextId.current += 1;
    const id = nextId.current;
    setEntries((current) => [...current, { id, role, text, sources }]);
  }

  async function ask(raw: string) {
    const text = raw.trim();
    if (!text || loading) return;

    if (text.length > MAX_QUESTION_LENGTH) {
      setError(
        `Das ist eine lange Frage. Fass sie bitte auf ${MAX_QUESTION_LENGTH} Zeichen zusammen, dann findet der Concierge die Stelle schneller.`,
      );
      return;
    }

    setError(null);
    setQuestion("");
    addEntry("guest", text);
    setLoading(true);
    setRinging(true);
    window.setTimeout(() => setRinging(false), 640);

    const controller = new AbortController();
    const timer = window.setTimeout(() => controller.abort(), REQUEST_TIMEOUT_MS);

    try {
      const response = await fetch(ASK_URL, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ question: text }),
        signal: controller.signal,
      });

      if (response.status === 429) {
        setError("Der Concierge kommt gerade nicht hinterher. Gib ihm einen Moment.");
        return;
      }
      if (!response.ok) {
        setError("Die Rezeption ist im Moment nicht besetzt. Versuch es gleich noch einmal.");
        return;
      }

      const data: AskResponse = await response.json();
      const parts = data.parts?.length ? data.parts : data.answer ? [data.answer] : [];
      const sources = Array.isArray(data.sources) ? data.sources : [];

      /* Leere Antwort ist ein Fehler, kein Gespraechsbeitrag. Wer hier still
         eine freundliche Platzhalterzeile einsetzt, verkauft ein Nichtwissen
         als Auskunft. */
      if (!parts.length) {
        setError("Der Concierge hat dazu nichts herausgegeben. Frag ihn gern anders herum.");
        return;
      }

      parts.forEach((part, index) => {
        addEntry("concierge", part, index === parts.length - 1 ? sources : undefined);
      });
    } catch {
      setError("Die Leitung zum Empfang ist abgerissen. Versuch es noch einmal.");
    } finally {
      window.clearTimeout(timer);
      setLoading(false);
    }
  }

  function submit(event: FormEvent) {
    event.preventDefault();
    void ask(question);
  }

  const empty = entries.length === 0;

  return (
    <main className="lobby">
      {/* Die Wand hinter dem Empfang. Reine Kulisse — sie liegt hinter allem und
          faengt keine Klicks ab. */}
      <KeyRackBackdrop className="key-rack" />

      {/* Kein min-h-screen + flex-1 hier: das streckte die Halle immer auf volle
          Fensterhoehe und riss zwischen der letzten Antwort und dem Eingabefeld
          ein totes Loch auf. Die Halle selbst (.lobby) haelt den dunklen Grund
          ueber den ganzen Viewport — der Inhalt darf kurz sein. */}
      <div className="relative z-10 mx-auto flex max-w-3xl flex-col px-4 py-8 md:py-12">
        <a
          href="/twitch/dashboard"
          className="mb-6 inline-flex w-fit items-center gap-2 text-sm text-[#b7aa91] no-underline transition-colors hover:text-[#efd49d]"
        >
          <ArrowLeft className="h-4 w-4" />
          Zurück zum Dashboard
        </a>

        {/* Der Tresen */}
        <header className="counter px-6 py-6 md:px-8 md:py-7 md:pr-52">
          {/* Der Portier selbst — das Original-Artwork, freigestellt. Er steht
              hinter dem Tresen und ragt ueber die Kante. */}
          <img
            src="/streamer/brand/doorman/concierge-key.png"
            alt=""
            aria-hidden="true"
            className="concierge-portrait hidden md:block"
          />
          <div className="flex items-start gap-5">
            <div className="min-w-0 flex-1">
              <div className="flex items-center gap-2.5">
                <p className="engraved font-display text-[11px] font-semibold uppercase md:text-xs">
                  Empfang
                </p>
                <BellRing
                  aria-hidden="true"
                  className={`bell h-4 w-4 shrink-0 ${ringing ? "bell-ringing" : ""}`}
                />
              </div>
              <h1 className="mt-1 font-display text-2xl font-semibold text-[#ece0c8] md:text-3xl">
                Frag den Concierge
              </h1>
              <p className="mt-2 text-sm leading-relaxed text-[#b7aa91]">
                Er kennt den Twitch-Bot: Einrichtung, Chat-Befehle, Auto-Raid, Overlay, Pläne,
                Datenschutz. Was nicht in der Hausakte steht, erfindet er nicht, sondern schickt
                dich weiter.
              </p>
            </div>
          </div>
        </header>

        {/* Das Gespräch */}
        <section
          aria-live="polite"
          aria-label="Gespräch mit dem Concierge"
          className="flex flex-col gap-4 py-8"
        >
          {empty ? (
            <div className="flex flex-col gap-4">
              {/* Lobby-Kunst: haengt nur, solange niemand am Tresen spricht */}
              <figure className="painting mx-auto mb-2 w-full max-w-sm">
                <img
                  src="/streamer/brand/doorman/doorman-tuer.webp"
                  alt="Gemälde: der Doorman vor der goldenen Tür"
                  width="640"
                  height="418"
                />
                <figcaption className="plaque">Der Portier · stets im Dienst</figcaption>
              </figure>
              <p className="text-sm text-[#b7aa91]">Womit fangen wir an?</p>
              <div className="flex flex-wrap gap-2">
                {SUGGESTIONS.map((suggestion) => (
                  <button
                    key={suggestion}
                    type="button"
                    onClick={() => void ask(suggestion)}
                    className="bell-button px-3.5 py-2 text-sm font-medium"
                  >
                    {suggestion}
                  </button>
                ))}
              </div>
            </div>
          ) : null}

          {entries.map((entry) =>
            entry.role === "guest" ? (
              <div key={entry.id} className="flex justify-end">
                <p className="guest-note max-w-[85%] px-4 py-2.5 text-sm leading-relaxed">
                  {entry.text}
                </p>
              </div>
            ) : (
              <div key={entry.id} className="flex items-start gap-3">
                <DoormanBadge className="mt-1 h-10 w-10 shrink-0" />
                <article className="ledger-sheet min-w-0 flex-1 px-5 py-4 md:px-6 md:py-5">
                  <p className="whitespace-pre-wrap text-[0.94rem] leading-relaxed">{entry.text}</p>

                  {entry.sources?.length ? (
                    <div className="ledger-rule mt-4 pt-3">
                      <p className="mb-2 flex items-center gap-1.5 text-[0.68rem] font-semibold uppercase tracking-[0.14em] text-[color:var(--ink-soft)]">
                        <BookOpen aria-hidden="true" className="h-3.5 w-3.5" />
                        Nachgeschlagen in
                      </p>
                      <ul className="flex list-none flex-wrap gap-1.5 p-0">
                        {entry.sources.map((source) => (
                          <li key={source} className="source-stamp">
                            {source}
                          </li>
                        ))}
                      </ul>
                    </div>
                  ) : null}
                </article>
              </div>
            ),
          )}

          {loading ? (
            <p className="flex items-center gap-2 pl-12 text-sm text-[#b7aa91]">
              Der Concierge blättert im Hausbuch
              <span aria-hidden="true" className="inline-flex gap-1">
                <span className="thinking-dot inline-block h-1.5 w-1.5 rounded-full bg-[#c8a86b]" />
                <span className="thinking-dot inline-block h-1.5 w-1.5 rounded-full bg-[#c8a86b]" />
                <span className="thinking-dot inline-block h-1.5 w-1.5 rounded-full bg-[#c8a86b]" />
              </span>
            </p>
          ) : null}

          {error ? (
            <div
              role="status"
              className="rounded-lg border border-[rgba(221,106,77,0.45)] bg-[rgba(221,106,77,0.09)] px-4 py-3 text-sm text-[#f0c4b6]"
            >
              <p>{error}</p>
              <a
                href={DISCORD_URL}
                target="_blank"
                rel="noreferrer noopener"
                className="mt-1.5 inline-flex items-center gap-1.5 text-[#efd49d] underline underline-offset-2"
              >
                Direkt im Discord fragen
                <ExternalLink className="h-3.5 w-3.5" />
              </a>
            </div>
          ) : null}

          <div ref={transcriptEnd} />
        </section>

        {/* Die Theke, an der man spricht */}
        <form onSubmit={submit} className="sticky bottom-4 z-20 flex gap-2">
          <label htmlFor="question" className="sr-only">
            Deine Frage an den Concierge
          </label>
          <input
            id="question"
            ref={inputRef}
            value={question}
            onChange={(event) => setQuestion(event.target.value)}
            placeholder="Frag mich etwas über den Bot…"
            maxLength={MAX_QUESTION_LENGTH}
            autoComplete="off"
            className="speak-field w-full rounded-xl px-4 py-3 text-sm backdrop-blur"
          />
          <button
            type="submit"
            disabled={loading || !question.trim()}
            className="brass-action inline-flex shrink-0 items-center gap-2 rounded-xl px-4 py-3 text-sm font-bold"
          >
            <Send aria-hidden="true" className="h-4 w-4" />
            <span className="sr-only md:not-sr-only">Fragen</span>
          </button>
        </form>

        <footer className="pt-6 text-center text-xs leading-relaxed text-[color:rgba(183,170,145,0.75)]">
          <img
            src="/streamer/brand/doorman/deco-key.svg"
            alt=""
            aria-hidden="true"
            className="key-divider"
          />
          Der Concierge antwortet nur aus der Hausakte des Bots. Persönliche Anliegen, Beschwerden
          und alles, was er nicht weiß, gehören{" "}
          <a
            href={DISCORD_URL}
            target="_blank"
            rel="noreferrer noopener"
            className="text-[#c8a86b] underline underline-offset-2"
          >
            in den Discord
          </a>
          .
        </footer>
      </div>
    </main>
  );
}
