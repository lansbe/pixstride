FROM rust:1.100.0-slim-trixie@sha256:e9d95b74296f7126a83ae3d041aeaba1ff4c478f13abc5cc034fcf7c131c3d4a AS toolchain
FROM ubuntu:26.04@sha256:065aa8626841f16057b78b027b464cd59c4fac36c204c72dcbf48d66f5fa8a96 AS build
COPY --from=toolchain /usr/local/cargo /usr/local/cargo
COPY --from=toolchain /usr/local/rustup /usr/local/rustup
ENV CARGO_HOME=/usr/local/cargo RUSTUP_HOME=/usr/local/rustup PATH=/usr/local/cargo/bin:$PATH
RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential ca-certificates libcurl4-openssl-dev libssl-dev openssl pkg-config libvips-dev libjxl-dev \
    libheif-plugin-aomenc libheif-plugin-aomdec libheif-plugin-x265 libheif-plugin-libde265 python3 fonts-dejavu-core \
    && rm -rf /var/lib/apt/lists/* && rustup component add rustfmt clippy
WORKDIR /build
COPY Cargo.toml Cargo.lock build.rs ./
COPY vendor ./vendor
COPY src ./src
COPY .github/checks/abi/ .github/checks/abi/
COPY scripts/native_check.py scripts/native_pool_check.py scripts/image_fixtures.py scripts/server_check.py scripts/source_check.py scripts/compatibility.py scripts/telemetry_check.py scripts/config_check.py scripts/s3_crypto_check.py ./scripts/
ENV RUSTFLAGS="--remap-path-prefix=/build=/src"
RUN cargo fetch --locked
RUN --network=none sh .github/checks/abi/run-abi-check.sh . \
    && cargo fmt --check \
    && cargo clippy --locked --offline -- -D warnings \
    && cargo test --locked --offline \
    && cargo clippy --all-features --locked --offline -- -D warnings \
    && cargo test --all-features --locked --offline \
    && cargo build --release --all-features --locked --offline \
    && nm -D --defined-only target/release/pixstride | awk '$3 ~ /^(malloc|calloc|realloc|reallocarray|free|cfree|posix_memalign|aligned_alloc|memalign|valloc|pvalloc|strdup|strndup|malloc_usable_size)$/ { seen[$3]=1 } END { for (s in seen) n++; exit(n != 14) }' \
    && python3 scripts/native_check.py --binary target/release/pixstride \
    && python3 scripts/native_pool_check.py --binary target/release/pixstride \
    && python3 scripts/telemetry_check.py --binary target/release/pixstride \
    && python3 scripts/config_check.py --binary target/release/pixstride \
    && python3 scripts/s3_crypto_check.py --binary target/release/pixstride \
    && python3 scripts/server_check.py --binary target/release/pixstride

FROM ubuntu:26.04@sha256:065aa8626841f16057b78b027b464cd59c4fac36c204c72dcbf48d66f5fa8a96
RUN apt-get update && apt-get upgrade -y && apt-get install -y --no-install-recommends \
    libcurl4t64 ca-certificates libvips42t64 libheif-plugin-aomenc libheif-plugin-aomdec \
    libheif-plugin-x265 libheif-plugin-libde265 fonts-dejavu-core \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /build/target/release/pixstride /usr/local/bin/pixstride
COPY THIRD_PARTY_NOTICES.md /usr/share/doc/pixstride/THIRD_PARTY_NOTICES.md
LABEL org.opencontainers.image.source="https://github.com/lansbe/pixstride" \
      org.opencontainers.image.licenses="MIT" \
      org.opencontainers.image.description="Bounded image transformation server written in Rust"
USER 65532:65532
ENV PIXSTRIDE_BIND=0.0.0.0 PIXSTRIDE_PORT=8080 MALLOC_ARENA_MAX=2 \
    MALLOC_MMAP_THRESHOLD_=4194304 MALLOC_TRIM_THRESHOLD_=4194304 MIMALLOC_ARENA_RESERVE=64MiB MIMALLOC_PURGE_DELAY=1000
EXPOSE 8080
STOPSIGNAL SIGTERM
HEALTHCHECK --interval=30s --timeout=3s --start-period=3s --retries=3 CMD ["pixstride", "healthcheck"]
ENTRYPOINT ["pixstride"]
