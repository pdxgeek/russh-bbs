#!/usr/bin/env bash
# Start a throwaway Postgres that REQUIRES TLS (D31): `ssl = on`, and a pg_hba.conf with only
# `hostssl` entries, so a client that does not use TLS is refused. The certificate is signed by a
# fresh test CA (postgres-tls-certs.sh). Used by the CI "postgres-tls" job and runnable locally.
#
#   postgres-tls-server.sh <dir> <port>
#
# Needs the Postgres server binaries (`apt-get install postgresql`), openssl, and root or sudo to
# run initdb as the `postgres` user. Prints the variables the tests read, one per line, as
# NAME=value; in CI they are appended to $GITHUB_ENV.
set -euo pipefail
dir="${1:?usage: $0 <dir> <port>}"
port="${2:?usage: $0 <dir> <port>}"
here="$(cd "$(dirname "$0")" && pwd)"
bin="$(ls -d /usr/lib/postgresql/*/bin | sort -V | tail -1)"
as_pg() { if [ "$(id -u)" = 0 ]; then runuser -u postgres -- "$@"; else sudo -u postgres "$@"; fi; }
as_root() { if [ "$(id -u)" = 0 ]; then "$@"; else sudo "$@"; fi; }

as_root rm -rf "$dir"
as_root mkdir -p "$dir"
as_root chown postgres "$dir"
as_root chmod 755 "$dir"

certs="$(mktemp -d)"
"$here/postgres-tls-certs.sh" "$certs" localhost >/dev/null
as_root install -d -o postgres -m 755 "$dir/certs"
as_root install -o postgres -m 644 "$certs/ca.crt" "$certs/server.crt" "$dir/certs/"
as_root install -o postgres -m 600 "$certs/server.key" "$dir/certs/"
rm -rf "$certs"

# A fresh random password per run, written to a file only to feed initdb, then deleted.
password="$(openssl rand -hex 16)"
pwfile="$dir/pwfile"
printf '%s' "$password" | as_root tee "$pwfile" >/dev/null
as_root chown postgres "$pwfile"
as_pg "$bin/initdb" -D "$dir/data" -U bbs -A scram-sha-256 --pwfile="$pwfile" >/dev/null
as_root rm -f "$pwfile"

cat <<CONF | as_root tee -a "$dir/data/postgresql.conf" >/dev/null
port = $port
listen_addresses = '127.0.0.1'
unix_socket_directories = '$dir'
ssl = on
ssl_cert_file = '$dir/certs/server.crt'
ssl_key_file = '$dir/certs/server.key'
fsync = off
CONF
# Only TLS connections are accepted; everything else is refused.
cat <<HBA | as_root tee "$dir/data/pg_hba.conf" >/dev/null
local   all  postgres  peer
hostssl all  all  127.0.0.1/32  scram-sha-256
host    all  all  0.0.0.0/0     reject
HBA
as_pg "$bin/pg_ctl" -D "$dir/data" -l "$dir/server.log" -w start >/dev/null

echo "RUSSH_BBS_TEST_POSTGRES_URL=postgres://bbs:$password@localhost:$port/postgres?sslmode=verify-full&sslrootcert=$dir/certs/ca.crt"
echo "RUSSH_BBS_TEST_POSTGRES_TLS_URL=postgres://bbs:$password@localhost:$port/postgres"
echo "RUSSH_BBS_TEST_POSTGRES_ROOT_CERT=$dir/certs/ca.crt"
