import unittest
from unittest.mock import patch

import setup_build_environment as SETUP


class BuildEnvironmentSetupPlanTests(unittest.TestCase):
    def test_windows_setup_includes_required_native_packagers(self):
        packages = {package_id for _, package_id in SETUP.WINDOWS_WINGET_PACKAGES}
        self.assertIn("Rustlang.Rustup", packages)
        self.assertIn("OpenJS.NodeJS.22", packages)
        self.assertIn("MasterPackager.MasterPackagerDev", packages)
        self.assertIn("NSIS.NSIS", packages)

    def test_visual_studio_setup_uses_native_cpp_workload_and_windows_sdk(self):
        self.assertIn("Microsoft.VisualStudio.Workload.VCTools", SETUP.WINDOWS_VS_COMPONENTS)
        self.assertIn("Microsoft.VisualStudio.Component.Windows11SDK.26100", SETUP.WINDOWS_VS_COMPONENTS)

    def test_linux_setup_contains_tauri_webkit_and_flatpak_build_dependencies(self):
        for package in (
            "build-essential",
            "libwebkit2gtk-4.1-dev",
            "libgtk-3-dev",
            "libssl-dev",
            "flatpak",
            "flatpak-builder",
            "dpkg-dev",
        ):
            self.assertIn(package, SETUP.UBUNTU_BUILD_PACKAGES)

    def test_wsl_setup_script_installs_matching_gnome_sdk_and_runtime(self):
        script = SETUP._ubuntu_build_setup_script()
        self.assertIn("org.gnome.Platform//51", script)
        self.assertIn("org.gnome.Sdk//51", script)
        self.assertIn("flatpak-builder", script)
        self.assertIn("GITMANAGER_BWRAP_CHECK", script)

    def test_windows_path_delegates_to_native_setup(self):
        with patch.object(SETUP.platform, "system", return_value="Windows"), patch.object(
            SETUP, "_setup_windows", return_value=0
        ) as windows:
            self.assertEqual(SETUP.setup_build_environment(), 0)
            windows.assert_called_once_with()

    def test_macos_path_delegates_to_apple_setup(self):
        with patch.object(SETUP.platform, "system", return_value="Darwin"), patch.object(
            SETUP, "_setup_macos", return_value=2
        ) as macos:
            self.assertEqual(SETUP.setup_build_environment(), 2)
            macos.assert_called_once_with()


if __name__ == "__main__":
    unittest.main()
