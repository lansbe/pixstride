ARG DEBIAN_IMAGE=debian:bookworm-slim@sha256:734a72b3d6142a71993b4bc580ca01d8ca687f77a2c422d96a961d3376b179bc
FROM ${DEBIAN_IMAGE} AS build
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates gcc golang-go libc6-dev libvips-dev pkg-config \
    && rm -rf /var/lib/apt/lists/*
# The upstream v1.2.4 image is single-architecture. Build its release commit
# against the same Debian libvips packages on each native runner.
RUN GOBIN=/out go install -trimpath -ldflags='-s -w -X main.Version=1.2.4' \
        github.com/h2non/imaginary@9357624e976790905f1b1bf4d059f8643c21e6fb

FROM ${DEBIAN_IMAGE}
RUN apt-get update && apt-get install -y --no-install-recommends libvips42 \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /out/imaginary /usr/local/bin/imaginary
USER 65534:65534
EXPOSE 8080
ENTRYPOINT ["imaginary", "-p", "8080", "-mount", "/images", "-http-cache-ttl", "0", "-allowed-origins", "http://disabled.invalid", "-disable-endpoints", "watermarkimage"]
