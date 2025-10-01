FROM ubuntu

# Base dependencies
RUN apt-get update && \
    DEBIAN_FRONTEND=noninteractive TZ=Etc/UTC apt-get -y install tzdata && \
    apt-get install -y python3 postgresql-client sqlite3 && \
    rm -rf /var/lib/apt/lists/*

USER ubuntu
WORKDIR /app

COPY reset.sh skip.sh status.sh db_order_retry.py /app/

ENTRYPOINT ["python3", "/app/db_order_retry.py"]
