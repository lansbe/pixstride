"""Isolated regression tests for release metadata and safe branch cleanup."""
import contextlib
import io
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "scripts"))
import project
from release_policy import publication_plan, sections, version_key


def run(*args, cwd):
    return subprocess.check_output(args, cwd=cwd, text=True, stderr=subprocess.STDOUT).strip()


class ReleasePolicyTests(unittest.TestCase):
    def test_semver_order_and_rejected_ambiguous_versions(self):
        versions = ["0.2.0-alpha.1", "0.2.0-beta.1", "0.2.0-beta.10", "0.2.0-rc.1", "0.2.0", "0.10.0", "1.0.0"]
        self.assertEqual(sorted(versions, key=version_key), versions)
        for value in ("01.2.3", "1.2", "1.2.3-beta.0", "1.2.3-beta.01", "1.2.3+build", "1.2.3;echo bad"):
            with self.subTest(value=value), self.assertRaises(ValueError):
                version_key(value)

    def test_prereleases_never_move_latest_and_channels_never_regress(self):
        tags = ["v0.1.1", "v0.2.0-beta.2"]
        plan = publication_plan("v0.2.0-beta.1", tags)
        self.assertTrue(plan["prerelease"])
        self.assertEqual((plan["channel"], plan["branch"]), ("beta", "develop"))
        self.assertFalse(plan["move_channel"])
        self.assertTrue(publication_plan("v0.2.0-beta.3", tags)["move_channel"])
        stable = publication_plan("v0.2.0", tags)
        self.assertEqual((stable["channel"], stable["branch"], stable["prerelease"]), ("latest", "master", False))
        self.assertFalse(publication_plan("v0.1.0", tags)["move_channel"])
        with self.assertRaises(ValueError):
            publication_plan("v0.1.1", tags)
        self.assertFalse(publication_plan("v0.2.0-beta.3", tags, "0.2.0-beta.4")["move_channel"])
        self.assertTrue(publication_plan("v0.2.0-beta.3", tags, "0.2.0-beta.3")["move_channel"])
        with self.assertRaises(ValueError):
            publication_plan("v0.2.0-beta.3", tags, "0.2.0")

    def test_invalid_changelog_cannot_supply_release_notes(self):
        for text in ("## [1.0.0] - 2026-01-01\n### Added\n- A\n",
                     "## [Unreleased]\n\n## [1.0.0] - 2026-02-30\n### Added\n- A\n",
                     "## [Unreleased]\n\n## [1.0.0] - 2026-01-01\n### Surprise\n- A\n"):
            with self.assertRaises(ValueError):
                sections(text)


class DocumentationTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="pixstride-project-test-")
        self.root = Path(self.directory.name)
        for name in ("Cargo.toml", "Cargo.lock", "README.md", "SECURITY.md", "CHANGELOG.md", "CONTRIBUTING.md", "COMPATIBILITY.md", "LICENSE"):
            shutil.copyfile(ROOT / name, self.root / name)
        shutil.copytree(ROOT / "src", self.root / "src")
        (self.root / ".github").mkdir()
        (self.root / "vendor/rs-vips").mkdir(parents=True)
        shutil.copyfile(ROOT / "vendor/rs-vips/README.md", self.root / "vendor/rs-vips/README.md")
        shutil.copytree(ROOT / ".github/checks/compatibility", self.root / ".github/checks/compatibility")
        run("git", "init", "-b", "master", cwd=self.root)
        run("git", "config", "user.name", "Fixture", cwd=self.root)
        run("git", "config", "user.email", "fixture@example.invalid", cwd=self.root)
        # Synthetic repositories must not execute the developer's global hooks.
        run("git", "config", "core.hooksPath", "/dev/null", cwd=self.root)
        # Fixed fixture metadata keeps these tests valid after real version bumps.
        current = project.package(self.root)["version"]
        for name in ("Cargo.toml", "Cargo.lock"):
            path = self.root / name
            path.write_text(re.sub(r'(?m)^version = "' + re.escape(current) + r'"$',
                                   'version = "0.1.1"', path.read_text()))
        (self.root / "CHANGELOG.md").write_text(
            "# Changelog\n\n## [Unreleased]\n\n### Added\n- Synthetic change.\n\n"
            "## [0.1.1] - 2026-01-01\n\n### Fixed\n- Synthetic baseline.\n")
        project.sync(root=self.root)

    def tearDown(self):
        self.directory.cleanup()

    def quiet(self, fn, *args, **kwargs):
        with contextlib.redirect_stdout(io.StringIO()):
            return fn(*args, **kwargs)

    def test_generated_sections_detect_drift_and_sync_is_idempotent(self):
        self.quiet(project.check, root=self.root)
        path = self.root / "README.md"
        rust = project.package(self.root)["rust-version"]
        path.write_text(path.read_text().replace(f"Rust {rust}", "Rust invalid"))
        with self.assertRaises(ValueError):
            project.sync(True, self.root)
        project.sync(root=self.root)
        self.assertEqual(project.sync(root=self.root), [])
        (self.root / "src/new_config.rs").write_text('const ENV: &str = "PIXSTRIDE_NEW_SETTING";\n')
        with self.assertRaises(ValueError):
            project.sync(True, self.root)

    def test_prepare_beta_preserves_stable_example_and_aligns_versions(self):
        before = (self.root / "README.md").read_text()
        self.quiet(project.prepare, "0.2.0-beta.1", root=self.root)
        pkg, entries = project.validate(self.root)
        self.assertEqual(pkg["version"], "0.2.0-beta.1")
        self.assertEqual(entries["Unreleased"], "")
        self.assertIn("ghcr.io/lansbe/pixstride:0.1.1", before)
        self.assertIn("ghcr.io/lansbe/pixstride:0.1.1", (self.root / "README.md").read_text())
        snapshot = (self.root / "Cargo.toml").read_bytes()
        with self.assertRaises(ValueError):
            self.quiet(project.prepare, "0.2.0-beta.1", root=self.root)
        self.assertEqual(snapshot, (self.root / "Cargo.toml").read_bytes())

    def test_broken_link_and_missing_release_notes_fail(self):
        path = self.root / "README.md"
        path.write_text(path.read_text() + "\n[Broken](absent.md)\n")
        with self.assertRaises(ValueError):
            self.quiet(project.check, root=self.root)
        # A metadata failure must not partially bump the version.
        snapshot = (self.root / "Cargo.toml").read_bytes()
        with self.assertRaises(ValueError):
            self.quiet(project.prepare, "0.2.0", root=self.root)
        self.assertEqual(snapshot, (self.root / "Cargo.toml").read_bytes())

    def test_runtime_change_requires_changelog_and_docs(self):
        run("git", "add", ".", cwd=self.root)
        run("git", "-c", "commit.gpgsign=false", "commit", "-m", "fixture baseline", cwd=self.root)
        base = run("git", "rev-parse", "HEAD", cwd=self.root)
        path = self.root / "src/config.rs"
        path.write_text(path.read_text() + "\n// fixture-only change\n")
        run("git", "add", ".", cwd=self.root)
        run("git", "-c", "commit.gpgsign=false", "commit", "-m", "fixture update", cwd=self.root)
        with self.assertRaisesRegex(ValueError, "CHANGELOG"):
            self.quiet(project.check, root=self.root, base=base)

    def test_only_public_docs_are_checked(self):
        (self.root / ".gitignore").write_text("private.md\n")
        (self.root / "private.md").write_text("[Local note](missing-file.md)")
        self.quiet(project.check, root=self.root)

    def test_release_tag_requires_correct_branch_and_empty_unreleased(self):
        self.quiet(project.prepare, "0.2.0-beta.1", root=self.root)
        run("git", "add", ".", cwd=self.root)
        run("git", "-c", "commit.gpgsign=false", "commit", "-m", "fixture release", cwd=self.root)
        run("git", "update-ref", "refs/remotes/origin/develop", "HEAD", cwd=self.root)
        run("git", "update-ref", "refs/remotes/origin/master", "HEAD", cwd=self.root)
        self.quiet(project.check, root=self.root, release_tag="v0.2.0-beta.1")
        with self.assertRaises(ValueError):
            self.quiet(project.check, root=self.root, release_tag="v0.2.0")
        path = self.root / "CHANGELOG.md"
        path.write_text(path.read_text().replace("## [Unreleased]", "## [Unreleased]\n\n### Changed\n- Stable summary."))
        with self.assertRaises(ValueError):
            self.quiet(project.check, root=self.root, release_tag="v0.2.0-beta.1")
        self.quiet(project.prepare, "0.2.0", root=self.root)
        self.assertIn("ghcr.io/lansbe/pixstride:0.2.0", (self.root / "README.md").read_text())
        run("git", "add", ".", cwd=self.root)
        run("git", "-c", "commit.gpgsign=false", "commit", "-m", "fixture stable", cwd=self.root)
        with self.assertRaises(subprocess.CalledProcessError):
            self.quiet(project.check, root=self.root, release_tag="v0.2.0")
        run("git", "update-ref", "refs/remotes/origin/master", "HEAD", cwd=self.root)
        self.quiet(project.check, root=self.root, release_tag="v0.2.0")


class CleanupTests(unittest.TestCase):
    def test_cleanup_preserves_unique_commits_and_checked_out_worktrees(self):
        with tempfile.TemporaryDirectory(prefix="pixstride-cleanup-test-") as directory:
            root = Path(directory)
            origin, work, other = root / "remote.git", root / "work", root / "other"
            run("git", "init", "--bare", "-b", "master", str(origin), cwd=root)
            run("git", "clone", str(origin), str(work), cwd=root)
            run("git", "config", "user.name", "Fixture", cwd=work)
            run("git", "config", "user.email", "fixture@example.invalid", cwd=work)
            run("git", "config", "core.hooksPath", "/dev/null", cwd=work)
            (work / "sample").write_text("baseline")
            run("git", "add", ".", cwd=work)
            run("git", "-c", "commit.gpgsign=false", "commit", "-m", "baseline", cwd=work)
            run("git", "branch", "develop", cwd=work)
            run("git", "push", "origin", "master", "develop", cwd=work)
            run("git", "branch", "merged", cwd=work)
            run("git", "branch", "checked-out", cwd=work)
            run("git", "worktree", "add", str(other), "checked-out", cwd=work)
            run("git", "switch", "-c", "unmerged", cwd=work)
            (work / "sample").write_text("unique work")
            run("git", "add", ".", cwd=work)
            run("git", "-c", "commit.gpgsign=false", "commit", "-m", "unique", cwd=work)
            run("git", "switch", "develop", cwd=work)
            run("git", "switch", "-c", "only-develop", cwd=work)
            (work / "development").write_text("integration-only work")
            run("git", "add", ".", cwd=work)
            run("git", "-c", "commit.gpgsign=false", "commit", "-m", "integration", cwd=work)
            run("git", "switch", "develop", cwd=work)
            run("git", "merge", "--ff-only", "only-develop", cwd=work)
            run("git", "push", "origin", "develop", cwd=work)
            run("git", "switch", "master", cwd=work)
            script = str(ROOT / "scripts/cleanup_branches.py")
            run(sys.executable, script, cwd=work)
            self.assertIn("merged", run("git", "branch", "--list", cwd=work))
            run(sys.executable, script, "--apply", cwd=work)
            names = set(run("git", "for-each-ref", "--format=%(refname:short)", "refs/heads", cwd=work).splitlines())
            self.assertEqual(names, {"master", "develop", "unmerged", "checked-out"})
            self.assertEqual((other / "sample").read_text(), "baseline")


if __name__ == "__main__":
    unittest.main()
