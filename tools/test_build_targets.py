import unittest

from build_targets import parse_build_selection


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


if __name__ == "__main__":
    unittest.main()
