# Additional third-party notices

The separate allocator diagnostic probes use the C mimalloc 3.3.2 source bundled with development dependency [libmimalloc-sys 0.1.49](https://crates.io/crates/libmimalloc-sys/0.1.49), under the MIT license. The service retains System allocation and does not link these components in its release dependency graph. This file accompanies diagnostic images in `/usr/share/doc/pixstride/`. The wrapper notice below also retains the attribution for historical mimalloc development comparisons.

## Native mimalloc 3.3.2

```text
MIT License

Copyright (c) 2018-2025 Microsoft Corporation, Daan Leijen

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## Rust mimalloc and libmimalloc-sys wrappers

```text
Copyright 2019 Octavian Oncescu

Permission is hereby granted, free of charge, to any
person obtaining a copy of this software and associated
documentation files (the "Software"), to deal in the
Software without restriction, including without
limitation the rights to use, copy, modify, merge,
publish, distribute, sublicense, and/or sell copies of
the Software, and to permit persons to whom the Software
is furnished to do so, subject to the following
conditions:

The above copyright notice and this permission notice
shall be included in all copies or substantial portions
of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF
ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED
TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT
SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR
IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
DEALINGS IN THE SOFTWARE.
```

## Native AVIF and HEIC codec libraries

The Docker image dynamically links unmodified libheif 1.23.7 (LGPL-3.0) and
Alliance for Open Media libaom 3.15.3 (BSD-2-Clause and AOM Patent License 1.0).
Their exact source archives, SHA256 checksums, license and patent texts, source
URLs and rebuild script are included under `/usr/share/doc/pixstride/codecs/`.
The build script also remains available in this repository at
`scripts/build_native_codecs.sh`. Headers and the build SDK are excluded from
the runtime image. Replaceable shared libraries live in `/opt/pixstride-codecs/lib/`.

- libheif source: https://github.com/strukturag/libheif/releases/tag/v1.23.7
- AOM source commit: https://aomedia.googlesource.com/aom/+/b141d9c45db236335351c8a4b2a14b3ade27c937

libheif registers its AOM AV1 and x265/libde265 HEVC backends internally;
external libheif plugin loading is disabled to avoid mixing plugin ABIs.
The x265, libde265, libsharpyuv shared libraries remain Ubuntu
packages, with their distribution copyright and license notices retained under
`/usr/share/doc/`. These libraries are separate from Pixstride's MIT-licensed
Rust implementation.

## Native libvips and Quantizr

The Docker image dynamically links unmodified libvips 8.18.7 (LGPL-2.1) and
Quantizr 1.4.3 (MIT, Sergey Alexandrovich). Quantizr supplies the general PNG
palette quantization backend. libvips and its modules are rebuilt together;
all native format capabilities remain checked before packaging.

Exact corresponding source archives, checksums, license texts, upstream source
URLs and `scripts/build_native_vips.sh` are included under
`/usr/share/doc/pixstride/vips/`. Development headers and tools stay in build
stages. Runtime libraries and modules live in `/opt/pixstride-codecs/lib/`.

- libvips source: https://github.com/libvips/libvips/releases/tag/v8.18.7
- Quantizr source commit: https://github.com/DarthSim/quantizr/tree/cfb26aaf3039ac1179d42a66cc7988c8c6feeba9
