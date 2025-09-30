FROM ubuntu

# Base dependencies
RUN RUN apt-get update && \
    DEBIAN_FRONTEND=noninteractive TZ=Etc/UTC apt-get -y install tzdata && \
    apt-get install -y python3 postgresql-client sqlite3 && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY reset.sh /app/reset.sh
COPY db_order_retry.py /app/db_order_retry.py

ENTRYPOINT ["python3", "/app/db_order_retry.py"]
