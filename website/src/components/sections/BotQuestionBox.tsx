import { useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { Bot, Loader2, Send } from "lucide-react";

const ASK_URL = "/twitch/api/v2/self-explainer/ask";

const SUGGESTIONS = [
  "Was macht der Bot eigentlich?",
  "Ist das nicht Scam?",
  "Muss ich was einrichten?",
  "Wann raidet der Bot?",
];

interface AskResponse {
  answer?: string;
  parts?: string[];
  grounded?: boolean;
}

/**
 * Frage-Box auf /streamer: stellt eine Frage an den grounded Self-Explainer-Endpoint
 * und zeigt die (ggf. in Teile gesplittete) Antwort als Bubbles. Das Reasoning-Modell
 * darf sich Zeit lassen, daher ein klarer "denkt nach"-Zustand statt blanker Wartezeit.
 */
export function BotQuestionBox() {
  const [question, setQuestion] = useState("");
  const [parts, setParts] = useState<string[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function ask(raw: string) {
    const q = raw.trim();
    if (!q || loading) return;

    setLoading(true);
    setError(null);
    setParts([]);

    const controller = new AbortController();
    const timer = window.setTimeout(() => controller.abort(), 125_000);
    try {
      const res = await fetch(ASK_URL, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ question: q }),
        signal: controller.signal,
      });

      if (res.status === 429) {
        setError("Zu viele Fragen gerade — probier's gleich nochmal.");
        return;
      }
      if (!res.ok) {
        setError("Hat nicht geklappt — versuch's nochmal oder schau in die FAQ.");
        return;
      }

      const data: AskResponse = await res.json();
      const answerParts =
        data.parts && data.parts.length
          ? data.parts
          : data.answer
            ? [data.answer]
            : [];
      if (!answerParts.length) {
        setError("Keine Antwort erhalten — versuch's nochmal.");
        return;
      }
      setParts(answerParts);
    } catch {
      setError("Verbindung fehlgeschlagen — versuch's nochmal.");
    } finally {
      window.clearTimeout(timer);
      setLoading(false);
    }
  }

  return (
    <div className="panel-card rounded-[2rem] p-8 md:p-10">
      <div className="flex items-center gap-3">
        <div className="flex h-10 w-10 items-center justify-center rounded-full border border-accent/40 bg-accent/15">
          <Bot size={20} className="text-accent" />
        </div>
        <div>
          <p className="text-sm uppercase tracking-[0.16em] text-primary">Frag den Bot</p>
          <h2 className="text-2xl font-bold text-text-primary md:text-3xl">
            Unsicher? Frag direkt nach.
          </h2>
        </div>
      </div>

      <p className="mt-4 max-w-2xl text-base leading-relaxed text-text-secondary">
        Tipp deine Frage ein — die Antwort kommt nur aus dem, was der Bot wirklich macht.
        Ehrlich, kein Verkaufsgerede.
      </p>

      <form
        className="mt-6 flex flex-col gap-3 sm:flex-row"
        onSubmit={(e) => {
          e.preventDefault();
          ask(question);
        }}
      >
        <input
          type="text"
          value={question}
          onChange={(e) => setQuestion(e.target.value)}
          placeholder="z. B. Was macht der Bot und ist das Scam?"
          maxLength={500}
          disabled={loading}
          className="flex-1 rounded-xl border border-border bg-[rgba(44, 35, 24,0.76)] px-5 py-4 text-text-primary outline-none transition-colors placeholder:text-text-secondary/60 focus:border-accent disabled:opacity-60"
        />
        <button
          type="submit"
          disabled={loading || !question.trim()}
          className="gradient-accent inline-flex items-center justify-center gap-2 rounded-xl px-6 py-4 font-semibold transition-all duration-200 hover:brightness-110 disabled:cursor-not-allowed disabled:opacity-50"
        >
          {loading ? <Loader2 size={18} className="animate-spin" /> : <Send size={18} />}
          {loading ? "Denkt nach…" : "Fragen"}
        </button>
      </form>

      {!parts.length && !loading && !error && (
        <div className="mt-4 flex flex-wrap gap-2">
          {SUGGESTIONS.map((s) => (
            <button
              key={s}
              type="button"
              onClick={() => {
                setQuestion(s);
                ask(s);
              }}
              className="rounded-full border border-border px-4 py-1.5 text-sm text-text-secondary transition-colors hover:border-accent hover:text-accent"
            >
              {s}
            </button>
          ))}
        </div>
      )}

      <div className="mt-6">
        {loading && (
          <div className="flex items-center gap-3 text-text-secondary">
            <Loader2 size={18} className="animate-spin text-accent" />
            <span>Der Bot überlegt kurz…</span>
          </div>
        )}

        {error && !loading && (
          <p className="rounded-xl border border-danger/30 bg-danger-soft px-5 py-4 text-sm text-danger">
            {error}
          </p>
        )}

        <AnimatePresence>
          {!loading &&
            parts.map((part, i) => (
              <motion.div
                key={i}
                initial={{ opacity: 0, y: 10 }}
                animate={{ opacity: 1, y: 0 }}
                transition={{ delay: i * 0.15 }}
                className="mb-3 flex gap-3"
              >
                <div className="mt-1 flex h-8 w-8 shrink-0 items-center justify-center rounded-full border border-accent/40 bg-accent/15">
                  <Bot size={16} className="text-accent" />
                </div>
                <div className="rounded-2xl rounded-tl-sm border border-border bg-[rgba(44, 35, 24,0.76)] px-5 py-3 leading-relaxed text-text-primary">
                  {part}
                </div>
              </motion.div>
            ))}
        </AnimatePresence>
      </div>
    </div>
  );
}
