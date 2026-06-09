# ADR 0001 — Discord über die interne Bridge, kein eigener Gateway

- **Status:** akzeptiert (2026-06-08)
- **Kontext-Doku:** [`00-overview.md`](../00-overview.md), [`04-cutover-plan.md`](../04-cutover-plan.md)

## Kontext

Der Python-Twitch-Bot lief als **Cog** innerhalb des größeren discord.py-Master-Bots
(Deadlock-Bots). Discord-Aktionen (Live-Embeds, Rollen-Sync, Invite-Codes) gehen heute schon über
den **Master-Broker** (interne HTTP-Bridge, Port 8770, geteilter `TWITCH_INTERNAL_API_TOKEN` +
`X-Idempotency-Key`).

Bei der Frage „voller Rust-Discord-Client?" stellt sich ein hartes technisches Limit: Discord
erlaubt pro Bot-Token nur **eine** Gateway-Session pro Shard. Ein neuer serenity/twilight-Client
mit demselben Token wie der Master-Bot würde eine Disconnect-Schleife auslösen.

## Entscheidung

Der Rust-Twitch-Bot öffnet **keinen** eigenen Discord-Gateway. Discord-Sends laufen weiter über die
bestehende interne Bridge (Master-Broker). **Jeder Bot besitzt nur seinen eigenen Teil**; cross-Bot
Discord-Aktionen werden relayed.

Technisch: `tb-transport-discord` stellt einen `DiscordBackend`-Trait mit den Impls `BrokerRelay`
(HTTP an 8770) und `HeadlessNoop` (Tests/headless). Gebaut und produktiv genutzt wird `BrokerRelay`.

## Konsequenzen

**Positiv:**
- Löst die Token-Kollision ohne zweiten Bot-Account und ohne Guild-Neueinrichtung.
- Niedrigstes Risiko; klare Bot-Grenzen (Single Responsibility pro Prozess).
- **Die separate „Discord-Gateway-Cutover"-Phase entfällt** — Discord ist über alle Phasen das
  Relay, kein Migrationsschritt. serenity ist aus dem kritischen Pfad raus.

**Negativ / zu beachten:**
- Inbound-Discord-**Events** (Button-Klicks, Slash-Command-Interaktionen für Twitch-Bot-Komponenten)
  müssen über die Bridge **zum** Rust-Bot weitergeleitet werden. Vor den Discord-berührenden Phasen
  (4/6) ist zu verifizieren, ob die Bridge das heute schon tut oder ob ein Inbound-Forwarding-Pfad
  ergänzt werden muss.
- Latenz/Kopplung an den Master-Broker bleibt bestehen.

## Verworfene Alternativen

- **Eigener Bot-Account/Token** (eigener Gateway): saubere Trennung + echte Interaktions-Events,
  aber neue Bot-Einladung + Rollen/Permissions in allen Guilds. Zurückgestellt; bei Bedarf später
  als neue `DiscordBackend`-Variante nachrüstbar, ohne Architektur-Bruch.
- **Master-Bot ganz ablösen**: größter Scope, betrifft Discord-Funktionen außerhalb des
  Twitch-Bots. Nicht Teil dieses Rewrites.
