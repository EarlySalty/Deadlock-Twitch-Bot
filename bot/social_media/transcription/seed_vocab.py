"""Initial-Seed fuer das Deadlock-Vokabular.

Quellen:
- Deadlock API (https://assets.deadlock-api.com / https://api.deadlock-api.com)
  laedt Heroes, Items, Abilities und legt sie als `source='deadlock_api'` ab.
- Slang-Layer: kuratierte Liste an Twitch/Stream-Slang und Hero-Nicknames,
  abgelegt als `source='manual'`.

Aufruf:
    python -m bot.social_media.transcription.seed_vocab
"""

from __future__ import annotations

import argparse
import asyncio
import logging
from typing import Iterable, Sequence

from .vocab import VocabEntry, bulk_upsert_vocab_entries

log = logging.getLogger("TwitchStreams.SocialMedia.SeedVocab")

DEADLOCK_API_HEROES_URL = "https://assets.deadlock-api.com/v2/heroes"
DEADLOCK_API_ITEMS_URL = "https://assets.deadlock-api.com/v2/items"

# Manuell gepflegter Slang-Layer (Twitch + Deadlock-Spezifika)
SLANG_TERMS: tuple[tuple[str, str, tuple[str, ...]], ...] = (
    # term, canonical, aliases
    ("ult", "Ultimate", ("ulti", "ultimate", "ult")),
    ("ulti", "Ultimate", ("ult", "ultimate")),
    ("buyback", "Buyback", ("buy back", "rebuy")),
    ("souls", "Souls", ("soul", "soul orbs", "soul orb")),
    ("soul orb", "Soul Orb", ("soul orbs", "orb", "orbs")),
    ("lane phase", "Lane Phase", ("laning phase", "lane")),
    ("midboss", "Midboss", ("mid boss", "mid-boss", "patron mid")),
    ("patron", "Patron", ("base boss", "endgame boss")),
    ("walker", "Walker", ("walkers", "boss tier 2", "tier 2 boss")),
    ("guardian", "Guardian", ("guardians", "boss tier 1", "tier 1 boss")),
    ("rejuv", "Rejuvenator", ("rejuvenator", "rejuv buff")),
    ("zip", "Zipline", ("zipline", "zips", "ziplines")),
    ("flex slot", "Flex Slot", ("flexslot", "flex")),
    ("greens", "Weapon Items", ("green items", "green slot", "weapon items")),
    ("oranges", "Vitality Items", ("orange items", "orange slot", "vitality items")),
    ("purples", "Spirit Items", ("purple items", "purple slot", "spirit items")),
    ("gank", "Gank", ("ganking", "ganked")),
    ("rotate", "Rotate", ("rotation", "rotating")),
    ("farm", "Farm", ("farming", "farmed")),
    ("split", "Splitpush", ("split push", "splitpush")),
    ("teamfight", "Teamfight", ("team fight", "tf")),
    ("ace", "Ace", ("aced", "team kill")),
    ("clutch", "Clutch", ("clutched", "clutching")),
    ("oneshot", "Oneshot", ("one shot", "1shot")),
    ("burst", "Burst", ("burst damage")),
)


def _slang_entries() -> list[VocabEntry]:
    out: list[VocabEntry] = []
    for term, canonical, aliases in SLANG_TERMS:
        out.append(
            VocabEntry(
                term=term.strip().lower(),
                canonical=canonical,
                category="slang",
                source="manual",
                aliases=aliases,
                weight=2,
            )
        )
    return out


def _build_heroes_entries(heroes_payload: Iterable[dict]) -> list[VocabEntry]:
    """Map Deadlock API heroes -> VocabEntry list (heroes + abilities)."""
    out: list[VocabEntry] = []
    for hero in heroes_payload or []:
        hero_name = (
            hero.get("name")
            or hero.get("display_name")
            or hero.get("english_name")
            or hero.get("internal_name")
        )
        if not hero_name:
            continue
        canonical = str(hero_name).strip()
        if not canonical:
            continue

        aliases = []
        for alias_key in ("internal_name", "english_name", "short_name", "alt_name"):
            alias = hero.get(alias_key)
            if alias and str(alias).strip() and str(alias).strip().lower() != canonical.lower():
                aliases.append(str(alias).strip())

        out.append(
            VocabEntry(
                term=canonical.lower(),
                canonical=canonical,
                category="hero",
                source="deadlock_api",
                aliases=tuple(aliases),
                weight=5,
            )
        )

        for ability in hero.get("abilities") or []:
            ability_name = (
                ability.get("name")
                or ability.get("display_name")
                or ability.get("english_name")
            )
            if not ability_name:
                continue
            canonical_ability = str(ability_name).strip()
            if not canonical_ability:
                continue
            ability_aliases: list[str] = []
            for key in ("internal_name", "english_name"):
                alias = ability.get(key)
                if alias and str(alias).strip().lower() != canonical_ability.lower():
                    ability_aliases.append(str(alias).strip())
            out.append(
                VocabEntry(
                    term=canonical_ability.lower(),
                    canonical=canonical_ability,
                    category="ability",
                    source="deadlock_api",
                    aliases=tuple(ability_aliases),
                    weight=3,
                )
            )
    return out


def _build_items_entries(items_payload: Iterable[dict]) -> list[VocabEntry]:
    out: list[VocabEntry] = []
    for item in items_payload or []:
        item_name = (
            item.get("name")
            or item.get("display_name")
            or item.get("english_name")
        )
        if not item_name:
            continue
        canonical = str(item_name).strip()
        if not canonical:
            continue
        aliases: list[str] = []
        for key in ("internal_name", "english_name", "short_name"):
            alias = item.get(key)
            if alias and str(alias).strip().lower() != canonical.lower():
                aliases.append(str(alias).strip())
        out.append(
            VocabEntry(
                term=canonical.lower(),
                canonical=canonical,
                category="item",
                source="deadlock_api",
                aliases=tuple(aliases),
                weight=4,
            )
        )
    return out


async def _fetch_json(session, url: str) -> list[dict]:
    try:
        async with session.get(url, timeout=20) as resp:
            if resp.status != 200:
                log.warning("Deadlock-API %s -> HTTP %s", url, resp.status)
                return []
            data = await resp.json()
    except Exception:
        log.exception("Deadlock-API request failed: %s", url)
        return []
    if isinstance(data, list):
        return [d for d in data if isinstance(d, dict)]
    if isinstance(data, dict):
        for key in ("data", "items", "heroes"):
            value = data.get(key)
            if isinstance(value, list):
                return [d for d in value if isinstance(d, dict)]
    return []


async def fetch_deadlock_vocab() -> list[VocabEntry]:
    """Lade Heroes/Abilities + Items aus der oeffentlichen Deadlock API."""
    try:
        import aiohttp
    except ImportError:
        log.warning("aiohttp nicht installiert, ueberspringe Deadlock-API-Seed")
        return []

    async with aiohttp.ClientSession() as session:
        heroes, items = await asyncio.gather(
            _fetch_json(session, DEADLOCK_API_HEROES_URL),
            _fetch_json(session, DEADLOCK_API_ITEMS_URL),
        )

    return [*_build_heroes_entries(heroes), *_build_items_entries(items)]


async def seed_vocab(*, include_slang: bool = True, include_api: bool = True) -> dict[str, int]:
    """Synchronisiere Vokabular. Gibt {written, skipped} zurueck."""
    entries: list[VocabEntry] = []
    if include_slang:
        entries.extend(_slang_entries())
    if include_api:
        entries.extend(await fetch_deadlock_vocab())

    written, skipped = bulk_upsert_vocab_entries(entries)
    log.info("Vocab-Seed: %s geschrieben, %s uebersprungen", written, skipped)
    return {"written": written, "skipped": skipped}


def seed_vocab_sync(*, include_slang: bool = True, include_api: bool = True) -> dict[str, int]:
    """Sync-Wrapper fuer `seed_vocab` (z.B. aus Admin-API)."""
    try:
        loop = asyncio.get_running_loop()
    except RuntimeError:
        loop = None
    if loop and loop.is_running():
        # Wir laufen schon in einem Loop -> Fallback: nur Slang-Layer (synchron).
        # Der Caller soll dann die async Variante benutzen.
        from concurrent.futures import ThreadPoolExecutor

        with ThreadPoolExecutor(max_workers=1) as pool:
            future = pool.submit(
                lambda: asyncio.run(
                    seed_vocab(include_slang=include_slang, include_api=include_api)
                )
            )
            return future.result()
    return asyncio.run(seed_vocab(include_slang=include_slang, include_api=include_api))


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Seed Deadlock-Vokabular in die DB.")
    parser.add_argument(
        "--no-api",
        action="store_true",
        help="Ueberspringe Deadlock-API-Aufruf (nur Slang-Layer)",
    )
    parser.add_argument(
        "--no-slang",
        action="store_true",
        help="Ueberspringe Slang-Layer (nur Deadlock-API)",
    )
    return parser.parse_args()


def main() -> int:
    logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(name)s %(message)s")
    args = parse_args()

    # Standalone-CLI braucht initialisiertes PG-Storage.
    try:
        from ...storage import pg as _storage_pg

        _storage_pg.prepare_runtime_storage()
    except Exception:
        log.exception("prepare_runtime_storage fehlgeschlagen; weiter mit best-effort")

    result = asyncio.run(
        seed_vocab(
            include_slang=not args.no_slang,
            include_api=not args.no_api,
        )
    )
    print(f"Seed abgeschlossen: written={result['written']} skipped={result['skipped']}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
