ALTER TABLE affiliate_accounts
    ADD COLUMN IF NOT EXISTS commission_rate_pct SMALLINT NOT NULL DEFAULT 30;

ALTER TABLE affiliate_accounts
    ADD CONSTRAINT affiliate_accounts_commission_rate_pct_check
    CHECK (commission_rate_pct >= 0 AND commission_rate_pct <= 100);
