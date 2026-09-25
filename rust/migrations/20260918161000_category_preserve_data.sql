-- Daten und bisherige Konfiguration bleiben vollständig erhalten.
-- Der Erhaltungsmodus hat Vorrang vor den historischen Retention-Feldern.
-- Es gibt weder eine Datenbereinigung noch eine verkürzte Ersatzfrist.
ALTER TABLE category_collector_config
    ADD COLUMN preserve_raw_data BOOLEAN NOT NULL DEFAULT TRUE
        CONSTRAINT category_collector_preserve_raw_data_required CHECK (preserve_raw_data);

COMMENT ON COLUMN category_collector_config.preserve_raw_data IS
    'Rohchat und Snapshots dauerhaft erhalten. Alte retention_days-Felder sind wirkungslos; 7/30/90 Tage sind nur Ansichtsfenster.';
