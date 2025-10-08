FROM boundless-base

USER ubuntu
WORKDIR /app

COPY reset.sh skip.sh status.sh db_order_retry.py /app/

ENTRYPOINT ["python3", "/app/db_order_retry.py"]
