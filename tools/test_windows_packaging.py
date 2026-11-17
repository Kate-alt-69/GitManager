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
    def test_exact_winget_package_ids_are_used(self):
        self.assertEqual(
            BUILD.WINDOWS_PACKAGING_WINGET_PACKAGES,
            (
                ("WiX Toolset v3", "WiXToolset.WiXToolset"),
                ("NSIS", "NSIS.NSIS"),
                ("Master Packager Dev", "MasterPackager.MasterPackagerDev"),
            ),
        )

    def test_nsis_bin_directory_is_searched_for_makensis(self):
        directories = [str(path).replace("\\", "/") for path in BUILD.windows_packaging_search_directories()]
        self.assertTrue(any(path.endswith("/NSIS/Bin") for path in directories))

    def test_tauri_builds_both_nsis_and_wix_installers(self):
        config = json.loads((ROOT / "src-tauri" / "tauri.windows.conf.json").read_text(encoding="utf-8"))
        self.assertEqual(set(config["bundle"]["targets"]), {"nsis", "msi"})

    def test_sidecar_staging_uses_the_selected_cross_compile_target(self):
        with patch.dict(os.environ, {"CARGO_BUILD_TARGET": "i686-pc-windows-msvc"}):
            self.assertEqual(BUILD.resolve_rust_target_triple(""), "i686-pc-windows-msvc")

    def test_python_runtime_is_mapped_as_a_recursive_directory(self):
        config = json.loads((ROOT / "src-tauri" / "tauri.conf.json").read_text(encoding="utf-8"))
        self.assertEqual(
            config["bundle"]["resources"].get("../tools/runtime/python/"),
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

    def test_master_packager_project_is_x64_msi_and_uses_the_shared_stage(self):
        config = json.loads(
            (ROOT / "installer" / "master-packager" / "package.json").read_text(encoding="utf-8")
        )
        self.assertEqual(config["outputTypes"], ["msi"])
        self.assertEqual(config["platform"], "x64")
        self.assertEqual(config["outputDirectory"], "build/.rcache/windows-installers")
        self.assertEqual(
            config["msi"]["installDir"],
            "[LocalAppDataFolder]kastrick's_software\\GitManager",
        )
        self.assertEqual(config["fileSystemEntries"][0]["sourcePath"], "%GITMANAGER_APP_SOURCE%")
        self.assertEqual(config["fileSystemEntries"][0]["targetPath"], "$.installDir")

    def test_store_msix_is_not_emitted_by_the_desktop_mpdev_manifest(self):
        desktop = json.loads(
            (ROOT / "installer" / "master-packager" / "package.json").read_text(encoding="utf-8")
        )
        store = json.loads(
            (ROOT / "installer" / "store" / "msix" / "package.json").read_text(encoding="utf-8")
        )
        self.assertEqual(desktop["outputTypes"], ["msi"])
        self.assertNotIn("msix", desktop)
        self.assertEqual(store["outputTypes"], ["msix"])

    def test_mpdev_comparison_is_documented_as_an_additional_desktop_installer(self):
        docs = (ROOT / "docs" / "windows-packaging.md").read_text(encoding="utf-8")
        self.assertIn("Additional comparison MSI", docs)
        self.assertIn("--master-packager", docs)


if __name__ == "__main__":
    unittest.main()
