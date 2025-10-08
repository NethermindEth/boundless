FROM ubuntu

# Base dependencies
RUN apt-get update && \
    DEBIAN_FRONTEND=noninteractive TZ=Etc/UTC \
        apt-get -y install tzdata && \
        apt-get install -y \
            python3 \
            postgresql-client \
            sqlite3 \
            build-essential \
            curl \
            pkg-config \
            libssl-dev \
            git \
            vim \
            tmux \
            htop \
            && \
    rm -rf /var/lib/apt/lists/*

# Install Rust, Foundry and Boundless CLI
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
RUN curl -L https://foundry.paradigm.xyz | bash
RUN /root/.foundry/bin/foundryup

WORKDIR /app
