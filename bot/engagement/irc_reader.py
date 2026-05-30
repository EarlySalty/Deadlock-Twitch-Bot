"""Engagement-Chat-Quelle via Twitch IRC (für nicht-onboardete Kanäle).

Für Kanäle, die dem Bot kein `channel:bot` per EventSub freigegeben haben (z. B.
befreundete, einwilligende Streamer ohne Partner-Onboarding), gibt es keinen
EventSub-`channel.chat.message`-Stream. Dieser Reader joint solche Kanäle
stattdessen **anonym über IRC** (`justinfan`), liest die Chat-Nachrichten und
routet sie in dieselbe Engagement-Pipeline wie der EventSub-Pfad.

Trennung der Transporte:
- **Lesen**: anonymes IRC (`irc.chat.twitch.tv:6667`, CAP `tags`+`commands`).
- **Schreiben**: Helix über den Engagement-Sende-Account (`stealth_sender`).

Kanalquelle: `twitch_engagement_settings` mit `enabled = TRUE AND irc_read = TRUE`.
Nur Kanäle, die der Betreiber bewusst freischaltet (Consent des Kanal-Eigentümers
vorausgesetzt). Der Reader läuft als Background-Task ab `event_ready`.
"""

from __future__ import annotations

import asyncio
import logging
import re
import threading

from bot.core.chat_bots import is_known_chat_bot
from bot.storage.pg import query_all, transaction

from . import get_pipeline, sender_auth
from .pipeline import IncomingMessage
from .stealth_sender import send as _stealth_send

log = logging.getLogger("TwitchStreams.Engagement.IRCReader")

IRC_HOST = "irc.chat.twitch.tv"
IRC_PORT = 6667
_ANON_NICK = "justinfan13371337"

# PRIVMSG ohne Tags-Präfix: :nick!user@host PRIVMSG #channel :text
_PRIVMSG_RE = re.compile(r"^:(?P<login>[^!]+)!\S+ PRIVMSG #(?P<channel>\S+) :(?P<text>.*)$")


def ensure_schema() -> None:
    """Fügt die irc_read-Spalte lazy hinzu (kein Eingriff in den Settings-Flow)."""
    with transaction() as conn:
        conn.execute(
            "ALTER TABLE twitch_engagement_settings "
            "ADD COLUMN IF NOT EXISTS irc_read BOOLEAN NOT NULL DEFAULT FALSE"
        )


def _sync_load_irc_channels() -> list[str]:
    ensure_schema()
    rows = query_all(
        "SELECT channel_login FROM twitch_engagement_settings "
        "WHERE enabled = TRUE AND irc_read = TRUE"
    )
    return [str(r[0]).strip().lower() for r in rows if r and r[0]]


def _parse_tags(raw: str) -> dict[str, str]:
    out: dict[str, str] = {}
    for kv in raw.split(";"):
        key, sep, value = kv.partition("=")
        if sep:
            out[key] = value
    return out


class EngagementIrcReader:
    """Anonymer IRC-Reader, der Chat in die Engagement-Pipeline speist."""

    def __init__(self) -> None:
        self.reader: asyncio.StreamReader | None = None
        self.writer: asyncio.StreamWriter | None = None
        self.connected = False
        self.running = False
        self.channels: set[str] = set()
        self._self_login = sender_auth.SENDER_LOGIN.lower()
        self._conn_task: asyncio.Task | None = None
        self._read_task: asyncio.Task | None = None
        self._refresh_task: asyncio.Task | None = None

    async def start(self) -> None:
        if self.running:
            return
        self.channels = set(await asyncio.to_thread(_sync_load_irc_channels))
        if not self.channels:
            log.info("Engagement-IRC: keine irc_read-Kanäle konfiguriert, Reader bleibt aus")
            return
        self.running = True
        self._conn_task = asyncio.create_task(self._connection_loop(), name="engagement-irc-conn")
        self._refresh_task = asyncio.create_task(self._channel_refresh_loop(), name="engagement-irc-refresh")
        log.info("Engagement-IRC-Reader gestartet für %s", sorted(self.channels))

    async def stop(self) -> None:
        self.running = False
        for task in (self._refresh_task, self._read_task, self._conn_task):
            if task:
                task.cancel()
                try:
                    await task
                except (asyncio.CancelledError, Exception):
                    pass
        await self._disconnect()

    async def _connect(self) -> bool:
        try:
            self.reader, self.writer = await asyncio.open_connection(IRC_HOST, IRC_PORT)
            self.writer.write(f"NICK {_ANON_NICK}\r\n".encode())
            self.writer.write(b"CAP REQ :twitch.tv/tags twitch.tv/commands\r\n")
            await self.writer.drain()
            while True:
                line = await asyncio.wait_for(self.reader.readline(), timeout=10.0)
                if not line:
                    return False
                msg = line.decode("utf-8", errors="ignore").strip()
                if msg.startswith(":tmi.twitch.tv 001"):
                    self.connected = True
                    log.info("Engagement-IRC verbunden (anonym)")
                    return True
                if msg.startswith("PING"):
                    self.writer.write(msg.replace("PING", "PONG").encode() + b"\r\n")
                    await self.writer.drain()
        except Exception:
            log.exception("Engagement-IRC: Connect fehlgeschlagen")
            self.connected = False
            return False

    async def _disconnect(self) -> None:
        if self.writer:
            try:
                self.writer.close()
                await self.writer.wait_closed()
            except Exception:
                pass
        self.connected = False
        self.reader = None
        self.writer = None

    async def _join(self, channel: str) -> None:
        if self.writer:
            self.writer.write(f"JOIN #{channel}\r\n".encode())
            await self.writer.drain()

    async def _connection_loop(self) -> None:
        while self.running:
            try:
                if not self.connected:
                    if await self._connect():
                        for ch in list(self.channels):
                            await self._join(ch)
                        if self._read_task:
                            self._read_task.cancel()
                        self._read_task = asyncio.create_task(self._read_loop(), name="engagement-irc-read")
                    else:
                        await asyncio.sleep(30)
                else:
                    await asyncio.sleep(10)
            except asyncio.CancelledError:
                break
            except Exception:
                log.exception("Engagement-IRC: Connection-Loop-Fehler")
                await asyncio.sleep(30)

    async def _read_loop(self) -> None:
        try:
            while self.running and self.connected and self.reader:
                line = await self.reader.readline()
                if not line:
                    self.connected = False
                    break
                msg = line.decode("utf-8", errors="ignore").strip()
                await self._handle_line(msg)
        except asyncio.CancelledError:
            pass
        except Exception:
            log.exception("Engagement-IRC: Read-Loop-Fehler")
            self.connected = False

    async def _channel_refresh_loop(self) -> None:
        """Hält die Kanal-Joins aktuell, falls irc_read-Channels sich ändern."""
        while self.running:
            await asyncio.sleep(300)
            try:
                latest = set(await asyncio.to_thread(_sync_load_irc_channels))
                new = latest - self.channels
                for ch in new:
                    await self._join(ch)
                self.channels = latest
            except Exception:
                log.debug("Engagement-IRC: channel-refresh fehlgeschlagen", exc_info=True)

    async def _handle_line(self, msg: str) -> None:
        if not msg:
            return
        if msg.startswith("PING"):
            if self.writer:
                self.writer.write(msg.replace("PING", "PONG").encode() + b"\r\n")
                await self.writer.drain()
            return

        tags: dict[str, str] = {}
        rest = msg
        if msg.startswith("@"):
            tag_part, _, rest = msg[1:].partition(" ")
            tags = _parse_tags(tag_part)

        m = _PRIVMSG_RE.match(rest)
        if not m:
            return
        await self._process_privmsg(tags, m.group("login"), m.group("channel"), m.group("text"))

    async def _process_privmsg(self, tags: dict, login: str, channel: str, text: str) -> None:
        login = (login or "").strip().lower()
        channel = (channel or "").strip().lower()
        text = (text or "").strip()
        if not login or not channel or not text:
            return
        # Eigene Nachrichten und bekannte Bots ignorieren (keine Selbst-Antwort-Loops)
        if login == self._self_login or is_known_chat_bot(login):
            return

        room_id = (tags.get("room-id") or "").strip()
        user_id = (tags.get("user-id") or "").strip()
        msg_id = (tags.get("id") or "").strip() or None
        if not room_id or not user_id:
            return

        try:
            result = await get_pipeline().handle(
                IncomingMessage(
                    channel_login=channel,
                    twitch_user_id=user_id,
                    twitch_login=login,
                    content=text,
                    message_id=msg_id,
                )
            )
        except Exception:
            log.exception("Engagement-IRC: Pipeline-Fehler für #%s", channel)
            return

        if not result.response_text:
            return
        try:
            sent = await _stealth_send(room_id, result.response_text)
            if sent is None:
                log.info("Engagement-IRC: kein Sende-Account, Antwort für #%s verworfen", channel)
        except Exception:
            log.exception("Engagement-IRC: Stealth-Send für #%s fehlgeschlagen", channel)


_reader: EngagementIrcReader | None = None
_lock = threading.Lock()


def ensure_started() -> None:
    """Idempotent — startet den IRC-Reader einmal pro Prozess (aus laufendem Loop)."""
    global _reader
    if _reader is not None:
        return
    with _lock:
        if _reader is not None:
            return
        try:
            loop = asyncio.get_running_loop()
        except RuntimeError:
            log.debug("Engagement-IRC: kein running loop, skip")
            return
        _reader = EngagementIrcReader()
        loop.create_task(_reader.start(), name="engagement-irc-start")
