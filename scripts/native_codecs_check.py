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


def check(prefix):
    root = prefix.resolve()
    for name in ['libaom.so.3', 'libheif.so.1']:
        if not (root / 'lib' / name).is_file():
            raise ValueError(f'missing private codec: {name}')
    # Use the service's SONAME search path, rather than absolute-path loading
    # that could hide a missing LD_LIBRARY_PATH and validate the wrong runtime.
    aom = c.CDLL('libaom.so.3', mode=c.RTLD_GLOBAL)
    heif = c.CDLL('libheif.so.1', mode=c.RTLD_GLOBAL)
    vips = c.CDLL('libvips.so.42')
    versions = {
        'libvips': function(vips, 'vips_version_string', c.c_char_p)().decode(),
        'libheif': function(heif, 'heif_get_version', c.c_char_p)().decode(),
        'aom': function(aom, 'aom_codec_version_str', c.c_char_p)().decode(),
    }
    expected = {'libvips': '8.18.0', 'libheif': '1.23.7', 'aom': 'v3.15.3'}
    if versions != expected:
        raise ValueError(f'loaded versions {versions}, expected {expected}')
    if function(vips, 'vips_init', c.c_int, c.c_char_p)(b'pixstride-codec-check'):
        raise ValueError('libvips initialization failed')
    function(vips, 'vips_concurrency_set', None, c.c_int)(1)
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
    libraries = {}
    for line in Path('/proc/self/maps').read_text().splitlines():
        path = line.split()[-1]
        if not path.startswith('/'):
            continue
        file = Path(path)
        if any(part in file.name for part in ['libvips.so', 'libheif', 'libaom.so']):
            resolved = file.resolve()
            if file.name.startswith(('libheif-', 'libheif_')):
                raise ValueError(f'unexpected external libheif plugin: {resolved}')
            if file.name.startswith(('libheif.so', 'libaom.so')) and not resolved.is_relative_to(root):
                raise ValueError(f'codec resolved outside the private prefix: {resolved}')
            libraries[str(resolved)] = hashlib.sha256(resolved.read_bytes()).hexdigest()
    return {'versions': versions, 'encoders': encoders, 'roundtrips': roundtrips, 'libraries': libraries}


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
