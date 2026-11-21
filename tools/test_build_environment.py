import unittest
from unittest.mock import patch

import setup_build_environment as SETUP


class BuildEnvironmentSetupPlanTests(unittest.TestCase):
    def test_windows_setup_includes_only_native_packagers_not_an_unconditional_rust_install(self):
        packages = {package_id for _, package_id in SETUP.WINDOWS_WINGET_PACKAGES}
        self.assertIn("OpenJS.NodeJS.22", packages)
        self.assertIn("MasterPackager.MasterPackagerDev", packages)
        self.assertIn("NSIS.NSIS", packages)
        self.assertNotIn("Rustlang.Rustup", packages)
        self.assertNotIn("Microsoft.EdgeWebView2Runtime", packages)

    def test_manually_installed_packagers_are_detected_without_winget(self):
        with patch.object(SETUP, "_winget_is_installed", return_value=False), patch.object(
            SETUP.shutil, "which",
            side_effect=lambda name: "C:/Tools/mpdev.exe" if name in ("mpdev", "mpdev.exe") else None,
        ):
            self.assertTrue(SETUP._windows_package_available("MasterPackager.MasterPackagerDev"))
        with patch.object(SETUP, "_winget_is_installed", return_value=False), patch.object(
            SETUP.shutil, "which",
            side_effect=lambda name: "C:/Tools/makensis.exe" if name in ("makensis", "makensis.exe") else None,
        ):
            self.assertTrue(SETUP._windows_package_available("NSIS.NSIS"))

    def test_visual_studio_setup_uses_native_cpp_workload_and_windows_sdk(self):
        self.assertIn("Microsoft.VisualStudio.Workload.VCTools", SETUP.WINDOWS_VS_COMPONENTS)
        self.assertIn("Microsoft.VisualStudio.Component.Windows11SDK.26100", SETUP.WINDOWS_VS_COMPONENTS)

    def test_linux_base_setup_does_not_install_flatpak_unless_requested(self):
        self.assertIn("libwebkit2gtk-4.1-dev", SETUP.UBUNTU_BUILD_PACKAGES)
        self.assertIn("libssl-dev", SETUP.UBUNTU_BUILD_PACKAGES)
        self.assertIn("dpkg-dev", SETUP.UBUNTU_BUILD_PACKAGES)
        self.assertNotIn("flatpak", SETUP.UBUNTU_BUILD_PACKAGES)
        self.assertNotIn("flatpak-builder", SETUP.UBUNTU_BUILD_PACKAGES)
        self.assertEqual(SETUP.FLATPAK_BUILD_PACKAGES, ("flatpak", "flatpak-builder"))

    def test_linux_script_installs_gnome_sdk_only_for_flatpak_target(self):
        linux_script = SETUP._ubuntu_build_setup_script(include_flatpak=False, configure_wsl=False)
        flatpak_script = SETUP._ubuntu_build_setup_script(include_flatpak=True, configure_wsl=False)
        self.assertNotIn("org.gnome.Platform//51", linux_script)
        self.assertNotIn("org.gnome.Sdk//51", linux_script)
        self.assertIn("org.gnome.Platform//51", flatpak_script)
        self.assertIn("org.gnome.Sdk//51", flatpak_script)
        self.assertIn("flatpak-builder", flatpak_script)
        self.assertIn("bwrap --unshare-user", flatpak_script)

    def test_linux_setup_reuses_existing_rust_instead_of_resetting_default(self):
        script = SETUP._ubuntu_build_setup_script(include_flatpak=False, configure_wsl=False)
        self.assertIn("if ! command -v cargo", script)
        self.assertIn("Using existing Rust toolchain", script)
        self.assertIn("rustup default stable", script)
        self.assertIn("if ! command -v cargo", script)

    def test_wsl_first_run_instructions_explain_invisible_password_entry(self):
        with patch.object(SETUP, "_log") as log:
            SETUP._log_wsl_first_run_instructions()
        instructions = " ".join(call.args[0] for call in log.call_args_list)
        self.assertIn("Enter new UNIX username", instructions)
        self.assertIn("Nothing appears while typing", instructions)
        self.assertIn("Retype new password", instructions)
        self.assertIn("py build.py setup-build-tools --build=all", instructions)

    def test_install_prompt_defaults_yes_for_requested_target_and_no_for_optional_target(self):
        with patch("builtins.input", return_value=""):
            self.assertTrue(SETUP._confirm_install("Ubuntu and WebKitGTK", "Linux", default_yes=True))
            self.assertFalse(SETUP._confirm_install("Ubuntu and WebKitGTK", "Linux", default_yes=False))

    def test_explicit_linux_target_on_windows_requests_wsl_with_yes_as_default(self):
        with patch.object(SETUP.platform, "system", return_value="Windows"), patch.object(
            SETUP.platform, "machine", return_value="AMD64"
        ), patch.object(SETUP, "_setup_wsl_linux", return_value=0) as wsl_setup:
            result = SETUP.setup_build_environment("linux(x64)")
            self.assertEqual(result, 0)
            wsl_setup.assert_called_once_with(include_flatpak=False, default_yes=True)

    def test_unqualified_windows_setup_does_not_install_unrequested_wsl_sdk(self):
        with patch.object(SETUP.platform, "system", return_value="Windows"), patch.object(
            SETUP, "_setup_windows", return_value=0
        ) as windows_setup, patch.object(SETUP, "_setup_wsl_linux") as wsl_setup:
            result = SETUP.setup_build_environment()
            self.assertEqual(result, 0)
            windows_setup.assert_called_once()
            self.assertEqual(windows_setup.call_args.kwargs["default_yes"], True)
            wsl_setup.assert_not_called()

    def test_unmatched_linux_arm64_target_does_not_install_x64_wsl_sdk(self):
        with patch.object(SETUP.platform, "system", return_value="Windows"), patch.object(
            SETUP.platform, "machine", return_value="AMD64"
        ), patch.object(SETUP, "_setup_wsl_linux") as wsl_setup:
            result = SETUP.setup_build_environment("linux(arm64)")
            self.assertEqual(result, 2)
            wsl_setup.assert_not_called()

    def test_flatpak_store_selection_requests_linux_plus_flatpak_sdk(self):
        with patch.object(SETUP.platform, "system", return_value="Windows"), patch.object(
            SETUP, "_setup_wsl_linux", return_value=0
        ) as wsl_setup:
            result = SETUP.setup_build_environment(None, "flatpak")
            self.assertEqual(result, 0)
            wsl_setup.assert_called_once_with(include_flatpak=True, default_yes=True)

    def test_all_setup_on_windows_prepares_windows_and_wsl_but_skips_macos(self):
        with patch.object(SETUP.platform, "system", return_value="Windows"), patch.object(
            SETUP.platform, "machine", return_value="AMD64"
        ), patch.object(SETUP, "_setup_windows", return_value=0) as windows_setup, patch.object(
            SETUP, "_setup_wsl_linux", return_value=0
        ) as wsl_setup:
            result = SETUP.setup_build_environment("all")
            self.assertEqual(result, 0)
            windows_setup.assert_called_once()
            wsl_setup.assert_called_once_with(include_flatpak=False, default_yes=True)

    def test_all_setup_on_macos_prepares_macos_and_skips_foreign_sdk_installers(self):
        with patch.object(SETUP.platform, "system", return_value="Darwin"), patch.object(
            SETUP.platform, "machine", return_value="arm64"
        ), patch.object(SETUP, "_setup_macos", return_value=0) as mac_setup, patch.object(
            SETUP, "_setup_native_linux"
        ) as linux_setup:
            result = SETUP.setup_build_environment("all")
            self.assertEqual(result, 0)
            mac_setup.assert_called_once()
            linux_setup.assert_not_called()

    def test_explicit_linux_target_on_macos_is_rejected(self):
        with patch.object(SETUP.platform, "system", return_value="Darwin"), patch.object(
            SETUP.platform, "machine", return_value="arm64"
        ), patch.object(SETUP, "_setup_native_linux") as linux_setup:
            result = SETUP.setup_build_environment("linux(x64)")
            self.assertEqual(result, 2)
            linux_setup.assert_not_called()

    def test_macos_sdk_is_not_claimed_available_on_windows(self):
        with patch.object(SETUP.platform, "system", return_value="Windows"), patch.object(
            SETUP, "_setup_windows", return_value=0
        ), patch.object(SETUP, "_setup_wsl_linux", return_value=0):
            self.assertEqual(SETUP.setup_build_environment("macos(arm64)"), 2)

if __name__ == "__main__":
    unittest.main()
