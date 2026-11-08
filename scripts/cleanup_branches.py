#!/usr/bin/env python3
"""Preview or delete local merged branches; preserve worktrees and unique commits."""
import argparse
import subprocess

PROTECTED = {"master", "develop"}


def git(*args):
    return subprocess.check_output(["git", *args], text=True).strip()


def merged(branch, target):
    return subprocess.run(["git", "merge-base", "--is-ancestor", branch, target],
                          stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL).returncode == 0


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--apply", action="store_true", help="delete eligible local branches after fetching")
    args = parser.parse_args()
    subprocess.run(["git", "fetch", "--prune", "origin"], check=True)
    checked_out = {line.removeprefix("branch refs/heads/")
                   for line in git("worktree", "list", "--porcelain").splitlines()
                   if line.startswith("branch refs/heads/")}
    branches = git("for-each-ref", "--format=%(refname:short)", "refs/heads/").splitlines()
    removed = 0
    for name in branches:
        if name in PROTECTED or name in checked_out:
            continue
        target = next((b for b in ("master", "develop") if merged(name, "refs/remotes/origin/" + b)), None)
        if not target:
            continue
        print(f"{'Delete' if args.apply else 'Would delete'} {name}: merged into origin/{target}")
        if args.apply:
            # -d rechecks ancestry at deletion and refuses every checked-out worktree.
            # Ephemeral upstream settings handle branches whose remote was auto-deleted.
            subprocess.run(["git", "-c", f"branch.{name}.remote=origin",
                            "-c", f"branch.{name}.merge=refs/heads/{target}",
                            "branch", "-d", "--", name], check=True)
            removed += 1
    if args.apply:
        print(f"Deleted {removed} local branch(es); permanent branches and worktrees preserved")
    else:
        print("Preview only. Run with --apply to delete eligible local branches.")


if __name__ == "__main__":
    main()
