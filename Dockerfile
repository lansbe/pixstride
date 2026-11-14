FROM rust:1.99.0-alpine3.24@sha256:0cce0a5e0e8ba67b455257a3a02a1d99005f382748789d6464460028810f1627 AS build
RUN apk add --no-cache curl-dev openssl-dev openssl pkgconf vips-dev libjxl-dev vips-heif vips-jxl libheif-aom libheif-x265 python3 font-dejavu \
    && rustup component add rustfmt clippy
WORKDIR /build
COPY Cargo.toml Cargo.lock build.rs ./
COPY vendor ./vendor
COPY src ./src
COPY .github/checks/abi/ .github/checks/abi/
COPY scripts/native_check.py scripts/image_fixtures.py scripts/server_check.py scripts/source_check.py scripts/compatibility.py scripts/telemetry_check.py scripts/config_check.py scripts/s3_crypto_check.py ./scripts/
ENV RUSTFLAGS="--remap-path-prefix=/build=/src -C target-feature=-crt-static"
RUN cargo fetch --locked
RUN --network=none sh .github/checks/abi/run-abi-check.sh . \
    && cargo fmt --check \
    && cargo clippy --locked --offline -- -D warnings \
    && cargo test --locked --offline \
    && cargo clippy --all-features --locked --offline -- -D warnings \
    && cargo test --all-features --locked --offline \
    && cargo build --release --all-features --locked --offline \
    && python3 scripts/native_check.py --binary target/release/pixstride \
    && python3 scripts/telemetry_check.py --binary target/release/pixstride \
    && python3 scripts/config_check.py --binary target/release/pixstride \
    && python3 scripts/s3_crypto_check.py --binary target/release/pixstride \
    && python3 scripts/server_check.py --binary target/release/pixstride

FROM alpine:3.24@sha256:f59abe656b692d901e184be3445e084786602108599d2a51d229c0f5a48c71bf
RUN apk upgrade --no-cache && apk add --no-cache libcurl libgcc ca-certificates vips vips-heif vips-jxl libheif-aom libheif-x265 font-dejavu
COPY --from=build /build/target/release/pixstride /usr/local/bin/pixstride
LABEL org.opencontainers.image.source="https://github.com/lansbe/pixstride" \
      org.opencontainers.image.licenses="MIT" \
      org.opencontainers.image.description="Bounded image transformation server written in Rust"
USER 65532:65532
ENV PIXSTRIDE_BIND=0.0.0.0 PIXSTRIDE_PORT=8080
EXPOSE 8080
STOPSIGNAL SIGTERM
HEALTHCHECK --interval=30s --timeout=3s --start-period=3s --retries=3 CMD ["pixstride", "healthcheck"]
ENTRYPOINT ["pixstride"]
