FROM nethermindeth/boundless-base

ARG BUILD_UNIQUE_ID
RUN echo "Unique ID: $BUILD_UNIQUE_ID (This is used to invalidate the cache)" && \
    /root/.cargo/bin/cargo install --locked --git https://github.com/boundless-xyz/boundless boundless-cli --branch release-1.1 --bin boundless && \
    /root/.cargo/bin/boundless completions bash > /etc/bash_completion.d/boundless && \
    echo "source /etc/bash_completion.d/boundless" >> /root/.bashrc

ENTRYPOINT ["/root/.cargo/bin/boundless"]
