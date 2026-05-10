"""Synchronous affiliate admin helpers for dashboard endpoints."""

from __future__ import annotations

from datetime import UTC, datetime
from typing import Any, Callable

from ..core.twitch_login import normalize_twitch_login
from ..dashboard.affiliate.gutschrift import AffiliateGutschriftService
from ..storage import pg as storage

_AFFILIATE_REVENUE_STATUSES: tuple[str, ...] = ("pending", "transferred")
_AFFILIATE_REVENUE_STATUS_PLACEHOLDERS = ", ".join(["%s"] * len(_AFFILIATE_REVENUE_STATUSES))


class AdminAffiliateNotFoundError(LookupError):
    """Raised when the requested affiliate resource does not exist."""


def _row_get_value(row: Any, key: str, index: int, default: Any = None) -> Any:
    if row is None:
        return default
    if hasattr(row, "get"):
        return row.get(key, default)
    values = tuple(row)
    return values[index] if index < len(values) else default


def _safe_int(value: Any, default: int = 0) -> int:
    try:
        return int(value)
    except (TypeError, ValueError):
        return default


def _normalize_login(raw_value: str) -> str | None:
    return normalize_twitch_login(raw_value)


def load_admin_affiliates_list(
    *,
    prepare_conn: Callable[[Any], None],
    is_missing_schema_error: Callable[[Exception], bool],
) -> dict[str, Any]:
    try:
        with storage.transaction() as conn:
            prepare_conn(conn)
            try:
                rows = conn.execute(  # nosemgrep: python.sqlalchemy.security.sqlalchemy-execute-raw-query.sqlalchemy-execute-raw-query
                    f"""
                    SELECT
                        a.twitch_login,
                        a.display_name,
                        a.is_active,
                        a.created_at,
                        COALESCE(claim_stats.total_claims, 0)       AS total_claims,
                        COALESCE(comm_stats.total_provision, 0)     AS total_provision,
                        claim_stats.last_claim_at,
                        COALESCE(pii.ust_status, 'unknown')         AS ust_status,
                        CASE WHEN pii.twitch_login IS NOT NULL THEN 1 ELSE 0 END AS has_pii
                    FROM affiliate_accounts a
                    LEFT JOIN affiliate_pii pii
                      ON pii.twitch_login = a.twitch_login
                    LEFT JOIN (
                        SELECT
                            affiliate_twitch_login,
                            COUNT(*) AS total_claims,
                            MAX(claimed_at) AS last_claim_at
                        FROM affiliate_streamer_claims
                        GROUP BY affiliate_twitch_login
                    ) claim_stats ON claim_stats.affiliate_twitch_login = a.twitch_login
                    LEFT JOIN (
                        SELECT
                            affiliate_twitch_login,
                            SUM(
                                CASE
                                    WHEN status IN ({_AFFILIATE_REVENUE_STATUS_PLACEHOLDERS})
                                    THEN commission_cents
                                    ELSE 0
                                END
                            ) AS total_provision
                        FROM affiliate_commissions
                        GROUP BY affiliate_twitch_login
                    ) comm_stats ON comm_stats.affiliate_twitch_login = a.twitch_login
                    ORDER BY a.created_at DESC
                    """,
                    [*_AFFILIATE_REVENUE_STATUSES],
                ).fetchall()
            except Exception as exc:
                if not is_missing_schema_error(exc):
                    raise
                rows = conn.execute(  # nosemgrep: python.sqlalchemy.security.sqlalchemy-execute-raw-query.sqlalchemy-execute-raw-query
                    f"""
                    SELECT
                        a.twitch_login,
                        a.display_name,
                        a.is_active,
                        a.created_at,
                        COALESCE(claim_stats.total_claims, 0)       AS total_claims,
                        COALESCE(comm_stats.total_provision, 0)     AS total_provision,
                        claim_stats.last_claim_at,
                        'unknown'                                   AS ust_status,
                        0                                           AS has_pii
                    FROM affiliate_accounts a
                    LEFT JOIN (
                        SELECT
                            affiliate_twitch_login,
                            COUNT(*) AS total_claims,
                            MAX(claimed_at) AS last_claim_at
                        FROM affiliate_streamer_claims
                        GROUP BY affiliate_twitch_login
                    ) claim_stats ON claim_stats.affiliate_twitch_login = a.twitch_login
                    LEFT JOIN (
                        SELECT
                            affiliate_twitch_login,
                            SUM(
                                CASE
                                    WHEN status IN ({_AFFILIATE_REVENUE_STATUS_PLACEHOLDERS})
                                    THEN commission_cents
                                    ELSE 0
                                END
                            ) AS total_provision
                        FROM affiliate_commissions
                        GROUP BY affiliate_twitch_login
                    ) comm_stats ON comm_stats.affiliate_twitch_login = a.twitch_login
                    ORDER BY a.created_at DESC
                    """,
                    [*_AFFILIATE_REVENUE_STATUSES],
                ).fetchall()
    except Exception as exc:
        if is_missing_schema_error(exc):
            return {"affiliates": []}
        raise

    affiliates = []
    for row in rows:
        total_provision_cents = _safe_int(_row_get_value(row, "total_provision", 5, 0), default=0)
        affiliates.append(
            {
                "login": str(_row_get_value(row, "twitch_login", 0, "") or "").strip(),
                "display_name": _row_get_value(row, "display_name", 1, None),
                "active": bool(_safe_int(_row_get_value(row, "is_active", 2, 1), default=1)),
                "total_claims": _safe_int(_row_get_value(row, "total_claims", 4, 0), default=0),
                "total_provision": round(total_provision_cents / 100.0, 2),
                "created_at": _row_get_value(row, "created_at", 3, None),
                "last_claim_at": _row_get_value(row, "last_claim_at", 6, None),
                "ust_status": str(_row_get_value(row, "ust_status", 7, "unknown") or "unknown").strip()
                or "unknown",
                "has_pii": bool(_safe_int(_row_get_value(row, "has_pii", 8, 0), default=0)),
            }
        )
    return {"affiliates": affiliates}


def load_admin_affiliate_gutschriften(
    *,
    prepare_conn: Callable[[Any], None],
    is_missing_schema_error: Callable[[Exception], bool],
    build_download_path: Callable[[int], str | None],
) -> dict[str, Any]:
    try:
        with storage.transaction() as conn:
            prepare_conn(conn)
            try:
                rows = conn.execute(
                    """
                    SELECT
                        g.id,
                        g.affiliate_twitch_login,
                        g.period_year,
                        g.period_month,
                        g.gutschrift_number,
                        g.net_amount_cents,
                        g.vat_amount_cents,
                        g.gross_amount_cents,
                        g.commission_ids,
                        g.affiliate_ust_status,
                        g.email_error,
                        g.pdf_generated_at,
                        g.email_sent_at,
                        g.created_at,
                        CASE WHEN g.pdf_blob IS NOT NULL THEN 1 ELSE NULL END AS pdf_blob,
                        a.display_name,
                        a.is_active,
                        COALESCE(pii.ust_status, 'unknown') AS ust_status,
                        CASE WHEN pii.twitch_login IS NOT NULL THEN 1 ELSE 0 END AS has_pii
                    FROM affiliate_gutschriften g
                    JOIN affiliate_accounts a
                      ON a.twitch_login = g.affiliate_twitch_login
                    LEFT JOIN affiliate_pii pii
                      ON pii.twitch_login = g.affiliate_twitch_login
                    ORDER BY g.period_year DESC, g.period_month DESC, g.id DESC
                    """
                ).fetchall()
            except Exception as exc:
                if not is_missing_schema_error(exc):
                    raise
                rows = conn.execute(
                    """
                    SELECT
                        g.id,
                        g.affiliate_twitch_login,
                        g.period_year,
                        g.period_month,
                        g.gutschrift_number,
                        g.net_amount_cents,
                        g.vat_amount_cents,
                        g.gross_amount_cents,
                        g.commission_ids,
                        g.affiliate_ust_status,
                        g.email_error,
                        g.pdf_generated_at,
                        g.email_sent_at,
                        g.created_at,
                        CASE WHEN g.pdf_blob IS NOT NULL THEN 1 ELSE NULL END AS pdf_blob,
                        a.display_name,
                        a.is_active,
                        'unknown' AS ust_status,
                        0 AS has_pii
                    FROM affiliate_gutschriften g
                    JOIN affiliate_accounts a
                      ON a.twitch_login = g.affiliate_twitch_login
                    ORDER BY g.period_year DESC, g.period_month DESC, g.id DESC
                    """
                ).fetchall()
    except Exception as exc:
        if is_missing_schema_error(exc):
            return {"gutschriften": [], "count": 0}
        raise

    documents = []
    for row in rows:
        payload = dict(AffiliateGutschriftService._row_to_metadata(row))
        row_id = _safe_int(payload.get("id"), default=0)
        payload["download_path"] = build_download_path(row_id)
        payload["affiliate_login"] = str(_row_get_value(row, "affiliate_twitch_login", 1, "") or "").strip()
        payload["display_name"] = _row_get_value(row, "display_name", 15, None)
        payload["active"] = bool(_safe_int(_row_get_value(row, "is_active", 16, 1), default=1))
        payload["ust_status"] = str(_row_get_value(row, "ust_status", 17, "unknown") or "unknown").strip() or "unknown"
        payload["has_pii"] = bool(_safe_int(_row_get_value(row, "has_pii", 18, 0), default=0))
        documents.append(payload)
    return {"gutschriften": documents, "count": len(documents)}


def load_admin_affiliate_gutschriften_for_login(
    login: str,
    *,
    prepare_conn: Callable[[Any], None],
    is_missing_schema_error: Callable[[Exception], bool],
    load_pii: Callable[[Any, str], dict[str, Any]],
    build_summary: Callable[..., dict[str, Any]],
    build_download_path: Callable[[int], str | None],
) -> dict[str, Any]:
    try:
        with storage.transaction() as conn:
            prepare_conn(conn)
            account_row = conn.execute(
                """
                SELECT twitch_login, display_name, is_active, created_at, updated_at
                FROM affiliate_accounts
                WHERE twitch_login = %s
                """,
                (login,),
            ).fetchone()
            if not account_row:
                raise AdminAffiliateNotFoundError(login)

            pii = load_pii(conn, login)
            readiness = AffiliateGutschriftService.build_readiness(pii)
            summary = build_summary(conn, affiliate_login=login)
            try:
                documents = AffiliateGutschriftService.list_for_affiliate(conn, login)
            except Exception as exc:
                if not is_missing_schema_error(exc):
                    raise
                documents = []
    except Exception as exc:
        if isinstance(exc, AdminAffiliateNotFoundError):
            raise
        if is_missing_schema_error(exc):
            raise AdminAffiliateNotFoundError(login) from exc
        raise

    items = []
    for document in documents:
        payload = dict(document)
        row_id = _safe_int(payload.get("id"), default=0)
        payload["download_path"] = build_download_path(row_id)
        items.append(payload)

    affiliate = {
        "login": str(_row_get_value(account_row, "twitch_login", 0, "") or "").strip(),
        "display_name": _row_get_value(account_row, "display_name", 1, None),
        "active": bool(_safe_int(_row_get_value(account_row, "is_active", 2, 1), default=1)),
        "created_at": _row_get_value(account_row, "created_at", 3, None),
        "updated_at": _row_get_value(account_row, "updated_at", 4, None),
    }
    return {
        "affiliate": affiliate,
        "ust_status": str(pii.get("ust_status") or "unknown"),
        "readiness": readiness,
        "gutschriften_summary": summary,
        "gutschriften": items,
    }


def load_admin_affiliate_gutschrift_pdf(
    gutschrift_id: int,
    *,
    prepare_conn: Callable[[Any], None],
    is_missing_schema_error: Callable[[Exception], bool],
) -> tuple[dict[str, Any], bytes] | None:
    try:
        with storage.transaction() as conn:
            prepare_conn(conn)
            row = conn.execute(
                """
                SELECT affiliate_twitch_login
                FROM affiliate_gutschriften
                WHERE id = %s
                """,
                (gutschrift_id,),
            ).fetchone()
            if not row:
                return None

            affiliate_login = _normalize_login(_row_get_value(row, "affiliate_twitch_login", 0, ""))
            if not affiliate_login:
                return None

            return AffiliateGutschriftService.get_pdf(
                conn,
                affiliate_login=affiliate_login,
                gutschrift_id=gutschrift_id,
            )
    except Exception as exc:
        if is_missing_schema_error(exc):
            return None
        raise


def load_admin_affiliate_detail(
    login: str,
    *,
    prepare_conn: Callable[[Any], None],
    is_missing_schema_error: Callable[[Exception], bool],
    load_pii: Callable[[Any, str], dict[str, Any]],
    build_summary: Callable[..., dict[str, Any]],
) -> dict[str, Any]:
    try:
        with storage.transaction() as conn:
            prepare_conn(conn)
            account_row = conn.execute(
                """
                SELECT
                    twitch_login, display_name, is_active, created_at,
                    email, stripe_connect_status, stripe_account_id, updated_at
                FROM affiliate_accounts
                WHERE twitch_login = %s
                """,
                (login,),
            ).fetchone()
            if not account_row:
                raise AdminAffiliateNotFoundError(login)

            claim_rows = conn.execute(  # nosemgrep: python.sqlalchemy.security.sqlalchemy-execute-raw-query.sqlalchemy-execute-raw-query
                f"""
                SELECT
                    c.id,
                    c.claimed_streamer_login,
                    c.claimed_at,
                    COALESCE(SUM(co.commission_cents), 0) AS commission_cents,
                    COUNT(co.id) AS commission_count
                FROM affiliate_streamer_claims c
                LEFT JOIN affiliate_commissions co
                    ON co.affiliate_twitch_login = c.affiliate_twitch_login
                    AND co.streamer_login = c.claimed_streamer_login
                    AND co.status IN ({_AFFILIATE_REVENUE_STATUS_PLACEHOLDERS})
                WHERE c.affiliate_twitch_login = %s
                GROUP BY c.id, c.claimed_streamer_login, c.claimed_at
                ORDER BY c.claimed_at DESC
                """,
                (*_AFFILIATE_REVENUE_STATUSES, login),
            ).fetchall()

            claim_stats_row = conn.execute(
                """
                SELECT COUNT(*) AS total_claims
                FROM affiliate_streamer_claims
                WHERE affiliate_twitch_login = %s
                """,
                (login,),
            ).fetchone()

            commission_stats_row = conn.execute(  # nosemgrep: python.sqlalchemy.security.sqlalchemy-execute-raw-query.sqlalchemy-execute-raw-query
                f"""
                SELECT
                    COALESCE(SUM(commission_cents), 0) AS total_provision,
                    COUNT(DISTINCT streamer_login) AS active_customers
                FROM affiliate_commissions
                WHERE affiliate_twitch_login = %s
                  AND status IN ({_AFFILIATE_REVENUE_STATUS_PLACEHOLDERS})
                """,
                (login, *_AFFILIATE_REVENUE_STATUSES),
            ).fetchone()

            pii = load_pii(conn, login)
            readiness = AffiliateGutschriftService.build_readiness(pii)
            gutschriften_summary = build_summary(conn, affiliate_login=login)
    except Exception as exc:
        if isinstance(exc, AdminAffiliateNotFoundError):
            raise
        if is_missing_schema_error(exc):
            raise AdminAffiliateNotFoundError(login) from exc
        raise

    stripe_id = str(_row_get_value(account_row, "stripe_account_id", 6, "") or "")
    masked_stripe = f"{stripe_id[:8]}...{stripe_id[-4:]}" if len(stripe_id) > 12 else stripe_id
    claims = [
        {
            "id": _safe_int(_row_get_value(row, "id", 0, 0), default=0),
            "customer_login": str(_row_get_value(row, "claimed_streamer_login", 1, "") or "").strip(),
            "claimed_at": _row_get_value(row, "claimed_at", 2, None),
            "commission_cents": _safe_int(_row_get_value(row, "commission_cents", 3, 0), default=0),
            "commission_count": _safe_int(_row_get_value(row, "commission_count", 4, 0), default=0),
        }
        for row in claim_rows
    ]

    total_claims = _safe_int(_row_get_value(claim_stats_row, "total_claims", 0, 0), default=0)
    total_provision_cents = _safe_int(
        _row_get_value(commission_stats_row, "total_provision", 0, 0),
        default=0,
    )
    return {
        "affiliate": {
            "login": str(_row_get_value(account_row, "twitch_login", 0, "") or "").strip(),
            "display_name": _row_get_value(account_row, "display_name", 1, None),
            "active": bool(_safe_int(_row_get_value(account_row, "is_active", 2, 1), default=1)),
            "created_at": _row_get_value(account_row, "created_at", 3, None),
            "email": _row_get_value(account_row, "email", 4, None),
            "stripe_connect_status": _row_get_value(account_row, "stripe_connect_status", 5, None),
            "stripe_account_id": masked_stripe or None,
            "updated_at": _row_get_value(account_row, "updated_at", 7, None),
        },
        "claims": claims,
        "stats": {
            "total_claims": total_claims,
            "total_provision": round(total_provision_cents / 100.0, 2),
            "avg_provision": round((total_provision_cents / max(total_claims, 1)) / 100.0, 2)
            if total_claims > 0
            else 0.0,
            "active_customers": _safe_int(
                _row_get_value(commission_stats_row, "active_customers", 1, 0),
                default=0,
            ),
        },
        "ust_status": str(pii.get("ust_status") or "unknown"),
        "pii_readiness": readiness,
        "gutschriften_summary": {
            "count": gutschriften_summary["total_gutschriften"],
            "total_gross_cents": gutschriften_summary["total_gutschrift_amount_cents"],
        },
    }


def toggle_admin_affiliate(login: str) -> dict[str, Any]:
    try:
        with storage.transaction() as conn:
            row = conn.execute(
                "SELECT is_active FROM affiliate_accounts WHERE twitch_login = %s",
                (login,),
            ).fetchone()
            if not row:
                raise AdminAffiliateNotFoundError(login)

            current = _safe_int(_row_get_value(row, "is_active", 0, 1), default=1)
            new_status = 0 if current else 1
            now = datetime.now(UTC).isoformat()
            conn.execute(
                """
                UPDATE affiliate_accounts
                SET is_active = %s, updated_at = %s
                WHERE twitch_login = %s
                """,
                (new_status, now, login),
            )
    except Exception as exc:
        if isinstance(exc, AdminAffiliateNotFoundError):
            raise
        normalized = str(exc).strip().lower()
        if any(marker in normalized for marker in ("does not exist", "no such table", "undefined table")):
            raise AdminAffiliateNotFoundError(login) from exc
        raise
    return {"login": login, "active": bool(new_status)}


def load_admin_affiliate_stats(
    *,
    month_start_iso: str,
    prepare_conn: Callable[[Any], None],
    is_missing_schema_error: Callable[[Exception], bool],
    build_summary: Callable[..., dict[str, Any]],
) -> dict[str, Any]:
    try:
        with storage.transaction() as conn:
            prepare_conn(conn)
            account_row = conn.execute(
                """
                SELECT
                    COUNT(*) AS total_affiliates,
                    COALESCE(SUM(CASE WHEN is_active = 1 THEN 1 ELSE 0 END), 0) AS active_affiliates
                FROM affiliate_accounts
                """
            ).fetchone()

            claim_row = conn.execute(
                """
                SELECT
                    COUNT(*) AS total_claims,
                    COALESCE(SUM(CASE WHEN claimed_at >= %s THEN 1 ELSE 0 END), 0)
                        AS this_month_claims
                FROM affiliate_streamer_claims
                """,
                (month_start_iso,),
            ).fetchone()

            commission_row = conn.execute(  # nosemgrep: python.sqlalchemy.security.sqlalchemy-execute-raw-query.sqlalchemy-execute-raw-query
                f"""
                SELECT
                    COALESCE(SUM(commission_cents), 0) AS total_provision,
                    COALESCE(
                        SUM(
                            CASE
                                WHEN created_at >= %s
                                 AND status IN ({_AFFILIATE_REVENUE_STATUS_PLACEHOLDERS})
                                THEN commission_cents
                                ELSE 0
                            END
                        ),
                        0
                    ) AS this_month_provision
                FROM affiliate_commissions
                WHERE status IN ({_AFFILIATE_REVENUE_STATUS_PLACEHOLDERS})
                """,
                (month_start_iso, *_AFFILIATE_REVENUE_STATUSES, *_AFFILIATE_REVENUE_STATUSES),
            ).fetchone()
            gutschrift_summary = build_summary(conn)
    except Exception as exc:
        if is_missing_schema_error(exc):
            return {
                "total_affiliates": 0,
                "active_affiliates": 0,
                "total_claims": 0,
                "total_provision": 0.0,
                "this_month_claims": 0,
                "this_month_provision": 0.0,
                "total_gutschriften": 0,
                "total_gutschrift_amount": 0.0,
                "pending_email_gutschriften": 0,
            }
        raise

    total_provision_cents = _safe_int(_row_get_value(commission_row, "total_provision", 0, 0), default=0)
    this_month_provision_cents = _safe_int(
        _row_get_value(commission_row, "this_month_provision", 1, 0),
        default=0,
    )
    return {
        "total_affiliates": _safe_int(_row_get_value(account_row, "total_affiliates", 0, 0), default=0),
        "active_affiliates": _safe_int(
            _row_get_value(account_row, "active_affiliates", 1, 0),
            default=0,
        ),
        "total_claims": _safe_int(_row_get_value(claim_row, "total_claims", 0, 0), default=0),
        "total_provision": round(total_provision_cents / 100.0, 2),
        "this_month_claims": _safe_int(
            _row_get_value(claim_row, "this_month_claims", 1, 0),
            default=0,
        ),
        "this_month_provision": round(this_month_provision_cents / 100.0, 2),
        "total_gutschriften": gutschrift_summary["total_gutschriften"],
        "total_gutschrift_amount": gutschrift_summary["total_gutschrift_amount"],
        "pending_email_gutschriften": gutschrift_summary["pending_email_gutschriften"],
    }


__all__ = [
    "AdminAffiliateNotFoundError",
    "load_admin_affiliate_detail",
    "load_admin_affiliate_gutschrift_pdf",
    "load_admin_affiliate_gutschriften",
    "load_admin_affiliate_gutschriften_for_login",
    "load_admin_affiliate_stats",
    "load_admin_affiliates_list",
    "toggle_admin_affiliate",
]
