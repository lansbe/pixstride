"""Translate the frozen imgproxy recipes to the weserv/images 5.1.2 HTTP filter."""

from urllib.parse import quote, urlencode


POSITIONS = {
    'ce': 'center', 'noea': 'top-right', 'soea': 'bottom-right',
    'sowe': 'bottom-left', 'nowe': 'top-left', 'sm': 'attention',
}
UNSUPPORTED = {
    'padding': 'weserv has no independent four-sided padding after a stretch',
    'double-filter': 'weserv applies sharpen before blur and cannot reverse their order',
    'pixelate': 'weserv has no block pixelation filter',
    'long-chain': 'weserv cannot apply the required filter order and four-sided padding',
}


def precrop(size, fields):
    width, height, gravity, *offsets = fields
    position = POSITIONS[gravity]
    result = {'precrop': 'true'}
    for index, (axis, extent, near, far) in enumerate((
        ('x', width, 'left', 'right'), ('y', height, 'top', 'bottom'),
    )):
        base = size[index]
        amount = float(extent)
        length = round(base * amount if amount < 1 else amount)
        delta = base - length
        offset = float(offsets[index]) if offsets else 0
        shift = round(offset * base if abs(offset) < 1 else offset)
        origin = delta - shift if far in position else shift
        if far not in position and near not in position:
            origin += round((delta + 1) / 2)
        result['c' + axis] = max(0, min(delta, origin))
        result['cw' if axis == 'x' else 'ch'] = length
    return result


def path(case):
    family, output = case['family'], case['format']
    if output not in ('jpg', 'png', 'webp', 'avif'):
        raise NotImplementedError(f'weserv has no {output.upper()} output format')
    if family in UNSUPPORTED:
        raise NotImplementedError(UNSUPPORTED[family])

    options = dict((parts[0], parts[1:]) for parts in
                   (option.split(':') for option in case['options'].split('/')))
    query = {'output': output}
    if 'rs' in options:
        mode, width, height = options['rs'][:3]
        query.update(w=int(width), h=int(height), fit={'fit': 'inside', 'fill': 'cover', 'force': 'fill'}[mode])
    if 'c' in options:
        query.update(precrop(case['source_dimensions'], options['c']))
    if 'g' in options:
        query['a'] = POSITIONS[options['g'][0]]
    if 'bg' in options:
        query['bg'] = options['bg'][0]
    if 'ex' in options:
        query.update(fit='contain', a=POSITIONS[options['ex'][1]], cbg=options['bg'][0])
    if 'rot' in options:
        query['ro'] = options['rot'][0]
    if 'fl' in options:
        query['flop'], query['flip'] = options['fl']
    if 'dpr' in options:
        query['dpr'] = options['dpr'][0]
    if 'z' in options:
        zx, zy = map(float, options['z'])
        query.update(w=round(query['w'] * zx), h=round(query['h'] * zy))
    if 'bl' in options:
        query['blur'] = options['bl'][0]
    if 'sh' in options:
        query['sharp'] = options['sh'][0]
    if 'q' in options:
        query['q'] = options['q'][0]
    return '/images/' + quote(case['source'], safe='') + '?' + urlencode(query)
