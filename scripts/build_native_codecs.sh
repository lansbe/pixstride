#!/bin/sh
# Rebuild unmodified, checksum-pinned upstream codecs; no SDK is shipped at runtime.
set -eu
prefix=${1:-/opt/pixstride-codecs}
jobs=${PIXSTRIDE_CODEC_BUILD_JOBS:-2}
case "$jobs" in 1|2) ;; *) echo 'codec build jobs must be 1 or 2' >&2; exit 1 ;; esac
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT HUP INT TERM
cd "$work"
aom_commit=b141d9c45db236335351c8a4b2a14b3ade27c937
aom_url=https://aomedia.googlesource.com/aom
heif_url=https://github.com/strukturag/libheif/releases/download/v1.23.7/libheif-1.23.7.tar.gz
# Gitiles tarballs contain regenerated metadata, so bind AOM to its verified Git
# object rather than trusting a changing archive checksum or a moving branch.
git init -q aom-git
git -C aom-git fetch --depth 1 "$aom_url" "$aom_commit"
test "$(git -C aom-git rev-parse 'FETCH_HEAD^{commit}')" = "$aom_commit"
git -C aom-git fsck --strict --no-reflogs
git -C aom-git archive --format=tar "$aom_commit" | gzip -n > aom.tar.gz
curl --fail --location --retry 3 --proto '=https' --tlsv1.2 "$heif_url" -o libheif.tar.gz
printf '%s  %s\n' f7483e818503ea6224c8863e4a56ac02c7089021a98472c811c12aa19faf2475 libheif.tar.gz | sha256sum --check
sha256sum aom.tar.gz libheif.tar.gz > SHA256SUMS
mkdir aom
tar -xzf aom.tar.gz -C aom
tar -xzf libheif.tar.gz
cmake -S aom -B aom-build -G Ninja -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_INSTALL_PREFIX="$prefix" -DCMAKE_INSTALL_LIBDIR=lib \
    -DBUILD_SHARED_LIBS=ON -DCONFIG_PIC=1 \
    -DENABLE_TESTS=OFF -DENABLE_EXAMPLES=OFF -DENABLE_TOOLS=OFF -DENABLE_DOCS=OFF
cmake --build aom-build --parallel "$jobs"
cmake --install aom-build
export PKG_CONFIG_PATH="$prefix/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
cmake -S libheif-1.23.7 -B heif-build -G Ninja -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_INSTALL_PREFIX="$prefix" -DCMAKE_INSTALL_LIBDIR=lib \
    -DCMAKE_PREFIX_PATH="$prefix" '-DCMAKE_INSTALL_RPATH=$ORIGIN' \
    -DBUILD_SHARED_LIBS=ON -DENABLE_PLUGIN_LOADING=OFF \
    -DWITH_AOM_ENCODER=ON -DWITH_AOM_DECODER=ON -DWITH_X265=ON -DWITH_LIBDE265=ON \
    -DWITH_LIBSHARPYUV=ON -DWITH_LIBSHARPYUV_INTERNAL=OFF \
    -DWITH_X264=OFF -DWITH_OpenH264_DECODER=OFF -DWITH_DAV1D=OFF -DWITH_KVAZAAR=OFF \
    -DWITH_EXAMPLES=OFF -DWITH_GDK_PIXBUF=OFF -DBUILD_TESTING=OFF -DBUILD_DOCUMENTATION=OFF
cmake --build heif-build --parallel "$jobs"
cmake --install heif-build
# Separate runtime payload from the headers/pkgconfig/cmake SDK retained in this stage.
mkdir -p /runtime/lib /runtime/share/doc/pixstride/codecs
cp -a "$prefix"/lib/libaom.so* "$prefix"/lib/libheif.so* /runtime/lib/
doc=/runtime/share/doc/pixstride/codecs
cp aom/LICENSE "$doc/AOM-LICENSE"
cp aom/PATENTS "$doc/AOM-PATENTS"
cp libheif-1.23.7/COPYING "$doc/libheif-COPYING"
cp /usr/share/common-licenses/GPL-3 "$doc/GPL-3"
# Include the exact source archives and their hashes with the dynamically linked
# libraries, so their corresponding source remains available with every image.
cp aom.tar.gz libheif.tar.gz SHA256SUMS "$doc/"
printf 'AOM commit: %s\nAOM source: %s\nlibheif source: %s\n' \
    "$aom_commit" "$aom_url" "$heif_url" > "$doc/SOURCES"
