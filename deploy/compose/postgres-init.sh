#!/bin/sh
set -eu
psql -v ON_ERROR_STOP=1 --username "$POSTGRES_USER" --dbname "$POSTGRES_DB" \
  -v migrate_password="$POSTGRES_MIGRATE_PASSWORD" \
  -v write_password="$POSTGRES_WRITE_PASSWORD" \
  -v read_password="$POSTGRES_READ_PASSWORD" <<'SQL'
CREATE ROLE meada_migrate LOGIN PASSWORD :'migrate_password';
CREATE ROLE meada_write LOGIN PASSWORD :'write_password';
CREATE ROLE meada_read LOGIN PASSWORD :'read_password';
GRANT CREATE ON DATABASE meada TO meada_migrate;
ALTER DEFAULT PRIVILEGES FOR ROLE meada_migrate GRANT USAGE ON SCHEMAS TO meada_write, meada_read;
ALTER DEFAULT PRIVILEGES FOR ROLE meada_migrate GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO meada_write;
ALTER DEFAULT PRIVILEGES FOR ROLE meada_migrate GRANT USAGE, SELECT ON SEQUENCES TO meada_write;
ALTER DEFAULT PRIVILEGES FOR ROLE meada_migrate GRANT SELECT ON TABLES TO meada_read;
SQL
