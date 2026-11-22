# Upstream's 5.1.2 container is amd64 only. Build the same NGINX module natively on both platforms.
FROM ubuntu:26.04@sha256:065aa8626841f16057b78b027b464cd59c4fac36c204c72dcbf48d66f5fa8a96 AS build
RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential ca-certificates curl libpcre2-dev libvips-dev meson ninja-build pkg-config zlib1g-dev \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /src
RUN curl -fsSL --retry 3 https://codeload.github.com/weserv/images/tar.gz/59fa7bd315a354b1f8e92845f4eadd83bc5f8bf2 -o weserv.tar.gz \
    && echo '1cd61be3076a97d0339ddd680b3e9d6f35956cd1ba3935820449eaea9d35bde9  weserv.tar.gz' | sha256sum -c - \
    && mkdir weserv && tar xf weserv.tar.gz -C weserv --strip-components=1 \
    && meson setup build weserv --prefix=/usr/local --libdir=lib \
    && meson compile -C build -j 2 && meson install -C build
RUN curl -fsSL --retry 3 https://nginx.org/download/nginx-1.31.8.tar.gz -o nginx.tar.gz \
    && echo '0652b760b3b772d2ec17b47c2fc15d9a95e6fe35cd5b877eadc1a62d9bc75af1  nginx.tar.gz' | sha256sum -c - \
    && tar xf nginx.tar.gz && cd nginx-1.31.8 \
    && PKG_CONFIG_PATH=/usr/local/lib/pkgconfig ./configure --prefix=/opt/nginx --add-module=/src/weserv \
    && make -j2 && make install

FROM ubuntu:26.04@sha256:065aa8626841f16057b78b027b464cd59c4fac36c204c72dcbf48d66f5fa8a96
RUN apt-get update && apt-get install -y --no-install-recommends \
        libpcre2-8-0 libvips42t64 libheif-plugin-aomenc zlib1g \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /usr/local/lib/libweserv.so* /usr/local/lib/
COPY --from=build /opt/nginx /opt/nginx
COPY nginx.conf /opt/nginx/conf/nginx.conf
RUN ldconfig
USER 65532:65532
EXPOSE 8080
ENTRYPOINT ["/opt/nginx/sbin/nginx", "-e", "stderr", "-g", "daemon off;"]
