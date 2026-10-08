# Modellnachweise

Native Session: `f61905e7-f7ff-405b-a6d7-090dec371fcb`.

Transcript-Basis: `/home/nathanael/.claude/projects/-home-nathanael-repos-Deadlock-Twitch-Bot/f61905e7-f7ff-405b-a6d7-090dec371fcb/subagents/workflows/`.

Nachweismethode: Jede JSONL-Zeile wird geparst und jedes vorhandene Feld `message.model` gezählt. Andere Modelle oder ein vollständig fehlender Modellbeleg sperren die Verwendung des Ergebnisses. Ein laufender Agent erhält erst nach Abschluss einen endgültigen Nachweis.

| Rolle | Run-ID | Transcript | Modell | Nachrichten | Abschluss | Transcript-SHA256 |
|---|---|---|---|---:|---|---|
| Inventar | wf_63f9910c-f6d | agent-a64e7b385898441c0.jsonl | gpt-6.1-sol | 69 | vollständig geprüft | ac11fec4d36eb759daafb4d5983fcba8a6794e570ea0386720d02bc707e12f1f |
| Aufgabenstand 1 | wf_f46218a0-dd2 | agent-a8d4210900ec8da8d.jsonl | gpt-6.1-sol | 15 | vollständig geprüft | 51e9a30cc1876b3311360c69c612a3c539de2ff37b05437815ca671e92c781af |
| Gate-/Deploy-Vorprüfung | wf_8b989b1c-1e8 | agent-a1f8aeed6c7779c6b.jsonl | gpt-6.1-sol | 123 | vollständig geprüft | 88c06b77f1f46f3b71007e28218d5fbbdf4d80026e7080b011b1c322f7589793 |
| Aufgabenstand 2 | wf_c04d3dd2-fbb | agent-a6fc565c73172fc87.jsonl | gpt-6.1-sol | 17 | vollständig geprüft | 0709d1dd71d9787175d92668aacdbeae387ce1b0d59000c934a0987cce1fd1a3 |
| R09 Nebenläufigkeit | wf_b0f0e2fe-347 | agent-a3cb8641e6e6a68df.jsonl | gpt-6.1-sol | 33 | vollständig geprüft | 436a1a19984392e83e5a86eccd6e6771f6dabdfed5ced7637e026f20de8e619a |
| R09 Fehlerbehandlung | wf_b0f0e2fe-347 | agent-a669cb6a850016ec9.jsonl | gpt-6.1-sol | 50 | vollständig geprüft | 4eba416e28f93496ffa504b5ef321a37485418eb369f957f52ab5b35da08f126 |
| R09 Bauqualität | wf_b0f0e2fe-347 | agent-a88b3416857fc4d40.jsonl | gpt-6.1-sol | 57 | vollständig geprüft | 6a7f112fb981458d689446b19aa7ae4be33af08ebce572e21c203747e9f448e2 |

## Laufende Prüfungen

- Die Vorprüfung `wf_8b989b1c-1e8` ist abgeschlossen und oben nachgewiesen.
- `wf_b0f0e2fe-347` wurde nach sechs Kontextabbrüchen gestoppt. Die drei abgeschlossenen Ergebnisse sind oben vollständig nachgewiesen. Synthetische API-Fehlermeldungen mit `message.model = <synthetic>` sind keine Modellaufrufe; solche Abbrüche zählen nicht als Review.
- Wiederaufnahme `wf_c2fafb5b-bac`: acht abgeschlossene Agenten ausschließlich Sol, vollständige Nachweise in MODELLE-W01R2.md.
- Dokumentationscheckpoint `wf_8b31ccc9-a29`: Agent a1f9000dcb691181f, 39 Sol-Nachrichten, SHA256 24eca5704329badaaf50e648ab3257d53df05f8e17a0e13f32d6c118a166dff5.
- Dokumentationsnachfolger `wf_bd8e1d58-d96`: Agent a067ebb2cc3d05d35, 43 Sol-Nachrichten, SHA256 1fb247bb8717afb0f383a888300fb9ca1e78aa08c20c78b37bdaf9be51756e11.
- Frische R09-Qualitätskritik `wf_37ee8d9d-602`: Agent `a1ea9dd26f72a7c58`, abgeschlossen, 35 Sol-Nachrichten, SHA256 `913af586bec5c2d3174e01129ac4a8b2e8bef5819fab2d0a62c48c63a803e966`.
- Leseabschnittsplanung `wf_9252ea77-9ce`: Agent `ae14693c3ef7a5b59`, abgeschlossen, 35 Sol-Nachrichten, SHA256 `e1dc9604e096c0ad5b6df7492936df93ab8ff5954aeba04df3f1cec39de7ede2`.
- Erster Fixversuch B01 `wf_6d010be4-da1`: kein Abschlussresultat nach Sitzungsende, 47 bisherige Sol-Nachrichten. Nicht als fertig gewertet; frischer Fixer übernimmt im Workflow `wf_b4f81318-dac` denselben sauberen Worktree.
- W02 `wf_cdc4c5ac-9bb` und B01-Wiederaufnahme `wf_b4f81318-dac` laufen. Endgültige Modellnachweise werden erst nach Abschluss ausgewertet.

Rohtranscripts werden nicht ins Repository kopiert, damit keine fremden Kontext- oder Werkzeugdaten veröffentlicht werden.
