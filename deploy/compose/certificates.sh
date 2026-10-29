#!/bin/sh
set -eu
umask 077

# Preserve the trust chain when Compose restarts this completed job.
if ! test -s /ca/ca.pem || ! test -s /valkey/cert.pem || ! test -s /valkey/key.pem || \
   ! test -s /store/rustfs_cert.pem || ! test -s /store/rustfs_key.pem || \
   ! test -s /postgres/cert.pem || ! test -s /postgres/key.pem; then
  # The CA key exists only in this job's tmpfs. Services receive their own key and the public CA.
  openssl req -x509 -newkey rsa:2048 -nodes -days 365 -subj /CN=Meada-compose-CA \
    -keyout /tmp/ca.key -out /ca/ca.pem
  for service in valkey store postgres; do
    openssl req -newkey rsa:2048 -nodes -subj "/CN=$service" \
      -keyout "/$service/key.pem" -out /tmp/service.csr
    printf 'subjectAltName=DNS:%s\nextendedKeyUsage=serverAuth\n' "$service" > /tmp/service.ext
    openssl x509 -req -in /tmp/service.csr -CA /ca/ca.pem -CAkey /tmp/ca.key \
      -set_serial "$(case "$service" in valkey) printf 2 ;; store) printf 3 ;; *) printf 4 ;; esac)" -days 365 \
      -extfile /tmp/service.ext -out "/$service/cert.pem"
  done
  mv /store/cert.pem /store/rustfs_cert.pem
  mv /store/key.pem /store/rustfs_key.pem
fi
digest=$(printf %s "$VALKEY_PASSWORD" | openssl dgst -sha256)
digest=${digest##* }
printf 'user default off\nuser meada on #%s ~{meada:compose}:* ~meada:compose:* +@all -@admin -@dangerous -scan\n' "$digest" > /valkey/users.acl
chmod 644 /ca/ca.pem /valkey/cert.pem /store/rustfs_cert.pem /postgres/cert.pem
chown -R 10001:10001 /valkey /store
# The postgres:alpine image runs as uid 70 and refuses a key that is not its own and mode 0600.
chown -R 70:70 /postgres
chown 10001:10001 /valkey-data /store-data /job-mirrors /job-indexes /node-one /node-two
chown 70:70 /postgres-data
