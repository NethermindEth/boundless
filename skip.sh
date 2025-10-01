#!/bin/bash

set -euo pipefail

DB_PATH=${DB_PATH:-/db/broker.db}

ORDER_ID_FRAGMENT="$1"
if [ -z "$ORDER_ID_FRAGMENT" ]; then
  echo "Usage: $0 <order_id_fragment>"
  echo "You can provide a partial or full order ID."
  exit 1
fi

SQLITE_RESET_SQL="
PRAGMA busy_timeout = 10000; UPDATE orders
SET data = json_set(
               json_set(data, '\$.status', 'Skipped'),
               '\$.proof_id', NULL
           )
WHERE id LIKE '%${ORDER_ID_FRAGMENT}%';
SELECT 'SQLite: Order status reset for ' || changes() || ' order(s).';
"

SQLITE_RESULT=$(sqlite3 "${DB_PATH}" "${SQLITE_RESET_SQL}")
echo "${SQLITE_RESULT}"
