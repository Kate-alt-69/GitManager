import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import gitmanager_cleaner as cleaner


class GitManagerCleanerTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name) / "repo"
        self.root.mkdir()
        subprocess.run(["git", "init", "-q", str(self.root)], check=True)
        subprocess.run(["git", "-C", str(self.root), "config", "user.name", "Test"], check=True)
        subprocess.run(["git", "-C", str(self.root), "config", "user.email", "test@example.invalid"], check=True)
        (self.root / "package.json").write_text('{"name":"fixture"}', encoding="utf-8")
        (self.root / ".gitignore").write_text("node_modules/\n.next/\n", encoding="utf-8")
        (self.root / ".next").mkdir()
        (self.root / ".next" / "cache.bin").write_bytes(b"x" * 1024)
        (self.root / "dist").mkdir()
        (self.root / "dist" / "bundle.js").write_bytes(b"built" * 128)
        (self.root / "node_modules").mkdir()
        (self.root / "node_modules" / "dep.js").write_text("module.exports = 1", encoding="utf-8")
        (self.root / "src").mkdir()
        (self.root / "src" / "app.js").write_text("source", encoding="utf-8")
        subprocess.run(["git", "-C", str(self.root), "add", "package.json", ".gitignore", "src/app.js"], check=True)
        subprocess.run(["git", "-C", str(self.root), "commit", "-qm", "fixture"], check=True)

    def tearDown(self):
        self.temp.cleanup()

    def test_preview_does_not_delete_and_dependencies_are_opt_in(self):
        plan = cleaner.build_repo_plan(self.root)
        paths = {item["relative_path"] for item in plan["candidates"]}
        self.assertIn(".next", paths)
        self.assertIn("dist", paths)
        self.assertNotIn("node_modules", paths)
        self.assertTrue((self.root / ".next").exists())
        with_deps = cleaner.build_repo_plan(self.root, include_dependencies=True)
        self.assertIn("node_modules", {item["relative_path"] for item in with_deps["candidates"]})

    def test_tracked_paths_are_skipped_by_default(self):
        (self.root / "Cargo.toml").write_text(
            "[package]\nname='fixture'\nversion='0.1.0'\nedition='2021'\n", encoding="utf-8"
        )
        (self.root / "target").mkdir()
        (self.root / "target" / "tracked.bin").write_text("tracked", encoding="utf-8")
        subprocess.run(["git", "-C", str(self.root), "add", "Cargo.toml", "target/tracked.bin"], check=True)
        subprocess.run(["git", "-C", str(self.root), "commit", "-qm", "tracked target"], check=True)
        plan = cleaner.build_repo_plan(self.root)
        self.assertNotIn("target", {item["relative_path"] for item in plan["candidates"]})
        self.assertIn("target", plan["skipped_tracked"])

    def test_apply_requires_exact_confirmation(self):
        plan = cleaner.build_repo_plan(self.root)
        with self.assertRaises(ValueError):
            cleaner.apply_plan(plan, confirmation="yes")
        self.assertTrue((self.root / ".next").exists())

    def test_apply_preserves_git_and_source(self):
        plan = cleaner.build_repo_plan(self.root)
        result = cleaner.apply_plan(plan, confirmation="YES")
        self.assertEqual(result["removed_count"], 2)
        self.assertFalse((self.root / ".next").exists())
        self.assertFalse((self.root / "dist").exists())
        self.assertTrue((self.root / ".git").exists())
        self.assertTrue((self.root / "src/app.js").exists())

    def test_commit_clean_unstages_generated_paths_without_deleting_worktree(self):
        subprocess.run(
            ["git", "-C", str(self.root), "add", "-f", "dist/bundle.js"],
            check=True,
        )
        subprocess.run(
            ["git", "-C", str(self.root), "commit", "-qm", "track generated output"],
            check=True,
        )
        result = cleaner.commit_clean_index(self.root, include_dependencies=False)
        self.assertEqual(result["removed_count"], 1)
        self.assertIn("dist", result["omitted_paths"])
        self.assertTrue(result["working_tree_preserved"])
        self.assertTrue((self.root / "dist" / "bundle.js").is_file())
        listed = subprocess.run(
            ["git", "-C", str(self.root), "ls-files", "--", "dist"],
            text=True, capture_output=True, check=True,
        )
        self.assertEqual(listed.stdout.strip(), "")
        exclude = (self.root / ".git" / "info" / "exclude").read_text(encoding="utf-8")
        self.assertIn("/dist/", exclude)

    def test_commit_clean_refuses_symlinked_exclude_before_changing_index(self):
        subprocess.run(
            ["git", "-C", str(self.root), "add", "-f", "dist/bundle.js"],
            check=True,
        )
        subprocess.run(
            ["git", "-C", str(self.root), "commit", "-qm", "track generated output"],
            check=True,
        )
        external = Path(self.temp.name) / "external-exclude"
        external.write_text("# user file\n", encoding="utf-8")
        exclude = self.root / ".git" / "info" / "exclude"
        exclude.unlink()
        try:
            exclude.symlink_to(external)
        except (OSError, NotImplementedError):
            self.skipTest("Symlinks are not available")
        with self.assertRaises(ValueError):
            cleaner.commit_clean_index(self.root, include_dependencies=False)
        listed = subprocess.run(
            ["git", "-C", str(self.root), "ls-files", "--", "dist"],
            text=True, capture_output=True, check=True,
        )
        self.assertIn("dist/bundle.js", listed.stdout)
        self.assertEqual(external.read_text(encoding="utf-8"), "# user file\n")

    def test_commit_clean_hook_installs_and_removes_only_its_own_hook(self):
        helper = Path(cleaner.__file__).resolve()
        installed = cleaner.set_commit_clean_hook(self.root, helper, enabled=True)
        self.assertTrue(installed["enabled"])
        hook = Path(installed["hook_path"])
        hooks = hook.parent
        self.assertIn("GitManager managed Commit Clean hook v1", hook.read_text(encoding="utf-8"))
        disabled = cleaner.set_commit_clean_hook(self.root, helper, enabled=False)
        self.assertFalse(disabled["enabled"])
        self.assertFalse(hook.exists())
        self.assertFalse((hooks / "gitmanager_cleaner.py").exists())

    def test_commit_clean_hook_refuses_to_replace_existing_user_hook(self):
        hooks = self.root / ".git" / "hooks"
        hooks.mkdir(exist_ok=True)
        existing = hooks / "pre-commit"
        existing.write_text("#!/bin/sh\\necho user-hook\\n", encoding="utf-8")
        with self.assertRaises(ValueError):
            cleaner.set_commit_clean_hook(self.root, Path(cleaner.__file__).resolve(), enabled=True)
        self.assertEqual(existing.read_text(encoding="utf-8"), "#!/bin/sh\\necho user-hook\\n")

    def test_custom_cleanignore_is_repository_specific(self):
        (self.root / ".gitmanager-cleanignore").write_text(".next/\n", encoding="utf-8")
        plan = cleaner.build_repo_plan(self.root)
        self.assertNotIn(".next", {item["relative_path"] for item in plan["candidates"]})

    def test_remote_urls_are_restricted_to_https_and_ssh(self):
        cleaner._validate_remote_url("https://github.com/example/project.git")
        cleaner._validate_remote_url("git@github.com:example/project.git")
        for bad in (
            "file:///tmp/repo", "-c core.hooksPath=/tmp",
            "https://host/repo\n--upload-pack=x",
            "https://user:secret@github.com/example/project.git",
            "ssh://git:secret@github.com/example/project.git",
        ):
            with self.assertRaises(ValueError):
                cleaner._validate_remote_url(bad)

    def test_symlinks_are_not_cleanup_candidates(self):
        external = Path(self.temp.name) / "external"
        external.mkdir()
        (external / "keep.txt").write_text("safe", encoding="utf-8")
        link = self.root / ".next-link"
        try:
            link.symlink_to(external, target_is_directory=True)
        except (OSError, NotImplementedError):
            self.skipTest("Symlinks are not available")
        self.assertFalse(cleaner._safe_candidate(link, self.root))
        self.assertTrue((external / "keep.txt").exists())

    def test_system_plan_only_contains_existing_paths_under_home(self):
        plan = cleaner.build_system_plan()
        home = Path.home().resolve()
        for item in plan["candidates"]:
            self.assertTrue(cleaner._is_relative_to(Path(item["path"]).resolve(), home))

    def test_managed_ignore_rules_only_cover_removed_candidates(self):
        plan = {
            "candidates": [
                {"path": "/tmp/clone/.next", "relative_path": ".next", "kind": "directory"},
                {"path": "/tmp/clone/target", "relative_path": "target", "kind": "directory"},
                {"path": "/tmp/clone/src/bin", "relative_path": "src/bin", "kind": "directory"},
            ]
        }
        rules = cleaner.managed_ignore_rules(plan, ["/tmp/clone/.next"])
        self.assertEqual(rules, ["/.next/"])
        self.assertNotIn("/target/", rules)
        self.assertNotIn("/src/bin/", rules)

    def test_remote_apply_never_omits_push_or_confirmation_guards(self):
        with self.assertRaises(ValueError):
            cleaner.remote_clean("https://github.com/example/repo.git", confirmation="YES", push=False)
        with self.assertRaises(ValueError):
            cleaner.remote_clean("https://github.com/example/repo.git", confirmation="no", push=True)


if __name__ == "__main__":
    unittest.main(verbosity=2)
