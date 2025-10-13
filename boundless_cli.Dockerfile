FROM boundless-base

RUN /root/.cargo/bin/cargo install --locked --git https://github.com/boundless-xyz/boundless boundless-cli --branch release-1.0 --bin boundless && \
    /root/.cargo/bin/boundless completions bash > /etc/bash_completion.d/boundless && \
    echo "source /etc/bash_completion.d/boundless" >> /root/.bashrc

ENTRYPOINT ["/root/.cargo/bin/boundless"]
