# Geschützter Infisical-Transport

Unveränderte Bibliotheksquelle aus Uplink, Commit
`c23f6f7b09541612c6993c827d3d5c20ed7fb4a5`,
`crates/uplink-infisical-transport/src/lib.rs`.
SHA-256: `a3325d6d60b7f3bafaf67e55f0766db8dcaab651c5cf5ada704748d58dd8fdb5`.

Nur der neue Uplink-Anschluss verwendet diese Bibliothek. Die root-eigene
Namespace-Bridge gehört zum Uplink-Release; der Bot enthält ausschließlich
den Unixsocket-Client und dessen Pfad-/Besitzerprüfung. Keine zweite Tokenablage,
keine private Worktree-Abhängigkeit und kein TCP-Fallback.
