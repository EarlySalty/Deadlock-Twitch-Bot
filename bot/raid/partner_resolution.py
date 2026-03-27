from __future__ import annotations

from collections.abc import Mapping
from dataclasses import dataclass
from typing import Any, Protocol


class PartnerLookup(Protocol):
    def __call__(
        self,
        *,
        twitch_user_id: str | None = None,
        twitch_login: str | None = None,
    ) -> Any: ...


class KnownStreamerLookup(Protocol):
    def __call__(
        self,
        *,
        broadcaster_id: str | None = None,
        broadcaster_login: str | None = None,
    ) -> Any: ...


@dataclass(slots=True, frozen=True)
class PartnerRaidArrivalResolution:
    classification: str | None
    source_resolution: str
    target_is_partner: bool
    from_broadcaster_id: str | None
    from_broadcaster_login: str
    to_broadcaster_id: str
    to_broadcaster_login: str

    def as_tuple(self) -> tuple[str | None, str]:
        return self.classification, self.source_resolution


def normalize_broadcaster_login(raw_value: str | None) -> str:
    return str(raw_value or "").strip().lower()


def is_partner_target_channel(
    *,
    broadcaster_id: str | None,
    broadcaster_login: str | None,
    partner_lookup: PartnerLookup,
) -> bool:
    broadcaster_key = str(broadcaster_id or "").strip()
    login_key = normalize_broadcaster_login(broadcaster_login)
    if not broadcaster_key and not login_key:
        return False
    row = partner_lookup(
        twitch_user_id=broadcaster_key or None,
        twitch_login=login_key or None,
    )
    return bool(row)


def _identity_value(identity: object, *field_names: str) -> str:
    if identity is None:
        return ""
    if isinstance(identity, Mapping):
        for field_name in field_names:
            value = identity.get(field_name)
            if value is None:
                continue
            text = str(value).strip()
            if text:
                return text
        return ""
    for field_name in field_names:
        value = getattr(identity, field_name, None)
        if value is None:
            continue
        text = str(value).strip()
        if text:
            return text
    return ""


def classify_partner_raid_arrival(
    *,
    from_broadcaster_login: str | None,
    from_broadcaster_id: str | None,
    to_broadcaster_id: str | None,
    to_broadcaster_login: str | None,
    partner_lookup: PartnerLookup,
    known_streamer_lookup: KnownStreamerLookup,
) -> PartnerRaidArrivalResolution:
    normalized_from_login = normalize_broadcaster_login(from_broadcaster_login)
    normalized_to_login = normalize_broadcaster_login(to_broadcaster_login)
    from_broadcaster_key = str(from_broadcaster_id or "").strip()
    to_broadcaster_key = str(to_broadcaster_id or "").strip()

    target_is_partner = is_partner_target_channel(
        broadcaster_id=to_broadcaster_key,
        broadcaster_login=normalized_to_login,
        partner_lookup=partner_lookup,
    )
    if not target_is_partner:
        return PartnerRaidArrivalResolution(
            classification=None,
            source_resolution="non_partner_target",
            target_is_partner=False,
            from_broadcaster_id=from_broadcaster_key or None,
            from_broadcaster_login=normalized_from_login,
            to_broadcaster_id=to_broadcaster_key,
            to_broadcaster_login=normalized_to_login,
        )

    known_source = known_streamer_lookup(
        broadcaster_id=from_broadcaster_key or None,
        broadcaster_login=normalized_from_login or None,
    )
    if known_source:
        source_resolution = (
            "known_streamer_id"
            if _identity_value(known_source, "twitch_user_id", "user_id")
            else "known_streamer_login"
        )
        return PartnerRaidArrivalResolution(
            classification="ours_to_partner",
            source_resolution=source_resolution,
            target_is_partner=True,
            from_broadcaster_id=from_broadcaster_key or None,
            from_broadcaster_login=normalized_from_login,
            to_broadcaster_id=to_broadcaster_key,
            to_broadcaster_login=normalized_to_login,
        )

    if not normalized_from_login and not from_broadcaster_key:
        return PartnerRaidArrivalResolution(
            classification="unknown_source_to_partner",
            source_resolution="missing_source_identity",
            target_is_partner=True,
            from_broadcaster_id=None,
            from_broadcaster_login="",
            to_broadcaster_id=to_broadcaster_key,
            to_broadcaster_login=normalized_to_login,
        )

    return PartnerRaidArrivalResolution(
        classification="external_to_partner",
        source_resolution="unmatched_source",
        target_is_partner=True,
        from_broadcaster_id=from_broadcaster_key or None,
        from_broadcaster_login=normalized_from_login,
        to_broadcaster_id=to_broadcaster_key,
        to_broadcaster_login=normalized_to_login,
    )


__all__ = [
    "KnownStreamerLookup",
    "PartnerLookup",
    "PartnerRaidArrivalResolution",
    "classify_partner_raid_arrival",
    "is_partner_target_channel",
    "normalize_broadcaster_login",
]
