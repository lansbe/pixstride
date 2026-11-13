"""Small public synthetic fixtures and independent container metadata readers."""
import binascii
import struct
import zlib


def chunk(kind, data):
    return struct.pack('>I', len(data)) + kind + data + struct.pack('>I', binascii.crc32(kind + data))


def png(width, height, channels=3, depth=8, rows=None, extra=()):
    color = {1: 0, 2: 4, 3: 2, 4: 6}[channels]
    if rows is None:
        maximum = (1 << depth) - 1
        rows = [[((x * 1031 + y * 661 + c * 7919) % maximum) for x in range(width) for c in range(channels)] for y in range(height)]
    packed = bytearray()
    for row in rows:
        packed.append(0)
        packed.extend(bytes(row) if depth == 8 else struct.pack('>' + 'H' * len(row), *row))
    return (b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', width, height, depth, color, 0, 0, 0))
            + b''.join(chunk(kind, data) for kind, data in extra) + chunk(b'IDAT', zlib.compress(packed)) + chunk(b'IEND', b''))


def read_png(data):
    if data[:8] != b'\x89PNG\r\n\x1a\n':
        raise ValueError('PNG required')
    offset, compressed, chunks = 8, bytearray(), []
    width = height = depth = color = interlace = None
    while offset < len(data):
        size = struct.unpack_from('>I', data, offset)[0]
        kind = data[offset+4:offset+8]
        body = data[offset+8:offset+8+size]
        checksum = struct.unpack_from('>I', data, offset+8+size)[0]
        if checksum != binascii.crc32(kind + body):
            raise ValueError('Invalid PNG checksum')
        chunks.append((kind, body))
        if kind == b'IHDR':
            width, height, depth, color, _, _, interlace = struct.unpack('>IIBBBBB', body)
        elif kind == b'IDAT':
            compressed.extend(body)
        offset += 12 + size
    if depth not in (8, 16) or color not in (0, 2, 4, 6) or interlace:
        raise ValueError('Unsupported test PNG representation')
    channels = {0: 1, 2: 3, 4: 2, 6: 4}[color]
    pixel_bytes = channels * depth // 8
    stride = width * pixel_bytes
    if width * height > 50_000_000:
        raise ValueError('Oversized test PNG')
    raw = zlib.decompress(compressed)
    if len(raw) != height * (stride + 1):
        raise ValueError('Invalid test PNG raster')
    previous, pixels = bytearray(stride), bytearray()
    for y in range(height):
        start = y * (stride + 1)
        filter_type = raw[start]
        row = bytearray(raw[start+1:start+1+stride])
        for x in range(stride):
            a = row[x-pixel_bytes] if x >= pixel_bytes else 0
            b = previous[x]
            c = previous[x-pixel_bytes] if x >= pixel_bytes else 0
            if filter_type == 0:
                prediction = 0
            elif filter_type == 1:
                prediction = a
            elif filter_type == 2:
                prediction = b
            elif filter_type == 3:
                prediction = (a+b)//2
            elif filter_type == 4:
                p = a+b-c
                errors = (abs(p-a), abs(p-b), abs(p-c))
                prediction = (a, b, c)[errors.index(min(errors))]
            else:
                raise ValueError('Invalid test PNG filter')
            row[x] = (row[x] + prediction) & 255
        pixels.extend(row)
        previous = row
    values = list(pixels) if depth == 8 else list(struct.unpack('>'+'H'*(len(pixels)//2), pixels))
    return (width, height, channels, depth), values, chunks


def animated_gif():
    header = b'GIF89a\x01\0\x01\0\x80\0\0\0\0\0\xff\0\0'
    loop = b'\x21\xff\x0bNETSCAPE2.0\x03\x01\x02\0\0'
    image = b'\x2c\0\0\0\0\x01\0\x01\0\0'
    first = b'\x21\xf9\x04\x08\x05\0\0\0' + image + b'\x02\x02\x44\x01\0'
    second = b'\x21\xf9\x04\x08\x0a\0\0\0' + image + b'\x02\x02\x4c\x01\0'
    return header + loop + first + second + b'\x3b'


def exif():
    """Little-endian TIFF metadata, independently generated ASCII and orientation tags."""
    tags = [(270, 'private-description'), (315, 'fixture-artist'), (33432, 'fixture-copyright')]
    offset = 8 + 2 + 12 * (len(tags) + 1) + 4
    entries, data = [], bytearray()
    for tag, text in tags:
        encoded = text.encode() + b'\0'
        entries.append(struct.pack('<HHII', tag, 2, len(encoded), offset + len(data)))
        data.extend(encoded)
    entries.append(struct.pack('<HHI', 274, 3, 1) + struct.pack('<H', 6) + b'\0\0')
    entries.sort(key=lambda entry: struct.unpack_from('<H', entry)[0])
    return b'II*\0\x08\0\0\0' + struct.pack('<H', len(entries)) + b''.join(entries) + b'\0'*4 + data


def rgb_icc():
    """Synthetic ICC v2 matrix RGB profile with gamma 2.2 and D50 primaries."""
    def xyz(values):
        return b'XYZ ' + b'\0'*4 + struct.pack('>iii', *(round(n*65536) for n in values))
    curve=b'curv'+b'\0'*4+struct.pack('>IH',1,round(2.2*256))
    description=b'Synthetic RGB gamma 2.2\0'
    tags=[(b'desc',b'desc'+b'\0'*4+struct.pack('>I',len(description))+description+b'\0'*78),
          (b'cprt',b'text'+b'\0'*4+b'Public synthetic fixture\0'),
          (b'wtpt',xyz((0.9642,1,0.8249))),
          (b'rXYZ',xyz((0.4360747,0.2225045,0.0139322))),
          (b'gXYZ',xyz((0.3850649,0.7168786,0.0971045))),
          (b'bXYZ',xyz((0.1430804,0.0606169,0.7141733))),
          (b'rTRC',curve),(b'gTRC',curve),(b'bTRC',curve)]
    header=bytearray(128)
    header[8:24]=b'\x02\x10\0\0mntrRGB XYZ '
    header[24:36]=struct.pack('>6H',2026,1,1,0,0,0)
    header[36:40]=b'acsp'
    header[68:80]=struct.pack('>iii',63190,65536,54061)
    table,body=bytearray(struct.pack('>I',len(tags))),bytearray()
    start=132+12*len(tags)
    for name,data in tags:
        table.extend(name+struct.pack('>II',start+len(body),len(data)))
        body.extend(data)
        body.extend(b'\0'*(-len(body)%4))
    struct.pack_into('>I',header,0,len(header)+len(table)+len(body))
    return header+table+body


def read_gif(data):
    width, height = struct.unpack_from('<HH', data, 6)
    offset = 13 + (3 * (2 << (data[10] & 7)) if data[10] & 128 else 0)
    delays, loop, pending = [], None, 0
    while offset < len(data) and data[offset] != 0x3b:
        tag = data[offset]
        offset += 1
        if tag == 0x21:
            label = data[offset]
            offset += 1
            blocks = []
            while data[offset]:
                size = data[offset]
                blocks.append(data[offset+1:offset+1+size])
                offset += size + 1
            offset += 1
            if label == 0xf9:
                pending = struct.unpack_from('<H', blocks[0], 1)[0] * 10
            if label == 0xff and blocks[0] == b'NETSCAPE2.0':
                loop = struct.unpack_from('<H', blocks[1], 1)[0]
        elif tag == 0x2c:
            flags = data[offset+8]
            offset += 9
            if flags & 128:
                offset += 3 * (2 << (flags & 7))
            offset += 1
            while data[offset]:
                offset += data[offset] + 1
            offset += 1
            delays.append(pending)
        else:
            raise ValueError('Invalid synthetic GIF')
    return (width, height), delays, loop


def read_webp_animation(data):
    offset, delays, loop = 12, [], None
    while offset + 8 <= len(data):
        kind = data[offset:offset+4]
        size = struct.unpack_from('<I', data, offset+4)[0]
        body = data[offset+8:offset+8+size]
        if kind == b'ANIM':
            loop = struct.unpack_from('<H', body, 4)[0]
        elif kind == b'ANMF':
            delays.append(int.from_bytes(body[12:15], 'little'))
        offset += 8 + size + size % 2
    return delays, loop
