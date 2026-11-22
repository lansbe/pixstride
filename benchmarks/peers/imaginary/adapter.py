"""Translate the frozen workloads to Imaginary v1.2.4's local-file HTTP API."""
import json
from urllib.parse import urlencode


def position(gravity, xspace, yspace):
    if gravity == 'ce':
        return (min(xspace, round((xspace + 1) / 2)),
                min(yspace, round((yspace + 1) / 2)))
    return (xspace if gravity.endswith('ea') else 0,
            yspace if gravity.startswith('so') else 0)


def extract(left, top, width, height):
    return 'extract', dict(left=left, top=top, areawidth=width, areaheight=height)


def fill(source_size, size, gravity):
    width, height = size
    if gravity == 'sm':
        return 'smartcrop', dict(width=width, height=height)
    # Only one axis overflows when filling. Imaginary accepts cardinal gravity.
    if gravity == 'ce':
        side = 'center'
    elif source_size[0] * height > width * source_size[1]:
        side = 'east' if gravity.endswith('ea') else 'west'
    else:
        side = 'south' if gravity.startswith('so') else 'north'
    return 'crop', dict(width=width, height=height, gravity=side)


def padding(width, height, left, top, right, bottom, color):
    # Imaginary centers an embed and resizes to fit its box. Extend one axis
    # at a time to preserve the content size, then extract asymmetric margins.
    dx, dy = max(left, right), max(top, bottom)
    box_width = width + 2 * dx
    background = dict(extend='background', background=color)
    steps = []
    if dx:
        steps.append(('resize', dict(width=box_width, height=height, **background)))
    if dy:
        steps.append(('resize', dict(width=box_width, height=height + 2 * dy, **background)))
    if left != right or top != bottom:
        steps.append(extract(dx - left, dy - top, width + left + right, height + top + bottom))
    return steps


def path(case):
    output = case['format']
    if output not in ('jpg', 'png', 'webp'):
        raise NotImplementedError(f'Imaginary v1.2.4 has no {output.upper()} output type')
    if case['source'].endswith('.bmp'):
        raise NotImplementedError('Imaginary v1.2.4 rejects BMP input MIME types')
    options = dict((key, values) for key, *values in
                   (item.split(':') for item in case['options'].split('/')))
    if 'sh' in options:
        raise NotImplementedError('Imaginary v1.2.4 does not expose the bimg sharpening filter')
    if 'pix' in options:
        raise NotImplementedError('Imaginary v1.2.4 does not expose block-average pixelation')

    src_width, src_height = case['source_dimensions']
    steps = []
    if 'c' in options:
        crop_w, crop_h, gravity, *offsets = options['c']
        width = round(src_width * float(crop_w)) if float(crop_w) < 1 else int(crop_w)
        height = round(src_height * float(crop_h)) if float(crop_h) < 1 else int(crop_h)
        left, top = position(gravity, src_width - width, src_height - height)
        if offsets:  # The catalog's southeast offsets are measured inward.
            left -= round(src_width * float(offsets[0]))
            top -= round(src_height * float(offsets[1]))
        steps.append(extract(left, top, width, height))
        src_width, src_height = width, height

    color = None
    if 'bg' in options:
        color = ','.join(str(channel) for channel in bytes.fromhex(options['bg'][0]))
    if 'rs' in options:
        mode, width, height, *_ = options['rs']
        width, height = int(width), int(height)
        if mode == 'fill':
            steps.append(fill((src_width, src_height), (width, height), options['g'][0]))
        elif 'ex' in options:
            scale = min(width / src_width, height / src_height)
            fit_w, fit_h = round(src_width * scale), round(src_height * scale)
            dx, dy = width - fit_w, height - fit_h
            left, top = position(options['ex'][1], dx, dy)
            steps.append(('resize', dict(width=fit_w, height=fit_h, force=True)))
            steps.extend(padding(fit_w, fit_h, left, top, dx - left, dy - top, color))
        elif mode == 'fit':
            steps.append(('fit', dict(width=width, height=height)))
        else:
            dpr = int(options.get('dpr', ['1'])[0])
            zx, zy = map(int, options.get('z', ['1', '1']))
            width, height = width * dpr * zx, height * dpr * zy
            steps.append(('resize', dict(width=width, height=height, force=True)))
        if 'bl' in options:
            steps[-1][1]['sigma'] = float(options['bl'][0])
        if 'pd' in options:
            top, right, bottom, left = map(int, options['pd'])
            steps.extend(padding(width, height, left, top, right, bottom, color))
    elif 'rot' in options:
        steps.append(('rotate', dict(rotate=int(options['rot'][0]))))
    elif 'fl' in options:
        horizontal, vertical = options['fl']
        if horizontal == '1':
            steps.append(('flip', dict(flop=vertical == '1')))
        else:
            steps.append(('flop', {}))
    else:
        raise NotImplementedError('Imaginary has no mapping for this recipe')

    final = steps[-1][1]
    final['type'] = 'jpeg' if output == 'jpg' else output
    if 'q' in options:
        final['quality'] = int(options['q'][0])
    if color or output == 'jpg':
        final['background'] = color or '255,255,255'
    query = dict(file=case['source'])
    if len(steps) == 1:
        endpoint, params = steps[0]
        query.update({key: str(value).lower() if isinstance(value, bool) else value
                      for key, value in params.items()})
    else:
        # Pipeline steps in Imaginary encode and decode between operations.
        # Preserve intermediate pixels and alpha with a lossless PNG.
        for _, params in steps[:-1]:
            params.update(type='png', compression=1)
        endpoint = 'pipeline'
        query['operations'] = json.dumps([dict(operation=name, params=params)
                                          for name, params in steps], separators=(',', ':'))
    return '/' + endpoint + '?' + urlencode(query)
