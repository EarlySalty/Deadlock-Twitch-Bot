import { useEffect, useRef, useState } from "react";
import type { FormEvent } from "react";
import { Bot, Loader2, MessageCircle, Send, X } from "lucide-react";

const ASK_URL = "/twitch/api/v2/self-explainer/ask";
const OPEN_EVENT = "ddc:open-support-chat";

const SUGGESTIONS = [
  "Was macht der Bot?",
  "Wie verbinde ich meinen Kanal?",
  "Ist das sicher?",
];

interface AskResponse {
  answer?: string;
  parts?: string[];
  sources?: string[];
}

interface ChatMessage {
  id: number;
  role: "user" | "bot";
  text: string;
  sources?: string[];
}

export function openSiteChatbot() {
  window.dispatchEvent(new Event(OPEN_EVENT));
}

export function SiteChatbot() {
  const [open, setOpen] = useState(false);
  const [question, setQuestion] = useState("");
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const messageId = useRef(0);

  useEffect(() => {
    const handleOpen = () => setOpen(true);
    window.addEventListener(OPEN_EVENT, handleOpen);
    return () => window.removeEventListener(OPEN_EVENT, handleOpen);
  }, []);

  useEffect(() => {
    if (!open) return;
    inputRef.current?.focus();

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") setOpen(false);
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [open]);

  function addMessage(role: ChatMessage["role"], text: string, sources?: string[]) {
    messageId.current += 1;
    setMessages((current) => [
      ...current,
      { id: messageId.current, role, text, sources },
    ]);
  }

  async function ask(raw: string) {
    const text = raw.trim();
    if (!text || loading) return;

    addMessage("user", text);
    setQuestion("");
    setLoading(true);
    setError(null);

    const controller = new AbortController();
    const timer = window.setTimeout(() => controller.abort(), 65000);

    try {
      const response = await fetch(ASK_URL, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ question: text }),
        signal: controller.signal,
      });

      if (response.status === 429) {
        setError("Zu viele Fragen gerade. Probier es gleich noch einmal.");
        return;
      }
      if (!response.ok) {
        setError("Die Antwort konnte nicht geladen werden.");
        return;
      }

      const data: AskResponse = await response.json();
      const answerParts =
        data.parts?.length ? data.parts : data.answer ? [data.answer] : [];
      const sources: string[] = Array.isArray(data.sources) ? data.sources : [];

      if (!answerParts.length) {
        setError("Der Bot hat keine Antwort geliefert.");
        return;
      }

      answerParts.forEach((part, index) => {
        addMessage("bot", part, index === answerParts.length - 1 ? sources : undefined);
      });
    } catch {
      setError("Verbindung fehlgeschlagen. Probier es noch einmal.");
    } finally {
      window.clearTimeout(timer);
      setLoading(false);
    }
  }

  function submit(event: FormEvent) {
    event.preventDefault();
    void ask(question);
  }

  return (
    <div className="fixed bottom-5 right-5 z-[100] sm:bottom-7 sm:right-7">
      {open && (
        <section
          role="dialog"
          aria-label="Hilfe zum Twitch-Bot"
          className="mb-3 flex h-[min(620px,calc(100vh-7rem))] w-[calc(100vw-2.5rem)] flex-col overflow-hidden rounded-2xl border border-border bg-[color:var(--theme-chatbot-bg,#091923f2)] shadow-2xl backdrop-blur-xl sm:w-[410px]"
        >
          <header className="flex items-center justify-between border-b border-border px-5 py-4">
            <div className="flex items-center gap-3">
              <span className="flex h-10 w-10 items-center justify-center rounded-full gradient-accent">
                <Bot size={20} className="text-white" />
              </span>
              <div>
                <p className="font-semibold text-text-primary">Bot-Hilfe</p>
                <p className="text-xs text-text-secondary">
                  Fragen zur Einrichtung und den Funktionen
                </p>
              </div>
            </div>
            <button
              type="button"
              onClick={() => setOpen(false)}
              aria-label="Chat schließen"
              className="rounded-lg p-2 text-text-secondary transition-colors hover:bg-white/10 hover:text-text-primary"
            >
              <X size={19} />
            </button>
          </header>

          <div
            className="flex-1 space-y-3 overflow-y-auto px-5 py-4"
            aria-live="polite"
          >
            {!messages.length && (
              <>
                <div className="max-w-[88%] rounded-2xl rounded-tl-sm border border-border bg-white/[0.05] px-4 py-3 text-sm leading-relaxed text-text-primary">
                  Frag mich, was der Twitch-Bot macht oder wie du deinen Kanal
                  verbindest. Die Einrichtung ist normalerweise mit einem Klick
                  auf „Autorisieren“ erledigt.
                </div>
                <div className="flex flex-wrap gap-2 pt-1">
                  {SUGGESTIONS.map((suggestion) => (
                    <button
                      key={suggestion}
                      type="button"
                      onClick={() => void ask(suggestion)}
                      className="rounded-full border border-border px-3 py-1.5 text-xs text-text-secondary transition-colors hover:border-accent hover:text-accent"
                    >
                      {suggestion}
                    </button>
                  ))}
                </div>
              </>
            )}

            {messages.map((message) => (
              <div
                key={message.id}
                className={`flex ${message.role === "user" ? "justify-end" : "justify-start"}`}
              >
                <div
                  className={`max-w-[88%] rounded-2xl px-4 py-3 text-sm leading-relaxed ${
                    message.role === "user"
                      ? "rounded-tr-sm gradient-accent text-white"
                      : "rounded-tl-sm border border-border bg-white/[0.05] text-text-primary"
                  }`}
                >
                  <p>{message.text}</p>
                  {message.sources?.length ? (
                    <p className="mt-2 border-t border-border/60 pt-2 text-xs text-text-secondary">
                      Quelle: {message.sources.join(", ")}
                    </p>
                  ) : null}
                </div>
              </div>
            ))}

            {loading && (
              <div className="flex items-center gap-2 text-sm text-text-secondary">
                <Loader2 size={16} className="animate-spin text-accent" />
                Antwort wird erstellt…
              </div>
            )}

            {error && (
              <p className="rounded-xl border border-red-500/30 bg-red-500/10 px-4 py-3 text-sm text-red-200">
                {error}
              </p>
            )}
          </div>

          <form onSubmit={submit} className="flex gap-2 border-t border-border p-4">
            <input
              ref={inputRef}
              value={question}
              onChange={(event) => setQuestion(event.target.value)}
              maxLength={500}
              disabled={loading}
              placeholder="Deine Frage …"
              aria-label="Frage an den Bot"
              className="min-w-0 flex-1 rounded-xl border border-border bg-white/[0.05] px-4 py-3 text-sm text-text-primary outline-none placeholder:text-text-secondary/60 focus:border-accent"
            />
            <button
              type="submit"
              disabled={loading || !question.trim()}
              aria-label="Frage senden"
              className="gradient-accent flex h-11 w-11 shrink-0 items-center justify-center rounded-xl text-white transition-opacity disabled:cursor-not-allowed disabled:opacity-50"
            >
              {loading ? <Loader2 size={18} className="animate-spin" /> : <Send size={18} />}
            </button>
          </form>
        </section>
      )}

      <button
        type="button"
        onClick={() => setOpen((current) => !current)}
        aria-label={open ? "Hilfe-Chat schließen" : "Hilfe bekommen"}
        aria-expanded={open}
        className="ml-auto flex items-center gap-2 rounded-full gradient-accent px-5 py-3.5 font-semibold text-white shadow-[0_12px_40px_rgba(6,182,212,0.3)] transition-transform hover:scale-[1.03]"
      >
        {open ? <X size={20} /> : <MessageCircle size={20} />}
        <span>{open ? "Schließen" : "Hilfe bekommen"}</span>
      </button>
    </div>
  );
}
