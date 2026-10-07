# Offener Zugang für den Live-Funktionsbeweis

status: aktiv, 2026-10-08

FRAGE AN ORCHESTRATOR: Für den produktiven Vorbereitungsweg fehlt ein erlaubter angemeldeter Browserkontext. `GET http://127.0.0.1:8769/social-media/api/clips/124768/tiktok/creator-info?streamer=earlysalty` antwortet ohne Anmeldung mit 401. DashboardAuthLevel verwendet den bestehenden Sessionzugang oder einen internen geheimen Wert. AuthLevel hat ausdrücklich keinen Loopback-Bypass mehr. Secrets oder vorhandene Sessions aus Dateien, DB oder Prozessumgebung werden nicht gelesen.

Empfehlung: Anmeldung in einem eigenen isolierten Moli-Profil bereitstellen, ohne persönliche Browserprofile zu verwenden. Danach kann der ausdrücklich erlaubte Vorschau-Vorbereitungspfad geprüft werden. Keine Zustimmung speichern, kein TikTok-Upload, keine Tokenrotation und kein Kontentrennen. Keine neue Auth-Abkürzung im Produktcode.

Arbeitsstand: Worktree `/home/nathanael/.worktrees/tb-tiktok-preview-freigabe-20261007`, Branch `fix/tiktok-preview-freigabe-20261007`, Ausgangs-HEAD `b0bd68248c3accc1771e938e6166c3a122ac154e`, eigene integrierte Änderungen noch in Abschlussprüfung. Browser-Fixture mit realer Komponente und synthetischem Konto: fünf beobachtete Fälle bestanden. Bau und Gate werden weitergeführt. Live-Abschluss und Cleanup erst mit belastbarem Funktionsnachweis.
