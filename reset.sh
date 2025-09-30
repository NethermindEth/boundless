#!/bin/bash

# This script performs a full reset of a failed order using the ORDER ID. It will:
# 1. Find the corresponding job_id (proof_id) from the SQLite database (broker.db).
# 2. Delete the job, tasks, and task dependencies from the PostgreSQL database (taskdb).
# 3. Reset the order's status to 'PendingProving' in the SQLite database.

set -euo pipefail

DB_PATH=${DB_PATH:-/db/broker.db}

ORDER_ID_FRAGMENT="$1"
if [ -z "$ORDER_ID_FRAGMENT" ]; then
  echo "Usage: $0 <order_id_fragment>"
  echo "You can provide a partial or full order ID."
  exit 1
fi

# echo ""
# echo "--- [Step 1/3] Finding Job ID (proof_id) for Order fragment: ${ORDER_ID_FRAGMENT} ---"

# The sqlite3 command to find the proof_id (job_id)
SQLITE_FIND_SQL="SELECT json_extract(data, '\$.proof_id') FROM orders WHERE id LIKE '%${ORDER_ID_FRAGMENT}%';"
JOB_ID=$(sqlite3 "${DB_PATH}" "${SQLITE_FIND_SQL}")

if [ -z "$JOB_ID" ] || [ "$JOB_ID" == "null" ]; then
  echo "Error: No order found with an ID fragment matching '${ORDER_ID_FRAGMENT}' or the order does not have a proof_id."
  exit 1
fi

# echo "Found Job ID: ${JOB_ID}"
# echo ""

# echo "--- [Step 2/3] Resetting PostgreSQL data for Job ID: ${JOB_ID} ---"
PG_HOST="${POSTGRES_HOST:-localhost}"
PG_PORT="${POSTGRES_PORT:-5432}"
PG_USER="${POSTGRES_USER:-worker}"
PG_DB="${POSTGRES_DB:-taskdb}"
PG_SQL="DELETE FROM public.task_deps WHERE job_id = '${JOB_ID}'; DELETE FROM public.tasks WHERE job_id = '${JOB_ID}'; DELETE FROM public.jobs WHERE id = '${JOB_ID}';"

PG_RESULT=$(psql -h "${PG_HOST}" -p "${PG_PORT}" -U "${PG_USER}" -d "${PG_DB}" -c "${PG_SQL}")
# echo "PostgreSQL cleanup complete."
# echo ""

# echo "--- [Step 3/3] Resetting SQLite order status to PendingProving ---"
# Use the original Order ID fragment to update the correct order
SQLITE_RESET_SQL="
PRAGMA busy_timeout = 10000; UPDATE orders
SET data = json_set(
               json_set(data, '\$.status', 'PendingProving'),
               '\$.proof_id', NULL
           )
WHERE id LIKE '%${ORDER_ID_FRAGMENT}%';
SELECT 'SQLite: Order status reset for ' || changes() || ' order(s).';
"

SQLITE_RESULT=$(sqlite3 "${DB_PATH}" "${SQLITE_RESET_SQL}")
echo "${SQLITE_RESULT}"

# echo ""
# echo "--- Reset complete. The broker should now pick up the order for proving. ---"
