FROM ubuntu:24.04

ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update \
    && apt-get install --yes --no-install-recommends \
        bc \
        binutils-aarch64-linux-gnu \
        bison \
        build-essential \
        ca-certificates \
        device-tree-compiler \
        flex \
        gcc-aarch64-linux-gnu \
        git \
        libgnutls28-dev \
        libncurses-dev \
        libssl-dev \
        lz4 \
        openssl \
        pkg-config \
        python3 \
        python3-cryptography \
        python3-jsonschema \
        python3-pycryptodome \
        python3-pyelftools \
        python3-setuptools \
        python3-yaml \
        swig \
        uuid-dev \
        xz-utils \
        zstd \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /src
