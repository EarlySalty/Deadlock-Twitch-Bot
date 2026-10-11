# Token storage boundary

For work on authentication and access, `docs/token-storage-db.md` applies. Existing repository rules remain in force. Do not create new token files; store account credentials only encrypted in the DB and pure session or one-time values as a lookup hash. Infrastructure keys stay in the existing secret manager.
