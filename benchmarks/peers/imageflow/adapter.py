"""Translate the frozen imgproxy recipes to the Imageflow.Server 0.9 HTTP API."""

from urllib.parse import quote, urlencode


ANCHORS = {
    'ce': 'middlecenter', 'noea': 'topright', 'soea': 'bottomright',
    'sowe': 'bottomleft', 'nowe': 'topleft',
}
UNSUPPORTED = {
    'padding': 'Imageflow cannot add four independent margins after an exact-size stretch',
    'blur': 'Imageflow has no Gaussian blur command',
    'sharpen': 'Imageflow resampling sharpness is a percentage, not a Gaussian sigma',
    'double-filter': 'Imageflow has no Gaussian blur followed by sigma-based sharpen',
    'smart-crop': 'Imageflow has no attention-based crop',
    'pixelate': 'Imageflow has no block pixelation command',
    'long-chain': 'Imageflow cannot express the blur, sharpen, and padding chain',
}


def crop_rectangle(size, fields):
    width, height, gravity, *offsets = fields
    axes = {
        'ce': ('center', 'center'), 'noea': ('end', 'start'), 'soea': ('end', 'end'),
        'sowe': ('start', 'end'), 'nowe': ('start', 'start'),
    }
    origins, ends = [], []
    for length, value, side, offset in zip(size, (width, height), axes[gravity], offsets or (0, 0)):
        value = float(value)
        extent = round(value * length if value < 1 else value)
        space = length - extent
        origin = {'start': 0, 'end': space, 'center': round((space + 1) / 2)}[side]
        offset = float(offset)
        shift = round(offset * length if abs(offset) < 1 else offset)
        origin = min(space, max(0, origin - shift if side == 'end' else origin + shift))
        origins.append(origin)
        ends.append(origin + extent)
    return ','.join(map(str, origins + ends))


def path(case):
    family, output = case['family'], case['format']
    if output not in ('jpg', 'png', 'webp'):
        raise NotImplementedError(f'Imageflow 2.1.1-rc10 has no {output.upper()} encoder')
    source = case['source']
    if source.rsplit('.', 1)[-1] not in ('jpg', 'png', 'gif', 'webp'):
        raise NotImplementedError(f'Imageflow.Server cannot process {case["input_kind"]} sources')
    if family in UNSUPPORTED:
        raise NotImplementedError(UNSUPPORTED[family])

    options = dict((parts[0], parts[1:]) for parts in
                   (option.split(':') for option in case['options'].split('/')))
    query = {'format': output}
    if 'rs' in options:
        mode, width, height = options['rs'][:3]
        query.update(w=int(width), h=int(height), mode={'fit': 'max', 'fill': 'crop', 'force': 'stretch'}[mode])
        # Match the catalog reference's three-lobe Lanczos resampling in sRGB.
        query.update({'down.filter': 'lanczos', 'down.colorspace': 'srgb',
                      'up.filter': 'lanczos', 'up.colorspace': 'srgb'})
    if 'c' in options:
        query['crop'] = crop_rectangle(case['source_dimensions'], options['c'])
    if 'g' in options:
        query['anchor'] = ANCHORS[options['g'][0]]
    if 'ex' in options:
        query.update(mode='pad', anchor=ANCHORS[options['ex'][1]])
    if 'bg' in options:
        query['bgcolor'] = options['bg'][0]
    if 'rot' in options:
        query['srotate'] = options['rot'][0]
    if 'fl' in options:
        query['sflip'] = {('1', '0'): 'x', ('0', '1'): 'y', ('1', '1'): 'xy'}[tuple(options['fl'])]
    if 'dpr' in options:
        query['dpr'] = options['dpr'][0]
    if 'z' in options:
        # The frozen zoom recipe stretches independently along the two axes.
        zx, zy = map(float, options['z'])
        query.update(w=round(query['w'] * zx), h=round(query['h'] * zy))
    if 'q' in options:
        query['quality'] = options['q'][0]
    if output == 'png':
        query['png.lossless'] = 'true'
    return '/images/' + quote(source, safe='') + '?' + urlencode(query)
