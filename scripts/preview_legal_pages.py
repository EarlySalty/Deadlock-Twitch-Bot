"""Minimal local preview server for Twitch legal pages."""

from __future__ import annotations

import argparse

from aiohttp import web

from bot.dashboard.admin.legal_mixin import _DashboardLegalMixin


class _LegalPreviewApp(_DashboardLegalMixin):
    """Local-only legal preview that bypasses the production human gate."""

    @staticmethod
    def _legal_page_request_is_blocked(_request: web.Request) -> bool:
        return False

    def _legal_gate_is_enabled(self) -> bool:
        return True

    def _legal_gate_cookie_is_valid(self, _request: web.Request) -> bool:
        return True

    async def landing(self, request: web.Request) -> web.StreamResponse:  # noqa: ARG002
        page = (
            "<!doctype html><html lang='de'><head><meta charset='utf-8'>"
            "<meta name='viewport' content='width=device-width,initial-scale=1'>"
            "<title>Legal Preview · EarlySalty</title>"
            "<style>"
            "body{margin:0;background:#0f172a;color:#e2e8f0;"
            "font-family:Segoe UI,Arial,sans-serif;line-height:1.6;}"
            ".wrap{max-width:760px;margin:0 auto;padding:48px 20px 64px;}"
            "h1{font-size:2rem;margin:0 0 12px;font-weight:800;}"
            "p{margin:0 0 14px;color:#cbd5e1;}"
            ".grid{display:grid;gap:14px;margin-top:28px;}"
            ".card{display:block;padding:18px 20px;border-radius:16px;"
            "background:#111c33;border:1px solid #334155;color:#f8fafc;text-decoration:none;}"
            ".card:hover{border-color:#60a5fa;background:#13213d;}"
            ".label{display:block;font-size:1rem;font-weight:700;margin-bottom:6px;}"
            ".meta{font-size:.95rem;color:#94a3b8;}"
            "</style></head><body><div class='wrap'>"
            "<h1>Local Legal Preview</h1>"
            "<p>Diese lokale Vorschau dient nur zum Testen der Twitch-Rechtsseiten.</p>"
            "<div class='grid'>"
            "<a class='card' href='/twitch/impressum'>"
            "<span class='label'>Impressum</span>"
            "<span class='meta'>Prüfe Rendering und noindex-Markup.</span>"
            "</a>"
            "<a class='card' href='/twitch/datenschutz'>"
            "<span class='label'>Datenschutz</span>"
            "<span class='meta'>Prüfe Rendering und noindex-Markup.</span>"
            "</a>"
            "<a class='card' href='/twitch/agb'>"
            "<span class='label'>AGB</span>"
            "<span class='meta'>Optionaler Vergleich zur restlichen Rechtsnavigation.</span>"
            "</a>"
            "</div></div></body></html>"
        )
        return web.Response(text=page, content_type="text/html")

    async def abbo_entry(self, request: web.Request) -> web.StreamResponse:  # noqa: ARG002
        page = (
            "<!doctype html><html lang='de'><head><meta charset='utf-8'>"
            "<meta name='viewport' content='width=device-width,initial-scale=1'>"
            "<title>Abo Placeholder · EarlySalty</title>"
            "<style>"
            "body{margin:0;background:#f8fafc;color:#0f172a;"
            "font-family:Segoe UI,Arial,sans-serif;line-height:1.7;}"
            ".wrap{max-width:700px;margin:0 auto;padding:40px 20px 60px;}"
            "h1{font-size:1.7rem;margin:0 0 8px;font-weight:800;}"
            "p{font-size:15px;color:#334155;margin:0 0 10px;}"
            "a{color:#2563eb;text-decoration:none;}"
            "a:hover{text-decoration:underline;}"
            "</style></head><body><div class='wrap'>"
            "<h1>Lokale Testseite</h1>"
            "<p>Dieser Platzhalter existiert nur, damit die Zurück-Links der Rechtsseiten lokal funktionieren.</p>"
            "<p><a href='/'>Zur Startübersicht</a></p>"
            "</div></body></html>"
        )
        return web.Response(text=page, content_type="text/html")


def build_app() -> web.Application:
    preview = _LegalPreviewApp()
    app = web.Application()
    app.add_routes(
        [
            web.get("/", preview.landing),
            web.get("/twitch/abbo", preview.abbo_entry),
            web.get("/twitch/impressum", preview.abbo_impressum),
            web.get("/twitch/datenschutz", preview.abbo_datenschutz),
            web.get("/twitch/agb", preview.abbo_agb),
        ]
    )
    return app


def main() -> None:
    parser = argparse.ArgumentParser(description="Preview Twitch legal pages locally.")
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=8088)
    args = parser.parse_args()
    web.run_app(build_app(), host=args.host, port=args.port)


if __name__ == "__main__":
    main()
