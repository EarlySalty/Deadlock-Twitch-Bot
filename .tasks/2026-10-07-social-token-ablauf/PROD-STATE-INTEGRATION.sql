\set ON_ERROR_STOP on
BEGIN READ ONLY;
SELECT current_database(), current_user;
SELECT version, success, encode(checksum, 'hex') AS checksum
FROM public._sqlx_migrations
WHERE version IN (20261007120000, 20261007213000);
COMMIT;
