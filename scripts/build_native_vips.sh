#!/bin/sh
# Immutable upstream vips/quantizr sources; keep the SDK separate from runtime.
set -eu
prefix=${1:-/opt/pixstride-codecs}
jobs=${PIXSTRIDE_CODEC_BUILD_JOBS:-2}
case "$jobs" in 1|2) ;; *) echo 'native build jobs must be 1 or 2' >&2; exit 1 ;; esac
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT HUP INT TERM
cd "$work"
quantizr_commit=cfb26aaf3039ac1179d42a66cc7988c8c6feeba9
quantizr_url=https://github.com/DarthSim/quantizr.git
vips_url=https://github.com/libvips/libvips/releases/download/v8.18.7/vips-8.18.7.tar.xz
git init -q quantizr-git
git -C quantizr-git fetch --depth 1 "$quantizr_url" "$quantizr_commit"
test "$(git -C quantizr-git rev-parse 'FETCH_HEAD^{commit}')" = "$quantizr_commit"
git -C quantizr-git fsck --strict --no-reflogs
git -C quantizr-git archive --format=tar --output="$work/quantizr.tar" "$quantizr_commit"
gzip -n quantizr.tar
curl --fail --location --retry 3 --proto '=https' --tlsv1.2 "$vips_url" -o vips.tar.xz
printf '%s  %s\n' 5baaead3b0bb20ffdb9e9ff09aa9fda08620923df77b63b436654cb5e0b3bf94 vips.tar.xz | sha256sum --check
mkdir quantizr vips
tar xzf quantizr.tar.gz -C quantizr
tar xf vips.tar.xz --strip-components=1 -C vips
test -f vips/LICENSE && test -f quantizr/LICENSE
python3 - <<'PY'
from pathlib import Path
import tomllib
assert tomllib.loads(Path('quantizr/Cargo.toml').read_text())['package']['version'] == '1.4.3'
PY
mkdir -p "$prefix/lib/pkgconfig" "$prefix/include"
cd quantizr
# Upstream has no Rust dependencies. Build its documented C API directly with
# the pinned Rust toolchain, avoiding an additional cargo-c dependency graph.
rustc --edition=2024 --crate-name=quantizr --crate-type=cdylib --cfg 'feature="capi"' \
    -C opt-level=3 -C lto=true -C codegen-units=1 -C strip=symbols \
    --remap-path-prefix="$work=/usr/src/pixstride-native-vips" \
    -C link-arg=-Wl,-soname,libquantizr.so.1 src/lib.rs -o "$prefix/lib/libquantizr.so.1.4.3"
ln -sf libquantizr.so.1.4.3 "$prefix/lib/libquantizr.so.1"
ln -sf libquantizr.so.1 "$prefix/lib/libquantizr.so"
cbindgen --config cbindgen.toml --output "$prefix/include/quantizr.h"
cat > "$prefix/lib/pkgconfig/quantizr.pc" <<EOF
prefix=$prefix
libdir=\${prefix}/lib
includedir=\${prefix}/include
Name: quantizr
Description: Quantizr RGBA palette quantization
Version: 1.4.3
Libs: -L\${libdir} -lquantizr
Cflags: -I\${includedir}
EOF
cd "$work"
export PKG_CONFIG_PATH="$prefix/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
export LD_LIBRARY_PATH="$prefix/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
flags="-D_FORTIFY_SOURCE=3 -fstack-protector-strong -ffile-prefix-map=$work=/usr/src/pixstride-native-vips"
meson setup vips-build vips --prefix="$prefix" --libdir=lib --buildtype=release \
    "-Dc_args=$flags" "-Dcpp_args=$flags" \
    -Dimagequant=disabled -Dquantizr=enabled -Dmodules=enabled \
    -Dheif=enabled -Djpeg-xl=enabled -Dpng=enabled -Djpeg=enabled -Dwebp=enabled -Dtiff=enabled -Drsvg=enabled \
    -Dintrospection=disabled -Dexamples=false -Dcplusplus=false -Ddocs=false
meson compile -C vips-build -j "$jobs"
meson install -C vips-build
python3 - "$prefix/lib/pixstride-vips-stack.json" <<'PY'
import json,sys
from pathlib import Path
Path(sys.argv[1]).write_text(json.dumps({'libvips':'8.18.7','quantizr':'1.4.3','quantizr_commit':'cfb26aaf3039ac1179d42a66cc7988c8c6feeba9','vips_source_sha256':'5baaead3b0bb20ffdb9e9ff09aa9fda08620923df77b63b436654cb5e0b3bf94'})+'\n')
PY
mkdir -p /vips-runtime/lib /vips-runtime/share/doc/pixstride/vips
cp -a "$prefix"/lib/libvips.so* "$prefix"/lib/libquantizr.so* "$prefix/lib/pixstride-vips-stack.json" /vips-runtime/lib/
cp -a "$prefix/lib/vips-modules-8.18" /vips-runtime/lib/
doc=/vips-runtime/share/doc/pixstride/vips
cp quantizr/LICENSE "$doc/quantizr-LICENSE"
cp vips/LICENSE "$doc/libvips-LICENSE"
cp /usr/share/common-licenses/GPL-2 "$doc/GPL-2"
cp quantizr.tar.gz vips.tar.xz "$doc/"
sha256sum quantizr.tar.gz vips.tar.xz > "$doc/SHA256SUMS"
printf 'Quantizr commit: %s\nQuantizr source: %s\nlibvips source: %s\n' "$quantizr_commit" "$quantizr_url" "$vips_url" > "$doc/SOURCES"
