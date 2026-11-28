#!/bin/sh
set -eu
umask 077

if ! test -s /ca/ca.pem || ! test -s /valkey/cert.pem || ! test -s /valkey/key.pem || \
   ! test -s /store/rustfs_cert.pem || ! test -s /store/rustfs_key.pem; then
  rm -f /postgres/cert.pem /postgres/key.pem
  openssl req -x509 -newkey rsa:2048 -nodes -days 365 -subj /CN=Meada-compose-CA \
    -keyout /tmp/ca.key -out /ca/ca.pem
  for service in valkey store; do
    openssl req -newkey rsa:2048 -nodes -subj "/CN=$service" \
      -keyout "/$service/key.pem" -out /tmp/service.csr
    printf 'subjectAltName=DNS:%s\nextendedKeyUsage=serverAuth\n' "$service" > /tmp/service.ext
    openssl x509 -req -in /tmp/service.csr -CA /ca/ca.pem -CAkey /tmp/ca.key \
      -set_serial "$(test "$service" = valkey && printf 2 || printf 3)" -days 365 \
      -extfile /tmp/service.ext -out "/$service/cert.pem"
  done
  mv /store/cert.pem /store/rustfs_cert.pem
  mv /store/key.pem /store/rustfs_key.pem
fi
if ! test -s /postgres/cert.pem || ! test -s /postgres/key.pem; then
  ca=/ca/ca.pem
  if ! test -s /tmp/ca.key; then
    openssl req -x509 -newkey rsa:2048 -nodes -days 365 -subj /CN=Meada-compose-Postgres-CA \
      -keyout /tmp/ca.key -out /tmp/ca.pem
    cat /tmp/ca.pem >> /ca/ca.pem
    ca=/tmp/ca.pem
  fi
  openssl req -newkey rsa:2048 -nodes -subj /CN=postgres \
    -keyout /postgres/key.pem -out /tmp/service.csr
  printf 'subjectAltName=DNS:postgres\nextendedKeyUsage=serverAuth\n' > /tmp/service.ext
  openssl x509 -req -in /tmp/service.csr -CA "$ca" -CAkey /tmp/ca.key -set_serial 4 -days 365 \
    -extfile /tmp/service.ext -out /postgres/cert.pem
fi
if test -d /accounts-private && test -d /accounts-public; then
  if ! test -s /accounts-private/private.pem || ! test -s /accounts-public/public.pem; then
    rm -f /accounts-private/private.pem /accounts-public/public.pem
    openssl genpkey -algorithm ed25519 -out /accounts-private/private.pem
    openssl pkey -in /accounts-private/private.pem -pubout -out /accounts-public/public.pem
  fi
  chmod 400 /accounts-private/private.pem
  chmod 444 /accounts-public/public.pem
  chown 10001:10001 /accounts-private /accounts-private/private.pem
fi
digest=$(printf %s "$VALKEY_PASSWORD" | openssl dgst -sha256)
digest=${digest##* }
printf 'user default off\nuser meada on #%s ~{meada:compose}:* ~meada:compose:* +@all -@admin -@dangerous -scan\n' "$digest" > /valkey/users.acl
chmod 644 /ca/ca.pem /valkey/cert.pem /store/rustfs_cert.pem /postgres/cert.pem
chown -R 10001:10001 /valkey /store
chown -R 70:70 /postgres
chown 10001:10001 /valkey-data /store-data /job-mirrors /job-indexes /node-one /node-two
chown 70:70 /postgres-data
