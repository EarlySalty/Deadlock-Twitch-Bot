"""Export Twitch legal pages as static HTML for no-server local preview."""

from __future__ import annotations

import argparse
import asyncio
from pathlib import Path

from bot.dashboard.admin.legal_mixin import _DashboardLegalMixin


class _ExportPreviewApp(_DashboardLegalMixin):
    """Static export helper that bypasses the production human gate."""

    @staticmethod
    def _legal_page_request_is_blocked(_request) -> bool:
        return False

    def _legal_gate_is_enabled(self) -> bool:
        return True

    def _legal_gate_cookie_is_valid(self, _request) -> bool:
        return True


def _iis_web_config() -> str:
    return """<?xml version="1.0" encoding="utf-8"?>
<configuration>
  <location path="twitch/impressum">
    <system.webServer>
      <httpProtocol>
        <customHeaders>
          <add name="X-Robots-Tag" value="noindex, nofollow" />
        </customHeaders>
      </httpProtocol>
    </system.webServer>
  </location>
  <location path="twitch/datenschutz">
    <system.webServer>
      <httpProtocol>
        <customHeaders>
          <add name="X-Robots-Tag" value="noindex, nofollow" />
        </customHeaders>
      </httpProtocol>
    </system.webServer>
  </location>
  <system.webServer>
    <defaultDocument enabled="true">
      <files>
        <clear />
        <add value="index.html" />
      </files>
    </defaultDocument>
    <rewrite>
      <rules>
        <rule name="RootIndex" stopProcessing="true">
          <match url="^$" />
          <action type="Rewrite" url="index.html" />
        </rule>
        <rule name="LegalIndexPages" stopProcessing="true">
          <match url="^(twitch/(abbo|impressum|datenschutz|agb))/?$" />
          <action type="Rewrite" url="{R:1}/index.html" />
        </rule>
      </rules>
    </rewrite>
    <httpProtocol>
      <customHeaders>
        <remove name="X-Powered-By" />
      </customHeaders>
    </httpProtocol>
  </system.webServer>
</configuration>
"""


def _root_index() -> str:
    return (
        "<!doctype html><html lang='de'><head><meta charset='utf-8'>"
        "<meta name='viewport' content='width=device-width,initial-scale=1'>"
        "<title>Legal Preview Export · EarlySalty</title>"
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
        "<h1>Static Legal Preview</h1>"
        "<p>Diese exportierten HTML-Dateien kannst du ohne laufenden Dienst lokal öffnen.</p>"
        "<div class='grid'>"
        "<a class='card' href='twitch/impressum/index.html'>"
        "<span class='label'>Impressum</span>"
        "<span class='meta'>Statische Vorschau mit noindex-Markup.</span>"
        "</a>"
        "<a class='card' href='twitch/datenschutz/index.html'>"
        "<span class='label'>Datenschutz</span>"
        "<span class='meta'>Statische Vorschau mit noindex-Markup.</span>"
        "</a>"
        "<a class='card' href='twitch/agb/index.html'>"
        "<span class='label'>AGB</span>"
        "<span class='meta'>Zum Vergleich der restlichen Rechtsnavigation.</span>"
        "</a>"
        "</div></div></body></html>"
    )


def _abbo_index() -> str:
    return (
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
        "<p><a href='../../index.html'>Zur Startübersicht</a></p>"
        "</div></body></html>"
    )


def _rewrite_for_static(html: str) -> str:
    replacements = {
        "href='/twitch/abbo'": "href='../abbo/index.html'",
        "href='/twitch/impressum'": "href='../impressum/index.html'",
        "href='/twitch/datenschutz'": "href='../datenschutz/index.html'",
        "href='/twitch/agb'": "href='../agb/index.html'",
    }
    result = html
    for source, target in replacements.items():
        result = result.replace(source, target)
    return result


def _ensure_noindex_meta(html: str) -> str:
    meta = "<meta name='robots' content='noindex, nofollow'>"
    if meta in html:
        return html
    marker = "<title>"
    if marker in html:
        return html.replace(marker, f"{meta}{marker}", 1)
    return html


async def _render_pages() -> dict[str, str]:
    preview = _ExportPreviewApp()
    impressum = await preview.abbo_impressum(None)
    datenschutz = await preview.abbo_datenschutz(None)
    agb = await preview.abbo_agb(None)
    return {
        "index.html": _root_index(),
        "web.config": _iis_web_config(),
        "twitch/abbo/index.html": _abbo_index(),
        "twitch/impressum/index.html": _ensure_noindex_meta(
            _rewrite_for_static(impressum.text)
        ),
        "twitch/datenschutz/index.html": _ensure_noindex_meta(
            _rewrite_for_static(datenschutz.text)
        ),
        "twitch/agb/index.html": _rewrite_for_static(agb.text),
    }


async def _export(output_dir: Path) -> None:
    pages = await _render_pages()
    for relative_path, html in pages.items():
        target = output_dir / relative_path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(html, encoding="utf-8")


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Export Twitch legal pages as static HTML for local preview."
    )
    parser.add_argument(
        "--output-dir",
        default="build/legal-preview",
        help="Target directory for exported HTML files.",
    )
    args = parser.parse_args()
    output_dir = Path(args.output_dir).resolve()
    asyncio.run(_export(output_dir))
    print(output_dir)


if __name__ == "__main__":
    main()
