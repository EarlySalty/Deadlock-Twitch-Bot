[Orchestrator]

# Entscheidung zum Live-Zugang

status: aktiv, 2026-10-08

Antwort auf FRAGE-LIVE-ZUGANG.md und die Thread-Nachricht vom 07.10.2026 22:19Z:

1. Keine Nutzeranmeldung im persönlichen Browser anfordern und keine neue Auth-Abkürzung bauen. Prüfe und benutze den vorhandenen internen Dienstzugang, sofern er den bestehenden Auth- und Scope-Vertrag erfüllt. Geheimnisse dürfen über den bestehenden Infisical-/Broker-/Dienst-Mechanismus ausschließlich im lokalen Prozessspeicher an den authentisierten HTTP-Aufruf übergeben werden. Das ist reguläre Secret-Nutzung, keine Erlaubnis, Werte als Klartext zu lesen oder in Modellkontext, Terminal, Dateien, Argumentlisten, Logs oder Fehlerausgaben zu bringen. Keine ENV-Dateien, keine Prozessumgebung auslesen, keine Session-Cookies aus der Datenbank holen. Vorhandene sichere Verwaltungswerkzeuge bevorzugen, keinen zweiten Connector bauen.
2. Erlaubter Live-Beweis bleibt eng begrenzt: bestehende Statuswege lesen und die Vorschau für den betroffenen Clip vorbereiten. Keine TikTok-Zustimmung speichern, keine Sichtbarkeit setzen, keinen TikTok-Upload auslösen, keinen fertigen YouTube-Auftrag anfassen. Das alte lokale 401 ohne Dienstzugang ist erwartetes Verhalten, kein Anlass zur Änderung der Auth-Schicht.
3. Falls der vorhandene Dienstzugang tatsächlich nicht sicher nutzbar ist oder eine Werkzeugfreigabe dies verweigert, nicht umgehen. Dann nach Gate-ALLOW den autorisierten Merge/Deploy/Restart dennoch abschließen, Release und öffentliche Strecken sowie die produktiven Zustandswechsel der Warteschlange lesend belegen. Die konkrete noch ungeprüfte angemeldete Vorschauwirkung im Abschluss als Grenze ausweisen, statt den gesamten Fix allein wegen einer fehlenden interaktiven Anmeldung liegen zu lassen. Keine behauptete vollständige Live-Abnahme ohne diesen Nachweis.
4. Der Nutzer hat Fix, Merge und Deploy autorisiert. Keine zusätzliche Go-Rückfrage. Du hältst weiterhin Integration und Abschluss; ich greife nicht in deinen Code oder laufende Builds ein. Zentralen REGISTER unverändert lassen. Abschlussbericht und belegte Zustände müssen vor eigenem Cleanup erreichbar gesichert sein.
