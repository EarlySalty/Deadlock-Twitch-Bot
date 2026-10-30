UPDATE twitch_partners
   SET inactivity_flagged_at = NULL
 WHERE inactivity_flagged_at IS NOT NULL
   AND status = 'active'
   AND NULLIF(partnered_at, '')::timestamptz > NOW() - INTERVAL '60 days';
