# Modellnachweise

Native Session: `f61905e7-f7ff-405b-a6d7-090dec371fcb`.

Transcript-Basis: `/home/nathanael/.claude/projects/-home-nathanael-repos-Deadlock-Twitch-Bot/f61905e7-f7ff-405b-a6d7-090dec371fcb/subagents/workflows/`.

Nachweismethode: Jede JSONL-Zeile wird geparst und jedes vorhandene Feld `message.model` gezählt. Andere Modelle oder ein vollständig fehlender Modellbeleg sperren die Verwendung des Ergebnisses. Ein laufender Agent erhält erst nach Abschluss einen endgültigen Nachweis.

| Rolle | Run-ID | Transcript | Modell | Nachrichten | Abschluss | Transcript-SHA256 |
|---|---|---|---|---:|---|---|
| Inventar | wf_63f9910c-f6d | agent-a64e7b385898441c0.jsonl | gpt-6.1-sol | 69 | vollständig geprüft | ac11fec4d36eb759daafb4d5983fcba8a6794e570ea0386720d02bc707e12f1f |
| Aufgabenstand 1 | wf_f46218a0-dd2 | agent-a8d4210900ec8da8d.jsonl | gpt-6.1-sol | 15 | vollständig geprüft | 51e9a30cc1876b3311360c69c612a3c539de2ff37b05437815ca671e92c781af |
| Gate-/Deploy-Vorprüfung | wf_8b989b1c-1e8 | agent-a1f8aeed6c7779c6b.jsonl | gpt-6.1-sol | 123 | vollständig geprüft | 88c06b77f1f46f3b71007e28218d5fbbdf4d80026e7080b011b1c322f7589793 |

## Laufende Prüfungen

- Die Vorprüfung `wf_8b989b1c-1e8` ist abgeschlossen und oben nachgewiesen.
- `wf_b0f0e2fe-347`: erste 14 gestartete Reviewer im Transcript mit `gpt-6.1-sol` belegt. Keine Modellabweichung in dieser Zwischenprüfung. Weitere Reviewer, Skeptiker und Qualitätskritiker sowie der endgültige Nachweis stehen aus.

Rohtranscripts werden nicht ins Repository kopiert, damit keine fremden Kontext- oder Werkzeugdaten veröffentlicht werden.
