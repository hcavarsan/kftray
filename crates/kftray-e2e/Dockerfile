FROM ubuntu:26.04@sha256:c098726dd9b9733f3e885f8de0ef375206a311b0a317eadc40d347b1115fb944

ARG NODE_VERSION=26.11.1
ARG PNPM_VERSION=12.9.1
ARG PNPM_SHA256=b4f58449ac02d9d24023ac89e17e05e8331f8ddc12ca1f489cf231bc2bb2bce1
ARG RUSTUP_VERSION=1.29.1
ARG RUSTUP_SHA256=dda7234360b7f578ca8b0ddcb80145646fa61a67c1720a5abc7051b35c9fcb71
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
    && curl -fsSLo /tmp/pnpm.tar.gz \
        "https://github.com/pnpm/pnpm/releases/download/v${PNPM_VERSION}/pnpm-linux-x64.tar.gz" \
    && echo "${PNPM_SHA256}  /tmp/pnpm.tar.gz" | sha256sum -c - \
    && mkdir -p /usr/local/lib/pnpm \
    && tar -xz -C /usr/local/lib/pnpm -f /tmp/pnpm.tar.gz \
    && ln -s /usr/local/lib/pnpm/pnpm /usr/local/bin/pnpm \
    && rm /tmp/pnpm.tar.gz

ENV RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:$PATH \
    CARGO_TERM_COLOR=always
RUN curl -fsSLo /tmp/rustup-init \
        "https://static.rust-lang.org/rustup/archive/${RUSTUP_VERSION}/x86_64-unknown-linux-gnu/rustup-init" \
    && echo "${RUSTUP_SHA256}  /tmp/rustup-init" | sha256sum -c - \
    && chmod +x /tmp/rustup-init \
    && /tmp/rustup-init -y --profile minimal --default-toolchain stable --component llvm-tools \
    && rm /tmp/rustup-init \
    && curl -fsSL "https://get.nexte.st/${NEXTEST_VERSION}/linux" | tar -xz -C /usr/local/cargo/bin \
    && curl -fsSL "https://github.com/taiki-e/cargo-llvm-cov/releases/download/v${LLVM_COV_VERSION}/cargo-llvm-cov-x86_64-unknown-linux-gnu.tar.gz" \
        | tar -xz -C /usr/local/cargo/bin

WORKDIR /work
COPY package.json pnpm-lock.yaml pnpm-workspace.yaml ./
COPY frontend/package.json frontend/
RUN pnpm install --frozen-lockfile

COPY . .

CMD ["cargo", "run", "--package", "kftray-e2e", "--", "run"]
