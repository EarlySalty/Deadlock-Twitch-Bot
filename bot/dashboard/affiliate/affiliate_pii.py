from __future__ import annotations

import json
from datetime import UTC, datetime
from typing import Any

from ...core.twitch_login import normalize_twitch_login
from ...compat.field_crypto import get_crypto


class AffiliatePII:
    ENCRYPTED_FIELDS = [
        "full_name",
        "email",
        "address_line1",
        "address_city",
        "address_zip",
    ]
    FIELD_COLUMN_MAP = {
        "full_name": "full_name_enc",
        "email": "email_enc",
        "address_line1": "address_line1_enc",
        "address_city": "address_city_enc",
        "address_zip": "address_zip_enc",
        "address_country": "address_country",
        "ust_status": "ust_status",
        "updated_at": "updated_at",
    }
    TAX_COLUMN = "tax_id_enc"
    VALID_UST_STATUS = {"kleinunternehmer", "regelbesteuert", "unknown"}
    REQUIRED_GUTSCHRIFT_FIELDS = (
        "full_name",
        "email",
        "address_line1",
        "address_city",
        "address_zip",
        "address_country",
    )
    FIELD_LABELS = {
        "full_name": "Vollstaendiger Name",
        "email": "Kontakt-E-Mail",
        "address_line1": "Strasse",
        "address_city": "Ort",
        "address_zip": "PLZ",
        "address_country": "Land",
        "tax_id": "Steuernummer oder USt-IdNr.",
        "vat_id": "USt-IdNr.",
        "ust_status": "USt-Status",
    }

    @classmethod
    def _aad(cls, field: str, twitch_login: str) -> str:
        return f"affiliate_pii|{field}|{twitch_login}"

    @classmethod
    def _normalize_login(cls, twitch_login: str) -> str:
        normalized = normalize_twitch_login(twitch_login)
        if not normalized:
            raise ValueError("twitch_login is required")
        return normalized

    @classmethod
    def _normalize_text(cls, value: Any) -> str:
        return str(value or "").strip()

    @classmethod
    def _normalize_country(cls, value: Any) -> str:
        normalized = str(value or "").strip().upper()
        return normalized or "DE"

    @classmethod
    def _normalize_ust_status(cls, value: Any) -> str:
        normalized = str(value or "").strip().lower()
        return normalized if normalized in cls.VALID_UST_STATUS else "unknown"

    @classmethod
    def _normalize_tax_bundle(
        cls,
        data: dict[str, Any],
        existing: dict[str, Any] | None = None,
    ) -> dict[str, str]:
        base = {
            "tax_id": cls._normalize_text((existing or {}).get("tax_id")),
            "vat_id": cls._normalize_text((existing or {}).get("vat_id")),
        }
        if "tax_id" in data:
            base["tax_id"] = cls._normalize_text(data.get("tax_id"))
        if "vat_id" in data:
            base["vat_id"] = cls._normalize_text(data.get("vat_id"))
        return base

    @classmethod
    def _serialize_tax_bundle(cls, data: dict[str, str]) -> str:
        tax_id = cls._normalize_text(data.get("tax_id"))
        vat_id = cls._normalize_text(data.get("vat_id"))
        if not vat_id:
            return tax_id
        return json.dumps(
            {
                "tax_id": tax_id,
                "vat_id": vat_id,
            },
            ensure_ascii=True,
            separators=(",", ":"),
        )

    @classmethod
    def _deserialize_tax_bundle(cls, raw_value: str) -> dict[str, str]:
        normalized = cls._normalize_text(raw_value)
        if not normalized:
            return {"tax_id": "", "vat_id": ""}
        if normalized.startswith("{"):
            try:
                parsed = json.loads(normalized)
            except json.JSONDecodeError:
                parsed = None
            if isinstance(parsed, dict):
                return {
                    "tax_id": cls._normalize_text(parsed.get("tax_id")),
                    "vat_id": cls._normalize_text(parsed.get("vat_id")),
                }
        return {"tax_id": normalized, "vat_id": ""}

    @classmethod
    def _default_payload(cls) -> dict[str, Any]:
        payload = {field: "" for field in cls.ENCRYPTED_FIELDS}
        payload["tax_id"] = ""
        payload["vat_id"] = ""
        payload["address_country"] = "DE"
        payload["ust_status"] = "unknown"
        payload["updated_at"] = None
        return payload

    @classmethod
    def missing_gutschrift_fields(cls, payload: dict[str, Any] | None) -> list[str]:
        current = dict(payload or {})
        missing = []
        for field in cls.REQUIRED_GUTSCHRIFT_FIELDS:
            if not cls._normalize_text(current.get(field)):
                missing.append(field)
        if not cls._normalize_text(current.get("tax_id")) and not cls._normalize_text(
            current.get("vat_id")
        ):
            missing.append("tax_id")
        return missing

    @classmethod
    def gutschrift_blockers(cls, payload: dict[str, Any] | None) -> list[str]:
        current = dict(payload or {})
        blockers: list[str] = []
        if cls._normalize_ust_status(current.get("ust_status")) == "unknown":
            blockers.append("USt-Status noch nicht angegeben.")
        for field in cls.missing_gutschrift_fields(current):
            blockers.append(f"{cls.FIELD_LABELS.get(field, field)} fehlt.")
        return blockers

    @classmethod
    def save_pii(cls, conn: Any, twitch_login: str, data: dict) -> None:
        normalized_login = cls._normalize_login(twitch_login)
        payload = dict(data or {})
        existing = conn.execute(
            """
            SELECT full_name_enc, email_enc, address_line1_enc, address_city_enc,
                   address_zip_enc, tax_id_enc, address_country, ust_status
            FROM affiliate_pii
            WHERE twitch_login = %s
            """,
            (normalized_login,),
        ).fetchone()

        encrypted_values: dict[str, Any] = {}
        crypto = None
        for field in cls.ENCRYPTED_FIELDS:
            column = cls.FIELD_COLUMN_MAP[field]
            if field not in payload:
                encrypted_values[column] = existing[column] if existing else None
                continue

            plaintext = cls._normalize_text(payload.get(field))
            if not plaintext:
                encrypted_values[column] = None
                continue

            if crypto is None:
                crypto = get_crypto()
            encrypted_values[column] = crypto.encrypt_field(
                plaintext,
                cls._aad(field, normalized_login),
            )

        existing_tax_bundle = {"tax_id": "", "vat_id": ""}
        if existing and existing[cls.TAX_COLUMN]:
            if crypto is None:
                crypto = get_crypto()
            existing_tax_bundle = cls._deserialize_tax_bundle(
                crypto.decrypt_field(
                    bytes(existing[cls.TAX_COLUMN]),
                    cls._aad("tax_id", normalized_login),
                )
            )
        tax_bundle = cls._normalize_tax_bundle(payload, existing_tax_bundle)
        tax_blob = existing[cls.TAX_COLUMN] if existing else None
        if "tax_id" in payload or "vat_id" in payload:
            serialized_tax = cls._serialize_tax_bundle(tax_bundle)
            if serialized_tax:
                if crypto is None:
                    crypto = get_crypto()
                tax_blob = crypto.encrypt_field(
                    serialized_tax,
                    cls._aad("tax_id", normalized_login),
                )
            else:
                tax_blob = None

        if "address_country" in payload:
            address_country = cls._normalize_country(payload.get("address_country"))
        elif existing:
            address_country = cls._normalize_country(existing["address_country"])
        else:
            address_country = "DE"

        if "ust_status" in payload:
            ust_status = cls._normalize_ust_status(payload.get("ust_status"))
        elif existing:
            ust_status = cls._normalize_ust_status(existing["ust_status"])
        else:
            ust_status = "unknown"

        updated_at = datetime.now(UTC).isoformat()
        conn.execute(
            """
            INSERT INTO affiliate_pii (
                twitch_login, full_name_enc, email_enc, address_line1_enc, address_city_enc,
                address_zip_enc, tax_id_enc, address_country, ust_status, updated_at
            ) VALUES (%s, %s, %s, %s, %s, %s, %s, %s, %s, %s)
            ON CONFLICT(twitch_login) DO UPDATE SET
                full_name_enc = excluded.full_name_enc,
                email_enc = excluded.email_enc,
                address_line1_enc = excluded.address_line1_enc,
                address_city_enc = excluded.address_city_enc,
                address_zip_enc = excluded.address_zip_enc,
                tax_id_enc = excluded.tax_id_enc,
                address_country = excluded.address_country,
                ust_status = excluded.ust_status,
                updated_at = excluded.updated_at
            """,
            (
                normalized_login,
                encrypted_values["full_name_enc"],
                encrypted_values["email_enc"],
                encrypted_values["address_line1_enc"],
                encrypted_values["address_city_enc"],
                encrypted_values["address_zip_enc"],
                tax_blob,
                address_country,
                ust_status,
                updated_at,
            ),
        )

    @classmethod
    def load_pii(cls, conn: Any, twitch_login: str) -> dict[str, Any]:
        normalized_login = cls._normalize_login(twitch_login)
        row = conn.execute(
            """
            SELECT full_name_enc, email_enc, address_line1_enc, address_city_enc,
                   address_zip_enc, tax_id_enc, address_country, ust_status, updated_at
            FROM affiliate_pii
            WHERE twitch_login = %s
            """,
            (normalized_login,),
        ).fetchone()
        if not row:
            return cls._default_payload()

        payload = cls._default_payload()
        crypto = None
        for field in cls.ENCRYPTED_FIELDS:
            blob = row[cls.FIELD_COLUMN_MAP[field]]
            if not blob:
                payload[field] = ""
                continue
            if crypto is None:
                crypto = get_crypto()
            payload[field] = crypto.decrypt_field(bytes(blob), cls._aad(field, normalized_login))

        tax_blob = row[cls.TAX_COLUMN]
        if tax_blob:
            if crypto is None:
                crypto = get_crypto()
            tax_bundle = cls._deserialize_tax_bundle(
                crypto.decrypt_field(bytes(tax_blob), cls._aad("tax_id", normalized_login))
            )
            payload["tax_id"] = tax_bundle["tax_id"]
            payload["vat_id"] = tax_bundle["vat_id"]

        payload["address_country"] = cls._normalize_country(row["address_country"])
        payload["ust_status"] = cls._normalize_ust_status(row["ust_status"])
        payload["updated_at"] = row["updated_at"]
        return payload

    @classmethod
    def migrate_from_plaintext(cls, conn: Any) -> int:
        rows = conn.execute(
            """
            SELECT a.twitch_login, a.email, a.full_name, a.address_line1, a.address_city,
                   a.address_zip, a.address_country
            FROM affiliate_accounts a
            LEFT JOIN affiliate_pii p
              ON p.twitch_login = a.twitch_login
            WHERE p.twitch_login IS NULL
              AND (
                TRIM(COALESCE(a.email, '')) <> ''
                OR TRIM(COALESCE(a.full_name, '')) <> ''
                OR TRIM(COALESCE(a.address_line1, '')) <> ''
                OR TRIM(COALESCE(a.address_city, '')) <> ''
                OR TRIM(COALESCE(a.address_zip, '')) <> ''
                OR TRIM(COALESCE(a.address_country, '')) <> ''
              )
            """
        ).fetchall()
        migrated = 0
        for row in rows:
            cls.save_pii(
                conn,
                str(row["twitch_login"] or ""),
                {
                    "email": row["email"],
                    "full_name": row["full_name"],
                    "address_line1": row["address_line1"],
                    "address_city": row["address_city"],
                    "address_zip": row["address_zip"],
                    "address_country": row["address_country"],
                },
            )
            conn.execute(
                """
                UPDATE affiliate_accounts
                SET email = '',
                    full_name = '',
                    address_line1 = '',
                    address_city = '',
                    address_zip = '',
                    address_country = ''
                WHERE twitch_login = %s
                """,
                (str(row["twitch_login"] or ""),),
            )
            migrated += 1
        return migrated
