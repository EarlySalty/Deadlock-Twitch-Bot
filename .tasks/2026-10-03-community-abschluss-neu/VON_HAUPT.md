# Von der Hauptsession an D2

## 04.10.2026, 00:35 Uhr: Rückstellweg für den Twitch-Deploy

Es gibt keinen zweiten Deployweg. `deploy-twitch-release` bleibt der Weg, der Wrapper wird nicht geändert.

`current` zeigt auf `releases/6e3fbcd5-dashboard-299495d1`. Der Wrapper stellt nur auf `releases/` plus Hex zurück und bricht deshalb ab. Derselbe Commit hat bereits den reinen Ordner `releases/6e3fbcd500394b3f9ec1d6f577f89707b36d5ec0`. Der ist nicht der Live-Stand: dort fehlen `bot/analytics/dashboard_v2/dist/index.html` in der Livefassung und die vier Assets `charts-BGgtzR2C.js`, `index-BS29FgN7.js`, `query-FwnJxEWc.js`, `react-BKtprR8h.js`.

Vor dem Deploy genau diese fünf Dateien aus dem Suffix-Ordner in den reinen SHA-Ordner übernehmen, den Suffix-Ordner behalten, und `current` auf `releases/6e3fbcd500394b3f9ec1d6f577f89707b36d5ec0` stellen. Danach denselben Wrapper mit `2169533539557964e1d21fc71f2bc3cfabe97c1a` ausführen. Kein Umbenennen des Suffix-Ordners, kein Löschen, keine Wrapperänderung.

## 03.10.2026, 23:45 Uhr: Nur die elf eigenen Migrationen

`dl-central-migrate` hat keinen Versionsfilter und bekommt keinen. Er wendet die ganze eingebettete Folge an.

`20260930220000` hasht Turnier- und Steam-Tokens und braucht den Cutover der zuständigen Leser und Schreiber. `20260930220300` ist Gemini-Browserzustand. Beides gehört nicht zur Communitybrücke und wird in diesem Abschluss nicht angewandt. Die Dateien bleiben auf main, kein Löschcommit.

Die elf eigenen Versionen mit dem vorhandenen Migrator anwenden. Dafür genau diese zwei Dateien nur im Build-Worktree aus der Migrationsmenge nehmen, nicht committen und nicht pushen. Danach den Baum wieder auf den gepushten SHA setzen. Würde derselbe Lauf noch weitere Versionen anwenden, die nicht zu den elf gehören, vorher stoppen und die Namen melden. Kein Apply aller 13.

## 03.10.2026, 23:28 Uhr: Fremdmigration und Liveabschluss

Version `2026100301` ist schon angewandt, die Prüfsumme passt zur Patchnotes-Quelle. Nicht neu ausführen und das SQL nicht ändern.

Genau diese eine Datei unverändert ins bestehende Bots-Migratorinventar übernehmen, damit der Migrator die angewandte Version wiederfindet:

`/home/nathanael/.worktrees/patchnotes-live-main/rust/patchnotes-bot/migrations/2026100301_brain_application_catalog.sql`

Keine weiteren Brain- oder Patchnotes-Migrationen mitnehmen. Danach den Liveabschluss ausführen: Merge, nur die eigenen ausstehenden Migrationen, Deploy, Neustart, Liveprüfung. Die beiden ALLOW-Gates bleiben gültig.
