import unittest
from types import SimpleNamespace
from unittest.mock import AsyncMock, patch

from bot.social_media.clip_manager import ClipManager
from bot.social_media.dashboard import SocialMediaDashboard
from bot.social_media.rendering import (
    render_social_media_dashboard,
    render_social_media_privacy,
    render_social_media_terms,
)


class SocialMediaDashboardRenderingTests(unittest.IsolatedAsyncioTestCase):
    def test_render_social_media_dashboard_replaces_placeholders(self) -> None:
        html = render_social_media_dashboard(
            safe_streamer_label="@partner_one",
            safe_streamer_data="partner_one",
        )

        self.assertTrue(html.lstrip().startswith("<!DOCTYPE html>"))
        self.assertIn('data-auth-streamer="partner_one"', html)
        self.assertIn("<strong>@partner_one</strong>", html)
        self.assertNotIn("__SAFE_STREAMER_LABEL__", html)
        self.assertNotIn("__SAFE_STREAMER_DATA__", html)

    def test_terms_and_privacy_templates_render_from_files(self) -> None:
        terms = render_social_media_terms()
        privacy = render_social_media_privacy()

        self.assertIn("Nutzungsbedingungen", terms)
        self.assertIn("Datenschutzhinweis", privacy)
        self.assertTrue(terms.lstrip().startswith("<!DOCTYPE html>"))
        self.assertTrue(privacy.lstrip().startswith("<!DOCTYPE html>"))

    async def test_index_uses_file_backed_dashboard_template(self) -> None:
        dashboard = SocialMediaDashboard(
            clip_manager=ClipManager(),
            auth_checker=lambda _request: True,
            auth_level_getter=lambda _request: "admin",
        )
        request = SimpleNamespace()

        response = await dashboard.index(request)

        self.assertEqual(response.status, 200)
        self.assertEqual(response.content_type, "text/html")
        self.assertIn("Social Media Clip Manager", response.text)
        self.assertIn('data-auth-streamer=""', response.text)

    async def test_oauth_disconnect_runs_storage_work_in_thread(self) -> None:
        dashboard = SocialMediaDashboard(
            clip_manager=ClipManager(),
            auth_checker=lambda _request: True,
            auth_level_getter=lambda _request: "admin",
        )
        request = SimpleNamespace(match_info={"platform": "youtube"}, query={"streamer": "partner_one"})

        with patch.object(
            dashboard,
            "_disconnect_platform_sync",
            return_value=None,
        ) as mocked_loader, patch(
            "bot.social_media.dashboard.asyncio.to_thread",
            new=AsyncMock(side_effect=lambda func, *args, **kwargs: func(*args, **kwargs)),
        ) as mocked_to_thread:
            response = await dashboard.oauth_disconnect(request)

        self.assertEqual(response.status, 200)
        mocked_loader.assert_called_once_with("youtube", "partner_one")
        mocked_to_thread.assert_awaited_once()
        self.assertIs(mocked_to_thread.await_args.args[0], mocked_loader)

    async def test_platform_status_runs_lookup_in_thread(self) -> None:
        dashboard = SocialMediaDashboard(
            clip_manager=ClipManager(),
            auth_checker=lambda _request: True,
            auth_level_getter=lambda _request: "admin",
        )
        request = SimpleNamespace(query={"streamer": "partner_one"})
        expected = {
            "youtube": {
                "connected": True,
                "username": "partner_one",
                "user_id": "42",
                "expires_at": None,
                "expired": False,
                "uses_global_fallback": False,
            }
        }

        with patch(
            "bot.social_media.credential_manager.SocialMediaCredentialManager.get_all_platforms_status",
            return_value=expected,
        ) as mocked_loader, patch(
            "bot.social_media.dashboard.asyncio.to_thread",
            new=AsyncMock(side_effect=lambda func, *args, **kwargs: func(*args, **kwargs)),
        ) as mocked_to_thread:
            response = await dashboard.api_platforms_status(request)

        self.assertEqual(response.status, 200)
        self.assertIn("platforms", response.text)
        mocked_loader.assert_called_once_with("partner_one")
        mocked_to_thread.assert_awaited_once()


if __name__ == "__main__":
    unittest.main()
