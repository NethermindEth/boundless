FROM boundless-base

RUN /root/.cargo/bin/cargo install --locked --git https://github.com/boundless-xyz/boundless boundless-cli --branch release-1.0 --bin boundless

ENTRYPOINT ["/root/.cargo/bin/boundless"]
