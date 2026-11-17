import unittest
from unittest.mock import patch

from build_targets import BuildTarget, can_build_locally, parse_build_selection, mpdev_platform_name


class BuildTargetSelectionTests(unittest.TestCase):
    def test_all_includes_every_supported_desktop_target(self):
        kind, targets = parse_build_selection("all")
        self.assertEqual(kind, "all")
        self.assertEqual(len(targets), 8)
        self.assertEqual(
            {target.triple for target in targets},
            {
                "x86_64-pc-windows-msvc",
                "i686-pc-windows-msvc",
                "aarch64-pc-windows-msvc",
                "x86_64-unknown-linux-gnu",
                "aarch64-unknown-linux-gnu",
                "armv7-unknown-linux-gnueabihf",
                "x86_64-apple-darwin",
                "aarch64-apple-darwin",
            },
        )

    def test_platform_group_selects_all_architectures_for_that_platform(self):
        kind, windows = parse_build_selection("windows")
        self.assertEqual(kind, "group")
        self.assertEqual({target.architecture for target in windows}, {"x64", "x86", "arm64"})
        _, linux = parse_build_selection("linux")
        self.assertEqual({target.architecture for target in linux}, {"x64", "arm64", "armv7"})

    def test_mpdev_uses_documented_platform_names(self):
        self.assertEqual(mpdev_platform_name("x86"), "x86")
        self.assertEqual(mpdev_platform_name("x64"), "x64")
        self.assertEqual(mpdev_platform_name("arm64"), "Arm64")
        self.assertEqual(mpdev_platform_name(" ARM64 "), "Arm64")
        with self.assertRaises(ValueError):
            mpdev_platform_name("armv7")

    def test_windows_version_is_metadata_not_a_different_binary_target(self):
        _, win10 = parse_build_selection("windows(10,x64)")
        _, win11 = parse_build_selection("windows(11,x64)")
        _, win11_x86 = parse_build_selection("windows(11,x86)")
        self.assertEqual(win10[0].triple, win11[0].triple)
        self.assertEqual(win10[0].output_name, "windows-x64")
        self.assertEqual(win11[0].output_name, "windows-x64")
        self.assertEqual(win11_x86[0].triple, "i686-pc-windows-msvc")

    def test_architecture_only_selectors_are_supported(self):
        _, linux = parse_build_selection("linux(arm64)")
        _, macos = parse_build_selection("macos(x64)")
        self.assertEqual(linux[0].triple, "aarch64-unknown-linux-gnu")
        _, linux_armv7 = parse_build_selection("linux(armv7)")
        self.assertEqual(linux_armv7[0].triple, "armv7-unknown-linux-gnueabihf")
        self.assertEqual(macos[0].triple, "x86_64-apple-darwin")

    def test_invalid_os_versions_and_architectures_are_rejected(self):
        for value in ("windows(12,x64)", "windows(10,arm64)", "windows(11,mips)", "linux(x86)", "macos(x86)"):
            with self.subTest(value=value):
                with self.assertRaises(ValueError):
                    parse_build_selection(value)


    def test_windows_arm64_selector_defaults_to_windows_11(self):
        target = BuildTarget("windows", "arm64", "aarch64-pc-windows-msvc")
        self.assertEqual(target.selector, "windows(11,arm64)")

    def test_windows_x64_host_can_build_x64_and_x86_but_not_arm64_without_arm_toolchain(self):
        with patch("build_targets.platform.system", return_value="Windows"), patch(
            "build_targets.platform.machine", return_value="AMD64"
        ):
            x64 = BuildTarget("windows", "x64", "x86_64-pc-windows-msvc")
            x86 = BuildTarget("windows", "x86", "i686-pc-windows-msvc")
            arm64 = BuildTarget("windows", "arm64", "aarch64-pc-windows-msvc")
            self.assertTrue(can_build_locally(x64)[0])
            self.assertTrue(can_build_locally(x86)[0])
            self.assertFalse(can_build_locally(arm64)[0])

    def test_windows_host_does_not_dispatch_or_claim_linux_targets_are_local(self):
        with patch("build_targets.platform.system", return_value="Windows"), patch(
            "build_targets.platform.machine", return_value="AMD64"
        ):
            linux = BuildTarget("linux", "x64", "x86_64-unknown-linux-gnu")
            allowed, reason = can_build_locally(linux)
            self.assertFalse(allowed)
            self.assertIn("requires a linux host", reason)

if __name__ == "__main__":
    unittest.main()
