# Auftrag: Fallliste aus dem Nutzer-Dashboard entfernen

Der Nutzer verlangt zur Liste „Gemeldete Fälle“ unter `/twitch/verwaltung#bot`: „entferne das warum auch immer sowas in eimem user dashboard aufaucht“.

Der Screenshot zeigt viele Einträge desselben Kontos mit 0 Prozent Sicherheit und dem Status „Wird beobachtet“. Die Ergänzung des Nutzers lautet: „zudem ist mein ganzes Dashboard voll damit idk das muss ein Bug sein“.

Die Fallliste, ihre Detailansichten und zugehörigen Moderationsknöpfe sollen vollständig aus der Nutzeroberfläche verschwinden. Eine Ersatz-Fallverwaltung ist nicht beauftragt. Die Einstellungen des Scam-Schutzes bleiben vorhanden. Die Rust-Endpunkte und echte gespeicherte Moderationsentscheidungen bleiben bestehen.

Zur Ursache gehört ein Fix im Schreibpfad: technische Judge-Ausfälle dürfen keine inhaltlichen 0-Prozent-Urteile und keine Fallflut erzeugen. Wiederkehrende gleichartige Fehler müssen die bestehende Begrenzung für Warnungen nutzen. Historische ausschließlich technisch erzeugte Fälle dürfen einmalig gezielt bereinigt werden.
