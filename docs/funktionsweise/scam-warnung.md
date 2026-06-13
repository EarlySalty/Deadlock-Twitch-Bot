# Scam-Warnungen im Chat

## Worum es geht

Im Umfeld kleiner Deadlock-Streamer tauchen immer wieder Betrugsversuche auf: gefälschte
"offizielle" Discord-Server, die sich als Community ausgeben, und fremde Chatter, die
Streamern Dienstleistungen, mehr Viewer oder Designs andrehen oder sie auf andere
Plattformen locken wollen. Der Bot warnt sowohl Zuschauer als auch Streamer vor solchen
mutmaßlichen Scams. Er formuliert dabei bewusst vorsichtig ("könnte Scam sein") statt
mit einem harten Vorwurf, und er macht transparent, was er gerade tut.

## Was der Bot tut

Es gibt zwei getrennte Arten von Scam-Warnungen:

**1. Warnung vor gefälschten "offiziellen" Servern (an den ganzen Chat)**

- Der Bot postet im Chat eine kurze Warnung, dass bestimmte kursierende Discord-Server,
  die sich als die offizielle deutsche Deadlock-Community ausgeben, **nicht** die echten
  sind und Fake/Scam sein könnten.
- In derselben Nachricht nennt er den einzigen echten, offiziellen Discord, damit
  Zuschauer wissen, wohin sie sich stattdessen wenden können.
- Der Text ist bewusst zurückhaltend formuliert ("könnten Fake/Scam sein", "könnte Scam
  sein") — also ein Hinweis und keine harte Anschuldigung.
- Es gibt mehrere Formulierungen, die abwechselnd benutzt werden, sodass nie zweimal
  hintereinander derselbe Wortlaut im selben Kanal erscheint.

**2. Warnung vor verdächtigen Chattern (Scam-/Service-Pitches)**

- Schreibt ein fremder Chatter eine Nachricht, die wie ein typischer Scam- oder
  Verkaufs-Pitch aussieht (z. B. jemandem "mehr Viewer", Designs/Logos/Overlays oder eine
  "Zusammenarbeit" verkaufen, oder den Streamer von Twitch weg auf eine andere Plattform
  ziehen), erkennt der Bot das und kann darauf reagieren.
- Bei einem klaren Verdacht postet er eine kurze, sichtbare Warnung an den Streamer mit
  dem Hinweis, dass es sich um einen möglichen Scam-/Pitch-Versuch handeln könnte, und der
  Empfehlung, die Person zu ignorieren bzw. zu bannen.
- Auch hier ist die Wortwahl vorsichtig ("potenzieller Pitcher", "könnte Scam sein") statt
  eines harten Vorwurfs.
- Bei leichteren Verdachtsfällen hält sich der Bot zurück und wartet ab, statt sofort
  öffentlich zu warnen.
- Ignoriert die verdächtige Person eine bereits ausgesprochene Warnung und macht trotzdem
  weiter, kann der Bot sie kurz in den Timeout setzen und eine letzte Warnung samt
  Bann-Empfehlung posten.

Beide Warnungen sind klar als Bot-Nachricht erkennbar (der Bot postet unter seinem
eigenen Account). Mehr dazu, wie sich der Bot generell als kein Scam zu erkennen gibt,
steht im Kapitel zum Selbst-Erklärer.

## Wann es passiert

**Server-Warnung an den Chat:**

- Sie läuft im selben Takt wie die normalen Promo-Nachrichten und nutzt einen
  Werbe-Slot — taucht also nur auf, wenn der Kanal live und genug Chat-Aktivität da ist.
- Sie erscheint **statt** einer regulären Promo, nicht zusätzlich, damit nie zwei
  Bot-Nachrichten direkt hintereinander kommen.
- Sie kommt deutlich seltener als gewöhnliche Promos: pro Kanal gibt es einen eigenen
  Abstand, sodass die Warnung regelmäßig, aber nicht in jedem Werbe-Slot auftaucht.
- Nach einem Bot-Neustart geht der Abstand nicht verloren — der Bot merkt sich, wann er
  zuletzt gewarnt hat, sodass die Warnung nicht bei jedem Neustart sofort wieder kommt.

**Warnung vor verdächtigen Chattern:**

- Sie wird ausgelöst, wenn die Nachricht eines fremden Chatters wie ein Scam-/Verkaufs-Pitch
  wirkt. Die genaue Erkennungslogik ist bewusst nicht dokumentiert; der Bot wägt mehrere
  Hinweise zu einer Gesamteinschätzung ab und ist dabei eher vorsichtig, damit echte
  Zuschauer praktisch nie fälschlich angesprochen werden.
- Nachrichten von Moderatoren und vom Streamer selbst lösen nie eine solche Warnung aus.
- Befehle (Nachrichten, die mit dem Befehls-Präfix beginnen) werden ebenfalls ignoriert.
- Welche Faktoren genau in die Einschätzung einfließen, bleibt aus Schutzgründen offen —
  der Bot wägt dafür mehrere Hinweise zu einer Gesamteinschätzung ab und ist dabei eher
  vorsichtig.
- Nach einer ausgesprochenen Warnung gibt es eine Ruhephase pro Person und pro Kanal,
  damit der Chat nicht mit wiederholten Warnungen zugespammt wird.

## Was Streamer/Viewer sehen

- **Server-Warnung:** Eine kurze, orange hervorgehobene Bot-Nachricht im Chat, die vor den
  gefälschten Servern warnt und den echten, offiziellen Discord nennt. Der Verweis auf den
  echten Server läuft über den Invite-/Bio-Mechanismus, nicht als nackter Link im Chat
  (Twitch-AutoMod blockt Links — siehe Hinweis unten).
- **Chatter-Warnung:** Eine kurze Bot-Nachricht, die die verdächtige Person mit @-Mention
  anspricht, sie als möglichen Pitcher/Scam einordnet und empfiehlt, sie zu ignorieren bzw.
  zu bannen. Bei Wiederholungstätern sieht man zusätzlich einen kurzen Timeout der Person
  plus eine abschließende Warnung.
- Beide Warnungen erscheinen unter dem Bot-Account, sind also klar als Bot-Nachricht
  erkennbar und nicht als Aussage des Streamers.

## Grenzen & Sonderfälle

- **Keine Links im Chat:** Twitch-AutoMod blockt URLs. Deshalb verweisen die Warnungen auf
  den echten Discord über den Invite-/Bio-Mechanismus statt als nackte URL — sonst würde
  die Nachricht verschwinden.
- **Warnung "stiehlt" den Promo-Slot:** Ist die Server-Warnung fällig, kommt im
  Werbe-Slot die Warnung statt der Promo, nicht beides. Das ist Absicht, damit keine
  doppelten Bot-Nachrichten entstehen.
- **Bewusst vorsichtige Wortwahl:** Der Bot behauptet nie als Tatsache, dass etwas Scam
  ist, sondern formuliert es als Möglichkeit. Das ist Absicht — sowohl um niemanden zu
  Unrecht zu beschuldigen als auch aus rechtlichen Gründen.
- **Konservative Erkennung:** Die Chatter-Erkennung ist absichtlich vorsichtig eingestellt.
  Im Zweifel wird eher nicht gewarnt, damit echte, harmlose Zuschauer (auch neue oder
  fremdsprachige) nicht fälschlich getroffen werden. Das kann dazu führen, dass ein
  geschickt formulierter Pitch im Einzelfall durchrutscht.
- **Eskalation nur bei Hartnäckigkeit:** Der kurze Timeout greift erst, wenn jemand eine
  bereits ausgesprochene Warnung ignoriert und weitermacht — nicht beim ersten Verdacht.
- **Abgrenzung zur normalen Moderation:** Diese Scam-Warnungen sind nicht dasselbe wie der
  automatische Anti-Viewer-Bot-Schutz. Werbe-Bots, die "mehr Viewer/Follower" verkaufen,
  räumt der Bot über die normale Moderation eigenständig weg. Die hier beschriebenen
  Warnungen sind ein zusätzlicher, sichtbarer Hinweis für Fälle, die eher den Streamer
  betreffen.

## Häufige Fragen

**F: Der Bot hat im Chat geschrieben, ein bestimmter Deadlock-Discord sei "Fake/Scam".
Warum macht er das?**
A: Es kursieren gefälschte Server, die sich als die offizielle deutsche Deadlock-Community
ausgeben. Der Bot warnt regelmäßig davor und nennt im selben Atemzug den einzigen echten,
offiziellen Discord, damit Zuschauer nicht auf die Fälschungen reinfallen. Er sagt bewusst
"könnte Scam sein" und nicht "ist Scam".

**F: Wie oft kommt diese Server-Warnung?**
A: Deutlich seltener als normale Promos. Sie läuft im Promo-Takt mit, nimmt aber nur ab
und zu einen Werbe-Slot ein. Pro Kanal gibt es einen eigenen Mindestabstand, sodass die
Warnung regelmäßig, aber nicht ständig erscheint. Sie ersetzt in diesem Slot die Promo,
kommt also nicht zusätzlich obendrauf.

**F: Der Bot hat einen meiner Chatter als möglichen Scammer markiert. Liegt er da
sicher richtig?**
A: Der Bot warnt nur bei einem deutlichen Verdacht und ist dabei eher vorsichtig
eingestellt, damit echte Zuschauer praktisch nie fälschlich getroffen werden. Es bleibt
aber eine Einschätzung, kein Urteil — deshalb formuliert er es als Möglichkeit und
empfiehlt nur, die Person zu ignorieren oder zu bannen. Die Entscheidung liegt am Ende
bei dir.

**F: Werden harmlose neue oder fremdsprachige Zuschauer aus Versehen als Scammer gewarnt?**
A: Das ist sehr unwahrscheinlich. Die Erkennung verdichtet mehrere Hinweise zu einer
Gesamteinschätzung und ist bewusst konservativ ausgelegt. Im Zweifel wird lieber nicht
gewarnt. Ein freundliches "hi, wie geht's" allein löst keine Warnung aus.

**F: Was passiert, wenn ein Scammer trotz Warnung weitermacht?**
A: Ignoriert die Person eine bereits ausgesprochene Warnung und pitcht weiter, kann der
Bot sie kurz in den Timeout setzen und eine letzte Warnung mit Bann-Empfehlung posten.
Beim ersten Verdacht passiert das noch nicht.

**F: Warum nennt der Bot in der Warnung keinen anklickbaren Link zum echten Discord?**
A: Twitch-AutoMod blockt URLs im Chat, dann würde die ganze Nachricht verschwinden.
Deshalb verweist der Bot auf den echten Server über den Invite-/Bio-Mechanismus statt als
nackten Link.

**F: Kann ich die Scam-Warnungen abschalten?**
A: Die Warnungen sind Teil des Schutzkonzepts und standardmäßig aktiv. Wenn du für deinen
Kanal generell keine Bot-Nachrichten/Promos möchtest, lässt sich die Bot-Werbung
werbefrei stellen — das beeinflusst, ob im Chat überhaupt solche Promo-/Warn-Slots
auftauchen. Für individuelle Sonderwünsche wende dich am besten direkt an das Team.

**F: Sind das echte Aussagen von mir als Streamer oder vom Bot?**
A: Vom Bot. Alle diese Warnungen postet der Bot unter seinem eigenen Account und sind
damit klar als Bot-Nachricht erkennbar, nicht als deine persönliche Aussage.
