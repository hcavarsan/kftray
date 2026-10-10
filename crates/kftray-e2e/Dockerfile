FROM ubuntu:26.04

ARG NODE_VERSION=26.11.1
ARG PNPM_VERSION=12.9.1
ARG NEXTEST_VERSION=0.9.146
ARG LLVM_COV_VERSION=0.9.1

ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get -o Acquire::Retries=3 update \
    && apt-get install -y \
        build-essential \
        ca-certificates \
        curl \
        file \
        git \
        libayatana-appindicator3-dev \
        librsvg2-dev \
        libssl-dev \
        libwebkit2gtk-4.1-dev \
        libxdo-dev \
        patchelf \
        pkg-config \
        xdg-utils \
        xvfb \
        xz-utils \
    && rm -rf /var/lib/apt/lists/*

RUN curl -fsSL "https://nodejs.org/dist/v${NODE_VERSION}/node-v${NODE_VERSION}-linux-x64.tar.xz" \
        | tar -xJ -C /usr/local --strip-components=1 \
    && npm install --global "pnpm@${PNPM_VERSION}"

ENV RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:$PATH \
    CARGO_TERM_COLOR=always
RUN curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable \
        --component llvm-tools \
    && curl -fsSL "https://get.nexte.st/${NEXTEST_VERSION}/linux" | tar -xz -C /usr/local/cargo/bin \
    && curl -fsSL "https://github.com/taiki-e/cargo-llvm-cov/releases/download/v${LLVM_COV_VERSION}/cargo-llvm-cov-x86_64-unknown-linux-gnu.tar.gz" \
        | tar -xz -C /usr/local/cargo/bin

WORKDIR /work
COPY package.json pnpm-lock.yaml pnpm-workspace.yaml ./
COPY frontend/package.json frontend/
RUN pnpm install --frozen-lockfile

COPY . .

CMD ["cargo", "run", "--package", "kftray-e2e", "--", "run"]
