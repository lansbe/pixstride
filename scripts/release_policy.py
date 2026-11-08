"""Release metadata shared by documentation and publishing (Python stdlib only)."""
import datetime as dt
import re
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
VERSION = re.compile(r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(?:-(alpha|beta|rc)\.([1-9][0-9]*))?")
HEADINGS = re.compile(r"^## \[([^\]]+)\](?: - (\d{4}-\d{2}-\d{2}))?$", re.M)
CATEGORIES = {"Added", "Changed", "Deprecated", "Removed", "Fixed", "Security"}


def version_key(value):
    match = VERSION.fullmatch(value)
    if not match:
        raise ValueError("Use X.Y.Z or X.Y.Z-alpha.N/beta.N/rc.N, without leading zeroes")
    major, minor, patch, stage, number = match.groups()
    return (int(major), int(minor), int(patch),
            {"alpha": 0, "beta": 1, "rc": 2, None: 3}[stage], int(number or 0))


def channel(value):
    version_key(value)
    return value.split("-", 1)[1].split(".")[0] if "-" in value else "latest"


def package(root=ROOT):
    return tomllib.loads((root / "Cargo.toml").read_text())["package"]


def sections(text):
    # Reference links are generated separately and do not belong to release notes.
    text = re.split(r"^\[Unreleased\]:", text, maxsplit=1, flags=re.M)[0]
    headings = list(HEADINGS.finditer(text))
    if len(headings) != len(re.findall(r"^## ", text, re.M)):
        raise ValueError("Malformed changelog release heading")
    if not headings or headings[0].group(1) != "Unreleased":
        raise ValueError("CHANGELOG.md must begin with an [Unreleased] section")
    entries = {}
    previous = None
    for i, match in enumerate(headings):
        name, date = match.groups()
        if name in entries:
            raise ValueError(f"Duplicate changelog section: {name}")
        body = text[match.end():headings[i + 1].start() if i + 1 < len(headings) else len(text)].strip()
        if name == "Unreleased":
            if date or i != 0:
                raise ValueError("Unreleased must be first and undated")
        else:
            key = version_key(name)
            if not date or dt.date.fromisoformat(date) > dt.datetime.now(dt.timezone.utc).date():
                raise ValueError(f"Invalid or future release date: {name}")
            if previous is not None and key >= previous:
                raise ValueError("List releases in descending semantic version order")
            previous = key
            if not re.search(r"^- \S", body, re.M):
                raise ValueError(f"Missing human-readable release notes: {name}")
        categories = re.findall(r"^### (.+)$", body, re.M)
        if any(c not in CATEGORIES for c in categories) or len(set(categories)) != len(categories):
            raise ValueError(f"Invalid or duplicate changelog category: {name}")
        if body and not categories:
            raise ValueError(f"Group changes under Keep a Changelog categories: {name}")
        entries[name] = body
    return entries


def reference_links(text, repo_url):
    entries = sections(text)
    versions = [v for v in entries if v != "Unreleased"]
    if not versions:
        raise ValueError("At least one release must be documented")
    body = re.split(r"^\[Unreleased\]:", text, maxsplit=1, flags=re.M)[0].rstrip()
    links = [f"[Unreleased]: {repo_url}/compare/v{versions[0]}...HEAD"]
    for i, value in enumerate(versions):
        suffix = f"compare/v{versions[i + 1]}...v{value}" if i + 1 < len(versions) else f"releases/tag/v{value}"
        links.append(f"[{value}]: {repo_url}/{suffix}")
    return body + "\n\n" + "\n".join(links) + "\n"


def validate(root=ROOT):
    pkg = package(root)
    version_key(pkg["version"])
    entries = sections((root / "CHANGELOG.md").read_text())
    versions = [v for v in entries if v != "Unreleased"]
    if not versions or versions[0] != pkg["version"]:
        raise ValueError("Cargo version must match the newest dated changelog section")
    lock = tomllib.loads((root / "Cargo.lock").read_text())
    own = [p for p in lock["package"] if p["name"] == pkg["name"]]
    if len(own) != 1 or own[0]["version"] != pkg["version"]:
        raise ValueError("Cargo.toml and Cargo.lock versions disagree")
    return pkg, entries


def publication_plan(tag, published_tags, current_channel=None):
    if not tag.startswith("v"):
        raise ValueError("Release tags must start with v")
    value = tag[1:]
    key = version_key(value)
    lane = channel(value)
    known = []
    for old in published_tags:
        if old == tag:
            raise ValueError("This GitHub release already exists; never replace a published version")
        if old.startswith("v") and VERSION.fullmatch(old[1:]):
            known.append(old[1:])
    # A prerelease never moves latest. Old tags cannot roll a channel backwards.
    peers = [v for v in known if channel(v) == lane]
    move = all(key > version_key(v) for v in peers)
    if current_channel is not None:
        if channel(current_channel) != lane:
            raise ValueError("Existing image belongs to a different release channel")
        # A previous job may have promoted its image before GitHub publication failed.
        # Equal versions may resume with the exact same digest; older ones never move it.
        move = move and key >= version_key(current_channel)
    return {"version": value, "channel": lane, "prerelease": lane != "latest",
            "branch": "master" if lane == "latest" else "develop", "move_channel": move}
