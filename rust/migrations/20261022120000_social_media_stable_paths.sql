UPDATE social_produced_videos
SET file_path = regexp_replace(file_path, '^/opt/deadlock/twitch/releases/[0-9a-f]{40}/', '/opt/deadlock/twitch/current/')
WHERE file_path ~ '^/opt/deadlock/twitch/releases/[0-9a-f]{40}/';

UPDATE social_produced_videos
SET metadata = regexp_replace(metadata::text, '/opt/deadlock/twitch/releases/[0-9a-f]{40}/', '/opt/deadlock/twitch/current/', 'g')::jsonb
WHERE metadata::text ~ '/opt/deadlock/twitch/releases/[0-9a-f]{40}/';

UPDATE social_production_jobs
SET metadata = regexp_replace(metadata::text, '/opt/deadlock/twitch/releases/[0-9a-f]{40}/', '/opt/deadlock/twitch/current/', 'g')::jsonb
WHERE metadata::text ~ '/opt/deadlock/twitch/releases/[0-9a-f]{40}/';

UPDATE social_ai_jobs
SET checkpoints = regexp_replace(checkpoints::text, '/opt/deadlock/twitch/releases/[0-9a-f]{40}/', '/opt/deadlock/twitch/current/', 'g')::jsonb
WHERE checkpoints::text ~ '/opt/deadlock/twitch/releases/[0-9a-f]{40}/';
