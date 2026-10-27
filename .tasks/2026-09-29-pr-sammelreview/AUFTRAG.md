# PR-Sammelreview Twitch-Bot (2026-09-29)

Nutzerwunsch: Jeder offene PR im Repo EarlySalty/Deadlock-Twitch-Bot bekommt genau einen Codex-Agenten. Der Agent reviewt seinen PR. Ist er gut, wird er über den lokalen Merge-Gate nach main gemergt. Hat er Probleme, schreibt der Agent die nötigen Nachbesserungen als Kommentar unter den PR und fixt NICHT selbst. Neustart der Dienste macht niemand von euch; gebaut und deployt wird einmal gesammelt am Ende (eigener Thread).

Du bist der einzige Thread für deinen PR. Keine Unter-Threads oder Unter-Agenten spawnen. Keine Codeänderungen am PR, keine Fix-Commits, kein Force-Push.

## Ablauf je PR

1. Stand holen: `gh pr view <nr> --repo EarlySalty/Deadlock-Twitch-Bot --json title,body,headRefName,baseRefName,isDraft,mergeable,files,commits` und `gh pr diff <nr>`.
2. Eigener Worktree, nie im geteilten Checkout `~/repos/Deadlock-Twitch-Bot` arbeiten:
   `git -C /home/nathanael/repos/Deadlock-Twitch-Bot fetch origin`
   `git -C /home/nathanael/repos/Deadlock-Twitch-Bot worktree add /home/nathanael/.worktrees/tb-pr-<nr> origin/<headRefName>` (detached).
3. Review gegen origin/main (Bugs, Security, Wirkung am Produktionspfad, Migrationen, Umlaute in Nutzertexten, keine Em-Dashes in Nutzertexten, keine Modellnamen einkompiliert, LLM-Aufrufe nur über tb-llm). Passende Prüfungen laufen lassen:
   - Rust: `cargo fmt --check`, `cargo clippy`, `cargo test` für die betroffenen Crates (Toolchain rustc 1.97 aus `~/.rustup/toolchains`, `/usr/bin/cargo` 1.75 bricht). Nie `| tail` ohne `set -o pipefail;`, lieber in eine Datei umleiten. Höchstens ein Release-Build gleichzeitig auf dem Host, also KEIN `--release`.
   - Frontend (Dependabot npm): im betroffenen Ordner `npm ci` und `npm run build`, bei vorhandenem Lint/Typecheck auch diese.
   - GitHub Actions (Dependabot actions): Changelog der Action auf Brüche prüfen (Inputs, Outputs, Node-Version), Nutzung im Workflow abgleichen.
   - Vorbestehende rote Tests auf origin/main sind Baseline, nicht dein Befund; im Zweifel gegen origin/main gegenprüfen.
4. Urteil:
   - GUT: Branch auf aktuelles origin/main bringen (wenn nötig `git rebase origin/main` im Worktree; nur konfliktfreies Rebase oder triviale Lockfile-Neugenerierung, sonst ist das ein Befund). Dann Merge nach main über den lokalen Gate: aus dem Worktree `git push origin HEAD:main` (Fast-Forward). Der Push löst den Merge-Gate-Hook aus; ALLOW ist Pflicht. Nie `gh pr merge`, nie Gate umgehen. Bewegt sich origin/main zwischendurch (andere PR-Threads mergen parallel), neu fetchen, erneut rebasen, erneut pushen. Danach prüfen, dass GitHub den PR als merged zeigt; sonst `gh pr close <nr> --comment "Gemergt über lokalen Gate als <sha>"`.
   - Gate-DENY: Der Befund wird nicht von dir gefixt. Die BLOCKING-Punkte als PR-Kommentar posten (siehe unten), Urteil NACHBESSERN.
   - PROBLEME: `gh pr comment <nr> --body-file <datei>` mit einer nummerierten Liste: je Punkt Datei:Zeile, was falsch ist, was genau gemacht werden muss. Deutsch, echte Umlaute, keine Em-Dashes. Kein Merge.
   - Draft-PRs (#998, #999) werden genauso reviewt. Bei GUT vorher `gh pr ready <nr>`.
5. Aufräumen: `git -C /home/nathanael/repos/Deadlock-Twitch-Bot worktree remove /home/nathanael/.worktrees/tb-pr-<nr>`. Branch des PR nach Merge löschen (`git push origin --delete <headRefName>`), bei Nachbesserung stehen lassen.
6. Kein Deploy, kein `deploy-twitch-release`, kein `systemctl restart`, keine Prod-Migration.

## Schlussmeldung (letzte Nachricht im Thread)

```
PR #<nr>: GEMERGT <sha> | NACHBESSERN (Kommentar-Link) | BLOCKIERT (Grund)
Geprüft: <Befehle und Ergebnis>
Migrationen: keine | <Dateien>
Hinweise für den Sammel-Deploy: <z. B. neue Migration, Caddy-Pfad, Config-Feld>
```

## Nachtrag 1: Scheduler-Gate (externe Review-Queue)

Der Merge-Gate lässt Pushes nach main nur mit SHA-gebundenem externem Review im Zustand `release` durch. Ein erster Push legt nur einen Queue-Eintrag an (`BLOCK: Scheduler-Review`). Du bist für deinen PR der unabhängige Reviewer (nicht Autor), deshalb schließt du den Queue-Weg selbst ab, ausschließlich über die Queue-Befehle, nie über Umgehung. `Q` steht für `python3 /home/nathanael/Documents/.claude/gpt-workers/gate_hook.py --external-queue`, Repo-Pfad immer `/home/nathanael/repos/Deadlock-Twitch-Bot`, Holder `tb-pr-<nr>`.

1. `Q list --repo <repo>`: den Eintrag mit genau deinem `head_sha` suchen (Pfad der State-Datei merken).
2. `Q lease-next --holder tb-pr-<nr> --repo <repo> --stage reviewer`. Liefert das einen fremden Eintrag (anderer head_sha), sofort `Q release --state-file <pfad> --holder tb-pr-<nr>` und erneut versuchen. Nie fremde Einträge beurteilen.
3. Eigenen Eintrag abschließen: `Q complete-review --state-file <pfad> --holder tb-pr-<nr> --verdict clean --base-sha <base> --head-sha <head> --detail "PR #<nr>: <Prüfungen>"`. Bei Befunden `--verdict block` mit `--finding` je Punkt, dann PR-Kommentar statt Merge.
4. Release: `Q lease-next --holder tb-pr-<nr> --repo <repo> --stage release` (wieder nur den eigenen head_sha), dann denselben Push `git push origin HEAD:main` erneut, als einzelner Git-Schritt. Nach erfolgreichem Push `Q complete-release --state-file <pfad> --holder tb-pr-<nr> --base-sha <base> --head-sha <head>`. Deploy entfällt hier ausdrücklich, er läuft gesammelt.
5. Hat sich origin/main inzwischen bewegt, ist der Eintrag an die alte Basis gebunden: fetch, rebase, neuer Push, neuer Eintrag, Schritte 2 bis 4 wiederholen.
6. Verlangt der Gate eine Publisher- oder Session-Bindung, die du nicht erfüllen kannst, nicht basteln: Lease freigeben und als BLOCKIERT mit dem wörtlichen Gate-Text melden.

## Nachtrag 2: eigenen Eintrag direkt leasen, Release mit der eigenen Hook-Session

`lease-next` filtert nicht nach SHA und liefert fremde Einträge der parallelen PR-Threads. Deshalb den eigenen Eintrag gezielt über seine State-Datei leasen (dieselbe Funktion, die `lease-next` intern aufruft):

```
python3 -c "import sys; sys.path.insert(0,'/home/nathanael/Documents/.claude/gpt-workers'); import gate_hook as g; from datetime import timedelta; print(g.lease_external_review('<state_file>', '<holder>', ttl=timedelta(minutes=30)))"
```

- Review-Stufe: Holder `tb-pr-<nr>`, danach `complete-review` wie in Nachtrag 1.
- Release-Stufe: Der Eintrag trägt `publisher_session_id`, das ist die Hook-Session, aus der dein eigener Push den Eintrag angelegt hat. Nur wenn der Eintrag nachweislich von deinem Push stammt (gleicher head_sha, von dir gepusht, created_at passt zu deinem Push), least du mit genau dieser ID als Holder und pushst sofort erneut aus derselben Session `git push origin HEAD:main`. Danach `complete-release` mit derselben ID als Holder.
- Fremde Einträge nie leasen, nie beurteilen, nie freigeben.

## Nachtrag 3: Lockfile-Konflikte bei Dependabot-PRs

Kollidiert ein Dependabot-PR nach einem anderen Merge nur in `package.json`/`package-lock.json` oder Workflow-Pins, ist das kein Befund: PR-Kommentar mit genau `@dependabot rebase` posten, auf den neuen Head warten (`gh pr view <nr> --json headRefOid`, alle 60 s, höchstens 20 Minuten), neuen Head kurz erneut prüfen, dann Nachtrag 1 und 2 für den neuen SHA. Kein NACHBESSERN-Kommentar dafür.
