#!/bin/bash

set -euo pipefail

DB_PATH=${DB_PATH:-/db/broker.db}

ORDER_ID_FRAGMENT="$1"
if [ -z "$ORDER_ID_FRAGMENT" ]; then
  echo "Usage: $0 <order_id_fragment>"
  echo "You can provide a partial or full order ID."
  exit 1
fi

SQLITE_FIND_SQL="SELECT json_extract(data, '\$.status') AS status, json_extract(data, '\$.error_msg') AS error_msg, datetime(json_extract(data, '\$.update_at'), 'unixepoch') AS updated_at_human FROM orders WHERE id LIKE '%${ORDER_ID_FRAGMENT}%';"


SQLITE_RESULT=$(sqlite3 "${DB_PATH}" "${SQLITE_FIND_SQL}")
echo "${SQLITE_RESULT}"
