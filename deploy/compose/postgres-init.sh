#!/bin/sh
set -eu
# Runs once, when the data volume is first initialised. The three roles ADR 0016 recommends:
# meada_migrate owns the schema and runs `meada postgres migrate`; meada_write is the console,
# the job's role check and the CLI twins; meada_read is the nodes. The default privileges give
# the other two roles the tables the migrate role creates later.
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
