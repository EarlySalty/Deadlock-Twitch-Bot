# Worker briefing: dashboard English, one area per worker

BRIEFING[WB-1]: Pflichtteile 5/5 | Timer 30 min | Worktree: /home/nathanael/.worktrees/ttb-de-<area>

The area specific part (area name, worktree, branch, file list) is in the prompt that points you here.

## 1. Goal and contract

User goal: the streamer dashboard `bot/dashboard_v2` is fully bilingual German/English (plan package D, decisions E13/E15). Your package is one area. Every user visible text in your files goes through the existing i18n so that English users see natural English and German users see exactly the German text they see today.

Mechanism (already on your base commit):

- The German text is the dictionary key. A missing English entry falls back to German.
- React components: `const t = useT();` or `const { t, locale } = useLanguage();` from `src/context/LanguageContext.tsx`.
- Non React code (helpers, view models, constants, api modules): `tr(text, params)` and `activeLocale()` from `src/i18n/dictionary.ts`. Use relative imports (`../i18n/dictionary`), the Node tests cannot resolve the `@` alias.
- Placeholders use `{name}`: `t('Noch {count} Tage', { count })`. The whole sentence with placeholders is the key. Never build keys by concatenation or template strings, never translate sentence fragments that are glued together; turn them into one sentence with placeholders.
- The provider remounts its children when the language changes, so calling `tr()`/`activeLocale()` during render or inside functions is correct. Calling them at module load time is wrong because it runs once in German. Module level constants with visible text therefore become functions (or `useMemo` inside the component) that call `tr()`/`t()` at call time. Prefer this. Only for a handful of strings where that is disproportionate, keep the German literal in the constant, translate at the render site with `t(value)` and list the literal in `IGNORED_LITERALS[<your file>]` in `tools/i18nCoverageData.ts`.
- Dates and numbers: replace `'de-DE'` with `locale` (from `useLanguage()`) or `activeLocale()`. `toLocaleString()`, `toLocaleDateString()`, `toLocaleTimeString()` and `Intl.*Format` without a locale get the locale passed explicitly. Do not touch time zones, they stay with the browser.
- Plurals: keep the existing German singular/plural branches, each branch is its own key. No new plural mechanism.
- API error messages are already translated centrally in `src/api/core.ts`. Strings that the Rust API sends and your files render (labels, verdicts, status texts): if they are a small closed set of fixed German literals, wrap the render with `t(value)` and add entries to your area file. Free text built by the backend (numbers, names, sentences assembled in Rust) stays untouched. In both cases list them in your report (file:line, field, Rust source if you found it).

English style:

- Natural, short English as on English Twitch and in the existing entries of `src/i18n/dictionary.ts`. Sentence case for buttons and headings. Address the streamer as "you". No AI filler.
- Glossary: Zuschauer → viewers, Ø Zuschauer → avg. viewers, Follower → followers, Abonnenten/Abos → subscribers/subs (follow the German wording), Kanal → channel, Raid → raid, VOD → VOD, Clip → clip, Chatter → chatters, Spitze/Peak → peak, Zuschauerbindung → viewer retention, Sendezeit/Streamzeit → stream time, Einnahmen → revenue, Werbung/Werbepause → ads/ad break, KI → AI, Einstellungen → Settings, Übersicht → Overview, Publikum → Audience, Wachstum → Growth, Planung → Planning, Zeitraum → period, Erneut versuchen → Try again, Speichern → Save, Abbrechen → Cancel, Schließen → Close.
- Before adding an entry, check whether the key already exists in the merged dictionary (`dictionaryFor('en')`, i.e. `src/i18n/dictionary.ts`, `src/i18n/*.ts`, `src/i18n/en/*.ts`). Reuse it, never add the same key with a different English value.

## 2. Ownership

- Write only your listed files, your own area dictionary `src/i18n/en/<area>.ts`, and tests that break because of your refactoring (adapt them without weakening what they guard).
- In `tools/i18nCoverageData.ts` you only remove your own files from `PENDING_FILES` and add `IGNORED_LITERALS` entries keyed by your own files. Do not edit `NEUTRAL_TEXTS`, `NON_VISIBLE_ATTRIBUTES` or `VISIBLE_PROPERTY_NAMES`; if you think one of them needs a change, say so in the report.
- Never edit: `src/i18n/dictionary.ts`, other area files, `src/i18n/socialStudio.ts`, `src/i18n/vodArchive.ts`, `tools/i18nCoverage.ts`, `src/context/LanguageContext.tsx`, `src/api/core.ts`, `package.json`, anything under `components/socialmedia/`, `pages/SocialMedia*.tsx` or with `social` in the path, `bot/admin_dashboard`, `website/`, Rust code.
- Do not change German text, not even typos or punctuation. `--preserve` enforces it.
- No code comments, no new libraries, no new AI calls, no refactors beyond what the translation needs, no formatting of untouched code.
- Other workers work in parallel in their own worktrees. Never touch another worktree.

## 3. Working state

- Your worktree and branch are in the prompt. Base and HEAD: `8379a0437`, clean. `node_modules` is installed in `<worktree>/bot/dashboard_v2`; run npm and npx there.
- Commit to your branch and push it (`git push -u origin <branch>`). Stage explicit paths only, never `git add -A` or `git add .`. Commit messages in English, ending with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. Commit verified intermediate states.
- No merge, no rebase onto main, no deploy. The integrator is the Teil-Orchestrator "Dashboard-Englisch" who spawned you.
- Hooks: before a recursive `grep`/`rg` run `graphify query "<topic>" --graph /home/nathanael/repos/Deadlock-Twitch-Bot/graphify-out/graph.json` once from the worktree root (valid 15 minutes, do not run it in parallel with the grep). Before running npm test, lint or build read `/home/nathanael/.claude/skills/rolle-test-waechter/SKILL.md`; a guardrail hook requires it.
- No secrets output. Branches that existed before your task and are not yours stay untouched.

## 4. Proof

Run in `<worktree>/bot/dashboard_v2`, write long output to `/tmp/ttb-de-<area>-*.log` and read the file; do not pipe runs whose exit code matters through `tail` or `grep`.

1. `npx tsx tools/i18nCoverage.ts <your files>` → `0 violations`.
2. `npx tsx tools/i18nCoverage.ts --preserve origin/main` → exit 0 (no German text lost or changed).
3. `npx tsc -b` → exit 0. `npx eslint <your changed files>` → exit 0.
4. `node --import tsx --test tests/i18n.test.ts tests/i18nCoverage.test.ts tests/languageProvider.test.tsx <existing tests that cover your files>` → green.
5. Once at the end: `npm test > /tmp/ttb-de-<area>-test.log 2>&1`. Only these known failures from main are allowed: `kein Hex-Wert ausserhalb der Industrial-Gold-Palette`, `keine Tailwind-Standardfarben (die ueberleben jeden Farb-Remap)`, `kein weisser Text auf heller Markenflaeche`, `tests/clipContest.test.ts`, `der Analytics-Rahmen in App.tsx setzt keine eigene Gesamtbreite`, `OBS-Hilfe trennt Uploadbudget und geprüfte Plattformausgabe`.
6. Read your area file top to bottom once and fix unnatural English.

Stop condition: all your files are off `PENDING_FILES` with zero violations and the checks above pass. If the area turns out much larger than expected, commit and push a verified intermediate state after about 90 minutes and report what is left.

## 5. Routing and report

Report to the Teil-Orchestrator that spawned you (main orchestrator session `5f7811c4-86ac-4398-83e4-f929f2139a47`). Never ask the user. Decide what the brief, rules and repo answer yourself and name the decision in the report. If you are truly blocked, end with `FRAGE AN ORCHESTRATOR:` plus situation, options, recommendation, branch, SHA and worktree.

Final report, short: branch, final pushed SHA, files done and files left, number of new entries, backend strings found (file:line, field, Rust source), `IGNORED_LITERALS` added with reason, check results with log paths, anything you think the integrator must know.
