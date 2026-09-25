# Unabhängige Prüfung B, Zwischenstand 18.09.2026

## H1: Reihenbildung verfälscht Durchschnitte und Intraday-Verlauf
kategorieWeltweitViewModel.ts::buildTrendreihe faltet alle API-Punkte eines Tages zu einem Tag und bildet jeweils (bisher+neu)/2. Drei Werte 0,0,100 ergeben so50 statt33,33; unterschiedliche poll_samples sind komplett ungewichtet. Außerdem verschwinden sämtliche intra-day-Gaps/Spitzen. Für Kategorie-Concurrency ist das falsch. API-Buckets mit vollem Zeitstempel direkt übernehmen, nach Abstand passende Null-Lücken einfügen (nicht ausgerechnet nur Tageswerte). Falls bewusst Tagesaggregation, dann gewichtete Summen mit poll_samples und explizit Tages-Schnitt beschriften, nicht beliebig nestedAvg. Bevorzugt stündliche API-Buckets ohne erneutes Aggregieren. Tests3Punkte am selbenTag, unterschiedlicheSamples und LückeinnerhalbTag.

## H2: Null-Schnitte müssen im TS-Vertrag zulässig bleiben
CategoryCollectorStreamLanguage/TopChannel.avg_viewers und Trend.avg_streams/avg_viewers sind derzeit number statt number|null. API-CONTRACT sagt unbekannte Schnitte=null. mittel(null,20) wird in JS10, also wieder erfundenerMesswert. Types+Berechnung+Tests korrigieren, nullnieals0durchArithmetik.

## M1: Fehlertext nicht Ursache erfinden
503 heißt nicht zwingend 'Datentabellen sind noch nicht eingerichtet', sondern kann DB-/Dienst-/Timeoutfehler sein. Generisch 'Sammler-Backend derzeit nicht verfügbar' plus freundlichTechnikfehler, Schemafehlt nurwenn error_codeesausweist. VorhandeneDaten bei vorübergehendemRefreshfehler dürfennichtalsaktuellohneHinweisdastehen.

## M2: Datum/Perioden robust behandeln
buildTrendreihe prüft nurtypeofbucket_atString; newDate('kaputt').toISOString wirft. UngültigeZeitstempelkontrolliertverwerfenoderFehler, nichtkomplettenAdmin-Tabcrashen. parseCollectorDays darf Number.parseInt('30irgendwas') nichtals30akzeptieren; genau7/30/90.

## Grenzen
Seite/Kartenroutingnochzuprüfen, nochkeinfinalesUrteil. Tests/BuildundReporttatsächlichabliefern, nichtnurFormalia. KeineRustdateienanfassen.
