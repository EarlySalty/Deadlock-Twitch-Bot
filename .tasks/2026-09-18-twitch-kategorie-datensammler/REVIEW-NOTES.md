# Unabhängige laufende Reviewnotizen

Stand des gerade sichtbaren Diffs, noch kein finales Urteil. Vor Freigabe erneut gegen fertigen Code prüfen.

## H1: mehrgliedrige Cursor-Schleifen
streams.rs::get_streams_by_category_full merkt nur previous_cursor. Das stoppt c1,c1, aber NICHT c1,c2,c1,c2 bei duplizierten Stream-IDs. Da das Cap auf deduplizierte streams.len() schaut, wächst dieses dann nicht mehr: endlose Request-Schleife möglich. Alle bereits gesehenen Cursor merken, zusätzlich Page-/Zeitbudget separat von eindeutiger Streamzahl; Test c1→c2→c1 mit stets gleichem Stream muss terminieren und complete=false liefern.

## H2: Ratelimit-Header noch nicht verarbeitet
client.rs::send_with_retry wiederholt ausschließlich 5xx/transiente Netzwerkfehler und gibt 401/403/429 direkt zurück (bestehendes Verhalten absichtlich). Bloße Nutzung dieses Helpers ist KEIN header-basierter Ratelimiter. Neuer Kategoriepfad muss Ratelimit-Remaining/Reset bzw. 429-Reset auswerten und weitere Polls/Seiten bis dahin drosseln. Gemeinsamen Bot-Client nicht durch unbedachte Retries mutierender Endpunkte ändern. Nachweis/Test für 429 und remaining=0 nötig.

## M1: Cap und vollständige Antwort
Die aktuelle Schleife kann ein hard_cap mitten in einer vollen 100er-Seite überschreiten und bei fehlendem Cursor dann complete=true melden. Entweder Cap explizit als Page-/Sicherheitsbudget definieren oder wirklich streamgenau kappen und unvollständig melden. Entscheidend: kein verschleierter Teilbestand.

## M2: kritische Felder/Fehlerwerte
StreamsResponse hat default data/pagination, HelixStream Default-Felder. Eine fehlerhafte 200-JSON-Form darf nicht als vollständig leere Kategorie gelten. Neue API-Seite bzw. Collector validiert IDs/started_at und game_id, bevor Roster/Snapshots übernommen werden. Keine leere Stream-ID als universeller Dedupe-Schlüssel. Fehlender Pflichtwert muss sichtbar fehlerhaft statt null Aktivität sein.

## H3: Sharding garantiert Obergrenze nicht
anon_chat.rs::synchronize_shards nimmt ceil(N/100) Shards und verteilt dann ungekappt per Jump-Hash. Bei 200 Kanälen channel_000..channel_199 liefert genau eure FNV+Jump-Funktion [103,97], bei300 [100,98,102]. Damit werden mehr als100 Räume je Verbindung gejoint, die beschriebene Grenze wird verletzt. Stateful stabile Zuordnung mit freien Slots (bestehende Zuordnungen behalten, neue Kanäle in nichtvolle Shards, ggf. weitere Shards) statt nur statistischer Verteilung. Test Obergrenze für200/300/>100 und geringe Rosteränderung ohne Umsortieren erforderlich.

## H4: Reader cancellation safety und tatsächlich bedienbarer PING
serve_verbindung verwendet reader.read_line(&mut line) direkt in tokio::select!. AsyncBufReadExt::read_line ist bei Cancellation nicht sicher; ein anderer fertiger Zweig kann Teilnachrichten verlieren, ohne Drop-Zähler. Lines::next_line oder cancel-safe read_until mit begrenztem Zeilenpuffer verwenden. Außerdem wartet der gewählte JOIN-Zweig auf throttle.slot().await bis10s und blockiert so den Reader; ein Limiter-Permit muss als konkurrierender Future im select laufen oder zentral separat zugestellt werden. Test fragmentierte IRC-Zeile+gleichzeitiger Join/Set sowie PING während erschöpftem globalem Joinbudget.

## H5: Idle-Liveness wird durch jeden Poll zurückgesetzt
serve_verbindung berechnet lese_frist bei jeder Schleifeniteration als Instant::now()+read_idle_before_ping. Set-Kanalroster alle60s hält diese240s-Frist endlos frisch, auch wenn die Verbindung keinerlei Antworten mehr liefert. last_received timestamp außerhalb der Schleife pflegen und nur bei echtem Netztraffic zurücksetzen, nicht bei Roster/Join. Test fortlaufende Set-Updates und schweigender Server muss trotzdem reconnecten.

## H6: Moderationslöschungen erreichen Sink nicht
Reader reicht ausschließlich Zeilen mit ' PRIVMSG ' weiter; CLEARMSG/CLEARCHAT werden verworfen. Damit kann der Collector gelöschte Rohtexte nicht wie beauftragt entfernen. Kontrollereignisse im selben geordneten Kanal mitgeben (Scout darf sie einfach ignorieren); kein eigener Twitch-Schreibaufruf. Nachlieferungen/dedup und Rollup-Transaktion beachten.

## M3: Connect-Drossel, Config-Bounds und Nick
Globaler Limiter drosselt bisher nur JOIN, nicht gleichzeitige Connect/Handshake-Versuche. Cap auf erlaubte Werte und positiven Queuegrößen sicherstellen, sonst JoinThrottle::slot bei0 panic. Separater Default-Nickbereich für Collector und Bot (oder sicherer zufälliger Prozess-Präfix) verbessert Identitätstrennung. Ein Client-PING ist ein sicherer Kontrollbefehl, Headerkommentar entsprechend ehrlich statt 'außer JOIN/PART/PONG nichts schreiben' trotz NICK/CAP/PING.

## H7: Retention ist eine Obergrenze, keine Mindest-Aufbewahrung
Migration setzt CHECK(retention_days>=90) und Kommentar mindestens90. Nutzerauftrag: Rohchat90Tage, danach weg, kein unbegrenztesWachstum. Default90, positive kleine Werte erlauben, Max90 solange keine neue explizite Freigabe; Retentionmuss unabhängig von disabled/rollup_enabled weiterlaufen. Rollup_enabled als optionalerAbschalter darf Pflichtretention nicht verhindern. StündlicheAggregatevorLöschen dauerhaftsichern, beiAusfallCatch-up nötig.

## H8: Unbelegte Kategorie-ID und Schema-Integrität
Migrationseed deadlock_game_id='142775730' ist bisher nicht aus existierendemCode oder Helix belegt. Vor dem erstenRoster/Snapshot exakt viaHelix auf Deadlock auflösen/validieren, niebloßgerateneIDzumSammelnverwenden. Die tatsächlicheTwitchgameID muss imReportmitLesenachweisbelegtsein; vorhandene twitch_live_state.last_game_id sindleer und taugennichtalsNachweis. Wenn ConfigID optionalleeroderstale, exakteNamenssuche (keinPrefixfallback) und validiertesID-Update.
Snapshot(Poll,Stream) braucht UNIQUEfüridempotenteWiederholung; sent_at/ids/viewerCountvalidieren; thumbnail_url fehltnochgegenAnforderung. source-room-id muss neben source-id verfügbarsein, umSharedChatkorrekt Originalquelle zuzuordnen. GetUsersMetadaten/ProfilecreatedAt fehlen nochals tatsächlicherDatenpfad, Tabellenallein keineFeaturebehauptung.

## Grenzen der Prüfung bisher
Noch kein Collector/Storage/Frontend zum Zeitpunkt dieser Notizen fertig. Keine Livefreigabe.
