"""Translate the frozen workloads to Thumbor 7.8's built-in transformations."""

import math
from urllib.parse import quote


ALIGNMENTS = {
    "ce": ("center", "middle"), "noea": ("right", "top"),
    "soea": ("right", "bottom"), "sowe": ("left", "bottom"), "nowe": ("left", "top"),
}


def crop_rectangle(dimensions, values):
    width, height, gravity, *offsets = values
    result = []
    for base, value, end, offset in zip(
        dimensions, (width, height), ("ea", "so"), offsets or (0, 0)
    ):
        amount = float(value)
        length = round(base * amount if amount < 1 else amount)
        delta = base - length
        shift = round(base * float(offset))
        if gravity == "ce":
            origin = round((delta + 1) / 2) + shift
        else:
            origin = delta - shift if end in gravity else shift
        start = max(0, min(delta, origin))
        result.append((start, start + length))
    (left, right), (top, bottom) = result
    return f"{left}x{top}:{right}x{bottom}"


def path(case):
    options = dict(item.split(":", 1) for item in case["options"].split("/"))
    fmt = case["format"]
    if fmt == "bmp":
        raise NotImplementedError("Thumbor's format filter has no BMP output")
    if "pix" in options:
        raise NotImplementedError("Thumbor has no built-in pixelation filter")
    if "pd" in options:
        raise NotImplementedError("Thumbor has no built-in padding filter")
    parts = ["unsafe"]
    filters = []
    if "c" in options:
        parts.append(crop_rectangle(case["source_dimensions"], options["c"].split(":")))
    if "rs" in options:
        mode, width, height, *_ = options["rs"].split(":")
        width, height = int(width), int(height)
        if mode == "fit":
            parts.append("fit-in")
        elif mode == "force":
            filters.append("stretch()")
        if "z" in options:
            x, y = map(int, options["z"].split(":"))
            width, height = width * x, height * y
        dpr = int(options.get("dpr", 1))
        parts.append(f"{width * dpr}x{height * dpr}")
    if "fl" in options:
        horizontal, vertical = options["fl"].split(":")
        w, h = case["source_dimensions"]
        parts.append(f"{-w if horizontal == '1' else w}x{-h if vertical == '1' else h}")
    if "g" in options:
        gravity = options["g"]
        if gravity == "sm":
            parts.append("smart")
        else:
            parts.extend(ALIGNMENTS[gravity])
    if "rot" in options:
        # Thumbor/Pillow specifies counter-clockwise angles.
        filters.append(f"rotate({-int(options['rot']) % 360})")
    if "bl" in options:
        sigma = options["bl"]
        filters.append(f"blur({math.ceil(3 * float(sigma))},{sigma})")
    if "sh" in options:
        # Thumbor uses wavelet sharpening. Use its radius for the workload's
        # spatial scale, holding the amount and luminance setting constant.
        filters.append(f"sharpen(1,{options['sh']},true)")
    if "ex" in options:
        gravity = options["ex"].split(":")[1]
        w, h = case["source_dimensions"]
        if gravity != "ce" and w * height != h * width:
            raise NotImplementedError("Thumbor's fill filter only centers the canvas")
        filters.append(f"fill({options['bg']},true)")
    elif "bg" in options or (fmt == "jpg" and case["input_kind"] == "gradient-alpha"):
        filters.append(f"background_color({options.get('bg', 'ffffff')})")
    if "q" in options:
        filters.append(f"quality({options['q']})")
    filters.append("format(" + ("jpeg" if fmt == "jpg" else fmt) + ")")
    parts.extend(["filters:" + ":".join(filters), quote(case["source"], safe="")])
    return "/" + "/".join(parts)
