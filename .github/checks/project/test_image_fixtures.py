"""Independent PNG fixture reader must accept valid native indexed output."""
import binascii
from pathlib import Path
import struct
import sys
import unittest
import zlib

sys.path.insert(0, str(Path(__file__).resolve().parents[3] / 'scripts'))
from image_fixtures import read_png


def indexed(width, depth, rows, palette=b'\x09\x1b\x51\x2a\x49\x77', alpha=None):
    def part(kind, body):
        return struct.pack('>I', len(body)) + kind + body + struct.pack('>I', binascii.crc32(kind + body))
    header = struct.pack('>IIBBBBB', width, len(rows), depth, 3, 0, 0, 0)
    return (b'\x89PNG\r\n\x1a\n' + part(b'IHDR', header)
            + (part(b'PLTE', palette) if palette is not None else b'')
            + (part(b'tRNS', alpha) if alpha is not None else b'')
            + part(b'IDAT', zlib.compress(b''.join(rows))) + part(b'IEND', b''))


class IndexedPngReaderTests(unittest.TestCase):
    def test_palette_samples_cross_byte_boundaries_at_each_standard_depth(self):
        rows = {1: b'\x00\x69\x80', 2: b'\x00\x14\x41\x40',
                4: b'\x00\x01\x10\x10\x01\x10', 8: b'\x00\x00\x01\x01\x00\x01\x00\x00\x01\x01'}
        expected = [9,27,81, 42,73,119, 42,73,119, 9,27,81, 42,73,119,
                    9,27,81, 9,27,81, 42,73,119, 42,73,119]
        for depth, row in rows.items():
            with self.subTest(depth=depth):
                header, pixels, _ = read_png(indexed(9, depth, [row]))
                self.assertEqual(header, (9, 1, 3, 8))
                self.assertEqual(pixels, expected)

    def test_palette_transparency_defaults_undeclared_entries_to_opaque(self):
        header, pixels, chunks = read_png(indexed(2, 8, [b'\x00\x00\x01'], alpha=b'\x00'))
        self.assertEqual(header, (2, 1, 4, 8))
        self.assertEqual(pixels, [9,27,81,0, 42,73,119,255])
        self.assertIn((b'tRNS', b'\x00'), chunks)

    def test_palette_filters_use_one_packed_byte_per_left_neighbor(self):
        # Unfiltered bytes are 0x69,0x80; Sub stores second byte as 0x17.
        header, pixels, _ = read_png(indexed(9, 1, [b'\x01\x69\x17', b'\x02\x00\x00']))
        self.assertEqual(header, (9, 2, 3, 8))
        self.assertEqual(pixels[:27], pixels[27:])
        self.assertEqual(pixels[-3:], [42,73,119])

    def test_invalid_or_missing_palette_entries_are_rejected(self):
        for palette, row, alpha in [(None, b'\x00\x00', None), (b'\x01', b'\x00\x00', None),
                                    (b'\x00\x00\x00', b'\x00\x01', None),
                                    (b'\x00\x00\x00', b'\x00\x00', b'\x00\xff')]:
            with self.subTest(palette=palette, row=row, alpha=alpha), self.assertRaises(ValueError):
                read_png(indexed(1, 8, [row], palette=palette, alpha=alpha))


if __name__ == '__main__':
    unittest.main()
