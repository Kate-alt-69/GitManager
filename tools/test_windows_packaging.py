import importlib.util
import json
import os
import sys
import unittest
from unittest.mock import patch
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("gitmanager_build", ROOT / "build.py")
BUILD = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = BUILD
SPEC.loader.exec_module(BUILD)


class WindowsPackagingConfigurationTests(unittest.TestCase):
    def test_only_mpdev_and_nsis_are_installed_for_windows_packaging(self):
        self.assertEqual(
            BUILD.WINDOWS_PACKAGING_WINGET_PACKAGES,
            (
                ("NSIS fallback installer", "NSIS.NSIS"),
                ("Master Packager Dev (required MSI/MSIX packager)", "MasterPackager.MasterPackagerDev"),
            ),
        )

    def test_nsis_bin_directory_is_searched_for_makensis(self):
        directories = [str(path).replace("\\", "/") for path in BUILD.windows_packaging_search_directories()]
        self.assertTrue(any(path.endswith("/NSIS/Bin") for path in directories))

    def test_tauri_keeps_nsis_only_as_the_windows_fallback(self):
        config = json.loads((ROOT / "src-tauri" / "tauri.windows.conf.json").read_text(encoding="utf-8"))
        self.assertEqual(config["bundle"]["targets"], ["nsis"])

    def test_sidecar_staging_uses_the_selected_cross_compile_target(self):
        with patch.dict(os.environ, {"CARGO_BUILD_TARGET": "i686-pc-windows-msvc"}):
            self.assertEqual(BUILD.resolve_rust_target_triple(""), "i686-pc-windows-msvc")

    def test_python_runtime_is_mapped_only_in_the_windows_bundle(self):
        main_config = json.loads((ROOT / "src-tauri" / "tauri.conf.json").read_text(encoding="utf-8"))
        windows_config = json.loads((ROOT / "src-tauri" / "tauri.windows.conf.json").read_text(encoding="utf-8"))
        self.assertEqual(
            main_config["bundle"]["resources"].get("../tools/gitmanager_cleaner.py"),
            "tools/gitmanager_cleaner.py",
        )
        self.assertNotIn("../tools/runtime/python/", main_config["bundle"]["resources"])
        self.assertEqual(
            windows_config["bundle"]["resources"].get("../tools/runtime/python/"),
            "python/",
        )

    def test_native_window_can_shrink_to_the_responsive_layout(self):
        config = json.loads((ROOT / "src-tauri" / "tauri.conf.json").read_text(encoding="utf-8"))
        window = config["app"]["windows"][0]
        self.assertLessEqual(window["minWidth"], 720)
        self.assertLessEqual(window["minHeight"], 520)

    def test_nsis_hook_forces_the_expected_path_and_checks_python(self):
        hooks = (ROOT / "src-tauri" / "windows" / "hooks.nsh").read_text(encoding="utf-8")
        self.assertIn(r"$LOCALAPPDATA\kastrick's_software\GitManager", hooks)
        self.assertIn(r"$INSTDIR\resources\python\python.exe", hooks)
        self.assertIn("NSIS_HOOK_POSTINSTALL", hooks)

    def test_mpdev_desktop_manifest_builds_both_msi_and_msix(self):
        config = json.loads(
            (ROOT / "installer" / "master-packager" / "package.json").read_text(encoding="utf-8")
        )
        self.assertEqual(config["outputTypes"], ["msi", "msix"])
        self.assertEqual(config["platform"], "x64")
        self.assertEqual(config["outputDirectory"], "build/.rcache/windows-installers")
        self.assertEqual(
            config["msi"]["installDir"],
            "[LocalAppDataFolder]kastrick's_software\\GitManager",
        )
        self.assertEqual(config["fileSystemEntries"][0]["sourcePath"], "%GITMANAGER_APP_SOURCE%")
        self.assertEqual(config["fileSystemEntries"][0]["targetPath"], "$.installDir")
        self.assertEqual(config["msix"]["digitalSignature"]["signWith"], "GenerateTestCertificate")

    def test_store_msix_has_its_own_identity_manifest(self):
        desktop = json.loads(
            (ROOT / "installer" / "master-packager" / "package.json").read_text(encoding="utf-8")
        )
        store = json.loads(
            (ROOT / "installer" / "store" / "msix" / "package.json").read_text(encoding="utf-8")
        )
        self.assertEqual(desktop["outputTypes"], ["msi", "msix"])
        self.assertEqual(store["outputTypes"], ["msix"])
        self.assertNotEqual(desktop["outputDirectory"], store["outputDirectory"])

    def test_windows_installer_build_invokes_mpdev_for_both_formats(self):
        source = (ROOT / "build.py").read_text(encoding="utf-8")
        self.assertIn("build_master_packager_installers(project_root, mpdev_manifest)", source)
        self.assertIn("prepare_master_packager_manifest(project_root)", source)
        self.assertIn('package_config["outputTypes"] = ["msi", "msix"]', source)
        self.assertIn("MPDEV is the required", source)

    def test_docs_describe_mpdev_as_mandatory_and_nsis_as_fallback(self):
        docs = (ROOT / "docs" / "windows-packaging.md").read_text(encoding="utf-8")
        self.assertIn("required Windows MSI/MSIX packager", docs)
        self.assertIn("NSIS fallback", docs)
        self.assertIn("--master-packager", docs)


if __name__ == "__main__":
    unittest.main()
