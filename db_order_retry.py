#!/usr/bin/env python3
"""
Database Order Retry Script

This script queries the broker SQLite database directly for failed orders
and implements retry functionality with grace periods to avoid duplicate processing.
"""

import argparse
import logging
import os
import subprocess
import sys
import time
from datetime import datetime, timedelta
from typing import Dict, List


class DatabaseOrderRetry:
    def __init__(
        self,
        grace_period_minutes: int = 30,
        max_retries: int = 3,
    ):
        """
        Initialize the database order retry handler.

        Args:
            grace_period_minutes: Minutes to wait before reprocessing same order ID
            max_retries: Maximum number of retries per order
        """
        self.grace_period_minutes = grace_period_minutes
        self.max_retries = max_retries
        self.processed_orders: Dict[str, datetime] = {}
        script_dir = os.path.dirname(os.path.abspath(__file__))
        self.retry_script = os.path.join(script_dir, "reset.sh")
        self.retried_orders: dict[str, int] = {}

        if not os.environ.get("RPC_URL") or not os.environ.get("PRIVATE_KEY"):
            raise ValueError("RPC_URL and PRIVATE_KEY must be set")

        # Setup logging
        logging.basicConfig(
            level=logging.INFO,
            format="%(asctime)s - %(levelname)s - %(message)s",
            handlers=[
                logging.FileHandler("db_order_retry.log"),
                logging.StreamHandler(sys.stdout),
            ],
        )
        self.logger = logging.getLogger(__name__)

    def get_failed_orders(self) -> List[Dict]:
        """
        Query the database for failed orders.

        Returns:
            List of failed order records
        """
        # Query for orders with status 'Failed' in the JSON data
        query = """
            SELECT id,
                    json_extract(data, '$.updated_at') as updated_at,
                    json_extract(data, '$.error_msg') as error_msg,
                    json_extract(data, '$.request.id') as request_id,
                    json_extract(data, '$.fulfillment_type') as fulfillment_type
            FROM orders 
            WHERE json_extract(data, '$.status') = 'Failed';
        """
        query = " ".join([q.strip() for q in query.split("\n") if q.strip()])
        rows = subprocess.check_output(
            f'sqlite3 {os.environ.get("DB_PATH", '/db/broker.db')} "{query}"',
            shell=True,
            timeout=120,  # 2 minutes
        )

        failed_orders = []
        for row in rows.split(b'\n'):
            if not row:
                continue
            row = row.decode("utf-8")
            data = row.split("|")
            order_data = {
                "id": data[0].split("-")[0],
                "updated_at": data[1],
                "error_msg": data[2],
                "request_id": data[3],
                "fulfillment_type": data[4],
            }
            failed_orders.append(order_data)

        return failed_orders

    def is_within_grace_period(self, order_id: str) -> bool:
        """
        Check if an order ID is within the grace period.

        Args:
            order_id: Order ID to check

        Returns:
            True if within grace period, False otherwise
        """
        if order_id not in self.processed_orders:
            return False

        last_processed = self.processed_orders[order_id]
        grace_period_end = last_processed + timedelta(minutes=self.grace_period_minutes)

        return datetime.now() < grace_period_end

    def should_retry_order(self, order_data: Dict) -> bool:
        """
        Determine if an order should be retried based on various criteria.

        Args:
            order_data: Order data dictionary

        Returns:
            True if order should be retried, False otherwise
        """
        order_id = order_data["id"]

        # Check grace period
        if self.is_within_grace_period(order_id):
            self.logger.info(f"Order {order_id} is within grace period, skipping")
            return False

        # Skip FulfillAfterLockExpire orders (as per original script logic)
        fulfillment_type = order_data.get("fulfillment_type", "")
        if "FulfillAfterLockExpire" in str(fulfillment_type):
            self.logger.info(f"Skipping FulfillAfterLockExpire order: {order_id}")
            return False

        # Check if order has been retried too many times
        retry_count = self.retried_orders.get(order_id, 0)
        if retry_count >= self.max_retries:
            self.logger.warning(
                f"Order {order_id} has exceeded max retries ({retry_count}/{self.max_retries})"
            )
            return False

        return True

    def execute_retry_script(self, order_id: str) -> int:
        """
        Execute the configured retry script for an order.

        Args:
            order_id: Order ID to retry

        Returns:
            Return code of the script
        """
        try:
            # Execute the script with order_id as argument
            result = subprocess.run(
                f"{self.retry_script} {order_id}",
                text=True,
                shell=True,
                capture_output=True,
                timeout=120,  # 2 minutes
            )

            if result.returncode == 0:
                self.logger.info(
                    f"Successfully executed retry script for order {order_id}"
                )
                if result.stdout:
                    self.logger.info(f"Script output: {result.stdout}")
            else:
                self.logger.error(
                    f"Retry script failed for order {order_id}: {result.stderr}"
                )
            return result.returncode

        except Exception as e:
            self.logger.error(f"Error executing retry script for order {order_id}: {e}")
        return -1

    def retry_order(self, order_data: Dict):
        """
        Retry an order.
        """
        order_id = order_data["id"]

        if not self.should_retry_order(order_data):
            self.logger.debug(
                f"Skipping order {order_id} - does not meet retry criteria"
            )
            return

        self.logger.info(f"Processing order: {order_id}")

        # Update retry count
        current_retry_count = self.retried_orders.get(order_id, 0)
        new_retry_count = current_retry_count + 1
        self.retried_orders[order_id] = new_retry_count

        for _ in range(30):
            if self.execute_retry_script(order_id) == 0:
                return
            self.logger.info(f"Retrying script for order {order_id}...")
            time.sleep(1)
        self.logger.error(
            f"Failed to execute script for order {order_id} after 30 attempts"
        )

    def run_continuous_monitoring(self, check_interval_minutes: int = 5):
        """
        Run continuous monitoring of failed orders.

        Args:
            check_interval_minutes: Minutes between database checks
        """
        self.logger.info("Starting continuous monitoring")
        self.logger.info(f"    Grace period:   {self.grace_period_minutes} minutes")
        self.logger.info(f"    Check interval: {check_interval_minutes} minutes")
        self.logger.info(f"    Max retries:    {self.max_retries}")
        self.logger.info(f"    Retry script:   {self.retry_script}")

        try:
            while True:
                failed_orders = self.get_failed_orders()
                # self.logger.info(f"Found {len(failed_orders)} failed orders")

                for order_data in failed_orders:
                    self.retry_order(order_data)

                # Wait for next check
                # self.logger.info(f"Waiting for {check_interval_minutes} minutes...")
                time.sleep(check_interval_minutes * 60)

        except KeyboardInterrupt:
            self.logger.info("Received interrupt signal, shutting down...")
        except Exception as e:
            self.logger.error(f"Unexpected error in continuous monitoring: {e}")


def main():
    """Main function to run the database order retry."""
    parser = argparse.ArgumentParser(description="Retry failed orders from database")
    parser.add_argument(
        "--grace-period",
        type=int,
        default=10,
        help="Grace period in minutes before reprocessing same order ID (default: 10)",
    )
    parser.add_argument(
        "--max-retries",
        type=int,
        default=3,
        help="Maximum number of retries per order (default: 3)",
    )
    parser.add_argument(
        "--check-interval",
        type=int,
        default=3,
        help="Minutes between checks (default: 3)",
    )

    args = parser.parse_args()

    # Check if database file exists
    retry_handler = DatabaseOrderRetry(
        grace_period_minutes=args.grace_period,
        max_retries=args.max_retries,
    )

    # Continuous monitoring mode
    retry_handler.run_continuous_monitoring(args.check_interval)


if __name__ == "__main__":
    main()
