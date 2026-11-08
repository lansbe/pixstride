#!/usr/bin/env python3
"""Synchronize generated Markdown and prepare reviewed, immutable releases."""
import argparse
import datetime as dt
import json
import re
import subprocess
import sys
from pathlib import Path

from release_policy import ROOT, channel, package, publication_plan, reference_links, sections, validate, version_key


def block(text, name, content):
    start, end = f"<!-- generated:{name}:start -->", f"<!-- generated:{name}:end -->"
    if text.count(start) != 1 or text.count(end) != 1:
        raise ValueError(f"Missing or duplicate generated block: {name}")
    return re.sub(re.escape(start) + r".*?" + re.escape(end),
                  lambda _: start + "\n" + content.rstrip() + "\n" + end,
                  text, flags=re.S)


def generated(root=ROOT):
    pkg, entries = validate(root)
    stable = next(v for v in entries if v != "Unreleased" and channel(v) == "latest")
    image = "ghcr.io/" + pkg["repository"].removeprefix("https://github.com/").lower()
    readme = (root / "README.md").read_text()
    readme = block(readme, "run", f'''```sh
docker run --rm --name pixstride -p 127.0.0.1:8080:8080 \\
  --read-only --cap-drop=ALL --security-opt=no-new-privileges \\
  --memory=512m --pids-limit=64 --cpus=2 \\
  -e PIXSTRIDE_ALLOW_UNSAFE=1 -e PIXSTRIDE_ALLOWED_HOSTS=upload.wikimedia.org \\
  {image}:{stable}
```''')
    readme = block(readme, "build", f'Linux and macOS, ARM64 or x86-64; Rust {pkg["rust-version"]} and libcurl development files.')
    names = set()
    for source in (root / "src").rglob("*.rs"):
        text = source.read_text()
        names.update(re.findall(r"\bPIXSTRIDE_[A-Z0-9_]+", text))
        names.update(n + "_FILE" for n in re.findall(r'secret\("(PIXSTRIDE_[A-Z0-9_]+)"\)', text))
    readme = block(readme, "configuration", "Configuration names: " + ", ".join(f"`{n}`" for n in sorted(names)) + ". See [SECURITY.md](SECURITY.md) for deployment requirements.")
    security = block((root / "SECURITY.md").read_text(), "support",
                     f"Only the latest stable release receives fixes (currently `{stable}`). Alpha, beta and release-candidate builds are for testing; no response-time SLA is offered.")
    return {"README.md": readme, "SECURITY.md": security,
            "CHANGELOG.md": reference_links((root / "CHANGELOG.md").read_text(), pkg["repository"])}


def sync(check=False, root=ROOT):
    stale = []
    for name, expected in generated(root).items():
        if (root / name).read_text() != expected:
            stale.append(name)
            if not check:
                (root / name).write_text(expected)
    if check and stale:
        raise ValueError("Stale generated documentation: " + ", ".join(stale) + "; run python3 scripts/project.py sync")
    return stale


def git(*args, root=ROOT):
    return subprocess.check_output(["git", "-C", str(root), *args], text=True).strip()


def check(root=ROOT, base=None, release_tag=None):
    sync(True, root)
    public_docs = git("ls-files", "--cached", "--others", "--exclude-standard", "-z", "--", "*.md", root=root)
    for name in filter(None, public_docs.split("\0")):
        path = root / name
        text = path.read_text()
        for target in re.findall(r"\[[^\]]*\]\(([^)]+)\)", text):
            if target.startswith(("https://", "http://", "mailto:", "#")):
                continue
            local = target.split("#")[0].strip("<>")
            resolved = (path.parent / local).resolve()
            if not resolved.is_relative_to(root.resolve()) or not resolved.exists():
                raise ValueError(f"Broken or nonportable local link in {path.name}: {target}")
    if base:
        if not re.fullmatch(r"[0-9a-f]{40}", base):
            raise ValueError("Expected an immutable base commit SHA")
        changed = set(git("diff", "--name-only", base, "HEAD", root=root).splitlines())
        runtime = {p for p in changed if p.startswith("src/") and not p.endswith(("tests.rs", "regressions.rs"))}
        if runtime and "CHANGELOG.md" not in changed:
            raise ValueError("Runtime changes need a human-written CHANGELOG.md entry")
        if runtime and not changed.intersection({"README.md", "SECURITY.md"}):
            raise ValueError("Review/update README.md or SECURITY.md with runtime changes")
        infrastructure = any(p.startswith(("scripts/", ".github/workflows/")) or p == "Dockerfile" for p in changed)
        if infrastructure and not changed.intersection({"CONTRIBUTING.md", "SECURITY.md"}):
            raise ValueError("Workflow changes need a CONTRIBUTING.md or SECURITY.md update")
    if release_tag:
        pkg, entries = validate(root)
        if release_tag != "v" + pkg["version"] or entries["Unreleased"]:
            raise ValueError("Tag/Cargo/changelog mismatch or nonempty Unreleased section")
        branch = "master" if channel(pkg["version"]) == "latest" else "develop"
        subprocess.run(["git", "-C", str(root), "merge-base", "--is-ancestor", "HEAD", f"origin/{branch}"], check=True)
    print("Documentation and release metadata are consistent")


def prepare(value, root=ROOT):
    check(root=root)
    pkg, entries = validate(root)
    if version_key(value) <= version_key(pkg["version"]) or value in entries:
        raise ValueError("New version must increase; never rewrite a released entry")
    body = entries["Unreleased"]
    if not re.search(r"^- \S", body, re.M):
        raise ValueError("Write the release summary in Unreleased first; include beta changes when promoting to stable")
    exists = subprocess.run(["git", "-C", str(root), "show-ref", "--verify", "--quiet", "refs/tags/v" + value])
    if exists.returncode == 0:
        raise ValueError("Tag already exists; refusing to reuse it")
    if exists.returncode != 1:
        raise ValueError("Cannot check existing tags")
    manifest = (root / "Cargo.toml").read_text()
    manifest, count = re.subn(r'(?m)^version = "' + re.escape(pkg["version"]) + r'"$', f'version = "{value}"', manifest, count=1)
    if count != 1:
        raise ValueError("Cannot locate package version")
    lock = (root / "Cargo.lock").read_text()
    pattern = r'(\[\[package\]\]\nname = "' + re.escape(pkg["name"]) + r'"\nversion = ")[^"]+("\n)'
    lock, count = re.subn(pattern, lambda m: m[1] + value + m[2], lock)
    if count != 1:
        raise ValueError("Cannot locate the package in Cargo.lock")
    changelog = (root / "CHANGELOG.md").read_text()
    date = dt.datetime.now(dt.timezone.utc).date().isoformat()
    changelog = changelog.replace("## [Unreleased]", f"## [Unreleased]\n\n## [{value}] - {date}", 1)
    for name, content in {"Cargo.toml": manifest, "Cargo.lock": lock, "CHANGELOG.md": changelog}.items():
        (root / name).write_text(content)
    sync(root=root)
    check(root=root)
    print(f"Prepared {value}. Review, merge into {'master' if channel(value) == 'latest' else 'develop'}, then push tag v{value}.")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    sub.add_parser("sync")
    checking = sub.add_parser("check")
    checking.add_argument("--base")
    checking.add_argument("--release-tag")
    sub.add_parser("prepare").add_argument("version")
    sub.add_parser("notes").add_argument("version")
    plan = sub.add_parser("plan")
    plan.add_argument("tag")
    plan.add_argument("published_tags", type=Path)
    plan.add_argument("--channel-version")
    args = parser.parse_args()
    if args.command == "sync":
        print("Updated: " + (", ".join(sync()) or "already current"))
    elif args.command == "check":
        check(base=args.base, release_tag=args.release_tag)
    elif args.command == "prepare":
        prepare(args.version)
    elif args.command == "notes":
        _, entries = validate()
        version_key(args.version)
        print(entries[args.version])
    else:
        print(json.dumps(publication_plan(args.tag, args.published_tags.read_text().splitlines(), args.channel_version)))


if __name__ == "__main__":
    try:
        main()
    except (ValueError, KeyError, OSError, subprocess.CalledProcessError) as error:
        sys.exit(str(error))
