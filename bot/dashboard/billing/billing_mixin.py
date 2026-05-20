"""Billing helper methods used by dashboard route handlers."""

from __future__ import annotations

import asyncio
import html
import os
from datetime import UTC, datetime, timedelta
from typing import Any
from urllib.parse import urlsplit
from uuid import uuid4

from aiohttp import web

from ... import storage
from ...core.constants import log
from ...entitlements.catalog import (
    ANALYTICS_TRIAL_PLAN_ID as _ANALYTICS_TRIAL_PLAN_ID,
    TRIAL_DURATION_DAYS as _TRIAL_DURATION_DAYS,
)
from ...entitlements import repository as _entitlements_repository
from ...entitlements.resolver import (
    resolve_plan_snapshot_for_refs as _resolve_plan_snapshot_for_refs,
)
try:
    from ...raid.partner_scores import (
        refresh_partner_raid_score,
        refresh_partner_raid_score_async,
    )
except Exception:  # pragma: no cover - partial deploy safety
    refresh_partner_raid_score = None  # type: ignore[assignment]
    refresh_partner_raid_score_async = None  # type: ignore[assignment]
from .billing_plans import (
    BILLING_CYCLE_DISCOUNTS as _BILLING_CYCLE_DISCOUNTS,
    BILLING_PLANS as _BILLING_PLANS,
    billing_cycle_label as _billing_cycle_label,
    billing_dump_price_id_mapping as _billing_dump_price_id_mapping,
    billing_dump_product_id_mapping as _billing_dump_product_id_mapping,
    billing_merge_price_id_defaults as _billing_merge_price_id_defaults,
    billing_merge_product_id_defaults as _billing_merge_product_id_defaults,
    billing_parse_price_id_mapping as _billing_parse_price_id_mapping,
    billing_parse_product_id_mapping as _billing_parse_product_id_mapping,
    billing_value_preview as _billing_value_preview,
    format_eur_cents as _format_eur_cents,
    normalize_billing_cycle as _normalize_billing_cycle,
)


class _DashboardBillingMixin:
    """Shared billing helper methods for dashboard route handlers."""

    def _billing_refresh_partner_raid_score_cache(
        self,
        *,
        twitch_user_id: str,
        twitch_login: str,
        reason: str,
    ) -> None:
        twitch_user_key = str(twitch_user_id or "").strip()
        if not twitch_user_key:
            return
        safe_reason = str(reason or "").replace("\r", "\\r").replace("\n", "\\n")[:64] or "unknown"
        subject_kind = "login" if str(twitch_login or "").strip() else "user_id"
        if callable(refresh_partner_raid_score_async):
            try:
                loop = asyncio.get_running_loop()
            except RuntimeError:
                loop = None
            if loop is not None:
                async def _run_async_refresh() -> None:
                    try:
                        await refresh_partner_raid_score_async(twitch_user_key)
                    except Exception:
                        log.debug(
                            "billing: partner raid score refresh failed (subject=%s, reason=%s)",
                            subject_kind,
                            safe_reason,
                            exc_info=True,
                        )
                    else:
                        log.info(
                            "billing: partner raid score refreshed (subject=%s, reason=%s)",
                            subject_kind,
                            safe_reason,
                        )

                loop.create_task(
                    _run_async_refresh(),
                    name=f"billing.partner_raid_score_refresh.{twitch_user_key}",
                )
                return
        if not callable(refresh_partner_raid_score):
            return
        try:
            refresh_partner_raid_score(twitch_user_key)
        except Exception:
            log.debug(
                "billing: partner raid score refresh failed (subject=%s, reason=%s)",
                subject_kind,
                safe_reason,
                exc_info=True,
            )
        else:
            log.info(
                "billing: partner raid score refreshed (subject=%s, reason=%s)",
                subject_kind,
                safe_reason,
            )

    @staticmethod
    def _billing_valid_plan_ids() -> set[str]:
        return {
            str(plan.get("id") or "").strip()
            for plan in _BILLING_PLANS
            if str(plan.get("id") or "").strip()
        }


    @staticmethod
    def _billing_plan_name_from_id(plan_id: str) -> str:
        return {
            "raid_boost": "raid_boost",
            "analysis_dashboard": "analysis",
            "bundle_analysis_raid_boost": "bundle",
        }.get(str(plan_id or "").strip(), "free")


    @staticmethod
    def _billing_row_value(row: Any, key: str, index: int, default: Any = None) -> Any:
        if row is None:
            return default
        if hasattr(row, "get"):
            return row.get(key, default)
        values = tuple(row)
        return values[index] if index < len(values) else default


    @staticmethod
    def _billing_parse_datetime_value(raw_value: Any) -> datetime | None:
        return _entitlements_repository.parse_datetime_value(raw_value)


    def _billing_normalize_plan_id(self, raw_plan_id: Any) -> str:
        plan_id = str(raw_plan_id or "").strip()
        return plan_id if plan_id in self._billing_valid_plan_ids() else ""

    def _billing_refresh_runtime_secrets(self) -> None:
        loader = getattr(self, "_load_secret_value", None)
        if not callable(loader):
            return
        key_targets = (
            (
                "_billing_stripe_publishable_key",
                ("STRIPE_PUBLISHABLE_KEY", "TWITCH_BILLING_STRIPE_PUBLISHABLE_KEY"),
            ),
            (
                "_billing_stripe_secret_key",
                ("STRIPE_SECRET_KEY", "TWITCH_BILLING_STRIPE_SECRET_KEY"),
            ),
            (
                "_billing_stripe_webhook_secret",
                ("STRIPE_WEBHOOK_SECRET", "TWITCH_BILLING_STRIPE_WEBHOOK_SECRET"),
            ),
            (
                "_billing_checkout_success_url",
                ("STRIPE_CHECKOUT_SUCCESS_URL", "TWITCH_BILLING_CHECKOUT_SUCCESS_URL"),
            ),
            (
                "_billing_checkout_cancel_url",
                ("STRIPE_CHECKOUT_CANCEL_URL", "TWITCH_BILLING_CHECKOUT_CANCEL_URL"),
            ),
            (
                "_billing_stripe_price_map_raw",
                ("STRIPE_PRICE_ID_MAP", "TWITCH_BILLING_STRIPE_PRICE_ID_MAP"),
            ),
            (
                "_billing_stripe_product_map_raw",
                ("STRIPE_PRODUCT_ID_MAP", "TWITCH_BILLING_STRIPE_PRODUCT_ID_MAP"),
            ),
        )
        for attr_name, keys in key_targets:
            try:
                value = str(loader(*keys) or "").strip()
            except Exception:
                continue
            if value:
                setattr(self, attr_name, value)


    def _billing_price_id_map(self) -> dict[str, dict[int, str]]:
        self._billing_refresh_runtime_secrets()
        vault_map = _billing_parse_price_id_mapping(getattr(self, "_billing_stripe_price_map_raw", ""))
        return _billing_merge_price_id_defaults(vault_map)


    def _billing_product_id_map(self) -> dict[str, str]:
        self._billing_refresh_runtime_secrets()
        vault_map = _billing_parse_product_id_mapping(getattr(self, "_billing_stripe_product_map_raw", ""))
        return _billing_merge_product_id_defaults(vault_map)


    def _billing_set_price_id_map(self, mapping: dict[str, dict[int, str]]) -> bool:
        payload = _billing_dump_price_id_mapping(mapping)
        setattr(self, "_billing_stripe_price_map_raw", payload)
        writer = getattr(self, "_write_keyring_secret", None)
        if not callable(writer):
            return False
        ok_primary = bool(writer("STRIPE_PRICE_ID_MAP", payload))
        ok_alias = bool(writer("TWITCH_BILLING_STRIPE_PRICE_ID_MAP", payload))
        return bool(ok_primary or ok_alias)


    def _billing_set_product_id_map(self, mapping: dict[str, str]) -> bool:
        payload = _billing_dump_product_id_mapping(mapping)
        setattr(self, "_billing_stripe_product_map_raw", payload)
        writer = getattr(self, "_write_keyring_secret", None)
        if not callable(writer):
            return False
        ok_primary = bool(writer("STRIPE_PRODUCT_ID_MAP", payload))
        ok_alias = bool(writer("TWITCH_BILLING_STRIPE_PRODUCT_ID_MAP", payload))
        return bool(ok_primary or ok_alias)


    def _billing_price_id_for_plan(
        self,
        plan_id: str,
        cycle_months: int,
        *,
        price_map: dict[str, dict[int, str]] | None = None,
    ) -> str:
        mapping = price_map if isinstance(price_map, dict) else self._billing_price_id_map()
        cycle = _normalize_billing_cycle(cycle_months)
        plan_cycles = mapping.get(str(plan_id or "").strip()) or {}
        return str(plan_cycles.get(cycle) or "").strip()


    def _billing_price_mapping_stats(self) -> dict[str, Any]:
        mapping = self._billing_price_id_map()
        required_plan_ids = [
            str(plan.get("id") or "")
            for plan in _BILLING_PLANS
            if int(plan.get("monthly_net_cents") or 0) > 0
        ]
        required_cycles = sorted(int(cycle) for cycle in _BILLING_CYCLE_DISCOUNTS.keys())
        required_slots = len(required_plan_ids) * len(required_cycles)
        mapped_slots = 0
        missing_slots: list[str] = []
        for plan_id in required_plan_ids:
            cycle_map = mapping.get(plan_id) or {}
            for cycle in required_cycles:
                price_id = str(cycle_map.get(cycle) or "").strip()
                if price_id:
                    mapped_slots += 1
                else:
                    missing_slots.append(f"{plan_id}:{cycle}")
        return {
            "required_slots": required_slots,
            "mapped_slots": mapped_slots,
            "missing_slots": missing_slots,
            "ready": bool(required_slots == 0 or mapped_slots >= required_slots),
        }


    @staticmethod
    def _billing_import_stripe() -> tuple[Any | None, str | None]:
        try:
            import stripe
        except Exception as exc:
            return None, str(exc)
        return stripe, None


    @staticmethod
    def _billing_stripe_obj_get(obj: Any, key: str, default: Any = None) -> Any:
        if obj is None:
            return default
        if isinstance(obj, dict):
            return obj.get(key, default)
        if hasattr(obj, key):
            return getattr(obj, key, default)
        getter = getattr(obj, "get", None)
        if callable(getter):
            try:
                return getter(key, default)
            except Exception:
                return default
        return default


    @staticmethod
    def _billing_epoch_to_iso(raw_epoch: Any) -> str | None:
        try:
            epoch = int(raw_epoch or 0)
        except (TypeError, ValueError):
            return None
        if epoch <= 0:
            return None
        return datetime.fromtimestamp(epoch, tz=UTC).isoformat()


    def _billing_ensure_storage_tables(self, conn: Any) -> None:
        conn.execute(
            """
            CREATE TABLE IF NOT EXISTS twitch_billing_events (
                stripe_event_id TEXT PRIMARY KEY,
                event_type TEXT NOT NULL,
                object_id TEXT,
                received_at TEXT NOT NULL,
                livemode INTEGER NOT NULL DEFAULT 0,
                payload TEXT NOT NULL
            )
            """
        )
        conn.execute(
            """
            CREATE TABLE IF NOT EXISTS twitch_billing_profiles (
                customer_reference TEXT PRIMARY KEY,
                recipient_name TEXT NOT NULL DEFAULT '',
                recipient_email TEXT NOT NULL DEFAULT '',
                company_name TEXT NOT NULL DEFAULT '',
                street_line1 TEXT NOT NULL DEFAULT '',
                postal_code TEXT NOT NULL DEFAULT '',
                city TEXT NOT NULL DEFAULT '',
                country_code TEXT NOT NULL DEFAULT '',
                vat_id TEXT NOT NULL DEFAULT '',
                updated_at TEXT NOT NULL
            )
            """
        )
        # Backfill-safe migration for older installs.
        for column_sql in (
            "company_name TEXT NOT NULL DEFAULT ''",
            "street_line1 TEXT NOT NULL DEFAULT ''",
            "postal_code TEXT NOT NULL DEFAULT ''",
            "city TEXT NOT NULL DEFAULT ''",
            "country_code TEXT NOT NULL DEFAULT ''",
            "vat_id TEXT NOT NULL DEFAULT ''",
        ):
            try:
                conn.execute(
                    "ALTER TABLE twitch_billing_profiles "
                    f"ADD COLUMN IF NOT EXISTS {column_sql}"
                )
            except Exception:
                try:
                    conn.execute(f"ALTER TABLE twitch_billing_profiles ADD COLUMN {column_sql}")  # nosemgrep: python.lang.security.audit.formatted-sql-query.formatted-sql-query, python.sqlalchemy.security.sqlalchemy-execute-raw-query.sqlalchemy-execute-raw-query
                except Exception:
                    pass


    def _billing_upsert_subscription_state(
        self,
        conn: Any,
        *,
        stripe_subscription_id: str,
        stripe_customer_id: str = "",
        customer_reference: str = "",
        status: str = "",
        plan_id: str = "",
        cycle_months: int | None = None,
        quantity: int | None = None,
        current_period_start: str | None = None,
        current_period_end: str | None = None,
        cancel_at_period_end: bool | None = None,
        canceled_at: str | None = None,
        ended_at: str | None = None,
        last_event_id: str = "",
    ) -> None:
        sub_id = str(stripe_subscription_id or "").strip()
        if not sub_id:
            return

        existing = conn.execute(
            """
            SELECT
                stripe_customer_id,
                customer_reference,
                status,
                plan_id,
                cycle_months,
                quantity,
                current_period_start,
                current_period_end,
                cancel_at_period_end,
                canceled_at,
                ended_at,
                last_event_id
            FROM twitch_billing_subscriptions
            WHERE stripe_subscription_id = %s
            """,
            (sub_id,),
        ).fetchone()
        existing_values = tuple(existing or ())

        def _existing(idx: int, fallback: Any = None) -> Any:
            if idx < len(existing_values):
                return existing_values[idx]
            return fallback

        final_customer_id = str(stripe_customer_id or _existing(0, "") or "").strip()
        final_customer_reference = str(customer_reference or _existing(1, "") or "").strip()
        final_status = str(status or _existing(2, "unknown") or "unknown").strip() or "unknown"
        final_plan_id = str(plan_id or _existing(3, "") or "").strip()
        try:
            final_cycle_months = int(cycle_months if cycle_months is not None else _existing(4, 1))
        except (TypeError, ValueError):
            final_cycle_months = 1
        final_cycle_months = max(1, final_cycle_months)
        try:
            final_quantity = int(quantity if quantity is not None else _existing(5, 1))
        except (TypeError, ValueError):
            final_quantity = 1
        final_quantity = min(max(1, final_quantity), 24)
        final_current_period_start = str(
            current_period_start if current_period_start is not None else (_existing(6, "") or "")
        ).strip() or None
        final_current_period_end = str(
            current_period_end if current_period_end is not None else (_existing(7, "") or "")
        ).strip() or None
        if cancel_at_period_end is None:
            final_cancel_at_period_end = int(_existing(8, 0) or 0)
        else:
            final_cancel_at_period_end = 1 if bool(cancel_at_period_end) else 0
        final_canceled_at = str(
            canceled_at if canceled_at is not None else (_existing(9, "") or "")
        ).strip() or None
        final_ended_at = str(ended_at if ended_at is not None else (_existing(10, "") or "")).strip() or None
        final_last_event_id = str(last_event_id or _existing(11, "") or "").strip() or None
        updated_at = datetime.now(UTC).isoformat()

        conn.execute(
            """
            INSERT INTO twitch_billing_subscriptions (
                stripe_subscription_id,
                stripe_customer_id,
                customer_reference,
                status,
                plan_id,
                cycle_months,
                quantity,
                current_period_start,
                current_period_end,
                cancel_at_period_end,
                canceled_at,
                ended_at,
                last_event_id,
                updated_at
            ) VALUES (%s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s)
            ON CONFLICT (stripe_subscription_id) DO UPDATE SET
                stripe_customer_id = EXCLUDED.stripe_customer_id,
                customer_reference = EXCLUDED.customer_reference,
                status = EXCLUDED.status,
                plan_id = EXCLUDED.plan_id,
                cycle_months = EXCLUDED.cycle_months,
                quantity = EXCLUDED.quantity,
                current_period_start = EXCLUDED.current_period_start,
                current_period_end = EXCLUDED.current_period_end,
                cancel_at_period_end = EXCLUDED.cancel_at_period_end,
                canceled_at = EXCLUDED.canceled_at,
                ended_at = EXCLUDED.ended_at,
                last_event_id = EXCLUDED.last_event_id,
                updated_at = EXCLUDED.updated_at
            """,
            (
                sub_id,
                final_customer_id,
                final_customer_reference,
                final_status,
                final_plan_id,
                final_cycle_months,
                final_quantity,
                final_current_period_start,
                final_current_period_end,
                final_cancel_at_period_end,
                final_canceled_at,
                final_ended_at,
                final_last_event_id,
                updated_at,
            ),
        )


    def _billing_subscription_payload_from_object(self, subscription_obj: Any) -> dict[str, Any]:
        subscription_id = str(self._billing_stripe_obj_get(subscription_obj, "id", "") or "").strip()
        customer_id = str(self._billing_stripe_obj_get(subscription_obj, "customer", "") or "").strip()
        status = str(self._billing_stripe_obj_get(subscription_obj, "status", "unknown") or "unknown").strip()
        metadata_obj = self._billing_stripe_obj_get(subscription_obj, "metadata", {})
        metadata = metadata_obj if isinstance(metadata_obj, dict) else {}
        items_obj = self._billing_stripe_obj_get(subscription_obj, "items", {})
        items_data = self._billing_stripe_obj_get(items_obj, "data", []) or []
        first_item = items_data[0] if isinstance(items_data, list) and items_data else {}
        price_obj = self._billing_stripe_obj_get(first_item, "price", {}) or {}
        price_metadata_obj = self._billing_stripe_obj_get(price_obj, "metadata", {})
        price_metadata = price_metadata_obj if isinstance(price_metadata_obj, dict) else {}
        recurring = self._billing_stripe_obj_get(price_obj, "recurring", {}) or {}
        interval = str(self._billing_stripe_obj_get(recurring, "interval", "") or "").strip()
        try:
            interval_count = int(self._billing_stripe_obj_get(recurring, "interval_count", 1) or 1)
        except (TypeError, ValueError):
            interval_count = 1
        cycle_months = interval_count if interval == "month" else 1
        try:
            quantity = int(self._billing_stripe_obj_get(first_item, "quantity", 1) or 1)
        except (TypeError, ValueError):
            quantity = 1
        plan_id = str(
            metadata.get("plan_id")
            or price_metadata.get("plan_id")
            or self._billing_stripe_obj_get(price_obj, "lookup_key", "")
            or ""
        ).strip()
        customer_reference = str(metadata.get("customer_reference") or "").strip()
        return {
            "stripe_subscription_id": subscription_id,
            "stripe_customer_id": customer_id,
            "customer_reference": customer_reference,
            "status": status,
            "plan_id": plan_id,
            "cycle_months": cycle_months,
            "quantity": quantity,
            "current_period_start": self._billing_epoch_to_iso(
                self._billing_stripe_obj_get(subscription_obj, "current_period_start", 0)
            ),
            "current_period_end": self._billing_epoch_to_iso(
                self._billing_stripe_obj_get(subscription_obj, "current_period_end", 0)
            ),
            "cancel_at_period_end": bool(
                self._billing_stripe_obj_get(subscription_obj, "cancel_at_period_end", False)
            ),
            "canceled_at": self._billing_epoch_to_iso(
                self._billing_stripe_obj_get(subscription_obj, "canceled_at", 0)
            ),
            "ended_at": self._billing_epoch_to_iso(
                self._billing_stripe_obj_get(subscription_obj, "ended_at", 0)
            ),
        }


    def _billing_sync_plan_to_streamer_plans(self, customer_reference: str, plan_id: str, status: str) -> None:
        """Sync twitch_billing_subscriptions -> streamer_plans nach Webhook."""
        if not customer_reference:
            return
        is_active = status in ("active", "trialing")
        plan_name = self._billing_plan_name_from_id(plan_id)
        effective_plan = plan_name if is_active else "free"
        refreshed_user_id = ""
        refreshed_login = str(customer_reference or "").strip().lower()
        try:
            with storage.transaction() as conn:
                conn.execute(
                    """
                    INSERT INTO streamer_plans (twitch_user_id, twitch_login, plan_name, expires_at)
                    SELECT twitch_user_id, twitch_login, %s, NULL
                    FROM twitch_streamers_partner_state
                    WHERE LOWER(twitch_login) = LOWER(%s)
                    ON CONFLICT(twitch_user_id) DO UPDATE SET
                        plan_name = EXCLUDED.plan_name,
                        expires_at = EXCLUDED.expires_at
                    """,
                    (effective_plan, customer_reference),
                )
                row = conn.execute(
                    """
                    SELECT twitch_user_id, twitch_login
                    FROM twitch_streamers_partner_state
                    WHERE LOWER(twitch_login) = LOWER(%s)
                    LIMIT 1
                    """,
                    (customer_reference,),
                ).fetchone()
                if row is not None:
                    refreshed_user_id = str(
                        self._billing_row_value(row, "twitch_user_id", 0, "") or ""
                    ).strip()
                    refreshed_login = str(
                        self._billing_row_value(row, "twitch_login", 1, refreshed_login) or refreshed_login
                    ).strip().lower()
        except Exception:
            log.debug("billing sync plan to streamer_plans failed", exc_info=True)
            return

        if refreshed_user_id:
            self._billing_refresh_partner_raid_score_cache(
                twitch_user_id=refreshed_user_id,
                twitch_login=refreshed_login,
                reason="billing_subscription_sync",
            )

    def _billing_grant_bonus_access_months(
        self,
        conn: Any,
        *,
        customer_reference: str,
        plan_id: str,
        period_end_iso: str,
        bonus_months: int,
    ) -> None:
        """Extend manual_plan_expires_at by bonus_months beyond the subscription period end."""
        if not customer_reference or not period_end_iso or bonus_months <= 0:
            return
        try:
            period_end_dt = datetime.fromisoformat(period_end_iso)
            if period_end_dt.tzinfo is None:
                period_end_dt = period_end_dt.replace(tzinfo=UTC)
            bonus_expires_at = period_end_dt + timedelta(days=bonus_months * 31)
        except Exception:
            log.debug("billing bonus grant: invalid period_end_iso=%r", period_end_iso, exc_info=True)
            return
        notes = f"bonus {bonus_months}mo: annual (auto {datetime.now(UTC).date().isoformat()})"
        updated_at = datetime.now(UTC).isoformat()
        try:
            conn.execute(
                """
                UPDATE streamer_plans
                SET
                    manual_plan_id = %s,
                    manual_plan_expires_at = %s,
                    manual_plan_notes = %s,
                    manual_plan_updated_at = %s
                WHERE twitch_user_id = (
                    SELECT twitch_user_id
                    FROM twitch_streamers_partner_state
                    WHERE LOWER(twitch_login) = LOWER(%s)
                    LIMIT 1
                )
                """,
                (plan_id, bonus_expires_at.isoformat(), notes, updated_at, customer_reference),
            )
        except Exception:
            log.debug("billing grant bonus access failed for %r", customer_reference, exc_info=True)

    def _billing_apply_webhook_event(
        self,
        conn: Any,
        *,
        stripe: Any,
        event_id: str,
        event_type: str,
        event_object: Any,
    ) -> str:
        event_name = str(event_type or "").strip()
        if not event_name:
            return "ignored_missing_type"

        if event_name.startswith("customer.subscription."):
            payload = self._billing_subscription_payload_from_object(event_object)
            payload["last_event_id"] = event_id
            self._billing_upsert_subscription_state(conn, **payload)
            self._billing_sync_plan_to_streamer_plans(
                customer_reference=str(payload.get("customer_reference") or ""),
                plan_id=str(payload.get("plan_id") or ""),
                status=str(payload.get("status") or ""),
            )
            return "subscription_state_updated"

        if event_name == "checkout.session.completed":
            mode = str(self._billing_stripe_obj_get(event_object, "mode", "") or "").strip()
            if mode != "subscription":
                return "checkout_ignored_non_subscription"
            subscription_id = str(
                self._billing_stripe_obj_get(event_object, "subscription", "") or ""
            ).strip()
            customer_id = str(self._billing_stripe_obj_get(event_object, "customer", "") or "").strip()
            metadata_obj = self._billing_stripe_obj_get(event_object, "metadata", {})
            metadata = metadata_obj if isinstance(metadata_obj, dict) else {}
            customer_reference = str(
                metadata.get("customer_reference")
                or self._billing_stripe_obj_get(event_object, "client_reference_id", "")
                or ""
            ).strip()

            payload: dict[str, Any] = {
                "stripe_subscription_id": subscription_id,
                "stripe_customer_id": customer_id,
                "customer_reference": customer_reference,
                "status": "active",
                "plan_id": str(metadata.get("plan_id") or "").strip(),
                "cycle_months": _normalize_billing_cycle(metadata.get("cycle_months")),
                "quantity": int(metadata.get("quantity") or 1)
                if str(metadata.get("quantity") or "").strip().isdigit()
                else 1,
                "current_period_start": None,
                "current_period_end": None,
                "cancel_at_period_end": False,
                "canceled_at": None,
                "ended_at": None,
                "last_event_id": event_id,
            }

            subscription_bonus_months = 0
            if subscription_id:
                try:
                    stripe_subscription = stripe.Subscription.retrieve(
                        subscription_id,
                        expand=["items.data.price"],
                    )
                except Exception:
                    stripe_subscription = None
                if stripe_subscription is not None:
                    subscription_payload = self._billing_subscription_payload_from_object(
                        stripe_subscription
                    )
                    if not str(subscription_payload.get("customer_reference") or "").strip():
                        subscription_payload["customer_reference"] = customer_reference
                    payload.update(subscription_payload)
                    payload["last_event_id"] = event_id
                    sub_meta_obj = self._billing_stripe_obj_get(stripe_subscription, "metadata", {})
                    sub_meta = sub_meta_obj if isinstance(sub_meta_obj, dict) else {}
                    try:
                        subscription_bonus_months = int(sub_meta.get("bonus_months") or 0)
                    except (TypeError, ValueError):
                        subscription_bonus_months = 0

            self._billing_upsert_subscription_state(conn, **payload)
            self._billing_sync_plan_to_streamer_plans(
                customer_reference=str(payload.get("customer_reference") or ""),
                plan_id=str(payload.get("plan_id") or ""),
                status=str(payload.get("status") or ""),
            )
            if subscription_bonus_months > 0 and str(payload.get("customer_reference") or "").strip():
                self._billing_grant_bonus_access_months(
                    conn,
                    customer_reference=str(payload["customer_reference"]),
                    plan_id=str(payload.get("plan_id") or ""),
                    period_end_iso=str(payload.get("current_period_end") or ""),
                    bonus_months=subscription_bonus_months,
                )
            return "checkout_subscription_recorded"

        if event_name == "invoice.payment_succeeded":
            subscription_id = str(
                self._billing_stripe_obj_get(event_object, "subscription", "") or ""
            ).strip()
            if not subscription_id:
                return "invoice_ignored_without_subscription"
            customer_id = str(self._billing_stripe_obj_get(event_object, "customer", "") or "").strip()
            self._billing_upsert_subscription_state(
                conn,
                stripe_subscription_id=subscription_id,
                stripe_customer_id=customer_id,
                status="active",
                last_event_id=event_id,
            )
            amount_paid = int(self._billing_stripe_obj_get(event_object, "amount_paid", 0) or 0)
            invoice_id = str(self._billing_stripe_obj_get(event_object, "id", "") or "").strip()
            period_start_ts = self._billing_stripe_obj_get(event_object, "period_start", None)
            period_end_ts = self._billing_stripe_obj_get(event_object, "period_end", None)
            from datetime import datetime, timezone as _tz
            period_start = datetime.fromtimestamp(int(period_start_ts), tz=_tz.utc).isoformat() if period_start_ts else None
            period_end = datetime.fromtimestamp(int(period_end_ts), tz=_tz.utc).isoformat() if period_end_ts else None
            try:
                self._affiliate_process_commission(
                    conn,
                    stripe=stripe,
                    stripe_event_id=event_id,
                    stripe_customer_id=customer_id,
                    amount_paid_cents=amount_paid,
                    currency=str(self._billing_stripe_obj_get(event_object, "currency", "eur") or "eur"),
                    invoice_id=invoice_id,
                    period_start=period_start,
                    period_end=period_end,
                )
            except Exception:
                log.exception("affiliate commission processing failed for event %s", event_id)
            return "invoice_payment_recorded"

        if event_name == "invoice.payment_failed":
            subscription_id = str(
                self._billing_stripe_obj_get(event_object, "subscription", "") or ""
            ).strip()
            if not subscription_id:
                return "invoice_ignored_without_subscription"
            customer_id = str(self._billing_stripe_obj_get(event_object, "customer", "") or "").strip()
            self._billing_upsert_subscription_state(
                conn,
                stripe_subscription_id=subscription_id,
                stripe_customer_id=customer_id,
                status="past_due",
                last_event_id=event_id,
            )
            return "invoice_failure_recorded"

        return "ignored_unsupported_event"

    def _billing_auth_sessions_for_request(self, request: Any) -> tuple[dict[str, Any], dict[str, Any]]:
        dashboard_session: dict[str, Any] = {}
        dashboard_getter = getattr(self, "_get_dashboard_auth_session", None)
        if callable(dashboard_getter):
            try:
                maybe_dashboard = dashboard_getter(request) or {}
                if isinstance(maybe_dashboard, dict):
                    dashboard_session = maybe_dashboard
            except Exception:
                dashboard_session = {}

        discord_admin_session: dict[str, Any] = {}
        admin_getter = getattr(self, "_get_discord_admin_session", None)
        if callable(admin_getter):
            try:
                maybe_admin = admin_getter(request) or {}
                if isinstance(maybe_admin, dict):
                    discord_admin_session = maybe_admin
            except Exception:
                discord_admin_session = {}

        return dashboard_session, discord_admin_session

    @staticmethod
    def _billing_refs_from_session(session: dict[str, Any]) -> list[str]:
        refs: list[str] = []
        for raw in (session.get("twitch_user_id"), session.get("twitch_login")):
            value = str(raw or "").strip()
            if value and value not in refs:
                refs.append(value)
        return refs

    def _billing_candidate_refs_for_request(self, request: Any) -> list[str]:
        """Collect trusted customer references from server-side sessions only."""
        dashboard_session, discord_admin_session = self._billing_auth_sessions_for_request(request)
        candidate_refs: list[str] = []

        for value in self._billing_refs_from_session(dashboard_session):
            if value and value not in candidate_refs:
                candidate_refs.append(value)

        for value in self._billing_refs_from_session(discord_admin_session):
            if value and value not in candidate_refs:
                candidate_refs.append(value)

        admin_user_id = str(discord_admin_session.get("user_id") or "").strip()
        if admin_user_id:
            admin_ref = f"discord_admin:{admin_user_id}"
            if admin_ref not in candidate_refs:
                candidate_refs.append(admin_ref)
        return candidate_refs

    def _billing_primary_ref_for_request(self, request: Any) -> str:
        refs = self._billing_candidate_refs_for_request(request)
        return refs[0] if refs else ""

    def _billing_customer_record_for_request(self, request: Any) -> dict[str, str]:
        """Return best matching Stripe customer/subscription ids for current request."""
        refs = self._billing_candidate_refs_for_request(request)
        fallback_ref = refs[0] if refs else ""
        fallback = {
            "customer_reference": fallback_ref,
            "stripe_customer_id": "",
            "stripe_subscription_id": "",
            "status": "",
        }
        if not refs:
            return fallback

        active_like_statuses = ("active", "trialing", "past_due")
        ordered_statuses = "CASE WHEN status IN (%s, %s, %s) THEN 0 ELSE 1 END"
        try:
            with storage.readonly_connection() as conn:
                self._billing_ensure_storage_tables(conn)
                for ref in refs:
                    row = conn.execute(  # nosemgrep: python.sqlalchemy.security.sqlalchemy-execute-raw-query.sqlalchemy-execute-raw-query
                        f"""
                        SELECT customer_reference, stripe_customer_id, stripe_subscription_id, status, updated_at
                        FROM twitch_billing_subscriptions
                        WHERE LOWER(customer_reference) = LOWER(%s)
                          AND TRIM(COALESCE(stripe_customer_id, '')) <> ''
                        ORDER BY {ordered_statuses}, updated_at DESC
                        LIMIT 1
                        """,
                        (ref, *active_like_statuses),
                    ).fetchone()
                    if not row:
                        continue
                    if hasattr(row, "get"):
                        return {
                            "customer_reference": str(row.get("customer_reference") or ref).strip(),
                            "stripe_customer_id": str(row.get("stripe_customer_id") or "").strip(),
                            "stripe_subscription_id": str(
                                row.get("stripe_subscription_id") or ""
                            ).strip(),
                            "status": str(row.get("status") or "").strip().lower(),
                        }
                    values = tuple(row)
                    return {
                        "customer_reference": str(values[0] if len(values) > 0 else ref).strip(),
                        "stripe_customer_id": str(values[1] if len(values) > 1 else "").strip(),
                        "stripe_subscription_id": str(values[2] if len(values) > 2 else "").strip(),
                        "status": str(values[3] if len(values) > 3 else "").strip().lower(),
                    }
        except Exception:
            log.debug("billing customer lookup failed; fallback to empty ids", exc_info=True)
        return fallback

    def _billing_profile_from_stripe_customer(self, stripe_customer_id: str) -> dict[str, str]:
        """Fetch invoice-relevant customer fields from Stripe for UI prefill."""
        customer_id = str(stripe_customer_id or "").strip()
        if not customer_id:
            return {}

        stripe, _import_error = self._billing_import_stripe()
        if stripe is None:
            return {}
        stripe_secret_key = str(getattr(self, "_billing_stripe_secret_key", "") or "").strip()
        if not stripe_secret_key:
            return {}
        stripe.api_key = stripe_secret_key

        try:
            customer_obj = stripe.Customer.retrieve(customer_id)
        except Exception:
            log.debug("billing stripe customer lookup failed for %s", customer_id, exc_info=True)
            return {}

        metadata_obj = self._billing_stripe_obj_get(customer_obj, "metadata", {}) or {}
        shipping_obj = self._billing_stripe_obj_get(customer_obj, "shipping", {}) or {}
        shipping_address = self._billing_stripe_obj_get(shipping_obj, "address", {}) or {}
        billing_address = self._billing_stripe_obj_get(customer_obj, "address", {}) or {}
        address_obj = billing_address if billing_address else shipping_address

        vat_id = ""
        try:
            tax_id_list = stripe.Customer.list_tax_ids(customer_id, limit=1)
            tax_ids = list(self._billing_stripe_obj_get(tax_id_list, "data", []) or [])
            if tax_ids:
                vat_id = str(self._billing_stripe_obj_get(tax_ids[0], "value", "") or "").strip()
        except Exception:
            vat_id = ""

        return {
            "recipient_name": str(
                self._billing_stripe_obj_get(customer_obj, "name", "")
                or self._billing_stripe_obj_get(shipping_obj, "name", "")
                or ""
            ).strip(),
            "recipient_email": str(self._billing_stripe_obj_get(customer_obj, "email", "") or "").strip(),
            "company_name": str(
                self._billing_stripe_obj_get(metadata_obj, "company_name", "")
                or self._billing_stripe_obj_get(metadata_obj, "company", "")
                or ""
            ).strip(),
            "street_line1": str(self._billing_stripe_obj_get(address_obj, "line1", "") or "").strip(),
            "postal_code": str(self._billing_stripe_obj_get(address_obj, "postal_code", "") or "").strip(),
            "city": str(self._billing_stripe_obj_get(address_obj, "city", "") or "").strip(),
            "country_code": str(self._billing_stripe_obj_get(address_obj, "country", "DE") or "DE")
            .strip()
            .upper(),
            "vat_id": vat_id,
        }

    @staticmethod
    def _billing_prefill_profile_from_stripe(
        profile: dict[str, str],
        stripe_profile: dict[str, str],
    ) -> tuple[dict[str, str], list[str]]:
        merged = dict(profile or {})
        imported_fields: list[str] = []
        keys = (
            "recipient_name",
            "recipient_email",
            "company_name",
            "street_line1",
            "postal_code",
            "city",
            "country_code",
            "vat_id",
        )
        for key in keys:
            current_value = str(merged.get(key) or "").strip()
            if current_value:
                continue
            stripe_value = str((stripe_profile or {}).get(key) or "").strip()
            if not stripe_value:
                continue
            merged[key] = stripe_value
            imported_fields.append(key)
        return merged, imported_fields

    def _billing_profile_for_request(self, request: Any) -> dict[str, str]:
        """Resolve persisted invoice recipient profile for current request."""
        dashboard_session, discord_admin_session = self._billing_auth_sessions_for_request(request)
        session = dashboard_session or discord_admin_session
        refs = self._billing_candidate_refs_for_request(request)
        primary_ref = refs[0] if refs else ""
        profile = {
            "customer_reference": primary_ref,
            "recipient_name": str(
                session.get("display_name") or session.get("twitch_login") or "Streamer Partner"
            ).strip(),
            "recipient_email": "",
            "company_name": "",
            "street_line1": "",
            "postal_code": "",
            "city": "",
            "country_code": "DE",
            "vat_id": "",
        }
        if not primary_ref:
            return profile

        try:
            with storage.readonly_connection() as conn:
                self._billing_ensure_storage_tables(conn)
                row = conn.execute(
                    """
                    SELECT
                        customer_reference,
                        recipient_name,
                        recipient_email,
                        company_name,
                        street_line1,
                        postal_code,
                        city,
                        country_code,
                        vat_id
                    FROM twitch_billing_profiles
                    WHERE LOWER(customer_reference) = LOWER(%s)
                    LIMIT 1
                    """,
                    (primary_ref,),
                ).fetchone()
            if not row:
                return profile
            if hasattr(row, "get"):
                profile["customer_reference"] = str(
                    row.get("customer_reference") or primary_ref
                ).strip()
                profile["recipient_name"] = str(
                    row.get("recipient_name") or profile["recipient_name"]
                ).strip()
                profile["recipient_email"] = str(row.get("recipient_email") or "").strip()
                profile["company_name"] = str(row.get("company_name") or "").strip()
                profile["street_line1"] = str(row.get("street_line1") or "").strip()
                profile["postal_code"] = str(row.get("postal_code") or "").strip()
                profile["city"] = str(row.get("city") or "").strip()
                profile["country_code"] = str(row.get("country_code") or "DE").strip().upper()
                profile["vat_id"] = str(row.get("vat_id") or "").strip()
                return profile
            values = tuple(row)
            profile["customer_reference"] = str(values[0] if len(values) > 0 else primary_ref).strip()
            profile["recipient_name"] = str(
                values[1] if len(values) > 1 else profile["recipient_name"]
            ).strip()
            profile["recipient_email"] = str(values[2] if len(values) > 2 else "").strip()
            profile["company_name"] = str(values[3] if len(values) > 3 else "").strip()
            profile["street_line1"] = str(values[4] if len(values) > 4 else "").strip()
            profile["postal_code"] = str(values[5] if len(values) > 5 else "").strip()
            profile["city"] = str(values[6] if len(values) > 6 else "").strip()
            profile["country_code"] = str(values[7] if len(values) > 7 else "DE").strip().upper()
            profile["vat_id"] = str(values[8] if len(values) > 8 else "").strip()
        except Exception:
            log.debug("billing profile lookup failed; using defaults", exc_info=True)
        return profile

    def _billing_upsert_profile(
        self,
        conn: Any,
        *,
        customer_reference: str,
        recipient_name: str,
        recipient_email: str,
        company_name: str = "",
        street_line1: str = "",
        postal_code: str = "",
        city: str = "",
        country_code: str = "DE",
        vat_id: str = "",
    ) -> None:
        reference = str(customer_reference or "").strip()
        if not reference:
            return
        now_iso = datetime.now(UTC).isoformat()
        conn.execute(
            """
            INSERT INTO twitch_billing_profiles (
                customer_reference,
                recipient_name,
                recipient_email,
                company_name,
                street_line1,
                postal_code,
                city,
                country_code,
                vat_id,
                updated_at
            ) VALUES (%s, %s, %s, %s, %s, %s, %s, %s, %s, %s)
            ON CONFLICT (customer_reference) DO UPDATE SET
                recipient_name = EXCLUDED.recipient_name,
                recipient_email = EXCLUDED.recipient_email,
                company_name = EXCLUDED.company_name,
                street_line1 = EXCLUDED.street_line1,
                postal_code = EXCLUDED.postal_code,
                city = EXCLUDED.city,
                country_code = EXCLUDED.country_code,
                vat_id = EXCLUDED.vat_id,
                updated_at = EXCLUDED.updated_at
            """,
            (
                reference,
                str(recipient_name or "").strip(),
                str(recipient_email or "").strip(),
                str(company_name or "").strip(),
                str(street_line1 or "").strip(),
                str(postal_code or "").strip(),
                str(city or "").strip(),
                str(country_code or "DE").strip().upper(),
                str(vat_id or "").strip(),
                now_iso,
            ),
        )

    def _billing_check_and_grant_trial_eligibility(
        self,
        twitch_user_id: str,
        twitch_login: str,
    ) -> bool:
        """Check if user is eligible for Analytics Trial and grant if eligible.

        Returns True if trial was granted, False otherwise.
        """
        paid_plan_ids = {"raid_boost", "analysis_dashboard", "bundle_analysis_raid_boost"}
        grace_period_hours = 24

        def _do_check_and_grant(conn: Any) -> bool:
            # Check if trial was already granted (immutable flag)
            trial_row = conn.execute(
                """
                SELECT trial_ever_granted FROM streamer_plans
                WHERE TRIM(COALESCE(twitch_user_id, '')) = %s
                   OR LOWER(COALESCE(twitch_login, '')) = LOWER(%s)
                LIMIT 1
                """,
                (twitch_user_id, twitch_login),
            ).fetchone()

            if trial_row and int(trial_row[0] or 0) == 1:
                return False

            # Check if first_login_at is more than 24 hours ago
            first_login_row = conn.execute(
                """
                SELECT first_login_at FROM streamer_plans
                WHERE TRIM(COALESCE(twitch_user_id, '')) = %s
                   OR LOWER(COALESCE(twitch_login, '')) = LOWER(%s)
                LIMIT 1
                """,
                (twitch_user_id, twitch_login),
            ).fetchone()

            if not first_login_row or not first_login_row[0]:
                return False

            first_login_at = first_login_row[0]
            if not first_login_at:
                return False

            # Parse first_login_at
            try:
                if isinstance(first_login_at, str):
                    if len(first_login_at) == 10 and first_login_at[4] == "-":
                        first_login_at = f"{first_login_at}T00:00:00+00:00"
                    parsed = datetime.fromisoformat(first_login_at.replace("Z", "+00:00"))
                else:
                    parsed = first_login_at
                if parsed.tzinfo is None:
                    parsed = parsed.replace(tzinfo=UTC)
                else:
                    parsed = parsed.astimezone(UTC)
            except Exception:
                log.debug("Failed to parse first_login_at for trial check: %s", first_login_at)
                return False

            hours_since_login = (datetime.now(tz=UTC) - parsed).total_seconds() / 3600
            if hours_since_login < grace_period_hours:
                return False

            # Check if user already has a paid billing subscription
            billing_row = conn.execute(
                """
                SELECT plan_id FROM twitch_billing_subscriptions
                WHERE LOWER(customer_reference) = LOWER(%s)
                  AND status IN ('active', 'trialing')
                LIMIT 1
                """,
                (twitch_user_id,),
            ).fetchone()

            if billing_row:
                existing_plan = str(billing_row[0] or "").strip()
                if existing_plan in paid_plan_ids:
                    return False

            # Check if user already has a manual paid plan set
            manual_row = conn.execute(
                """
                SELECT manual_plan_id FROM streamer_plans
                WHERE TRIM(COALESCE(twitch_user_id, '')) = %s
                   OR LOWER(COALESCE(twitch_login, '')) = LOWER(%s)
                LIMIT 1
                """,
                (twitch_user_id, twitch_login),
            ).fetchone()

            if manual_row:
                existing_manual_plan = str(manual_row[0] or "").strip()
                if existing_manual_plan and existing_manual_plan != "raid_free":
                    return False

            # Grant the trial and set immutable flag
            trial_expires_at = (datetime.now(tz=UTC) + timedelta(days=_TRIAL_DURATION_DAYS)).isoformat()
            now = datetime.now(tz=UTC).isoformat()
            conn.execute(
                """
                INSERT INTO streamer_plans (twitch_user_id, twitch_login, manual_plan_id, manual_plan_expires_at, trial_ever_granted, manual_plan_notes, manual_plan_updated_at)
                VALUES (%s, %s, %s, %s, 1, 'Trial granted after 24h grace period', %s)
                ON CONFLICT (twitch_user_id) DO UPDATE SET
                    manual_plan_id = EXCLUDED.manual_plan_id,
                    manual_plan_expires_at = EXCLUDED.manual_plan_expires_at,
                    trial_ever_granted = 1,
                    manual_plan_notes = EXCLUDED.manual_plan_notes,
                    manual_plan_updated_at = EXCLUDED.manual_plan_updated_at
                """,
                (twitch_user_id, twitch_login, _ANALYTICS_TRIAL_PLAN_ID, trial_expires_at, now),
            )
            log.info(
                "Granted %d-day Analytics Trial to %s (%s) after %d-hour grace period",
                _TRIAL_DURATION_DAYS,
                twitch_login,
                twitch_user_id,
                grace_period_hours,
            )
            return True

        try:
            with storage.transaction() as conn:
                return _do_check_and_grant(conn)
        except Exception:
            log.exception("Failed to check/grant trial eligibility for %s (%s)", twitch_login, twitch_user_id)
            return False

    def _billing_start_trial_for_user(self, twitch_user_id: str, twitch_login: str) -> str:
        """Manually start trial for a user. Returns status: granted | already_used | has_paid_plan | error."""
        all_paid_plan_ids = {
            "chat_quiet", "raid_boost", "analysis_dashboard",
            "bundle_chat_quiet_raid_boost", "bundle_werbefrei_analyse",
            "bundle_komplett", "bundle_analysis_raid_boost",
        }

        def _do_grant(conn: Any) -> str:
            trial_row = conn.execute(
                "SELECT trial_ever_granted FROM streamer_plans "
                "WHERE TRIM(COALESCE(twitch_user_id,''))=%s OR LOWER(COALESCE(twitch_login,''))=LOWER(%s) LIMIT 1",
                (twitch_user_id, twitch_login),
            ).fetchone()
            if trial_row and int(trial_row[0] or 0) == 1:
                return "already_used"

            billing_row = conn.execute(
                "SELECT plan_id FROM twitch_billing_subscriptions "
                "WHERE LOWER(customer_reference)=LOWER(%s) AND status IN ('active','trialing') LIMIT 1",
                (twitch_user_id,),
            ).fetchone()
            if billing_row and str(billing_row[0] or "").strip() in all_paid_plan_ids:
                return "has_paid_plan"

            manual_row = conn.execute(
                "SELECT manual_plan_id FROM streamer_plans "
                "WHERE TRIM(COALESCE(twitch_user_id,''))=%s OR LOWER(COALESCE(twitch_login,''))=LOWER(%s) LIMIT 1",
                (twitch_user_id, twitch_login),
            ).fetchone()
            if manual_row and str(manual_row[0] or "").strip() in all_paid_plan_ids:
                return "has_paid_plan"

            now = datetime.now(tz=UTC).isoformat()
            trial_expires_at = (datetime.now(tz=UTC) + timedelta(days=_TRIAL_DURATION_DAYS)).isoformat()
            conn.execute(
                """
                INSERT INTO streamer_plans
                    (twitch_user_id, twitch_login, manual_plan_id, manual_plan_expires_at,
                     trial_ever_granted, manual_plan_notes, manual_plan_updated_at)
                VALUES (%s, %s, %s, %s, 1, '30-day trial started by user', %s)
                ON CONFLICT (twitch_user_id) DO UPDATE SET
                    manual_plan_id = EXCLUDED.manual_plan_id,
                    manual_plan_expires_at = EXCLUDED.manual_plan_expires_at,
                    trial_ever_granted = 1,
                    manual_plan_notes = EXCLUDED.manual_plan_notes,
                    manual_plan_updated_at = EXCLUDED.manual_plan_updated_at
                """,
                (twitch_user_id, twitch_login, _ANALYTICS_TRIAL_PLAN_ID, trial_expires_at, now),
            )
            log.info("Trial started by user %s (%s), expires %s", twitch_login, twitch_user_id, trial_expires_at)
            return "granted"

        try:
            with storage.transaction() as conn:
                return _do_grant(conn)
        except Exception:
            log.exception("Trial start failed for %s (%s)", twitch_login, twitch_user_id)
            return "error"

    def _billing_current_plan_for_request(self, request: Any) -> dict[str, Any]:
        """Resolve current plan for authenticated user; fallback to Basic (raid_free)."""
        candidate_refs = self._billing_candidate_refs_for_request(request)
        if not candidate_refs:
            return _entitlements_repository.build_plan_snapshot(
                manual_override=None,
                billing_subscription=None,
                fallback_ref="",
            )

        # Extract twitch_user_id and twitch_login for trial check
        twitch_user_id = ""
        twitch_login = ""
        for ref in candidate_refs:
            if ref and ref.isdigit():
                twitch_user_id = ref
            elif ref:
                twitch_login = ref

        # Check and grant trial eligibility (separate transaction)
        if twitch_user_id and twitch_login:
            try:
                self._billing_check_and_grant_trial_eligibility(twitch_user_id, twitch_login)
            except Exception:
                log.debug("Trial eligibility check failed for %s (%s)", twitch_login, twitch_user_id, exc_info=True)

        try:
            with storage.readonly_connection() as conn:
                return _resolve_plan_snapshot_for_refs(
                    candidate_refs,
                    conn=conn,
                    fallback_ref=candidate_refs[0],
                )
        except Exception:
            log.debug("billing current-plan lookup failed; fallback to basic", exc_info=True)

        return _entitlements_repository.build_plan_snapshot(
            manual_override=None,
            billing_subscription=None,
            fallback_ref=candidate_refs[0] if candidate_refs else "",
        )


    def _billing_admin_plan_rows_for_all_streamers(self) -> list[dict[str, Any]]:
        rows_payload: list[dict[str, Any]] = []
        try:
            with storage.readonly_connection() as conn:
                base_rows = conn.execute(
                    """
                    SELECT
                        s.twitch_login,
                        s.twitch_user_id,
                        s.manual_partner_opt_out,
                        s.archived_at,
                        s.is_on_discord,
                        p.plan_name,
                        p.manual_plan_id,
                        p.manual_plan_expires_at,
                        p.manual_plan_notes,
                        p.manual_plan_updated_at
                    FROM twitch_streamers_partner_state s
                    LEFT JOIN streamer_plans p
                      ON (
                           s.twitch_user_id IS NOT NULL
                           AND s.twitch_user_id = p.twitch_user_id
                         )
                      OR (
                           s.twitch_user_id IS NULL
                           AND LOWER(COALESCE(p.twitch_login, '')) = LOWER(s.twitch_login)
                         )
                    ORDER BY LOWER(s.twitch_login) ASC
                    """
                ).fetchall()
                for row in list(base_rows or []):
                    twitch_login = str(self._billing_row_value(row, "twitch_login", 0, "") or "").strip()
                    twitch_user_id = str(self._billing_row_value(row, "twitch_user_id", 1, "") or "").strip()
                    refs = [value for value in (twitch_user_id, twitch_login) if value]
                    effective_plan = _resolve_plan_snapshot_for_refs(
                        refs,
                        conn=conn,
                        fallback_ref=twitch_login or twitch_user_id,
                    )
                    rows_payload.append(
                        {
                            "twitch_login": twitch_login,
                            "twitch_user_id": twitch_user_id,
                            "manual_partner_opt_out": bool(
                                self._billing_row_value(row, "manual_partner_opt_out", 2, 0)
                            ),
                            "archived_at": str(
                                self._billing_row_value(row, "archived_at", 3, "") or ""
                            ).strip()
                            or None,
                            "is_on_discord": bool(self._billing_row_value(row, "is_on_discord", 4, 0)),
                            "legacy_plan_name": str(
                                self._billing_row_value(row, "plan_name", 5, "free") or "free"
                            ).strip()
                            or "free",
                            "manual_override": effective_plan.get("manual_override"),
                            "billing_subscription": effective_plan.get("billing_subscription"),
                            "effective_plan": effective_plan,
                            "effective_plan_id": str(effective_plan.get("plan_id") or "raid_free"),
                            "effective_plan_source": str(
                                effective_plan.get("source") or "default_basic"
                            ),
                        }
                    )
        except Exception:
            log.exception("billing admin plan rows lookup failed")
            return []
        return rows_payload


    def _billing_admin_set_manual_plan(
        self,
        *,
        twitch_login: str,
        plan_id: Any,
        expires_at: Any = None,
        notes: Any = None,
    ) -> dict[str, Any]:
        normalized_login = str(twitch_login or "").strip().lower()
        if not normalized_login:
            raise ValueError("twitch_login_required")
        normalized_plan_id = self._billing_normalize_plan_id(plan_id)
        if not normalized_plan_id:
            raise ValueError("unknown_plan_id")
        expires_at_dt = self._billing_parse_datetime_value(expires_at)
        expires_at_iso = expires_at_dt.isoformat() if expires_at_dt else None
        notes_value = str(notes or "").strip()
        if len(notes_value) > 1000:
            notes_value = notes_value[:1000]
        updated_at_iso = datetime.now(UTC).isoformat()

        with storage.transaction() as conn:
            streamer_row = conn.execute(
                """
                SELECT twitch_user_id, twitch_login
                FROM twitch_streamers_partner_state
                WHERE LOWER(twitch_login) = LOWER(%s)
                LIMIT 1
                """,
                (normalized_login,),
            ).fetchone()
            if not streamer_row:
                raise ValueError("unknown_streamer")
            twitch_user_id = str(
                self._billing_row_value(streamer_row, "twitch_user_id", 0, "") or ""
            ).strip()
            canonical_login = str(
                self._billing_row_value(streamer_row, "twitch_login", 1, normalized_login)
                or normalized_login
            ).strip()
            if not twitch_user_id:
                raise ValueError("streamer_user_id_missing")

            conn.execute(
                """
                INSERT INTO streamer_plans (twitch_user_id, twitch_login, activated_at)
                VALUES (%s, %s, CURRENT_TIMESTAMP)
                ON CONFLICT (twitch_user_id) DO UPDATE SET
                    twitch_login = EXCLUDED.twitch_login
                """,
                (twitch_user_id, canonical_login),
            )
            conn.execute(
                """
                UPDATE streamer_plans
                SET
                    twitch_login = %s,
                    manual_plan_id = %s,
                    manual_plan_expires_at = %s,
                    manual_plan_notes = %s,
                    manual_plan_updated_at = %s
                WHERE twitch_user_id = %s
                """,
                (
                    canonical_login,
                    normalized_plan_id,
                    expires_at_iso,
                    notes_value,
                    updated_at_iso,
                    twitch_user_id,
                ),
            )

        self._billing_refresh_partner_raid_score_cache(
            twitch_user_id=twitch_user_id,
            twitch_login=canonical_login,
            reason="billing_manual_plan_set",
        )

        for row in self._billing_admin_plan_rows_for_all_streamers():
            if str(row.get("twitch_login") or "").strip().lower() == normalized_login:
                return row
        raise ValueError("manual_plan_save_failed")


    def _billing_admin_clear_manual_plan(self, *, twitch_login: str) -> dict[str, Any]:
        normalized_login = str(twitch_login or "").strip().lower()
        if not normalized_login:
            raise ValueError("twitch_login_required")
        updated_at_iso = datetime.now(UTC).isoformat()

        with storage.transaction() as conn:
            streamer_row = conn.execute(
                """
                SELECT twitch_user_id, twitch_login
                FROM twitch_streamers_partner_state
                WHERE LOWER(twitch_login) = LOWER(%s)
                LIMIT 1
                """,
                (normalized_login,),
            ).fetchone()
            if not streamer_row:
                raise ValueError("unknown_streamer")
            twitch_user_id = str(
                self._billing_row_value(streamer_row, "twitch_user_id", 0, "") or ""
            ).strip()
            if not twitch_user_id:
                raise ValueError("streamer_user_id_missing")

            conn.execute(
                """
                UPDATE streamer_plans
                SET
                    manual_plan_id = NULL,
                    manual_plan_expires_at = NULL,
                    manual_plan_notes = '',
                    manual_plan_updated_at = %s
                WHERE twitch_user_id = %s
                """,
                (updated_at_iso, twitch_user_id),
            )

        self._billing_refresh_partner_raid_score_cache(
            twitch_user_id=twitch_user_id,
            twitch_login=normalized_login,
            reason="billing_manual_plan_clear",
        )

        for row in self._billing_admin_plan_rows_for_all_streamers():
            if str(row.get("twitch_login") or "").strip().lower() == normalized_login:
                return row
        raise ValueError("manual_plan_clear_failed")


    def _billing_build_invoice_preview(
        self,
        *,
        plan: dict[str, Any],
        cycle_months: int,
        quantity: int,
        customer_reference: str,
        customer_name: str,
        customer_email: str,
        customer_profile: dict[str, str] | None = None,
    ) -> dict[str, Any]:
        now_utc = datetime.now(UTC)
        line_total_cents = int((plan.get("price") or {}).get("total_net_cents") or 0)
        subtotal_cents = line_total_cents * max(1, int(quantity or 1))
        invoice_number = f"INV-{now_utc.strftime('%Y%m%d')}-{uuid4().hex[:8].upper()}"
        profile = dict(customer_profile or {})
        company_name = str(profile.get("company_name") or "").strip()
        street_line1 = str(profile.get("street_line1") or "").strip()
        postal_code = str(profile.get("postal_code") or "").strip()
        city = str(profile.get("city") or "").strip()
        country_code = str(profile.get("country_code") or "").strip().upper()
        vat_id = str(profile.get("vat_id") or "").strip()
        return {
            "invoice_number": invoice_number,
            "issued_at": now_utc.isoformat(),
            "issued_date": now_utc.date().isoformat(),
            "due_date": now_utc.date().isoformat(),
            "currency": "EUR",
            "seller": {
                "name": "Deadlock Partner Network",
                "address": "EarlySalty Operations",
                "email": "billing@deutsche-deadlock-community.de",
                "website": "https://deutsche-deadlock-community.de",
            },
            "customer": {
                "reference": customer_reference,
                "name": customer_name,
                "email": customer_email,
                "company_name": company_name,
                "street_line1": street_line1,
                "postal_code": postal_code,
                "city": city,
                "country_code": country_code,
                "vat_id": vat_id,
            },
            "line_items": [
                {
                    "plan_id": str(plan.get("id") or ""),
                    "name": str(plan.get("name") or "Abo"),
                    "description": str(plan.get("description") or ""),
                    "cycle_months": cycle_months,
                    "cycle_label": _billing_cycle_label(cycle_months),
                    "quantity": max(1, int(quantity or 1)),
                    "unit_net_cents": line_total_cents,
                    "line_total_net_cents": subtotal_cents,
                    "unit_net_label": _format_eur_cents(line_total_cents),
                    "line_total_net_label": _format_eur_cents(subtotal_cents),
                }
            ],
            "totals": {
                "subtotal_net_cents": subtotal_cents,
                "subtotal_net_label": _format_eur_cents(subtotal_cents),
                "tax_cents": 0,
                "tax_label": _format_eur_cents(0),
                "grand_total_cents": subtotal_cents,
                "grand_total_label": _format_eur_cents(subtotal_cents),
                "tax_mode": "net_only",
                "tax_note": "Nettoabrechnung - die finale Steuerberechnung erfolgt im Bezahlvorgang.",
            },
        }


    def _billing_render_invoice_html(self, invoice: dict[str, Any]) -> str:
        customer = dict(invoice.get("customer") or {})
        seller = dict(invoice.get("seller") or {})
        totals = dict(invoice.get("totals") or {})
        line_item = (list(invoice.get("line_items") or []) or [{}])[0]
        plan_name = html.escape(str(line_item.get("name") or "Abo"))
        cycle_label = html.escape(str(line_item.get("cycle_label") or ""))
        customer_name = html.escape(str(customer.get("name") or "Partner"))
        customer_email = html.escape(str(customer.get("email") or ""))
        customer_reference = html.escape(str(customer.get("reference") or ""))
        customer_company_name = html.escape(str(customer.get("company_name") or ""))
        customer_street_line1 = html.escape(str(customer.get("street_line1") or ""))
        customer_postal_code = html.escape(str(customer.get("postal_code") or ""))
        customer_city = html.escape(str(customer.get("city") or ""))
        customer_country_code = html.escape(str(customer.get("country_code") or ""))
        customer_vat_id = html.escape(str(customer.get("vat_id") or ""))
        customer_city_line = " ".join(
            value for value in [customer_postal_code, customer_city] if value
        ).strip()
        customer_rows = [f"<p><strong>{customer_name}</strong></p>"]
        if customer_company_name:
            customer_rows.append(f"<p>{customer_company_name}</p>")
        if customer_street_line1:
            customer_rows.append(f"<p>{customer_street_line1}</p>")
        if customer_city_line:
            customer_rows.append(f"<p>{customer_city_line}</p>")
        if customer_country_code:
            customer_rows.append(f"<p>{customer_country_code}</p>")
        if customer_email:
            customer_rows.append(f"<p>{customer_email}</p>")
        if customer_vat_id:
            customer_rows.append(f"<p>USt-IdNr: {customer_vat_id}</p>")
        customer_rows.append(f"<p>Referenz: {customer_reference}</p>")
        customer_rows_html = "".join(customer_rows)
        invoice_number = html.escape(str(invoice.get("invoice_number") or ""))
        issued_date = html.escape(str(invoice.get("issued_date") or ""))
        due_date = html.escape(str(invoice.get("due_date") or ""))
        return (
            "<!doctype html><html lang='de'><head><meta charset='utf-8'>"
            "<meta name='viewport' content='width=device-width,initial-scale=1'>"
            "<title>Rechnung</title>"
            "<style>"
            "body{margin:0;background:#f3f4f6;color:#0f172a;font-family:'Segoe UI',Arial,sans-serif;}"
            ".wrap{max-width:920px;margin:28px auto;padding:0 16px 30px;}"
            ".invoice{background:#fff;border:1px solid #d1d5db;border-radius:18px;"
            "box-shadow:0 18px 50px rgba(15,23,42,.08);overflow:hidden;}"
            ".hero{background:linear-gradient(135deg,#0f172a,#1d4ed8);color:#e2e8f0;"
            "padding:20px 24px;display:flex;justify-content:space-between;gap:16px;flex-wrap:wrap;}"
            ".hero h1{margin:0;font-size:1.5rem;letter-spacing:.03em;}"
            ".meta{font-size:13px;line-height:1.5;}"
            ".addr-row{display:grid;grid-template-columns:1fr 1fr;gap:14px;padding:16px 24px 0;}"
            ".box{border:1px solid #e5e7eb;border-radius:12px;padding:12px;background:#fafafa;}"
            ".box h2{margin:0 0 8px;font-size:1rem;}"
            ".box p{margin:4px 0;font-size:13px;color:#334155;}"
            "table{width:100%;border-collapse:collapse;margin:14px 0 0;}"
            "th,td{padding:12px 24px;border-bottom:1px solid #e5e7eb;text-align:left;font-size:13px;}"
            "th{font-size:12px;color:#475569;text-transform:uppercase;letter-spacing:.02em;background:#f8fafc;}"
            ".totals-full{padding:18px 24px 0;}"
            ".totals-card{width:100%;border:1px solid #e5e7eb;border-radius:12px;padding:12px;background:#f8fafc;}"
            ".row{display:flex;justify-content:space-between;font-size:13px;padding:6px 0;color:#334155;}"
            ".row.total{border-top:1px solid #d1d5db;margin-top:6px;padding-top:10px;"
            "font-size:16px;font-weight:700;color:#0f172a;}"
            ".row.net-note{font-size:12px;color:#94a3b8;padding-top:8px;border-top:none;}"
            ".actions{display:flex;gap:10px;flex-wrap:wrap;padding:18px 24px 24px;}"
            ".btn{display:inline-block;padding:9px 13px;border-radius:10px;text-decoration:none;font-weight:600;}"
            ".btn-primary{background:#2563eb;color:#fff;}"
            ".btn-ghost{background:#fff;color:#0f172a;border:1px solid #cbd5e1;cursor:pointer;}"
            "@media print{"
            ".actions{display:none;}"
            "body{background:#fff;}"
            ".invoice{box-shadow:none;border:none;}"
            "}"
            "</style></head><body><main class='wrap'>"
            "<article class='invoice'>"
            "<header class='hero'>"
            "<div><h1>Rechnung</h1>"
            "<div class='meta'>Deadlock Partner Network<br>billing@deutsche-deadlock-community.de</div></div>"
            "<div class='meta'>"
            f"Rechnungsnr: <strong>{invoice_number}</strong><br>"
            f"Ausgestellt: {issued_date}<br>"
            f"F&auml;llig: {due_date}</div>"
            "</header>"
            "<section class='addr-row'>"
            "<div class='box'><h2>Von</h2>"
            f"<p><strong>{html.escape(str(seller.get('name') or ''))}</strong></p>"
            f"<p>{html.escape(str(seller.get('address') or ''))}</p>"
            f"<p>{html.escape(str(seller.get('email') or ''))}</p>"
            "</div>"
            "<div class='box'><h2>Rechnung an</h2>"
            f"{customer_rows_html}"
            "</div>"
            "</section>"
            "<table><thead><tr>"
            "<th>Leistung</th><th>Zyklus</th><th>Menge</th>"
            "<th>Einzelpreis (netto)</th><th>Gesamt (netto)</th>"
            "</tr></thead><tbody>"
            "<tr>"
            f"<td>{plan_name}</td>"
            f"<td>{cycle_label}</td>"
            f"<td>{int(line_item.get('quantity') or 1)}</td>"
            f"<td>{html.escape(str(line_item.get('unit_net_label') or '0,00 EUR'))}</td>"
            f"<td>{html.escape(str(line_item.get('line_total_net_label') or '0,00 EUR'))}</td>"
            "</tr></tbody></table>"
            "<div class='totals-full'><div class='totals-card'>"
            f"<div class='row'><span>Zwischensumme (netto)</span>"
            f"<strong>{html.escape(str(totals.get('subtotal_net_label') or '0,00 EUR'))}</strong></div>"
            "<div class='row'><span>MwSt. (wird im Checkout berechnet)</span><strong>—</strong></div>"
            f"<div class='row total'><span>Gesamtbetrag (netto)</span>"
            f"<strong>{html.escape(str(totals.get('grand_total_label') or '0,00 EUR'))}</strong></div>"
            "<div class='row net-note'>"
            f"<span>{html.escape(str(totals.get('tax_note') or 'Nettoabrechnung — Steuer wird im Stripe Checkout konfiguriert.'))}</span>"
            "</div>"
            "</div></div>"
            "</article>"
            "<div class='actions'>"
            "<a class='btn btn-primary' href='/twitch/abbo'>Zur Abo &Uuml;bersicht</a>"
            "<button class='btn btn-ghost' onclick='window.print()'>Als PDF speichern</button>"
            "</div>"
            "</main></body></html>"
        )


    def _billing_stripe_readiness_payload(self) -> dict[str, Any]:
        self._billing_refresh_runtime_secrets()
        publishable_key = str(getattr(self, "_billing_stripe_publishable_key", "") or "").strip()
        secret_key = str(getattr(self, "_billing_stripe_secret_key", "") or "").strip()
        webhook_secret = str(getattr(self, "_billing_stripe_webhook_secret", "") or "").strip()
        success_url = str(getattr(self, "_billing_checkout_success_url", "") or "").strip()
        cancel_url = str(getattr(self, "_billing_checkout_cancel_url", "") or "").strip()
        mapping_stats = self._billing_price_mapping_stats()
        mapped_slots = int(mapping_stats.get("mapped_slots") or 0)
        required_slots = int(mapping_stats.get("required_slots") or 0)
        price_map_ready = bool(mapping_stats.get("ready"))

        checks = [
            {
                "id": "stripe_publishable_key",
                "label": "Stripe Publishable Key",
                "ready": bool(publishable_key),
                "env_keys": [
                    "STRIPE_PUBLISHABLE_KEY",
                    "TWITCH_BILLING_STRIPE_PUBLISHABLE_KEY",
                ],
                "value_preview": _billing_value_preview(publishable_key, secret=True),
            },
            {
                "id": "stripe_secret_key",
                "label": "Stripe Secret Key",
                "ready": bool(secret_key),
                "env_keys": [
                    "STRIPE_SECRET_KEY",
                    "TWITCH_BILLING_STRIPE_SECRET_KEY",
                ],
                "value_preview": _billing_value_preview(secret_key, secret=True),
            },
            {
                "id": "stripe_webhook_secret",
                "label": "Stripe Webhook Secret",
                "ready": bool(webhook_secret),
                "env_keys": [
                    "STRIPE_WEBHOOK_SECRET",
                    "TWITCH_BILLING_STRIPE_WEBHOOK_SECRET",
                ],
                "value_preview": _billing_value_preview(webhook_secret, secret=True),
            },
            {
                "id": "checkout_success_url",
                "label": "Checkout Success URL",
                "ready": bool(success_url),
                "env_keys": [
                    "STRIPE_CHECKOUT_SUCCESS_URL",
                    "TWITCH_BILLING_CHECKOUT_SUCCESS_URL",
                ],
                "value_preview": _billing_value_preview(success_url, secret=False),
            },
            {
                "id": "checkout_cancel_url",
                "label": "Checkout Cancel URL",
                "ready": bool(cancel_url),
                "env_keys": [
                    "STRIPE_CHECKOUT_CANCEL_URL",
                    "TWITCH_BILLING_CHECKOUT_CANCEL_URL",
                ],
                "value_preview": _billing_value_preview(cancel_url, secret=False),
            },
            {
                "id": "stripe_price_id_map",
                "label": "Stripe Product/Price Mapping",
                "ready": price_map_ready,
                "env_keys": [
                    "STRIPE_PRICE_ID_MAP",
                    "TWITCH_BILLING_STRIPE_PRICE_ID_MAP",
                ],
                "value_preview": f"{mapped_slots}/{required_slots} Price IDs",
            },
        ]
        missing = [str(check["id"]) for check in checks if not bool(check["ready"])]
        checkout_ready = all(
            (
                bool(publishable_key),
                bool(secret_key),
                bool(success_url),
                bool(cancel_url),
            )
        )
        webhook_ready = bool(webhook_secret)
        return {
            "provider": "stripe",
            "integration_state": "live" if (checkout_ready and price_map_ready) else "planned",
            "checkout_ready": checkout_ready,
            "webhook_ready": webhook_ready,
            "price_map_ready": price_map_ready,
            "required_price_ids": required_slots,
            "mapped_price_ids": mapped_slots,
            "missing_price_slots": list(mapping_stats.get("missing_slots") or []),
            "ready_for_live": bool(checkout_ready and webhook_ready and price_map_ready),
            "checks": checks,
            "missing": missing,
        }


    async def _billing_read_request_body(self, request: web.Request) -> dict[str, Any]:
        body: dict[str, Any] = {}
        try:
            payload = await request.json()
            if isinstance(payload, dict):
                body = dict(payload)
        except Exception:
            body = {}
        if body:
            return body
        try:
            return dict(await request.post())
        except Exception:
            return {}


    @staticmethod
    def _billing_parse_http_url(raw: str | None):
        value = str(raw or "").strip()
        if not value:
            return None
        try:
            parsed = urlsplit(value)
        except Exception:
            return None
        scheme = str(parsed.scheme or "").strip().lower()
        host = str(parsed.hostname or "").strip().lower()
        if scheme not in {"http", "https"}:
            return None
        if not host or not parsed.netloc:
            return None
        if parsed.username or parsed.password:
            return None
        if scheme == "http" and host not in {"127.0.0.1", "localhost", "::1"}:
            return None
        return parsed


    def _billing_checkout_allowed_redirect_hosts(self) -> list[str]:
        hosts: list[str] = []
        seen: set[str] = set()

        def _add_host(raw_host: str | None) -> None:
            host = str(raw_host or "").strip().lower()
            if not host or host in seen:
                return
            seen.add(host)
            hosts.append(host)

        for raw_url in (
            getattr(self, "_billing_checkout_success_url", ""),
            getattr(self, "_billing_checkout_cancel_url", ""),
        ):
            parsed = self._billing_parse_http_url(raw_url)
            if parsed:
                _add_host(parsed.hostname)

        # Optional operator override (comma/space separated host patterns):
        # - exact host: example.com
        # - wildcard suffix: *.example.com
        raw_allowlist = (
            os.getenv("TWITCH_BILLING_CHECKOUT_ALLOWED_HOSTS")
            or os.getenv("STRIPE_CHECKOUT_ALLOWED_HOSTS")
            or ""
        )
        for token in str(raw_allowlist).replace(";", ",").replace(" ", ",").split(","):
            _add_host(token)

        # Keep localhost available for local/test environments.
        for local_host in ("localhost", "127.0.0.1", "::1"):
            _add_host(local_host)

        return hosts


    def _billing_is_http_url(self, raw: str | None) -> bool:
        parsed = self._billing_parse_http_url(raw)
        if not parsed:
            return False
        host = str(parsed.hostname or "").strip().lower()
        if not host:
            return False

        for candidate in self._billing_checkout_allowed_redirect_hosts():
            rule = str(candidate or "").strip().lower()
            if not rule:
                continue
            if rule.startswith("*."):
                suffix = rule[2:]
                if host == suffix or host.endswith(f".{suffix}"):
                    return True
                continue
            if host == rule:
                return True
        return False
