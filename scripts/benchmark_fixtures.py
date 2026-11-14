"""Shared, checksum-pinned public corpus; preparation is outside timed work."""
import argparse
import hashlib
import io
import json
from pathlib import Path
import re
import time
import urllib.error
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
PILLOW_VERSION = '13.0.0'
SOURCES = [
    {'name': 'landscape.jpg',
     'url': 'https://upload.wikimedia.org/wikipedia/commons/3/3f/Fronalpstock_big.jpg',
     'page': 'https://commons.wikimedia.org/wiki/File:Fronalpstock_big.jpg',
     'sha256': '24eb29eccdf0af691b406d1a3d22c0ef5d761cc454d8a917848c33958f6fc857',
     'credit': 'Fronalpstock, Hannes Roest (Hannes Röst)', 'license': 'CC BY-SA 3.0',
     'license_url': 'https://creativecommons.org/licenses/by-sa/3.0/'},
    {'name': 'earth.jpg',
     'url': 'https://upload.wikimedia.org/wikipedia/commons/9/97/The_Earth_seen_from_Apollo_17.jpg',
     'page': 'https://commons.wikimedia.org/wiki/File:The_Earth_seen_from_Apollo_17.jpg',
     'sha256': '51049ed4e34514880bfbe39d34e2dd2c56e31f55919e7f4f0b34ef1698cb9f50',
     'credit': 'The Blue Marble, NASA / Apollo 17 crew', 'license': 'Public domain (US NASA work)',
     'license_url': 'https://www.nasa.gov/nasa-brand-center/images-and-media/'},
]
MIMES = {'jpg': 'image/jpeg', 'png': 'image/png', 'webp': 'image/webp', 'bmp': 'image/bmp'}


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def corpus_hash(manifest):
    return sha256(json.dumps(manifest['files'], sort_keys=True, separators=(',', ':')).encode())


def verify_fixtures(root):
    root = Path(root)
    manifest = json.loads((root / 'manifest.json').read_text())
    if manifest.get('schema_version') != 1 or not manifest.get('files'):
        raise ValueError('Unsupported or empty fixture manifest')
    for name, metadata in manifest['files'].items():
        if not re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9_.-]*', name):
            raise ValueError('Fixture names must be plain filenames')
        path = root / name
        if path.is_symlink() or not path.is_file() or path.stat().st_size > 64 * 1024 * 1024:
            raise ValueError('Fixture must be a bounded regular file')
        data = path.read_bytes()
        if len(data) != metadata['bytes'] or sha256(data) != metadata['sha256']:
            raise ValueError('Fixture checksum mismatch: ' + name)
    if manifest.get('corpus_sha256', corpus_hash(manifest)) != corpus_hash(manifest):
        raise ValueError('Fixture manifest digest mismatch')
    return manifest


def load_cases(path, files):
    document = json.loads(Path(path).read_text())
    cases = document.get('cases', [])
    if document.get('schema_version') != 1 or len(cases) != 25:
        raise ValueError('The full protocol requires exactly 25 cases')
    ids = set()
    for case in cases:
        if not re.fullmatch(r'[a-z0-9][a-z0-9-]*', case['id']) or case['id'] in ids:
            raise ValueError('Cases require distinct plain identifiers')
        ids.add(case['id'])
        if case['source'] not in files or case['format'] not in MIMES:
            raise ValueError('Case refers to an unavailable fixture or format')
        if len(case['dimensions']) != 2 or any(type(v) is not int or v < 1 for v in case['dimensions']):
            raise ValueError('Cases require literal positive output dimensions')
        if len(case['options']) > 4096 or not re.fullmatch(r'[A-Za-z0-9_:.\-/]*', case['options']):
            raise ValueError('Invalid benchmark processing options')
    return cases


def download(source, cache):
    path = cache / source['name']
    if path.exists() and sha256(path.read_bytes()) == source['sha256']:
        return path
    for attempt in range(3):
        try:
            request = urllib.request.Request(source['url'], headers={
                'User-Agent': 'PixstrideBenchmark/1.0 (https://github.com/lansbe/pixstride)'})
            with urllib.request.urlopen(request, timeout=45) as response:
                data = response.read(32 * 1024 * 1024 + 1)
            if len(data) > 32 * 1024 * 1024 or sha256(data) != source['sha256']:
                raise ValueError('Public source checksum or size mismatch: ' + source['name'])
            path.write_bytes(data)
            return path
        except (OSError, urllib.error.URLError):
            if attempt == 2:
                raise
            time.sleep(2 ** attempt)


def normalized_photo(path):
    from PIL import Image, ImageCms, ImageOps
    with Image.open(path) as original:
        oriented = ImageOps.exif_transpose(original)
        profile = original.info.get('icc_profile')
        if profile:
            converted = ImageCms.profileToProfile(oriented, ImageCms.ImageCmsProfile(io.BytesIO(profile)),
                                                  ImageCms.createProfile('sRGB'), outputMode='RGB')
        else:
            converted = oriented.convert('RGB')
        # Pillow propagates info through resize/convert; an RGB profile on L is invalid.
        converted.info.clear()
        return converted


def prepare(root, cache):
    import PIL
    from PIL import Image, ImageDraw, ImageOps, features
    if PIL.__version__ != PILLOW_VERSION:
        raise ValueError('Install the pinned Pillow==' + PILLOW_VERSION)
    root, cache = Path(root), Path(cache)
    root.mkdir(parents=True, exist_ok=True)
    cache.mkdir(parents=True, exist_ok=True)
    images = {}
    for source in SOURCES:
        images[source['name']] = normalized_photo(download(source, cache))
    landscape = images['landscape.jpg'].resize((2048, 920), Image.Resampling.LANCZOS)
    earth = ImageOps.fit(images['earth.jpg'], (1600, 1600), Image.Resampling.LANCZOS)
    landscape.save(root / 'landscape.jpg', quality=90, subsampling=0, optimize=False)
    landscape.save(root / 'landscape.png', compress_level=6)
    landscape.save(root / 'progressive.jpg', quality=90, subsampling=0, progressive=True)
    earth.save(root / 'earth.jpg', quality=90, subsampling=0)
    earth.save(root / 'earth.tiff', compression='raw')
    alpha = Image.new('RGBA', (1024, 768), (0, 0, 0, 0))
    draw = ImageDraw.Draw(alpha)
    for i in range(80):
        x, y = (i * 149) % 900, (i * 97) % 640
        draw.ellipse((x, y, x + 125, y + 125), fill=((i * 53) % 256, (i * 97) % 256, 190, 32 + (i * 7) % 224))
    alpha.save(root / 'alpha.png')
    graphic = Image.new('RGB', (2048, 1536), '#f2f4f8')
    draw = ImageDraw.Draw(graphic)
    for i in range(128):
        x, y = (i * 157) % 1900, (i * 83) % 1400
        draw.rectangle((x, y, x + 147, y + 91), fill=((i * 41) % 256, (i * 67) % 256, (i * 103) % 256))
    graphic.save(root / 'graphics.png')
    landscape.resize((1024, 768), Image.Resampling.LANCZOS).convert('L').save(root / 'gray.png')
    rectangles = ''.join(f'<rect x="{i*73%900}" y="{i*61%640}" width="124" height="96" fill="#{i*53271%16777216:06x}"/>' for i in range(80))
    (root / 'chart.svg').write_text('<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="768"><rect width="1024" height="768" fill="#f2f4f8"/>' + rectangles + '</svg>')
    names = ['landscape.jpg', 'landscape.png', 'progressive.jpg', 'earth.jpg', 'earth.tiff',
             'alpha.png', 'graphics.png', 'gray.png', 'chart.svg']
    manifest = {'schema_version': 1, 'sources': SOURCES, 'files': {},
                'generator': {'pillow': PIL.__version__, 'jpeg': features.version_codec('jpg'),
                              'zlib': features.version_codec('zlib')},
                'changes': 'Photos converted to sRGB, resized/cropped and re-encoded without metadata; synthetic graphics generated deterministically.'}
    for name in names:
        path = root / name
        if name.endswith('.svg'):
            dimensions = [1024, 768]
        else:
            with Image.open(path) as image:
                dimensions = list(image.size)
        data = path.read_bytes()
        path.chmod(0o644)
        manifest['files'][name] = {'bytes': len(data), 'sha256': sha256(data), 'dimensions': dimensions}
    manifest['corpus_sha256'] = corpus_hash(manifest)
    (root / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
    attribution = '# Benchmark corpus attribution\n\n' + manifest['changes'] + '\n\n'
    for source in SOURCES:
        attribution += f"- {source['credit']}: {source['page']}; {source['license']}, {source['license_url']}\n"
    attribution += '\nFronalpstock derivatives remain CC BY-SA 3.0. Synthetic fixtures: MIT, like Pixstride. Media are not part of the source repository.\n'
    (root / 'ATTRIBUTION.md').write_text(attribution)
    root.chmod(0o755)
    verify_fixtures(root)
    print('Prepared shared corpus ' + manifest['corpus_sha256'])
    return manifest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--cache', type=Path, required=True)
    args = parser.parse_args()
    prepare(args.output, args.cache)


if __name__ == '__main__':
    main()
