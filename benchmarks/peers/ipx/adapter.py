"""Translate the frozen workloads to IPX 4.1 HTTP modifiers."""

from urllib.parse import quote


POSITIONS = {
    "ce": "centre", "noea": "northeast", "soea": "southeast",
    "sowe": "southwest", "nowe": "northwest", "sm": "attention",
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
        result.append((max(0, min(delta, origin)), length))
    (left, width), (top, height) = result
    return left, top, width, height


def path(case):
    options = dict(item.split(":", 1) for item in case["options"].split("/"))
    fmt = case["format"]
    if fmt == "bmp":
        raise NotImplementedError("IPX has no BMP output format")
    if case["source"].endswith(".bmp"):
        raise NotImplementedError("The bundled sharp/libvips cannot decode BMP")
    if "pix" in options:
        raise NotImplementedError("IPX has no pixelation modifier")
    if "pd" in options and "bl" in options:
        raise NotImplementedError("Sharp adds padding before blur and sharpening")

    mods = ["f_" + ("jpeg" if fmt == "jpg" else fmt)]
    if "c" in options:
        rect = crop_rectangle(case["source_dimensions"], options["c"].split(":"))
        mods.append("extract_" + "_".join(map(str, rect)))
    if "rs" in options:
        mode, width, height, *upscale = options["rs"].split(":")
        width, height = int(width), int(height)
        if "z" in options:
            x, y = map(int, options["z"].split(":"))
            width, height = width * x, height * y
        dpr = int(options.get("dpr", 1))
        fit = {"force": "fill", "fit": "inside", "fill": "cover"}[mode]
        if "ex" in options:
            fit = "contain"
            mods.append("pos_" + POSITIONS[options["ex"].split(":")[1]])
        elif "g" in options:
            mods.append("pos_" + POSITIONS[options["g"]])
        if upscale == ["1"]:
            mods.append("enlarge")
        mods.extend([f"s_{width * dpr}x{height * dpr}", "fit_" + fit])
    if "rot" in options:
        mods.append("rotate_" + options["rot"])
    if "fl" in options:
        horizontal, vertical = options["fl"].split(":")
        if horizontal == "1":
            mods.append("flop")
        if vertical == "1":
            mods.append("flip")
    if "pd" in options:
        sides = options["pd"].split(":")
        mods.append("extend_" + "_".join(sides * 4 if len(sides) == 1 else sides))
    if "bg" in options or (fmt == "jpg" and case["input_kind"] == "gradient-alpha"):
        mods.extend(["b_" + options.get("bg", "ffffff"), "flatten"])
    for option, modifier in (("bl", "blur"), ("sh", "sharpen"), ("q", "q")):
        if option in options:
            mods.append(modifier + "_" + options[option])
    return "/" + ",".join(mods) + "/" + quote(case["source"], safe="")
