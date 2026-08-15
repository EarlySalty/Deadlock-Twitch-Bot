-- Dreht die Zuordnung anhand der Sicherungstabelle um.

UPDATE streamer_plans AS s
   SET manual_plan_id = b.manual_plan_id,
       plan_name = b.plan_name
  FROM streamer_plans_pricing_backup_20260816 AS b
 WHERE s.twitch_user_id = b.twitch_user_id;

DROP TABLE IF EXISTS streamer_plans_pricing_backup_20260816;
