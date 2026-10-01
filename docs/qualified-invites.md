# Persönliche Discord-Links und Streamer-Empfehlungen

Stand: 27. September 2026. Zusammengehöriges Paket zu Deadlock-Bots, Branch `codex/qualified-discord-invites`. Keine Produktionsmigration, kein Merge, kein Deploy und kein Neustart gehören zu diesem Arbeitsabschluss.

## Bestandsprüfung vor dem Bau

Graphify wurde vor der Implementierung ausgewertet: `graphify-out/graph.json`, Graph-Commit `8c800bb92ebaa6c4b396cb20d9590a06ab1ca105`. Die Arbeitsbasis `3dbf95f2` war neuer; die gefundenen Pfade wurden im tatsächlichen Quelltext nachgeprüft.

Relevante Knoten waren `commands.rs::cmd_dldc`, `affiliate.rs::claim_streamer_at`, `tb-raid::partner_setup::promote_streamer_to_partner`, `tb-internal-api::streamer_lifecycle::promote_streamer_to_partner` und die Funktionen in `affiliate_claim_window.rs`. Die allgemeinen `referral_url`-Funktionen des Announcement- und Telemetriepfads messen Discord→Twitch-Klicks und sind keine Partner-Empfehlungen. `!invite` ist ein Spielzugangs-/Discord-Befehl und ebenfalls nicht die Partneraufnahme.

Die vorhandene dauerhafte Werberzuordnung ist `affiliate_streamer_claims`, geschrieben im bereits authentifizierten Affiliate-/Vertriebler-Claim-Flow. Ein allgemeiner zweiter Partner-Referral-Datensatz wurde im geprüften Bestand nicht gefunden. Dieses Paket nutzt ausdrücklich den bestehenden Claim-Flow; es führt weder ein zusätzliches OAuth noch ein zweites Anmelde- oder Werberformular ein. Ein Partner ohne entsprechenden bestehenden Claim erhält dadurch nicht automatisch einen Referral-Credit.

## `!dldc` und `!dlde`

Die Entscheidung Broadcaster/Zuschauer erfolgt über die beiden Twitch-User-IDs aus dem Chatereignis, nicht über Login, Anzeigenamen oder Badges. Ein Broadcaster bekommt seinen bisherigen Kanallink. Ein Zuschauer fordert einen persönlichen Link über den vorhandenen `BrokerRelay` an.

Die erfolgreiche Chatantwort ist ausschließlich die URL. Es gibt keine vorangestellte Discord-Beschriftung, keine Erwähnung, keine Punkte und keine Scores. Andere Invite-Fragen und `!invite` werden durch diesen Befehlspfad nicht umdefiniert.

Der neue typisierte Relay-Aufruf verwendet denselben HTTP-Client, denselben Infisical-gelieferten internen Token, dieselbe Retry- und Idempotenzmechanik wie bestehende Broker-Aktionen. Er akzeptiert nur eine numerische Loopback-Adresse. Es gibt keine Redirect-Verfolgung und keine zusätzliche Secret-Ablage. Ungültige Broker-Antworten oder URLs außerhalb `https://discord.gg/` werden nicht im Chat ausgegeben. Ist der Broker oder die persönliche Erstellung nicht verfügbar, verwendet der Bot den vorhandenen Kanallink.

```text
POST /internal/master/v1/twitch/personal-invite
```

```json
{
  "streamer_login": "streamer",
  "streamer_twitch_user_id": "42",
  "inviter_twitch_user_id": "43"
}
```

Die IDs im Beispiel sind synthetisch. Die bestehende Broker-Hülle liefert `result.invite_url` und `result.personal`. Das Backend in Deadlock-Bots löst Guild und Zielkanal aus seiner gespeicherten Zuordnung auf. Der Twitch-Aufrufer kann keinen beliebigen Discord-Zielkanal übergeben.

Der vorhandene Export `/internal/twitch/v1/streamer-invites` enthält zusätzlich Twitch-User-ID und Discord-Kanal-ID. Fehlende Twitch-IDs in bisherigen Invite-Zeilen werden über die vorhandene aktive Partneransicht aufgelöst. Der bestehende `dl-twitch-invite-sync` übernimmt die Felder in die zentrale Datenbank. Es wird keine zweite Synchronisation aufgebaut.

## Discord-Grenze

Discord dokumentiert Fehler `30016` für maximal 1.000 Einladungen je Guild. Verifiziert am 27. September 2026:

- https://docs.discord.com/developers/topics/opcodes-and-status-codes
- https://docs.discord.com/developers/resources/channel#create-channel-invite

Die persönliche Erstellung ist dauerhaft und unbegrenzt nutzbar. Der Dateikonfigurations-Deckel, die Guild-Reserve, ihre Transaktionssperre und die Fallback-Zuordnung liegen ausschließlich in Deadlock-Bots. Dort ist `personal_links_per_channel = 10` und eine Reserve von 50 Einladungen in der Repository-Konfiguration eingestellt. Ohne neue Konfigurationssektion werden keine neuen persönlichen Links angelegt; gespeicherte Links bleiben nutzbar. Ein Kanal-Fallback hat bei einer späteren Join-Attribution keinen Zuschauer als Werber.

## Qualifizierte Discord-Joins

Der zentrale Broker liefert über

```text
GET /internal/master/v1/twitch/qualified-invites?since=<RFC3339>
```

die Zustände `pending`, `qualified` und `expired` mit `join_id`, `streamer_login`, nullable `inviter_twitch_user_id`, `joined_at`, `qualified_at` und `updated_at`. Der gekoppelte PR enthält den vollständigen Cursor-/Wasserstand-Vertrag. `since` bezieht sich auf Änderungen und lässt spät qualifizierte, ältere Joins nicht verloren gehen. Weder Discord-Namen noch Nachrichteninhalte werden zurückgegeben.

Die zentrale Qualifikation prüft 14 Tage ursprüngliche Mitgliedschaft und die Aktivität innerhalb von 30 Tagen. Dieser Twitch-PR baut bewusst keine Wochenquests, keinen Punkte-/Level-Ledger und keine Raid-Belohnung: Diese späteren Verbraucher können den stabilen Join-Schlüssel verwenden.

## Streamer-Referral-Credit

Die einzige neue Twitch-Migration ist `20260927010000_streamer_referral_credits.sql`. Sie legt `twitch_streamer_referral_credits` an. Die Quellzuordnung bleibt in `affiliate_streamer_claims`.

Ein Credit entsteht nur bei der ersten neu angelegten aktiven Partnerschaft, wenn bereits vorher ein passender, noch gültiger Claim existiert. Das vorhandene Reservierungsfenster beträgt vier Tage und wird nicht kopiert: Seine Definition liegt jetzt gemeinsam in `tb-domain::referral_window` und wird von der bisherigen Affiliate-Logik wieder exportiert. Die bestehende monetäre Nachmeldefrist bleibt für Provisionen unverändert, erzeugt aber keinen nachträglichen Effort-Credit.

Der Werber wird über `affiliate_accounts.twitch_user_id` aufgelöst und muss selbst aktiver Partner sein. Inaktive, archivierte, ausgeschiedene oder manuell abgemeldete Werber qualifizieren nicht. Selbstempfehlungen und mehrdeutige Claim-/Partnerzeilen werden abgewiesen. Der Claim des bestehenden Flows ist nach Ziel-Login gespeichert; der neue permanente Credit wird dagegen mit den verifizierten Twitch-IDs beider Konten geschrieben. Ein in der Zwischenzeit umbenanntes Ziel wird ohne passend auflösbaren bestehenden Claim nicht nachträglich anhand einer Namensvermutung kreditiert.

Beide produktiven Aktivierungspfade verwenden denselben Credit-Writer:

- OAuth-/Partner-Setup in `tb-raid/src/partner_setup.rs`.
- Interne Verifikation in `tb-internal-api/src/streamer_lifecycle.rs`.

Partneraktivierung und Credit stehen in derselben Postgres-Transaktion. Scheitert der Credit-Insert, wird eine neue Aktivierung ebenfalls zurückgerollt. Die Verifikation verwendet dafür jetzt explizit eine Transaktion. Bestehende aktive Partner, Reauths und Reaktivierungen erzeugen keinen neuen Credit. Konfliktbehandlung und Primärschlüssel auf der geworbenen Twitch-ID verhindern Doppelvergabe; Trigger verhindern nachträgliches Ändern oder Löschen.

Verbraucher lesen:

```sql
SELECT source_id, event_type, streamer_twitch_user_id, streamer_login,
       referred_twitch_user_id, referred_login, source_type,
       source_claimed_at, credited_at
FROM twitch_streamer_referral_credits
ORDER BY credited_at, source_id;
```

`event_type` ist `streamer_referral`, `source_id` ist `streamer_referral:<geworbene Twitch-ID>`. Es werden in diesem Paket keine numerischen Punkte vergeben und keine monetären Provisionsdaten geändert.

## Tests und gemeinsamer Abschluss

Die neuen Tests prüfen echte Chatereignis-IDs, den reinen URL-Text, Broker-Hüllen und Fallbacks, stabile Idempotenzschlüssel, unzulässige URLs und Gegenstellen, erste Aktivierung, alte und nachträgliche Claims, doppelte Aktivierung sowie atomaren Rollback. Die bestehenden Partner-Suites verwenden die tatsächliche neue Credit-Migration statt einer abweichenden nachgebauten Tabelle.

Postgres-Nachweise werden nur gegen isolierte Wegwerf-Datenbanken ausgeführt. `docs/qualified-invites-evidence.md` und die PR-Beschreibung halten ausgeführte Befehle, Resultate und offene Punkte fest. Die PR-Beschreibung enthält zusätzlich exakten Head-SHA und GitHub-Actions-Links.

Die PR bleibt Draft: Der bestehende Release-Gate-Workflow im gekoppelten Discord-Repo kann sonst automatisch mergen. Beide Paketstände sind gemeinsam zu prüfen; die Freigabe für einen späteren Rollout ist durch diese PRs nicht erteilt.
