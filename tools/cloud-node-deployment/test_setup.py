from __future__ import annotations

import importlib.util
import io
import json
import os
import stat
import tempfile
import unittest
import zipfile
from pathlib import Path


SETUP_PATH = Path(__file__).with_name("setup.py")
SPEC = importlib.util.spec_from_file_location("rbe_node_setup", SETUP_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError("Could not load RBE node setup.py for tests.")
setup = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(setup)


class SecretFilePermissionTests(unittest.TestCase):
    def test_secret_file_permissions_are_restricted(self) -> None:
        with tempfile.TemporaryDirectory(prefix="gitmanager-rbe-node-test-") as directory:
            secret_file = Path(directory) / "node-private-key"
            setup.secure_write(secret_file, "test-only-private-key\n", secret=True)

            if os.name == "nt":
                sid = setup.windows_current_user_sid()
                setup.restrict_secret_file_permissions(secret_file)
                system_root = Path(os.environ["SystemRoot"])
                powershell = system_root / "System32" / "WindowsPowerShell" / "v1.0" / "powershell.exe"
                quoted_path = "'" + str(secret_file).replace("'", "''") + "'"
                result = setup.run_checked(
                    [
                        powershell,
                        "-NoLogo",
                        "-NoProfile",
                        "-NonInteractive",
                        "-Command",
                        f"(Get-Acl -LiteralPath {quoted_path}).Sddl",
                    ],
                    timeout=15,
                )
                self.assertEqual(result.returncode, 0, result.stderr or result.stdout)
                sddl = result.stdout.strip().upper()
                self.assertIn("D:", sddl)
                dacl = sddl.split("D:", 1)[1].split("S:", 1)[0]
                self.assertIn(sid.upper(), dacl)
                for broad_sid in ("S-1-1-0", "S-1-5-11", "S-1-5-32-545"):
                    self.assertNotIn(broad_sid, dacl)
            else:
                self.assertEqual(stat.S_IMODE(secret_file.stat().st_mode), 0o600)
                setup.restrict_secret_file_permissions(secret_file)
                self.assertEqual(stat.S_IMODE(secret_file.stat().st_mode), 0o600)


class BootstrapManifestTests(unittest.TestCase):
    def test_manifest_has_all_supported_targets_and_release_endpoint(self) -> None:
        manifest = setup.load_manifest()
        self.assertEqual(set(manifest["targets"]), set(setup.REQUIRED_FILES))
        self.assertEqual(
            manifest["releaseApi"],
            "https://api.github.com/repos/Kastrick8/RBE/releases/latest",
        )

    def test_bootstrap_contains_health_route_and_starter_settings(self) -> None:
        self.assertTrue((setup.PACKAGE_ROOT / "api" / "health.route").is_file())
        self.assertTrue((setup.PACKAGE_ROOT / "settings.json").is_file())
        self.assertTrue((setup.PACKAGE_ROOT / "setting.node.cn.json").is_file())
        config = json.loads((setup.PACKAGE_ROOT / "setting.node.cn.json").read_text(encoding="utf-8"))
        self.assertEqual(config["formatVersion"], 1)
        self.assertEqual(config["node"]["mode"], "primary")
        self.assertEqual(config["node"]["backupVersions"], 5)
        self.assertEqual(config["node"]["videoChunkBytes"], 4 * 1024 * 1024)
        self.assertIn("replication", config)
        self.assertFalse(config["replication"]["requireBootRecovery"])
        route = (setup.PACKAGE_ROOT / "api" / "health.route").read_text(encoding="utf-8")
        self.assertIn(":import[net]", route)
        self.assertIn("net.ping()", route)
        self.assertNotIn("private.health()", route)

    def test_safe_relative_member_rejects_traversal_and_windows_drive_paths(self) -> None:
        for unsafe in ("../backend", "dep/../../backend", "/backend", r"dep\backend", "C:/backend"):
            with self.subTest(path=unsafe):
                with self.assertRaises(setup.SetupError):
                    setup.safe_relative_member(unsafe)
        self.assertEqual(setup.safe_relative_member("./dep/container"), "dep/container")

    def test_checksum_manifest_requires_one_matching_entry(self) -> None:
        digest = "a" * 64
        setup.verify_checksum_manifest(
            f"{digest}  rbe-runtime.zip\n".encode("utf-8"),
            "rbe-runtime.zip",
            digest,
        )
        with self.assertRaises(setup.SetupError):
            setup.verify_checksum_manifest(
                f"{'b' * 64}  rbe-runtime.zip\n".encode("utf-8"),
                "rbe-runtime.zip",
                digest,
            )


class RuntimeArchiveTests(unittest.TestCase):
    def make_windows_archive(self, *, symlink_cloud_node=False) -> bytes:
        output = io.BytesIO()
        names = setup.REQUIRED_FILES["windows-x64"]
        with zipfile.ZipFile(output, "w", compression=zipfile.ZIP_DEFLATED) as archive:
            for name in names:
                payload = b'{"release":"test"}' if name == "build-info.json" else b"test-runtime-binary"
                if symlink_cloud_node and name == "cloud_node.exe":
                    info = zipfile.ZipInfo(name)
                    info.create_system = 3
                    info.external_attr = (stat.S_IFLNK | 0o777) << 16
                    archive.writestr(info, b"target")
                else:
                    archive.writestr(name, payload)
        return output.getvalue()

    def test_extracts_complete_windows_runtime_without_archive_extractall(self) -> None:
        payload = self.make_windows_archive()
        with tempfile.TemporaryDirectory(prefix="gitmanager-rbe-extract-") as directory:
            stage = Path(directory)
            setup.extract_runtime_archive(payload, "zip", "windows-x64", stage)
            for relative in setup.REQUIRED_FILES["windows-x64"]:
                self.assertTrue((stage / relative).is_file(), relative)
            self.assertEqual(
                json.loads((stage / "build-info.json").read_text(encoding="utf-8")),
                {"release": "test"},
            )

    def test_rejects_symlink_for_required_runtime_binary(self) -> None:
        payload = self.make_windows_archive(symlink_cloud_node=True)
        with tempfile.TemporaryDirectory(prefix="gitmanager-rbe-symlink-") as directory:
            with self.assertRaises(setup.SetupError):
                setup.extract_runtime_archive(payload, "zip", "windows-x64", Path(directory))


if __name__ == "__main__":
    unittest.main()
