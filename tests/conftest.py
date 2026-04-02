from __future__ import annotations

import contextlib
import os
from unittest.mock import patch

import pytest

_ANALYTICS_DSN_AVAILABLE = bool(os.getenv("TWITCH_ANALYTICS_DSN"))
_KEYRING_PROBE_SERVICE_NAME = "keyring-availability-check"
_KEYRING_PROBE_USERNAME = "backend-check"

try:
    import keyring as _keyring_mod
    _keyring_mod.get_password(_KEYRING_PROBE_SERVICE_NAME, _KEYRING_PROBE_USERNAME)
    _KEYRING_AVAILABLE = True
except Exception:
    _KEYRING_AVAILABLE = False

_STUB_FINGERPRINT = {
    "fingerprint": "ci-stub",
    "hostHash": "ci-h",
    "databaseHash": "ci-db",
    "portHash": "ci-p",
    "engine": "postgres",
}


@pytest.fixture(autouse=True)
def _ci_environment_stubs():
    """
    Stub environment-dependent singletons when running without full CI credentials.

    - When TWITCH_ANALYTICS_DSN is absent: stub analytics DB fingerprint functions so
      tests that build the internal API / dashboard service app can run without a live DB.
    - When no keyring backend is available: stub keyring.get_password to return None so
      tests that indirectly call keyring (e.g. via SocialMediaCredentialManager) don't
      raise NoKeyringError.
    """
    patches: list = []

    if not _ANALYTICS_DSN_AVAILABLE:
        patches += [
            patch(
                "bot.internal_api.app.analytics_db_fingerprint_details",
                return_value=_STUB_FINGERPRINT,
            ),
            patch(
                "bot.dashboard_service.app.analytics_db_fingerprint_details",
                return_value=_STUB_FINGERPRINT,
            ),
            patch(
                "bot.storage.pg.analytics_db_fingerprint",
                return_value="ci-stub",
            ),
            patch(
                "bot.storage.pg.analytics_db_fingerprint_details",
                return_value=_STUB_FINGERPRINT,
            ),
            # _affiliate_register_routes calls storage.transaction() at app-build
            # time to run schema migrations.  Without a DSN, stub the whole
            # method so that build_v2_app() succeeds in unit tests.
            patch(
                "bot.dashboard.affiliate.affiliate_mixin"
                "._DashboardAffiliateMixin._affiliate_register_routes",
                return_value=None,
            ),
        ]

    if not _KEYRING_AVAILABLE:
        # keyring.get_password in field_crypto is imported inside a function,
        # so we stub get_crypto itself to avoid the keyring call entirely.
        patches.append(
            patch(
                "bot.compat.field_crypto.get_crypto",
                return_value=None,
            )
        )
        patches.append(
            patch(
                "bot.social_media.credential_manager.get_crypto",
                return_value=None,
            )
        )

    if not patches:
        yield
        return

    with contextlib.ExitStack() as stack:
        for p in patches:
            stack.enter_context(p)
        yield
