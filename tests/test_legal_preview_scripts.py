import unittest

from aiohttp.test_utils import TestClient, TestServer

from scripts import export_legal_preview, preview_legal_pages


class LegalPreviewScriptsTests(unittest.IsolatedAsyncioTestCase):
    async def test_preview_server_serves_legal_page_without_gate_configuration(self) -> None:
        app = preview_legal_pages.build_app()

        async with TestServer(app) as server:
            async with TestClient(server) as client:
                response = await client.get("/twitch/impressum")
                body = await response.text()

        self.assertEqual(response.status, 200)
        self.assertIn("Impressum", body)
        self.assertEqual(
            response.headers.get("X-Robots-Tag"),
            "noindex, nofollow, noarchive, nosnippet, noimageindex",
        )

    async def test_static_export_renders_legal_pages_without_gate_configuration(self) -> None:
        pages = await export_legal_preview._render_pages()

        impressum = pages["twitch/impressum/index.html"]
        datenschutz = pages["twitch/datenschutz/index.html"]

        self.assertIn("Impressum", impressum)
        self.assertIn("Datenschutzerklärung", datenschutz)
        self.assertIn("<meta name='robots' content='noindex, nofollow'>", impressum)
        self.assertIn("href='../datenschutz/index.html'", impressum)

