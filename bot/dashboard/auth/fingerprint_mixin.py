"""Handlers for the post-login JS fingerprint collection flow."""

from __future__ import annotations

import hashlib
from typing import Any

from aiohttp import web


_FP_COLLECT_HTML = """<!DOCTYPE html>
<html lang="de">
<head>
<meta charset="utf-8">
<title>Admin-Authentifizierung</title>
<style>
body {
  font-family: system-ui, sans-serif;
  background: #0d1117;
  color: #e6edf3;
  display: flex;
  align-items: center;
  justify-content: center;
  height: 100vh;
  margin: 0;
}
.box { text-align: center; }
p { opacity: 0.7; font-size: 0.95rem; }
</style>
</head>
<body>
<div class="box">
  <p>Sicherheitspruefung laeuft...</p>
  <noscript><p style="color:#f85149">JavaScript ist erforderlich.</p></noscript>
</div>
<script>
(function() {
  function canvasHash() {
    try {
      var c = document.createElement("canvas");
      var ctx = c.getContext("2d");
      ctx.textBaseline = "top";
      ctx.font = "14px Arial";
      ctx.fillStyle = "#f60";
      ctx.fillRect(125, 1, 62, 20);
      ctx.fillStyle = "#069";
      ctx.fillText("DDC-Admin-Auth | " + (navigator.language || ""), 2, 15);
      ctx.fillStyle = "rgba(102,204,0,0.7)";
      ctx.fillText("DDC-Admin-Auth | " + (navigator.language || ""), 4, 17);
      return c.toDataURL();
    } catch (err) {
      return "no-canvas";
    }
  }

  function rawFingerprint() {
    var timezone = "";
    try {
      timezone = Intl.DateTimeFormat().resolvedOptions().timeZone || "";
    } catch (err) {}
    return [
      (screen.width || 0) + "x" + (screen.height || 0),
      timezone,
      navigator.language || "",
      String(navigator.hardwareConcurrency || 0),
      canvasHash()
    ].join("||");
  }

  function sha256hex(str) {
    if (window.crypto && window.crypto.subtle) {
      var enc = new TextEncoder();
      return window.crypto.subtle.digest("SHA-256", enc.encode(str)).then(function(buf) {
        return Array.from(new Uint8Array(buf)).map(function(b) {
          return b.toString(16).padStart(2, "0");
        }).join("").slice(0, 32);
      });
    }

    var h = 0;
    for (var i = 0; i < str.length; i++) {
      h = ((h << 5) - h + str.charCodeAt(i)) | 0;
    }
    return Promise.resolve(("00000000" + Math.abs(h).toString(16)).slice(-8).padStart(32, "0"));
  }

  sha256hex(rawFingerprint()).then(function(hash) {
    var form = document.createElement("form");
    form.method = "POST";
    form.action = "/twitch/auth/fingerprint";

    var fp = document.createElement("input");
    fp.type = "hidden";
    fp.name = "fp";
    fp.value = hash;
    form.appendChild(fp);

    document.body.appendChild(form);
    form.submit();
  });
})();
</script>
</body>
</html>
"""


async def fingerprint_page(server: Any, request: web.Request) -> web.Response:
    """Serve the JS fingerprint collector for authenticated admin sessions."""
    cookie_name = str(
        getattr(server, "_discord_admin_cookie_name", None)
        or getattr(server, "_session_cookie_name", None)
        or "twitch_dash_session"
    )
    session_id = (request.cookies.get(cookie_name) or "").strip()
    if not session_id:
        raise web.HTTPFound("/twitch/auth/discord/login")

    getter = getattr(server, "_get_discord_admin_session", None)
    session = getter(request) if callable(getter) else None
    if not session:
        raise web.HTTPFound("/twitch/auth/discord/login")

    return web.Response(
        text=_FP_COLLECT_HTML,
        content_type="text/html",
        charset="utf-8",
    )


async def fingerprint_submit(server: Any, request: web.Request) -> web.StreamResponse:
    """Persist the JS fingerprint and complete the Discord admin login flow."""
    cookie_name = str(
        getattr(server, "_discord_admin_cookie_name", None)
        or getattr(server, "_session_cookie_name", None)
        or "twitch_dash_session"
    )
    session_id = (request.cookies.get(cookie_name) or "").strip()
    if not session_id:
        raise web.HTTPFound("/twitch/auth/discord/login")

    getter = getattr(server, "_get_discord_admin_session", None)
    session = getter(request) if callable(getter) else None
    if not session:
        raise web.HTTPFound("/twitch/auth/discord/login")

    try:
        form_data = await request.post()
    except Exception:
        form_data = {}
    raw_fp = str(form_data.get("fp") or "").strip().lower()

    if raw_fp and 8 <= len(raw_fp) <= 64 and all(ch in "0123456789abcdef" for ch in raw_fp):
        session["js_fp"] = raw_fp
    else:
        session["js_fp"] = hashlib.sha256(b"fallback").hexdigest()[:32]
    session["fp_pending"] = False

    cache_getter = getattr(server, "_dashboard_auth_state_cache", None)
    if callable(cache_getter):
        try:
            cache_getter("_discord_admin_sessions").put(session_id, session)
        except Exception:
            pass

    repo_getter = getattr(server, "_dashboard_auth_state_repo", None)
    if callable(repo_getter):
        try:
            repo = repo_getter()
            repo.save_discord_admin_session(
                session_id=session_id,
                payload=session,
                created_at=float(session.get("created_at") or 0.0),
                expires_at=float(session.get("expires_at") or 0.0),
            )
        except Exception:
            pass

    safe_redirect = getattr(server, "_safe_internal_redirect", None)
    destination = str(session.get("post_fp_destination") or "/twitch/admin").strip() or "/twitch/admin"
    if callable(safe_redirect):
        destination = safe_redirect(destination, fallback="/twitch/admin")
    else:
        destination = "/twitch/admin"
    raise web.HTTPSeeOther(destination)
