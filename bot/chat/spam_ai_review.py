"""Auto-Improvement für den Spam-Filter via MiniMax M3.

Wenn der bestehende Spam-Filter ein Signal erkennt (spam_score > 0) aber der
Ban-Threshold nicht erreicht wird, fragt MiniMax asynchron nach ob es echter
Viewer-Bot/SMM-Spam ist. Bei Bestätigung wird das Kernmuster in der DB
gespeichert und fließt künftig direkt in den Spam-Score ein.

Nur Nachrichten mit vorhandenen Filter-Treffern werden geprüft — keine
allgemeine URL- oder Keyword-Erkennung.
"""

from __future__ import annotations

import json
import logging
import re
import time
from datetime import UTC, datetime

log = logging.getLogger("TwitchStreams.SpamAI")

# Cooldown pro (channel, chatter_login) — verhindert doppelte AI-Calls.
_REVIEW_COOLDOWN: dict[tuple[str, str], float] = {}
_REVIEW_COOLDOWN_SEC = 300.0

# Domain-Pattern: erkenne Viewer-Service-Adressen in verdächtigen Nachrichten.
# Nur .ru/.online/.xyz/.site — die TLDs die in den Logs ausschließlich für Spam genutzt werden.
_SPAM_DOMAIN_RE = re.compile(
    r"\b\S+\.(?:ru|online|xyz|site)\b"         # Spam-typische TLDs
    r"|(?:\bstreamboo\b|\bsmmbest\b|\bsmmhype\b|\bsmmtop\b|\bprmxy\b|\bprmup\b)",
    re.IGNORECASE,
)

# Pattern-Cache (aus DB, TTL=2min)
_PATTERN_CACHE: list[tuple[str, str]] | None = None
_PATTERN_CACHE_TS: float = 0.0
_PATTERN_CACHE_TTL = 120.0

# Safe-Pattern-Cache (AI-gelernte False-Positive-Whitelist, gleiche TTL)
_SAFE_CACHE: list[str] | None = None
_SAFE_CACHE_TS: float = 0.0

_SCHEMA_ENSURED = False
_SAFE_SCHEMA_ENSURED = False

_SPAM_REVIEW_SYSTEM_PROMPT = (
    "Du bist ein Spam-Erkennungs-Assistent speziell für Twitch Viewer-Bot-Spam und SMM-Dienste.\n"
    "\n"
    "Die Nachricht wurde bereits von einem regelbasierten Filter als VERDÄCHTIG markiert "
    "(hatte Teilübereinstimmungen mit bekannten Spam-Mustern). Deine Aufgabe: bestätige oder "
    "widerlege ob es sich um Werbung für Viewer-Bot-Services, SMM-Dienste oder ähnliche "
    "Twitch-Manipulation handelt.\n"
    "\n"
    "Antworte NUR mit einem JSON-Objekt, ohne Markdown, ohne <think>-Block:\n"
    '{"is_spam": true/false, '
    '"pattern": "Kernmuster oder null", '
    '"pattern_type": "phrase" oder "fragment", '
    '"reason": "Begründung max 80 Zeichen"}\n'
    "\n"
    "Bei is_spam=true: pattern = kürzestes eindeutiges Spam-Kernmuster "
    "(Domain/Service-Name/Phrase).\n"
    "Bei is_spam=false: pattern = das harmlose Schlüsselwort/die Wendung, die den "
    "Fehlalarm ausgelöst hat und künftig NICHT mehr verdächtig sein soll "
    "(z.B. 'best viewers', 'cheap viewers'), oder null wenn nicht eindeutig.\n"
    "\n"
    "is_spam=true NUR bei: Viewer-Kauf, Bot-Views, Bot-Follower, SMM-Services, "
    "neue/abgewandelte Schreibweisen bekannter Spam-Dienste (Leerzeichen in Domains, "
    "Sonderzeichen, leicht veränderte Namen).\n"
    "is_spam=false bei allem anderen — normale Chat-Nachrichten, Komplimente an den "
    "Streamer ('best viewers'), normale URLs, Selbstpromotion, Community-Werbung.\n"
    "Im Zweifel: is_spam=false."
)


def _should_review_now(channel: str, chatter_login: str) -> bool:
    key = (channel, (chatter_login or "").lower())
    now = time.monotonic()
    if now - _REVIEW_COOLDOWN.get(key, 0.0) < _REVIEW_COOLDOWN_SEC:
        return False
    _REVIEW_COOLDOWN[key] = now
    if len(_REVIEW_COOLDOWN) > 2048:
        stale = [k for k, ts in _REVIEW_COOLDOWN.items() if now - ts > _REVIEW_COOLDOWN_SEC * 4]
        for k in stale:
            _REVIEW_COOLDOWN.pop(k, None)
    return True


def _invalidate_pattern_cache() -> None:
    global _PATTERN_CACHE, _PATTERN_CACHE_TS
    _PATTERN_CACHE = None
    _PATTERN_CACHE_TS = 0.0


def _invalidate_safe_cache() -> None:
    global _SAFE_CACHE, _SAFE_CACHE_TS
    _SAFE_CACHE = None
    _SAFE_CACHE_TS = 0.0


def load_learned_patterns() -> list[tuple[str, str]]:
    """Gibt gelernte [(pattern, pattern_type)] aus DB zurück (gecacht)."""
    global _PATTERN_CACHE, _PATTERN_CACHE_TS
    now = time.monotonic()
    if _PATTERN_CACHE is not None and (now - _PATTERN_CACHE_TS) < _PATTERN_CACHE_TTL:
        return _PATTERN_CACHE

    try:
        from ..storage import readonly_connection

        with readonly_connection() as conn:
            rows = conn.execute(
                "SELECT pattern, pattern_type FROM twitch_auto_learned_spam_patterns ORDER BY created_at"
            ).fetchall()
        result = [(str(r[0]), str(r[1])) for r in (rows or [])]
        _PATTERN_CACHE = result
        _PATTERN_CACHE_TS = now
        return result
    except Exception:
        return _PATTERN_CACHE or []


def load_safe_patterns() -> list[str]:
    """Gibt AI-gelernte Safe-Muster (False-Positive-Whitelist) aus DB zurück."""
    global _SAFE_CACHE, _SAFE_CACHE_TS
    now = time.monotonic()
    if _SAFE_CACHE is not None and (now - _SAFE_CACHE_TS) < _PATTERN_CACHE_TTL:
        return _SAFE_CACHE

    try:
        from ..storage import readonly_connection

        with readonly_connection() as conn:
            rows = conn.execute(
                "SELECT pattern FROM twitch_auto_learned_safe_patterns ORDER BY created_at"
            ).fetchall()
        result = [str(r[0]) for r in (rows or [])]
        _SAFE_CACHE = result
        _SAFE_CACHE_TS = now
        return result
    except Exception:
        return _SAFE_CACHE or []


async def _call_minimax(content: str) -> dict | None:
    try:
        from ..core.llm_providers import LLMProviderBootstrapError, get_minimax_client

        client = get_minimax_client(timeout=15.0, async_client=True)
    except Exception as exc:
        log.debug("MiniMax-Client nicht verfügbar: %s", exc)
        return None

    messages = [
        {"role": "system", "content": _SPAM_REVIEW_SYSTEM_PROMPT},
        {"role": "user", "content": f"Nachricht: {content[:500]}"},
    ]
    try:
        response = await client.chat.completions.create(
            model="MiniMax-M3",
            messages=messages,
            max_tokens=200,
            temperature=0.0,
        )
        # Verbrauch ins gemeinsame MiniMax-Ledger (best-effort, wirft nie).
        try:
            import sys, os as _os
            _d = _os.path.expanduser("~/Documents/.claude/minimax-usage")
            if _d not in sys.path:
                sys.path.insert(0, _d)
            import minimax_usage as _mmu
            _usage = getattr(response, "usage", None)
            _mmu.record(source="twitch-bot",
                        tokens_in=int(getattr(_usage, "prompt_tokens", 0) or 0),
                        tokens_out=int(getattr(_usage, "completion_tokens", 0) or 0),
                        model="MiniMax-M3", purpose="spam-review", success=True)
        except Exception:
            pass
        raw = (response.choices[0].message.content or "").strip() if response.choices else ""
        raw = re.sub(r"<think>.*?</think>", "", raw, flags=re.DOTALL | re.IGNORECASE).strip()
        m = re.search(r"\{.*\}", raw, re.DOTALL)
        if not m:
            log.debug("Spam-AI: kein JSON in Antwort: %.200s", raw)
            return None
        return json.loads(m.group())
    except json.JSONDecodeError:
        log.debug("Spam-AI: JSON-Parse-Fehler")
        return None
    except Exception as exc:
        log.debug("Spam-AI MiniMax-Call fehlgeschlagen: %s", type(exc).__name__)
        return None


async def _save_pattern(
    pattern: str,
    pattern_type: str,
    source_message: str,
    source_channel: str,
    reasoning: str,
) -> None:
    global _SCHEMA_ENSURED
    try:
        from ..storage import transaction

        with transaction() as conn:
            if not _SCHEMA_ENSURED:
                conn.execute(
                    """
                    CREATE TABLE IF NOT EXISTS twitch_auto_learned_spam_patterns (
                        pattern TEXT PRIMARY KEY,
                        pattern_type TEXT NOT NULL DEFAULT 'fragment',
                        source_message TEXT,
                        source_channel TEXT,
                        minimax_reasoning TEXT,
                        hit_count INT NOT NULL DEFAULT 0,
                        created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
                    )
                    """
                )
                _SCHEMA_ENSURED = True
            conn.execute(
                """
                INSERT INTO twitch_auto_learned_spam_patterns
                    (pattern, pattern_type, source_message, source_channel, minimax_reasoning, created_at)
                VALUES (%s, %s, %s, %s, %s, %s)
                ON CONFLICT (pattern) DO UPDATE SET
                    hit_count = twitch_auto_learned_spam_patterns.hit_count + 1
                """,
                (
                    pattern.lower(),
                    pattern_type,
                    source_message[:500],
                    source_channel,
                    reasoning[:200],
                    datetime.now(UTC).isoformat(),
                ),
            )
        _invalidate_pattern_cache()
        log.info(
            "Spam-AI: neues Muster gelernt [%s] '%s' aus #%s",
            pattern_type,
            pattern,
            source_channel,
        )
    except Exception:
        log.debug("Spam-AI: Muster konnte nicht gespeichert werden", exc_info=True)


async def _save_safe_pattern(
    pattern: str,
    source_message: str,
    source_channel: str,
    reasoning: str,
) -> None:
    """Persistiert ein AI-bestätigtes Safe-Muster (False-Positive-Whitelist)."""
    global _SAFE_SCHEMA_ENSURED
    try:
        from ..storage import transaction

        with transaction() as conn:
            if not _SAFE_SCHEMA_ENSURED:
                conn.execute(
                    """
                    CREATE TABLE IF NOT EXISTS twitch_auto_learned_safe_patterns (
                        pattern TEXT PRIMARY KEY,
                        source_message TEXT,
                        source_channel TEXT,
                        minimax_reasoning TEXT,
                        hit_count INT NOT NULL DEFAULT 0,
                        created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
                    )
                    """
                )
                _SAFE_SCHEMA_ENSURED = True
            conn.execute(
                """
                INSERT INTO twitch_auto_learned_safe_patterns
                    (pattern, source_message, source_channel, minimax_reasoning, created_at)
                VALUES (%s, %s, %s, %s, %s)
                ON CONFLICT (pattern) DO UPDATE SET
                    hit_count = twitch_auto_learned_safe_patterns.hit_count + 1
                """,
                (
                    pattern.lower(),
                    source_message[:500],
                    source_channel,
                    reasoning[:200],
                    datetime.now(UTC).isoformat(),
                ),
            )
        _invalidate_safe_cache()
        log.info(
            "Spam-AI: neues SAFE-Muster gelernt '%s' aus #%s (False-Positive)",
            pattern,
            source_channel,
        )
    except Exception:
        log.debug("Spam-AI: Safe-Muster konnte nicht gespeichert werden", exc_info=True)


def _review_worthwhile(content: str, spam_reasons: list[str]) -> bool:
    """
    Prüft ob ein AI-Review sinnvoll ist.
    Gibt False zurück wenn der Verdacht ausschließlich auf dem sehr breiten
    'viewer + name'-Muster beruht und kein Domain-Signal vorliegt.
    """
    reasons = set(spam_reasons)
    # Phrase/Fragment/Learned-Treffer → immer prüfen
    if any(
        r.startswith("Phrase(") or r.startswith("Fragment(") or r.startswith("Learned-")
        for r in reasons
    ):
        return True
    # @unknown mention → prüfen
    if any("mention" in r for r in reasons):
        return True
    # viewer + name → nur wenn gleichzeitig Spam-Domain erkennbar
    if "Muster: viewer + name" in reasons:
        return bool(_SPAM_DOMAIN_RE.search(content or ""))
    return False


async def run_spam_ai_review(
    *,
    content: str,
    channel: str,
    chatter_login: str,
    spam_score: int,
    spam_reasons: list[str],
) -> None:
    """Fire-and-forget Entrypoint. Als asyncio.create_task() aufrufen."""
    if not _review_worthwhile(content, spam_reasons):
        return
    if not _should_review_now(channel, chatter_login):
        return

    result = await _call_minimax(content)
    if result is None:
        return

    is_spam = bool(result.get("is_spam"))
    pattern = (result.get("pattern") or "").strip().lower()
    pattern_type = str(result.get("pattern_type") or "fragment").strip().lower()
    reason = str(result.get("reason") or "").strip()

    if not is_spam:
        # False-Positive: das harmlose Auslöser-Muster in die Safe-Whitelist lernen,
        # damit ähnliche Nachrichten künftig negativ gescort werden (außer es kommt
        # zusätzlich ein hartes Spam-Signal hinzu — das hat im Scoring Vorrang).
        if pattern and len(pattern) >= 4:
            await _save_safe_pattern(
                pattern=pattern,
                source_message=content,
                source_channel=channel,
                reasoning=reason,
            )
        else:
            log.debug(
                "Spam-AI: kein Spam (score=%d, chatter=%s, channel=%s)",
                spam_score, chatter_login, channel,
            )
        return

    log.warning(
        "Spam-AI bestätigt: chatter=%s channel=%s score=%d pattern='%s' type=%s reason=%s",
        chatter_login,
        channel,
        spam_score,
        pattern,
        pattern_type,
        reason,
    )

    if pattern and len(pattern) >= 4:
        await _save_pattern(
            pattern=pattern,
            pattern_type=pattern_type if pattern_type in ("phrase", "fragment") else "fragment",
            source_message=content,
            source_channel=channel,
            reasoning=reason,
        )
