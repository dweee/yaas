# Ubuntu supplies the ABI used by the distributable AppImage.
FROM ubuntu:24.04@sha256:534baea6a22c03a63003dbc8dbe78fe34bc0d7e595d9a9dc9834884ff530eb55

ARG BUILD_UID=1000
ARG BUILD_GID=1000
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
    bash ca-certificates curl file git pkg-config unzip wget xz-utils zip tar \
    clang cmake ninja-build build-essential libgtk-3-dev libasound2-dev \
    libmpv-dev libmimalloc-dev libblas3 liblapack3 libblkid-dev liblzma-dev \
    libsecret-1-dev libjsoncpp-dev libssl-dev fuse libfuse2t64 \
    desktop-file-utils zsync p7zip-full patchelf python3 \
    && rm -rf /var/lib/apt/lists/*

ENV RUSTUP_HOME=/opt/rustup CARGO_HOME=/opt/cargo
ENV PATH=/opt/flutter/bin:/opt/cargo/bin:/opt/rinf/bin:/opt/pub-cache/bin:$PATH
RUN curl -fsSL https://static.rust-lang.org/rustup/archive/1.28.2/x86_64-unknown-linux-gnu/rustup-init -o /tmp/rustup-init \
    && chmod +x /tmp/rustup-init \
    && /tmp/rustup-init -y --profile minimal --default-toolchain 1.98.1 --no-modify-path \
    && rm /tmp/rustup-init \
    && cargo install rinf_cli --version 8.10.1 --locked --root /opt/rinf \
    && rm -rf /opt/cargo/registry /opt/cargo/git /tmp/cargo-install*

RUN git clone --depth 1 --branch 3.47.0 https://github.com/flutter/flutter.git /opt/flutter \
    && test "$(git -C /opt/flutter rev-parse HEAD)" = 4cf24164269a5ebf0c16a028a00727d0e77bbb05 \
    && git config --system --add safe.directory /opt/flutter \
    && flutter config --no-analytics --enable-linux-desktop \
    && flutter precache --linux

ENV PUB_CACHE=/opt/pub-cache
RUN dart pub global activate fastforge 0.6.12
RUN curl -fsSL https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage -o /usr/local/bin/appimagetool \
    && echo '95cbe7cce9717fce90c484e34052ee7c7f1d7635b33c12525b4776826a7d29b6  /usr/local/bin/appimagetool' | sha256sum -c - \
    && chmod +x /usr/local/bin/appimagetool

RUN apt-get update && apt-get install -y --no-install-recommends locate \
    && rm -rf /var/lib/apt/lists/*

RUN chown -R ${BUILD_UID}:${BUILD_GID} /opt/flutter /opt/pub-cache \
    && mkdir -p /workspace /cache
ENV APPIMAGE_EXTRACT_AND_RUN=1
WORKDIR /workspace
