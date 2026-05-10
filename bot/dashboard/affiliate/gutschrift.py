from __future__ import annotations

import json
from datetime import UTC, date, datetime
from decimal import Decimal, ROUND_HALF_UP
from typing import Any

from ...core.twitch_login import normalize_twitch_login
from .affiliate_email import AffiliateEmailSender
from .affiliate_pii import AffiliatePII

_MONTH_LABELS = (
    "",
    "Januar",
    "Februar",
    "Maerz",
    "April",
    "Mai",
    "Juni",
    "Juli",
    "August",
    "September",
    "Oktober",
    "November",
    "Dezember",
)


class AffiliateGutschriftService:
    TRANSFERRED_STATUS = "transferred"
    STATUS_BLOCKED = "blocked"
    STATUS_GENERATED = "generated"
    STATUS_EMAILED = "emailed"
    STATUS_EMAIL_FAILED = "email_failed"
    VAT_RATE_PERCENT = Decimal("19.00")

    @staticmethod
    def _row_value(row: Any, key: str, default: Any = None) -> Any:
        if row is None:
            return default
        if hasattr(row, "get"):
            return row.get(key, default)
        try:
            return row[key]
        except Exception:
            return default

    @classmethod
    def _normalize_login(cls, value: Any) -> str:
        normalized = normalize_twitch_login(value)
        if not normalized:
            raise ValueError("affiliate_login is required")
        return normalized

    @classmethod
    def _normalize_year_month(cls, year: int, month: int) -> tuple[int, int]:
        normalized_year = int(year)
        normalized_month = int(month)
        if normalized_year < 2000 or normalized_month < 1 or normalized_month > 12:
            raise ValueError("invalid period")
        return normalized_year, normalized_month

    @classmethod
    def _period_start(cls, year: int, month: int) -> datetime:
        normalized_year, normalized_month = cls._normalize_year_month(year, month)
        return datetime(normalized_year, normalized_month, 1, tzinfo=UTC)

    @classmethod
    def _next_period_start(cls, year: int, month: int) -> datetime:
        normalized_year, normalized_month = cls._normalize_year_month(year, month)
        if normalized_month == 12:
            return datetime(normalized_year + 1, 1, 1, tzinfo=UTC)
        return datetime(normalized_year, normalized_month + 1, 1, tzinfo=UTC)

    @classmethod
    def period_label(cls, year: int, month: int) -> str:
        normalized_year, normalized_month = cls._normalize_year_month(year, month)
        return f"{_MONTH_LABELS[normalized_month]} {normalized_year}"

    @classmethod
    def build_readiness(cls, profile: dict[str, Any] | None) -> dict[str, Any]:
        current = dict(profile or {})
        blockers = AffiliatePII.gutschrift_blockers(current)
        warnings: list[str] = []
        if str(current.get("ust_status") or "").strip().lower() == "regelbesteuert" and not str(
            current.get("vat_id") or ""
        ).strip():
            warnings.append(
                "USt-IdNr. ist leer. Bitte nur dann leer lassen, wenn keine vergeben wurde."
            )
        return {
            "can_generate": not blockers,
            "blockers": blockers,
            "warnings": warnings,
            "missing_fields": AffiliatePII.missing_gutschrift_fields(current),
            "ust_status": str(current.get("ust_status") or "unknown"),
        }

    @classmethod
    def _vat_amount_cents(cls, net_amount_cents: int, ust_status: str) -> int:
        if str(ust_status or "").strip().lower() != "regelbesteuert":
            return 0
        return int(
            (
                Decimal(int(net_amount_cents)) * cls.VAT_RATE_PERCENT / Decimal("100")
            ).quantize(Decimal("1"), rounding=ROUND_HALF_UP)
        )

    @staticmethod
    def _format_eur_cents(cents: int, currency: str = "EUR") -> str:
        amount = (Decimal(int(cents)) / Decimal("100")).quantize(Decimal("0.01"))
        return f"{str(amount).replace('.', ',')} {str(currency or 'EUR').upper()}"

    @classmethod
    def _pdf_safe(cls, value: Any) -> str:
        return str(value or "").encode("latin-1", errors="replace").decode("latin-1")

    @classmethod
    def _default_seller(cls) -> dict[str, str]:
        return {
            "name": "[STEUERBERATER: Firmenname]",
            "company": "[STEUERBERATER: Firmierung]",
            "street": "[STEUERBERATER: Adresse]",
            "postal_code": "",
            "city": "",
            "country": "DE",
            "email": "billing@example.invalid",
            "website": "",
            "tax_id": "[STEUERBERATER: Steuernummer/USt-IdNr.]",
        }

    @classmethod
    def _seller_name(cls, seller: dict[str, Any]) -> str:
        company = str(seller.get("company") or "").strip()
        name = str(seller.get("name") or "").strip()
        return company or name or cls._default_seller()["name"]

    @classmethod
    def _combine_address(
        cls,
        *,
        street: Any,
        postal_code: Any,
        city: Any,
        country: Any,
    ) -> str:
        lines: list[str] = []
        street_text = str(street or "").strip()
        if street_text:
            lines.append(street_text)
        postal_city = " ".join(
            part
            for part in (str(postal_code or "").strip(), str(city or "").strip())
            if part
        )
        if postal_city:
            lines.append(postal_city)
        country_text = str(country or "").strip().upper()
        if country_text:
            lines.append(country_text)
        return "\n".join(lines)

    @classmethod
    def _seller_address(cls, seller: dict[str, Any]) -> str:
        return cls._combine_address(
            street=seller.get("street"),
            postal_code=seller.get("postal_code"),
            city=seller.get("city"),
            country=seller.get("country"),
        )

    @classmethod
    def _affiliate_address(cls, profile: dict[str, Any]) -> str:
        return cls._combine_address(
            street=profile.get("address_line1"),
            postal_code=profile.get("address_zip"),
            city=profile.get("address_city"),
            country=profile.get("address_country"),
        )

    @classmethod
    def _affiliate_tax_id(cls, profile: dict[str, Any]) -> str:
        lines: list[str] = []
        tax_id = str(profile.get("tax_id") or "").strip()
        vat_id = str(profile.get("vat_id") or "").strip()
        if tax_id:
            lines.append(f"Steuernummer: {tax_id}")
        if vat_id:
            lines.append(f"USt-IdNr.: {vat_id}")
        return "\n".join(lines)

    @classmethod
    def _json_array(cls, values: list[int]) -> str:
        return json.dumps(values, ensure_ascii=True, separators=(",", ":"))

    @classmethod
    def _commission_ids_from_row(cls, row: Any) -> list[int]:
        raw = str(cls._row_value(row, "commission_ids", "") or "").strip()
        if not raw:
            return []
        try:
            parsed = json.loads(raw)
        except json.JSONDecodeError:
            return []
        if not isinstance(parsed, list):
            return []
        normalized: list[int] = []
        for value in parsed:
            try:
                normalized.append(int(value))
            except (TypeError, ValueError):
                continue
        return normalized

    @classmethod
    def _status_from_row(cls, row: Any) -> str:
        if not cls._row_value(row, "pdf_generated_at"):
            return cls.STATUS_BLOCKED
        if str(cls._row_value(row, "email_error", "") or "").strip():
            return cls.STATUS_EMAIL_FAILED
        if cls._row_value(row, "email_sent_at"):
            return cls.STATUS_EMAILED
        return cls.STATUS_GENERATED

    @classmethod
    def _note_text(cls, ust_status: str) -> str:
        if str(ust_status or "").strip().lower() == "kleinunternehmer":
            return "Gemäß § 19 UStG wird keine Umsatzsteuer berechnet."
        return ""

    @classmethod
    def _table_columns(cls, conn: Any, table_name: str) -> set[str]:
        normalized_name = str(table_name or "").strip().lower()
        if not normalized_name:
            return set()

        try:
            rows = conn.execute(
                """
                SELECT column_name
                FROM information_schema.columns
                WHERE table_schema = current_schema() AND table_name = %s
                ORDER BY ordinal_position
                """,
                (normalized_name,),
            ).fetchall()
        except Exception:
            rows = []
        columns = {
            str(cls._row_value(row, "column_name", row[0] if row else "") or "").strip().lower()
            for row in rows
        }
        return columns

    @classmethod
    def ensure_schema(cls, conn: Any) -> None:
        gutschrift_columns = cls._table_columns(conn, "affiliate_gutschriften")
        if gutschrift_columns:
            additions = (
                ("vat_rate_percent", "NUMERIC(5,2) NOT NULL DEFAULT 0"),
                ("affiliate_name", "TEXT NOT NULL DEFAULT ''"),
                ("affiliate_address", "TEXT NOT NULL DEFAULT ''"),
                ("affiliate_tax_id", "TEXT"),
                ("affiliate_ust_status", "TEXT NOT NULL DEFAULT 'unknown'"),
                ("issuer_name", "TEXT NOT NULL DEFAULT ''"),
                ("issuer_address", "TEXT NOT NULL DEFAULT ''"),
                ("issuer_tax_id", "TEXT NOT NULL DEFAULT ''"),
                ("pdf_generated_at", "TEXT"),
                ("email_sent_at", "TEXT"),
                ("email_error", "TEXT"),
                ("commission_ids", "TEXT"),
                ("created_at", "TEXT NOT NULL DEFAULT ''"),
            )
            for column_name, definition in additions:
                if column_name in gutschrift_columns:
                    continue
                conn.execute(  # nosemgrep: python.lang.security.audit.formatted-sql-query.formatted-sql-query, python.sqlalchemy.security.sqlalchemy-execute-raw-query.sqlalchemy-execute-raw-query
                    f"ALTER TABLE affiliate_gutschriften ADD COLUMN {column_name} {definition}"
                )
                gutschrift_columns.add(column_name)

    @classmethod
    def _row_to_metadata(cls, row: Any) -> dict[str, Any]:
        year = int(cls._row_value(row, "period_year", 0) or 0)
        month = int(cls._row_value(row, "period_month", 0) or 0)
        row_id = int(cls._row_value(row, "id", 0) or 0)
        commission_ids = cls._commission_ids_from_row(row)
        return {
            "id": row_id,
            "period_year": year,
            "period_month": month,
            "period_label": cls.period_label(year, month) if year and month else "",
            "gutschrift_number": str(cls._row_value(row, "gutschrift_number", "") or ""),
            "status": cls._status_from_row(row),
            "net_amount_cents": int(cls._row_value(row, "net_amount_cents", 0) or 0),
            "vat_amount_cents": int(cls._row_value(row, "vat_amount_cents", 0) or 0),
            "gross_amount_cents": int(cls._row_value(row, "gross_amount_cents", 0) or 0),
            "commission_count": len(commission_ids),
            "commission_ids": commission_ids,
            "note_text": cls._note_text(str(cls._row_value(row, "affiliate_ust_status", "") or "")),
            "last_error": str(cls._row_value(row, "email_error", "") or ""),
            "generated_at": cls._row_value(row, "pdf_generated_at"),
            "emailed_at": cls._row_value(row, "email_sent_at"),
            "created_at": cls._row_value(row, "created_at"),
            "download_path": (
                f"/twitch/api/affiliate/gutschriften/{row_id}/pdf" if row_id > 0 else None
            ),
            "has_pdf": bool(cls._row_value(row, "pdf_blob")),
        }

    @classmethod
    def _next_gutschrift_number(cls, conn: Any, year_month: str) -> str:
        normalized_year_month = str(year_month or "").strip()
        if len(normalized_year_month) != 6 or not normalized_year_month.isdigit():
            raise ValueError("invalid year_month")
        counter_columns = cls._table_columns(conn, "affiliate_gutschrift_counter")
        if {"year_month", "last_seq"}.issubset(counter_columns):
            conn.execute(
                """
                INSERT INTO affiliate_gutschrift_counter (year_month, last_seq)
                VALUES (%s, 0)
                ON CONFLICT(year_month) DO NOTHING
                """,
                (normalized_year_month,),
            )
            row = conn.execute(
                """
                UPDATE affiliate_gutschrift_counter
                SET last_seq = last_seq + 1
                WHERE year_month = %s
                RETURNING last_seq
                """,
                (normalized_year_month,),
            ).fetchone()
            next_seq = int(cls._row_value(row, "last_seq", row[0] if row else 0) or 0)
        elif {"counter_year", "last_counter"}.issubset(counter_columns):
            legacy_key = int(normalized_year_month)
            now_iso = datetime.now(UTC).isoformat()
            if "updated_at" in counter_columns:
                conn.execute(
                    """
                    INSERT INTO affiliate_gutschrift_counter (counter_year, last_counter, updated_at)
                    VALUES (%s, 0, %s)
                    ON CONFLICT(counter_year) DO NOTHING
                    """,
                    (legacy_key, now_iso),
                )
                row = conn.execute(
                    """
                    UPDATE affiliate_gutschrift_counter
                    SET last_counter = last_counter + 1,
                        updated_at = %s
                    WHERE counter_year = %s
                    RETURNING last_counter
                    """,
                    (now_iso, legacy_key),
                ).fetchone()
            else:
                conn.execute(
                    """
                    INSERT INTO affiliate_gutschrift_counter (counter_year, last_counter)
                    VALUES (%s, 0)
                    ON CONFLICT(counter_year) DO NOTHING
                    """,
                    (legacy_key,),
                )
                row = conn.execute(
                    """
                    UPDATE affiliate_gutschrift_counter
                    SET last_counter = last_counter + 1
                    WHERE counter_year = %s
                    RETURNING last_counter
                    """,
                    (legacy_key,),
                ).fetchone()
            next_seq = int(cls._row_value(row, "last_counter", row[0] if row else 0) or 0)
        else:
            raise RuntimeError("affiliate_gutschrift_counter schema is incompatible")
        if next_seq <= 0:
            raise RuntimeError("could not allocate gutschrift number")
        return f"GS-{normalized_year_month}-{next_seq:04d}"

    @classmethod
    def due_periods(
        cls,
        conn: Any,
        *,
        as_of: datetime | None = None,
    ) -> list[tuple[str, int, int]]:
        now_utc = (as_of or datetime.now(UTC)).astimezone(UTC)
        current_period_start = now_utc.replace(day=1, hour=0, minute=0, second=0, microsecond=0)
        rows = conn.execute(
            """
            SELECT affiliate_twitch_login, created_at
            FROM affiliate_commissions
            WHERE status = %s
            """,
            (cls.TRANSFERRED_STATUS,),
        ).fetchall()
        periods: set[tuple[str, int, int]] = set()
        for row in rows:
            try:
                login = cls._normalize_login(cls._row_value(row, "affiliate_twitch_login", ""))
            except ValueError:
                continue
            created_at_raw = str(cls._row_value(row, "created_at", "") or "").strip()
            if not login or not created_at_raw:
                continue
            try:
                created_at = datetime.fromisoformat(created_at_raw.replace("Z", "+00:00"))
            except ValueError:
                continue
            if created_at.tzinfo is None:
                created_at = created_at.replace(tzinfo=UTC)
            created_at = created_at.astimezone(UTC)
            if created_at >= current_period_start:
                continue
            periods.add((login, created_at.year, created_at.month))
        return sorted(periods, key=lambda item: (item[1], item[2], item[0]))

    @classmethod
    def list_for_affiliate(cls, conn: Any, affiliate_login: str) -> list[dict[str, Any]]:
        rows = conn.execute(
            """
            SELECT *
            FROM affiliate_gutschriften
            WHERE affiliate_twitch_login = %s
            ORDER BY period_year DESC, period_month DESC, id DESC
            """,
            (cls._normalize_login(affiliate_login),),
        ).fetchall()
        return [cls._row_to_metadata(row) for row in rows]

    @classmethod
    def get_pdf(
        cls,
        conn: Any,
        *,
        affiliate_login: str,
        gutschrift_id: int,
    ) -> tuple[dict[str, Any], bytes] | None:
        row = conn.execute(
            """
            SELECT *
            FROM affiliate_gutschriften
            WHERE id = %s AND affiliate_twitch_login = %s
            """,
            (int(gutschrift_id), cls._normalize_login(affiliate_login)),
        ).fetchone()
        if not row:
            return None
        pdf_blob = cls._row_value(row, "pdf_blob")
        if not pdf_blob:
            return None
        return cls._row_to_metadata(row), bytes(pdf_blob)

    @classmethod
    def generate_gutschrift_pdf(cls, data: dict[str, Any]) -> bytes:
        try:
            from fpdf import FPDF
        except Exception as exc:  # pragma: no cover - import guard
            raise RuntimeError(f"fpdf2 import failed: {exc}") from exc

        pdf = FPDF()
        pdf.set_auto_page_break(auto=True, margin=14)
        pdf.add_page()

        pdf.set_font("Helvetica", "B", 18)
        pdf.cell(120, 10, cls._pdf_safe("GUTSCHRIFT"), border=0)
        pdf.set_font("Helvetica", "", 10)
        pdf.cell(
            0,
            10,
            cls._pdf_safe(str(data.get("gutschrift_number") or "")),
            align="R",
            ln=1,
        )
        pdf.cell(
            0,
            6,
            cls._pdf_safe(f"Datum: {data.get('issue_date_label') or ''}"),
            ln=1,
        )
        pdf.ln(3)

        pdf.set_font("Helvetica", "B", 10)
        pdf.cell(0, 7, cls._pdf_safe("Leistungsempfaenger (Aussteller):"), ln=1)
        pdf.set_font("Helvetica", "", 10)
        pdf.multi_cell(
            0,
            5,
            cls._pdf_safe(
                "\n".join(
                    part
                    for part in (
                        str(data.get("issuer_name") or "").strip(),
                        str(data.get("issuer_address") or "").strip(),
                        str(data.get("issuer_tax_id") or "").strip(),
                    )
                    if part
                )
            ),
            border=1,
        )
        pdf.ln(3)

        pdf.set_font("Helvetica", "B", 10)
        pdf.cell(0, 7, cls._pdf_safe("Leistender (Empfaenger der Gutschrift):"), ln=1)
        pdf.set_font("Helvetica", "", 10)
        pdf.multi_cell(
            0,
            5,
            cls._pdf_safe(
                "\n".join(
                    part
                    for part in (
                        str(data.get("affiliate_name") or "").strip(),
                        str(data.get("affiliate_address") or "").strip(),
                        str(data.get("affiliate_tax_id") or "").strip(),
                    )
                    if part
                )
            ),
            border=1,
        )
        pdf.ln(4)

        pdf.set_font("Helvetica", "", 10)
        pdf.cell(
            0,
            6,
            cls._pdf_safe(f"Leistungszeitraum: {data.get('period_label') or ''}"),
            ln=1,
        )
        pdf.ln(2)
        pdf.multi_cell(
            0,
            6,
            cls._pdf_safe(
                f"Vermittlungsleistung (Provision 30 %) fuer {data.get('period_label') or ''}"
            ),
        )
        pdf.ln(1)

        pdf.cell(
            0,
            6,
            cls._pdf_safe(f"Nettobetrag: {data.get('net_amount_label') or ''}"),
            ln=1,
        )
        if str(data.get("affiliate_ust_status") or "").strip().lower() == "regelbesteuert":
            pdf.cell(
                0,
                6,
                cls._pdf_safe(
                    f"USt {data.get('vat_rate_label') or '19 %'}: {data.get('vat_amount_label') or ''}"
                ),
                ln=1,
            )
        else:
            pdf.cell(
                0,
                6,
                cls._pdf_safe("Gem. § 19 UStG: keine USt"),
                ln=1,
            )
        pdf.set_font("Helvetica", "B", 11)
        pdf.cell(
            0,
            8,
            cls._pdf_safe(f"Gesamtbetrag: {data.get('gross_amount_label') or ''}"),
            ln=1,
        )
        pdf.set_font("Helvetica", "", 10)
        pdf.ln(2)
        pdf.multi_cell(
            0,
            6,
            cls._pdf_safe(
                "Diese Gutschrift gilt als Rechnung im Sinne des § 14 Abs. 2 Satz 2 UStG."
            ),
            border=1,
        )

        output = pdf.output(dest="S")
        if isinstance(output, (bytes, bytearray)):
            return bytes(output)
        return str(output).encode("latin-1", errors="replace")

    @classmethod
    def _load_existing(
        cls,
        conn: Any,
        *,
        affiliate_login: str,
        year: int,
        month: int,
    ) -> Any:
        return conn.execute(
            """
            SELECT *
            FROM affiliate_gutschriften
            WHERE affiliate_twitch_login = %s AND period_year = %s AND period_month = %s
            """,
            (cls._normalize_login(affiliate_login), int(year), int(month)),
        ).fetchone()

    @classmethod
    def _store_gutschrift(
        cls,
        conn: Any,
        *,
        affiliate_login: str,
        year: int,
        month: int,
        gutschrift_number: str,
        net_amount_cents: int,
        vat_amount_cents: int,
        gross_amount_cents: int,
        affiliate_name: str,
        affiliate_address: str,
        affiliate_tax_id: str,
        affiliate_ust_status: str,
        issuer_name: str,
        issuer_address: str,
        issuer_tax_id: str,
        pdf_blob: bytes,
        pdf_generated_at: str,
        email_sent_at: str | None,
        email_error: str | None,
        commission_ids: list[int],
        created_at: str,
    ) -> Any:
        vat_rate_percent = (
            str(cls.VAT_RATE_PERCENT)
            if str(affiliate_ust_status or "").strip().lower() == "regelbesteuert"
            else "0.00"
        )
        conn.execute(
            """
            INSERT INTO affiliate_gutschriften (
                gutschrift_number,
                affiliate_twitch_login,
                period_year,
                period_month,
                net_amount_cents,
                vat_rate_percent,
                vat_amount_cents,
                gross_amount_cents,
                affiliate_name,
                affiliate_address,
                affiliate_tax_id,
                affiliate_ust_status,
                issuer_name,
                issuer_address,
                issuer_tax_id,
                pdf_blob,
                pdf_generated_at,
                email_sent_at,
                email_error,
                commission_ids,
                created_at
            ) VALUES (%s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s)
            ON CONFLICT(affiliate_twitch_login, period_year, period_month) DO UPDATE SET
                gutschrift_number = excluded.gutschrift_number,
                net_amount_cents = excluded.net_amount_cents,
                vat_rate_percent = excluded.vat_rate_percent,
                vat_amount_cents = excluded.vat_amount_cents,
                gross_amount_cents = excluded.gross_amount_cents,
                affiliate_name = excluded.affiliate_name,
                affiliate_address = excluded.affiliate_address,
                affiliate_tax_id = excluded.affiliate_tax_id,
                affiliate_ust_status = excluded.affiliate_ust_status,
                issuer_name = excluded.issuer_name,
                issuer_address = excluded.issuer_address,
                issuer_tax_id = excluded.issuer_tax_id,
                pdf_blob = excluded.pdf_blob,
                pdf_generated_at = excluded.pdf_generated_at,
                email_sent_at = excluded.email_sent_at,
                email_error = excluded.email_error,
                commission_ids = excluded.commission_ids
            """,
            (
                gutschrift_number,
                cls._normalize_login(affiliate_login),
                int(year),
                int(month),
                int(net_amount_cents),
                vat_rate_percent,
                int(vat_amount_cents),
                int(gross_amount_cents),
                affiliate_name,
                affiliate_address,
                affiliate_tax_id or None,
                affiliate_ust_status,
                issuer_name,
                issuer_address,
                issuer_tax_id,
                pdf_blob,
                pdf_generated_at,
                email_sent_at,
                email_error,
                cls._json_array(commission_ids),
                created_at,
            ),
        )
        return cls._load_existing(
            conn,
            affiliate_login=affiliate_login,
            year=year,
            month=month,
        )

    @classmethod
    def _send_email_for_row(
        cls,
        conn: Any,
        *,
        email_sender: AffiliateEmailSender,
        recipient_email: str,
        recipient_name: str,
        row: Any,
        currency: str,
    ) -> tuple[Any, str]:
        normalized_email = str(recipient_email or "").strip()
        if not normalized_email:
            return row, cls.STATUS_GENERATED

        pdf_blob = cls._row_value(row, "pdf_blob")
        if not pdf_blob:
            return row, cls.STATUS_GENERATED

        try:
            email_sender.send_gutschrift(
                recipient_email=normalized_email,
                recipient_name=str(recipient_name or "").strip(),
                gutschrift_number=str(cls._row_value(row, "gutschrift_number", "") or ""),
                period_label=cls.period_label(
                    int(cls._row_value(row, "period_year", 0) or 0),
                    int(cls._row_value(row, "period_month", 0) or 0),
                ),
                gross_amount_label=cls._format_eur_cents(
                    int(cls._row_value(row, "gross_amount_cents", 0) or 0),
                    currency,
                ),
                pdf_bytes=bytes(pdf_blob),
                filename=(
                    f"{str(cls._row_value(row, 'gutschrift_number', 'gutschrift') or 'gutschrift')}.pdf"
                ),
            )
        except Exception as exc:
            conn.execute(
                """
                UPDATE affiliate_gutschriften
                SET email_sent_at = NULL, email_error = %s
                WHERE id = %s
                """,
                (str(exc)[:500], int(cls._row_value(row, "id", 0) or 0)),
            )
            updated_row = conn.execute(
                "SELECT * FROM affiliate_gutschriften WHERE id = %s",
                (int(cls._row_value(row, "id", 0) or 0),),
            ).fetchone()
            return updated_row, cls.STATUS_EMAIL_FAILED

        sent_at = datetime.now(UTC).isoformat()
        conn.execute(
            """
            UPDATE affiliate_gutschriften
            SET email_sent_at = %s, email_error = NULL
            WHERE id = %s
            """,
            (sent_at, int(cls._row_value(row, "id", 0) or 0)),
        )
        updated_row = conn.execute(
            "SELECT * FROM affiliate_gutschriften WHERE id = %s",
            (int(cls._row_value(row, "id", 0) or 0),),
        ).fetchone()
        return updated_row, cls.STATUS_EMAILED

    @classmethod
    def generate_for_period(
        cls,
        conn: Any,
        *,
        affiliate_login: str,
        year: int,
        month: int,
        email_sender: AffiliateEmailSender | None = None,
        seller: dict[str, Any] | None = None,
        force: bool = False,
    ) -> dict[str, Any]:
        normalized_login = cls._normalize_login(affiliate_login)
        normalized_year, normalized_month = cls._normalize_year_month(year, month)
        period_start = cls._period_start(normalized_year, normalized_month)
        next_period_start = cls._next_period_start(normalized_year, normalized_month)
        existing = cls._load_existing(
            conn,
            affiliate_login=normalized_login,
            year=normalized_year,
            month=normalized_month,
        )

        profile = AffiliatePII.load_pii(conn, normalized_login)
        readiness = cls.build_readiness(profile)

        if existing and cls._row_value(existing, "pdf_blob") and not force:
            action = "existing"
            current_row = existing
            if (
                email_sender is not None
                and not cls._row_value(existing, "email_sent_at")
                and str(profile.get("email") or "").strip()
            ):
                current_row, action = cls._send_email_for_row(
                    conn,
                    email_sender=email_sender,
                    recipient_email=str(profile.get("email") or ""),
                    recipient_name=str(profile.get("full_name") or ""),
                    row=existing,
                    currency="eur",
                )
            return {
                "ok": True,
                "document": cls._row_to_metadata(current_row),
                "action": action,
                "readiness": readiness,
            }

        commission_rows = conn.execute(
            """
            SELECT id, commission_cents, currency
            FROM affiliate_commissions
            WHERE affiliate_twitch_login = %s
              AND status = %s
              AND created_at >= %s
              AND created_at < %s
            ORDER BY id ASC
            """,
            (
                normalized_login,
                cls.TRANSFERRED_STATUS,
                period_start.isoformat(),
                next_period_start.isoformat(),
            ),
        ).fetchall()

        if not commission_rows:
            return {
                "ok": False,
                "status": "no_commissions",
                "affiliate_login": normalized_login,
                "period_year": normalized_year,
                "period_month": normalized_month,
                "readiness": readiness,
            }

        if readiness["blockers"]:
            return {
                "ok": False,
                "status": cls.STATUS_BLOCKED,
                "affiliate_login": normalized_login,
                "period_year": normalized_year,
                "period_month": normalized_month,
                "readiness": readiness,
            }

        commission_ids = [
            int(cls._row_value(row, "id", 0) or 0)
            for row in commission_rows
            if int(cls._row_value(row, "id", 0) or 0) > 0
        ]
        net_amount_cents = sum(
            int(cls._row_value(row, "commission_cents", 0) or 0) for row in commission_rows
        )
        currency = str(cls._row_value(commission_rows[0], "currency", "eur") or "eur").lower()
        ust_status = str(profile.get("ust_status") or "unknown").strip().lower()
        vat_amount_cents = cls._vat_amount_cents(net_amount_cents, ust_status)
        gross_amount_cents = net_amount_cents + vat_amount_cents

        effective_seller = {**cls._default_seller(), **dict(seller or {})}
        year_month = f"{normalized_year:04d}{normalized_month:02d}"
        gutschrift_number = str(cls._row_value(existing, "gutschrift_number", "") or "").strip()
        if not gutschrift_number:
            gutschrift_number = cls._next_gutschrift_number(conn, year_month)

        affiliate_name = str(profile.get("full_name") or "").strip()
        affiliate_address = cls._affiliate_address(profile)
        affiliate_tax_id = cls._affiliate_tax_id(profile)
        issuer_name = cls._seller_name(effective_seller)
        issuer_address = cls._seller_address(effective_seller)
        issuer_tax_id = str(effective_seller.get("tax_id") or "").strip()
        pdf_generated_at = datetime.now(UTC).isoformat()
        created_at = str(cls._row_value(existing, "created_at", "") or "").strip() or pdf_generated_at
        issue_date = next_period_start.date()

        pdf_bytes = cls.generate_gutschrift_pdf(
            {
                "gutschrift_number": gutschrift_number,
                "issue_date_label": cls._format_issue_date(issue_date),
                "period_label": cls.period_label(normalized_year, normalized_month),
                "net_amount_label": cls._format_eur_cents(net_amount_cents, currency),
                "vat_rate_label": "19 %",
                "vat_amount_label": cls._format_eur_cents(vat_amount_cents, currency),
                "gross_amount_label": cls._format_eur_cents(gross_amount_cents, currency),
                "affiliate_name": affiliate_name,
                "affiliate_address": affiliate_address,
                "affiliate_tax_id": affiliate_tax_id,
                "affiliate_ust_status": ust_status,
                "issuer_name": issuer_name,
                "issuer_address": issuer_address,
                "issuer_tax_id": issuer_tax_id,
            }
        )

        row = cls._store_gutschrift(
            conn,
            affiliate_login=normalized_login,
            year=normalized_year,
            month=normalized_month,
            gutschrift_number=gutschrift_number,
            net_amount_cents=net_amount_cents,
            vat_amount_cents=vat_amount_cents,
            gross_amount_cents=gross_amount_cents,
            affiliate_name=affiliate_name,
            affiliate_address=affiliate_address,
            affiliate_tax_id=affiliate_tax_id,
            affiliate_ust_status=ust_status,
            issuer_name=issuer_name,
            issuer_address=issuer_address,
            issuer_tax_id=issuer_tax_id,
            pdf_blob=pdf_bytes,
            pdf_generated_at=pdf_generated_at,
            email_sent_at=None,
            email_error=None,
            commission_ids=commission_ids,
            created_at=created_at,
        )
        action = cls.STATUS_GENERATED

        recipient_email = str(profile.get("email") or "").strip()
        if email_sender is not None and recipient_email:
            row, action = cls._send_email_for_row(
                conn,
                email_sender=email_sender,
                recipient_email=recipient_email,
                recipient_name=str(profile.get("full_name") or ""),
                row=row,
                currency=currency,
            )

        return {
            "ok": True,
            "document": cls._row_to_metadata(row),
            "action": action,
            "readiness": readiness,
        }

    @classmethod
    def generate_monthly_gutschriften(
        cls,
        conn: Any,
        year: int,
        month: int,
        *,
        email_sender: AffiliateEmailSender | None = None,
        seller: dict[str, Any] | None = None,
        affiliate_login: str | None = None,
        force: bool = False,
    ) -> list[dict[str, Any]]:
        normalized_year, normalized_month = cls._normalize_year_month(year, month)
        normalized_login = cls._normalize_login(affiliate_login) if affiliate_login else ""
        if normalized_login:
            return [
                cls.generate_for_period(
                    conn,
                    affiliate_login=normalized_login,
                    year=normalized_year,
                    month=normalized_month,
                    email_sender=email_sender,
                    seller=seller,
                    force=force,
                )
            ]

        period_start = cls._period_start(normalized_year, normalized_month)
        next_period_start = cls._next_period_start(normalized_year, normalized_month)
        affiliate_rows = conn.execute(
            """
            SELECT DISTINCT affiliate_twitch_login
            FROM affiliate_commissions
            WHERE status = %s
              AND created_at >= %s
              AND created_at < %s
            ORDER BY affiliate_twitch_login ASC
            """,
            (
                cls.TRANSFERRED_STATUS,
                period_start.isoformat(),
                next_period_start.isoformat(),
            ),
        ).fetchall()
        results: list[dict[str, Any]] = []
        for row in affiliate_rows:
            try:
                login = cls._normalize_login(cls._row_value(row, "affiliate_twitch_login", ""))
            except ValueError:
                continue
            if not login:
                continue
            results.append(
                cls.generate_for_period(
                    conn,
                    affiliate_login=login,
                    year=normalized_year,
                    month=normalized_month,
                    email_sender=email_sender,
                    seller=seller,
                    force=force,
                )
            )
        return results

    @classmethod
    def run_pending(
        cls,
        conn: Any,
        *,
        email_sender: AffiliateEmailSender | None = None,
        seller: dict[str, Any] | None = None,
        as_of: datetime | None = None,
        limit: int = 100,
    ) -> list[dict[str, Any]]:
        results: list[dict[str, Any]] = []
        for affiliate_login, year, month in cls.due_periods(conn, as_of=as_of)[: max(1, int(limit or 100))]:
            results.append(
                cls.generate_for_period(
                    conn,
                    affiliate_login=affiliate_login,
                    year=year,
                    month=month,
                    email_sender=email_sender,
                    seller=seller,
                    force=False,
                )
            )
        return results

    @staticmethod
    def _format_issue_date(value: date) -> str:
        return value.strftime("%d.%m.%Y")
