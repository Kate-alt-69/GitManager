import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import build


class WorkspaceBuildCacheTests(unittest.TestCase):
    def make_workspace(self, root: Path) -> None:
        (root / "Cargo.toml").write_text('[workspace]\nmembers = ["src-tauri", "crates/app"]\n', encoding="utf-8")
        (root / "Cargo.lock").write_text("# locked dependencies\n", encoding="utf-8")
        (root / "src-tauri" / "src").mkdir(parents=True)
        (root / "src-tauri" / "src" / "main.rs").write_text("fn main() {}\n", encoding="utf-8")
        (root / "crates" / "app" / "src").mkdir(parents=True)
        (root / "crates" / "app" / "src" / "lib.rs").write_text("pub fn value() -> u8 { 1 }\n", encoding="utf-8")

    def rust_tool_versions(self):
        return [
            (0, "cargo 1.94.0\n", ""),
            (0, "rustc 1.94.0\nhost: x86_64-pc-windows-msvc\n", ""),
        ]

    def test_fingerprint_is_stable_and_changes_with_rust_source(self):
        with tempfile.TemporaryDirectory(prefix="gitmanager-rust-cache-") as temporary:
            root = Path(temporary)
            self.make_workspace(root)
            with patch("build.run_capture", side_effect=self.rust_tool_versions()):
                first, _ = build.workspace_build_fingerprint(
                    root, "x86_64-pc-windows-msvc", "release", ""
                )
            with patch("build.run_capture", side_effect=self.rust_tool_versions()):
                same, _ = build.workspace_build_fingerprint(
                    root, "x86_64-pc-windows-msvc", "release", ""
                )
            self.assertEqual(first, same)

            source = root / "crates" / "app" / "src" / "lib.rs"
            source.write_text("pub fn value() -> u8 { 2 }\n", encoding="utf-8")
            with patch("build.run_capture", side_effect=self.rust_tool_versions()):
                changed, _ = build.workspace_build_fingerprint(
                    root, "x86_64-pc-windows-msvc", "release", ""
                )
            self.assertNotEqual(first, changed)

    def test_cache_requires_every_expected_artifact_and_matching_hashes(self):
        with tempfile.TemporaryDirectory(prefix="gitmanager-rust-cache-") as temporary:
            root = Path(temporary)
            artifacts = []
            for name in ("gitm.exe", "gitmanager-setup.exe", "crash_helper.exe", "gitmanager.service.exe"):
                artifact = root / name
                artifact.write_bytes((name + "-binary").encode("utf-8"))
                artifacts.append((artifact, name))

            cache_path = root / "cache" / "workspace.json"
            build.write_workspace_build_cache(
                cache_path, "fingerprint-1", artifacts, "x86_64-pc-windows-msvc", "release"
            )
            self.assertTrue(
                build.workspace_build_cache_matches(cache_path, "fingerprint-1", artifacts)
            )
            self.assertFalse(
                build.workspace_build_cache_matches(cache_path, "fingerprint-2", artifacts)
            )

            artifacts[2][0].write_bytes(b"tampered binary")
            self.assertFalse(
                build.workspace_build_cache_matches(cache_path, "fingerprint-1", artifacts)
            )

            artifacts[3][0].unlink()
            self.assertFalse(
                build.workspace_build_cache_matches(cache_path, "fingerprint-1", artifacts)
            )

    def test_first_run_adopts_recent_complete_cargo_outputs_without_rebuilding(self):
        with tempfile.TemporaryDirectory(prefix="gitmanager-rust-cache-") as temporary:
            root = Path(temporary)
            self.make_workspace(root)
            target = "x86_64-pc-windows-msvc"
            target_dir = root / "build" / ".rcache"
            profile_dir = target_dir / target / "release"
            fingerprint_dir = profile_dir / ".fingerprint"
            fingerprint_dir.mkdir(parents=True)
            (fingerprint_dir / "cargo-fingerprint").write_text("present", encoding="utf-8")

            source = root / "src-tauri" / "src" / "main.rs"
            artifacts = []
            for name in ("gitm.exe", "gitmanager-setup.exe", "crash_helper.exe", "gitmanager.service.exe"):
                artifact = profile_dir / name
                artifact.write_bytes((name + "-binary").encode("utf-8"))
                artifacts.append((artifact, name))

            with patch.dict(
                "build.os.environ",
                {"CARGO_TARGET_DIR": str(target_dir), "CARGO_BUILD_TARGET": target},
                clear=False,
            ):
                cache_path = build.workspace_build_cache_path(root, target, "release")
                adopted = build.adopt_existing_workspace_build(
                    root,
                    "fingerprint-1",
                    [root / "Cargo.toml", root / "Cargo.lock", source],
                    artifacts,
                    target,
                    "release",
                    cache_path,
                )
            self.assertTrue(adopted)
            self.assertTrue(
                build.workspace_build_cache_matches(cache_path, "fingerprint-1", artifacts)
            )

    def test_first_run_does_not_adopt_outputs_older_than_source_inputs(self):
        with tempfile.TemporaryDirectory(prefix="gitmanager-rust-cache-") as temporary:
            root = Path(temporary)
            self.make_workspace(root)
            target = "x86_64-pc-windows-msvc"
            target_dir = root / "build" / ".rcache"
            profile_dir = target_dir / target / "release"
            fingerprint_dir = profile_dir / ".fingerprint"
            fingerprint_dir.mkdir(parents=True)
            (fingerprint_dir / "cargo-fingerprint").write_text("present", encoding="utf-8")
            artifacts = []
            for name in ("gitm.exe", "gitmanager-setup.exe", "crash_helper.exe", "gitmanager.service.exe"):
                artifact = profile_dir / name
                artifact.write_bytes((name + "-binary").encode("utf-8"))
                artifacts.append((artifact, name))

            source = root / "src-tauri" / "src" / "main.rs"
            # Make the source input newer than the existing binaries.
            source.write_text("fn main() { /* changed after the cached build */ }\n", encoding="utf-8")
            with patch.dict(
                "build.os.environ",
                {"CARGO_TARGET_DIR": str(target_dir), "CARGO_BUILD_TARGET": target},
                clear=False,
            ):
                cache_path = build.workspace_build_cache_path(root, target, "release")
                adopted = build.adopt_existing_workspace_build(
                    root,
                    "fingerprint-1",
                    [root / "Cargo.toml", root / "Cargo.lock", source],
                    artifacts,
                    target,
                    "release",
                    cache_path,
                )
            self.assertFalse(adopted)
            self.assertFalse(cache_path.exists())


if __name__ == "__main__":
    unittest.main()
