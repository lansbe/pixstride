FROM rust:1.100.0-slim-trixie@sha256:e9d95b74296f7126a83ae3d041aeaba1ff4c478f13abc5cc034fcf7c131c3d4a AS toolchain
FROM ubuntu:26.04@sha256:d66b366dac61b575bee2aef07e9c26e8070096fbe84a3d4d3eb0da069b5d76db AS codecs-build
RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential ca-certificates cmake curl git libde265-dev libsharpyuv-dev libx265-dev ninja-build nasm pkg-config \
    && rm -rf /var/lib/apt/lists/*
COPY scripts/build_native_codecs.sh /usr/local/bin/build-native-codecs
RUN sh /usr/local/bin/build-native-codecs /opt/pixstride-codecs

FROM ubuntu:26.04@sha256:d66b366dac61b575bee2aef07e9c26e8070096fbe84a3d4d3eb0da069b5d76db AS vips-build
COPY --from=toolchain /usr/local/cargo /usr/local/cargo
COPY --from=toolchain /usr/local/rustup /usr/local/rustup
ENV CARGO_HOME=/usr/local/cargo RUSTUP_HOME=/usr/local/rustup PATH=/usr/local/cargo/bin:$PATH
RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential ca-certificates libcurl4-openssl-dev libssl-dev openssl pkg-config libvips-dev libjxl-dev \
    libde265-0 libx265-215 libsharpyuv0 python3 fonts-dejavu-core \
    && rm -rf /var/lib/apt/lists/* && rustup component add rustfmt clippy
COPY --from=codecs-build /opt/pixstride-codecs/ /opt/pixstride-codecs/
ENV LD_LIBRARY_PATH=/opt/pixstride-codecs/lib PKG_CONFIG_PATH=/opt/pixstride-codecs/lib/pkgconfig
RUN apt-get update && apt-get install -y --no-install-recommends \
    meson ninja-build cbindgen git curl libde265-dev libx265-dev libsharpyuv-dev \
    && rm -rf /var/lib/apt/lists/*
COPY scripts/build_native_vips.sh /usr/local/bin/build-native-vips
RUN sh /usr/local/bin/build-native-vips /opt/pixstride-codecs
ENV VIPS_PLUGIN_PATH=/opt/pixstride-codecs/lib/vips-modules-8.18

FROM ubuntu:26.04@sha256:d66b366dac61b575bee2aef07e9c26e8070096fbe84a3d4d3eb0da069b5d76db AS codec-runtime
# The base image includes Pebble, while Pixstride is the runtime entry point.
RUN apt-get update && apt-get upgrade -y && apt-get install -y --no-install-recommends \
    libcurl4t64 ca-certificates libvips42t64 libde265-0 libx265-215 libsharpyuv0 fonts-dejavu-core \
    && rm -f /usr/bin/pebble \
    && rm -rf /var/lib/apt/lists/*
COPY --from=codecs-build /runtime/lib/ /opt/pixstride-codecs/lib/
COPY --from=codecs-build /runtime/share/ /usr/share/
COPY --from=vips-build /vips-runtime/lib/ /opt/pixstride-codecs/lib/
COPY --from=vips-build /vips-runtime/share/ /usr/share/
COPY scripts/build_native_vips.sh /usr/share/doc/pixstride/vips/build_native_vips.sh
COPY scripts/build_native_codecs.sh /usr/share/doc/pixstride/codecs/build_native_codecs.sh
COPY THIRD_PARTY_NOTICES.md /usr/share/doc/pixstride/THIRD_PARTY_NOTICES.md
ENV LD_LIBRARY_PATH=/opt/pixstride-codecs/lib VIPS_PLUGIN_PATH=/opt/pixstride-codecs/lib/vips-modules-8.18

FROM codec-runtime AS codec-runtime-check
USER root
RUN apt-get update && apt-get install -y --no-install-recommends python3 && rm -rf /var/lib/apt/lists/*
COPY scripts/native_codecs_check.py /checks/scripts/native_codecs_check.py
COPY .github/checks/project/test_native_codecs.py /checks/.github/checks/project/test_native_codecs.py
RUN PIXSTRIDE_CODEC_TEST_PREFIX=/opt/pixstride-codecs python3 -m unittest discover -s /checks/.github/checks/project -p test_native_codecs.py -v \
    && python3 /checks/scripts/native_codecs_check.py > /codec-check.json

FROM vips-build AS compile
WORKDIR /build
COPY Cargo.toml Cargo.lock build.rs ./
COPY vendor ./vendor
COPY src ./src
COPY .github/checks/abi/ .github/checks/abi/
COPY scripts/native_codecs_check.py ./scripts/
ENV RUSTFLAGS="--remap-path-prefix=/build=/src"
RUN cargo fetch --locked
RUN --network=none python3 scripts/native_codecs_check.py \
    && sh .github/checks/abi/run-abi-check.sh . \
    && cargo fmt --check \
    && cargo clippy --locked --offline -- -D warnings \
    && cargo test --locked --offline \
    && cargo clippy --all-features --locked --offline -- -D warnings \
    && cargo test --all-features --locked --offline \
    && cargo build --release --all-features --locked --offline \
    && nm -D --defined-only target/release/pixstride | awk '$3 ~ /^(mi_|malloc$|calloc$|realloc$|reallocarray$|free$|cfree$|posix_memalign$|aligned_alloc$|memalign$|valloc$|pvalloc$|strdup$|strndup$|malloc_usable_size$)/ { bad=1 } END { exit(bad) }'
FROM compile AS build
COPY scripts/native_check.py scripts/native_pool_check.py scripts/image_fixtures.py scripts/server_check.py scripts/source_check.py scripts/compatibility.py scripts/telemetry_check.py scripts/config_check.py scripts/s3_crypto_check.py ./scripts/
RUN --network=none python3 scripts/native_check.py --binary target/release/pixstride \
    && python3 scripts/native_pool_check.py --binary target/release/pixstride \
    && python3 scripts/telemetry_check.py --binary target/release/pixstride \
    && python3 scripts/config_check.py --binary target/release/pixstride \
    && python3 scripts/s3_crypto_check.py --binary target/release/pixstride \
    && python3 scripts/server_check.py --binary target/release/pixstride

FROM codec-runtime AS runtime
COPY --from=codec-runtime-check /codec-check.json /usr/share/doc/pixstride/codecs/runtime-check.json
COPY --from=build /build/target/release/pixstride /usr/local/bin/pixstride
LABEL org.opencontainers.image.source="https://github.com/lansbe/pixstride" \
      org.opencontainers.image.licenses="MIT" \
      org.opencontainers.image.description="Bounded image transformation server written in Rust"
USER 65532:65532
ENV PIXSTRIDE_BIND=0.0.0.0 PIXSTRIDE_PORT=8080 MALLOC_ARENA_MAX=1 \
    MALLOC_MMAP_THRESHOLD_=4194304 MALLOC_TRIM_THRESHOLD_=4194304 GLIBC_TUNABLES=glibc.malloc.hugetlb=0
EXPOSE 8080
STOPSIGNAL SIGTERM
HEALTHCHECK --interval=30s --timeout=3s --start-period=3s --retries=3 CMD ["pixstride", "healthcheck"]
ENTRYPOINT ["pixstride"]
