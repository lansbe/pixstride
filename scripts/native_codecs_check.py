#!/usr/bin/env python3
"""Fail closed unless the loaded Linux codec stack supports real AVIF/HEIC roundtrips."""
import argparse
import ctypes as c
import hashlib
import json
from pathlib import Path
import sys


def function(lib, name, restype, *argtypes):
    method = getattr(lib, name)
    method.restype = restype
    method.argtypes = list(argtypes)
    return method



def palette_probe(vips, unref, free):
    """Independent synthetic RGB result, sensitive to the actual quantizer backend."""
    data = bytes(value for y in range(64) for x in range(64) for value in
                 [(x * 71 + y * 29) % 256, (x * 13 + y * 97) % 256, (x * 53 + y * 17) % 256])
    raw = function(vips, 'vips_image_new_from_memory_copy', c.c_void_p,
                   c.c_void_p, c.c_size_t, c.c_int, c.c_int, c.c_int, c.c_int)(data, len(data), 64, 64, 3, 0)
    typed, encoded, length = c.c_void_p(), c.c_void_p(), c.c_size_t()
    decoded = memory = None
    try:
        if not raw or function(vips, 'vips_copy', c.c_int, c.c_void_p, c.POINTER(c.c_void_p))(
                raw, c.byref(typed), b'interpretation', c.c_int(22), None):
            raise ValueError('palette fixture creation failed')
        if function(vips, 'vips_pngsave_buffer', c.c_int, c.c_void_p,
                    c.POINTER(c.c_void_p), c.POINTER(c.c_size_t))(
                typed, c.byref(encoded), c.byref(length), b'palette', c.c_int(1),
                b'bitdepth', c.c_int(8), b'filter', c.c_int(0), b'keep', c.c_int(0), None):
            raise ValueError('palette PNG encoding failed')
        decoded = function(vips, 'vips_image_new_from_buffer', c.c_void_p,
                           c.c_void_p, c.c_size_t, c.c_char_p)(encoded, length.value, b'', None)
        size = c.c_size_t()
        if decoded:
            memory = function(vips, 'vips_image_write_to_memory', c.c_void_p,
                              c.c_void_p, c.POINTER(c.c_size_t))(decoded, c.byref(size))
        if not memory or size.value != 64 * 64 * 3:
            raise ValueError('palette RGB materialization failed')
        digest = hashlib.sha256(c.string_at(memory, size.value)).hexdigest()
        expected = 'a38c850f0ff23a0b3856bd6ee036c7c282d6b1f1701fb42231b2c26f9c2d2618'
        if digest != expected:
            raise ValueError(f'palette RGB differs from pinned quantizr: {digest}')
        return {'rgb_sha256': digest, 'rgb_bytes': size.value}
    finally:
        if memory:
            free(memory)
        if decoded:
            unref(decoded)
        if typed:
            unref(typed)
        if raw:
            unref(raw)
        function(vips, 'vips_thread_shutdown', None)()
        if encoded:
            free(encoded)


def check(prefix):
    root = prefix.resolve()
    for name in ['libaom.so.3', 'libheif.so.1', 'libvips.so.42', 'libquantizr.so.1']:
        if not (root / 'lib' / name).is_file():
            raise ValueError(f'missing private codec: {name}')
    # Use the service's SONAME search path, rather than absolute-path loading
    # that could hide a missing LD_LIBRARY_PATH and validate the wrong runtime.
    aom = c.CDLL('libaom.so.3', mode=c.RTLD_GLOBAL)
    heif = c.CDLL('libheif.so.1', mode=c.RTLD_GLOBAL)
    c.CDLL('libquantizr.so.1', mode=c.RTLD_GLOBAL)
    stack = json.loads((root / 'lib/pixstride-vips-stack.json').read_text())
    if stack.get('quantizr') != '1.4.3' or stack.get('quantizr_commit') != 'cfb26aaf3039ac1179d42a66cc7988c8c6feeba9':
        raise ValueError('wrong quantizr source identity')
    if stack.get('libvips') != '8.18.7' or stack.get('vips_source_sha256') != '5baaead3b0bb20ffdb9e9ff09aa9fda08620923df77b63b436654cb5e0b3bf94':
        raise ValueError('wrong libvips source identity')
    vips = c.CDLL('libvips.so.42')
    versions = {
        'libvips': function(vips, 'vips_version_string', c.c_char_p)().decode(),
        'libheif': function(heif, 'heif_get_version', c.c_char_p)().decode(),
        'aom': function(aom, 'aom_codec_version_str', c.c_char_p)().decode(),
        'quantizr': stack['quantizr'],
    }
    expected = {'libvips': '8.18.7', 'libheif': '1.23.7', 'aom': 'v3.15.3', 'quantizr': '1.4.3'}
    if versions != expected:
        raise ValueError(f'loaded versions {versions}, expected {expected}')
    if function(vips, 'vips_init', c.c_int, c.c_char_p)(b'pixstride-codec-check'):
        raise ValueError('libvips initialization failed')
    function(vips, 'vips_concurrency_set', None, c.c_int)(1)
    function(vips, 'vips_cache_set_max', None, c.c_int)(0)
    descriptors = function(heif, 'heif_get_encoder_descriptors', c.c_int,
                           c.c_int, c.c_char_p, c.POINTER(c.c_void_p), c.c_int)
    descriptor_id = function(heif, 'heif_encoder_descriptor_get_id_name', c.c_char_p, c.c_void_p)
    encoders = {}
    for name, compression, required in [('avif', 4, 'aom'), ('heic', 1, 'x265')]:
        count = descriptors(compression, None, None, 0)
        pointers = (c.c_void_p * count)()
        descriptors(compression, None, pointers, count)
        encoders[name] = [descriptor_id(p).decode() for p in pointers]
        if required not in encoders[name]:
            raise ValueError(f'missing {required} encoder: {encoders[name]}')
        if not function(heif, 'heif_have_decoder_for_format', c.c_int, c.c_int)(compression):
            raise ValueError(f'missing {name} decoder')
    black = function(vips, 'vips_black', c.c_int, c.POINTER(c.c_void_p), c.c_int, c.c_int)
    save = function(vips, 'vips_heifsave_buffer', c.c_int,
                    c.c_void_p, c.POINTER(c.c_void_p), c.POINTER(c.c_size_t))
    load = function(vips, 'vips_image_new_from_buffer', c.c_void_p,
                    c.c_void_p, c.c_size_t, c.c_char_p)
    export = function(vips, 'vips_image_write_to_memory', c.c_void_p,
                      c.c_void_p, c.POINTER(c.c_size_t))
    unref = function(c.CDLL('libgobject-2.0.so.0'), 'g_object_unref', None, c.c_void_p)
    free = function(c.CDLL('libglib-2.0.so.0'), 'g_free', None, c.c_void_p)
    error = function(vips, 'vips_error_buffer', c.c_char_p)
    image = c.c_void_p()
    if black(c.byref(image), 64, 64, None):
        raise ValueError(error().decode())
    roundtrips = []
    try:
        for name, compression in [('avif', 4), ('heic', 1)]:
            encoded = c.c_void_p()
            length = c.c_size_t()
            if save(image, c.byref(encoded), c.byref(length), b'compression',
                    c.c_int(compression), b'Q', c.c_int(65), b'effort', c.c_int(1), None):
                raise ValueError(f'{name} encode failed: {error().decode()}')
            decoded = None
            try:
                decoded = load(encoded, length.value, b'', None)
                if not decoded:
                    raise ValueError(f'{name} decode failed: {error().decode()}')
                size = c.c_size_t()
                pixels = export(decoded, c.byref(size))
                if not pixels or size.value == 0:
                    raise ValueError(f'{name} materialization failed: {error().decode()}')
                free(pixels)
                width = function(vips, 'vips_image_get_width', c.c_int, c.c_void_p)(decoded)
                height = function(vips, 'vips_image_get_height', c.c_int, c.c_void_p)(decoded)
                if (width, height) != (64, 64):
                    raise ValueError(f'{name} dimensions changed to {width}x{height}')
                roundtrips.append(name)
            finally:
                if decoded:
                    unref(decoded)
                free(encoded)
    finally:
        unref(image)
    palette = palette_probe(vips, unref, free)
    libraries = {}
    for line in Path('/proc/self/maps').read_text().splitlines():
        path = line.split()[-1]
        if not path.startswith('/'):
            continue
        file = Path(path)
        if any(part in file.name for part in ['libvips.so', 'libheif', 'libaom.so', 'libquantizr.so', 'libimagequant.so', 'vips-']):
            resolved = file.resolve()
            if file.name.startswith('libimagequant.so'):
                raise ValueError('unexpected imagequant backend loaded')
            if file.name.startswith(('libheif-', 'libheif_')):
                raise ValueError(f'unexpected external libheif plugin: {resolved}')
            if file.name.startswith(('libvips.so', 'libheif.so', 'libaom.so', 'libquantizr.so', 'vips-')) and not resolved.is_relative_to(root):
                raise ValueError(f'codec resolved outside the private prefix: {resolved}')
            libraries[str(resolved)] = hashlib.sha256(resolved.read_bytes()).hexdigest()
    return {'versions': versions, 'encoders': encoders, 'roundtrips': roundtrips, 'palette_probe': palette, 'libraries': libraries}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--codec-prefix', type=Path, default=Path('/opt/pixstride-codecs'))
    args = parser.parse_args()
    try:
        result = check(args.codec_prefix)
    except (OSError, ValueError, AttributeError) as error:
        print(f'codec stack rejected: {error}', file=sys.stderr)
        return 1
    print(json.dumps(result, sort_keys=True))
    return 0


if __name__ == '__main__':
    sys.exit(main())
