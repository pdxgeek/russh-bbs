#!/usr/bin/env bash
# Regenerate the committed `.sqlx/` offline query data for BOTH backends (D34).
#
# sqlx checks every `query!` against the one database named by DATABASE_URL, so each backend is
# built alone (the `sqlite` and `postgres` crate features) against a scratch database made from
# that backend's migrations. Needs: python3 (for SQLite) and a Postgres you can create a
# database on, given as POSTGRES_ADMIN_URL (for example postgres://user:pw@localhost:5432/postgres).
#
#   POSTGRES_ADMIN_URL=postgres://bbs:pw@localhost:5432/postgres crates/store/regenerate-sqlx.sh
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT
rm -rf "$here/.sqlx"
mkdir -p "$here/.sqlx"

# ---- SQLite ----
python3 - "$here/migrations/sqlite" "$scratch/regen.db" <<'PY'
import sqlite3, sys, pathlib
db = sqlite3.connect(sys.argv[2])
for f in sorted(pathlib.Path(sys.argv[1]).glob("*.sql")):
    db.executescript(f.read_text())
db.commit()
PY
(cd "$here" && SQLX_OFFLINE=false DATABASE_URL="sqlite://$scratch/regen.db" SQLX_OFFLINE_DIR="$here/.sqlx" \
    cargo check -p russh-bbs-store --no-default-features --features sqlite)

# ---- Postgres ----
: "${POSTGRES_ADMIN_URL:?set POSTGRES_ADMIN_URL to build the Postgres query data}"
name="bbs_regen_$$"
psql "$POSTGRES_ADMIN_URL" -v ON_ERROR_STOP=1 -qc "CREATE DATABASE $name"
trap 'psql "$POSTGRES_ADMIN_URL" -qc "DROP DATABASE IF EXISTS $name" >/dev/null; rm -rf "$scratch"' EXIT
regen_url="${POSTGRES_ADMIN_URL%/*}/$name"
for f in "$here"/migrations/postgres/*.sql; do
    psql "$regen_url" -v ON_ERROR_STOP=1 -q -f "$f"
done
(cd "$here" && SQLX_OFFLINE=false DATABASE_URL="$regen_url" SQLX_OFFLINE_DIR="$here/.sqlx" \
    cargo check -p russh-bbs-store --no-default-features --features postgres)
echo "wrote $(ls "$here/.sqlx" | wc -l) query files to $here/.sqlx"
