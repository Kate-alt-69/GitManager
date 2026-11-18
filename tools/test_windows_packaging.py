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

from package_windows_store import configure_store_msix_identity


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
        # Supply Windows install roots explicitly so this path-construction
        # test is deterministic when the suite runs on Linux/macOS CI.
        with patch.dict(
            os.environ,
            {
                "ProgramFiles(x86)": r"C:\Program Files (x86)",
                "ProgramFiles": r"C:\Program Files",
                "LOCALAPPDATA": r"C:\Users\GitManager\AppData\Local",
            },
            clear=True,
        ):
            directories = [
                str(path).replace("\\", "/")
                for path in BUILD.windows_packaging_search_directories()
            ]
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

    def test_nsis_installer_uses_the_kastrick_default_and_loads_hooks(self):
        config = json.loads((ROOT / "src-tauri" / "tauri.windows.conf.json").read_text(encoding="utf-8"))
        nsis = config["bundle"]["windows"]["nsis"]
        self.assertEqual(nsis["installMode"], "currentUser")
        self.assertEqual(nsis["template"], "windows/installer.nsi")
        self.assertEqual(nsis["installerHooks"], "windows/hooks.nsh")
        template = (ROOT / "src-tauri" / "windows" / "installer.nsi").read_text(encoding="utf-8")
        self.assertIn(r"$LOCALAPPDATA\kastrick's_software", template)
        self.assertNotIn("$LOCALAPPDATA\\${PRODUCTNAME}", template)

    def test_nsis_preinstall_respects_browse_selection_and_verifies_python(self):
        hooks = (ROOT / "src-tauri" / "windows" / "hooks.nsh").read_text(encoding="utf-8")
        self.assertNotIn("StrCpy $INSTDIR", hooks)
        self.assertNotIn("install_directory_invalid", hooks)
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
            config["installDir"],
            "%LOCALAPPDATA%\\kastrick's_software\\GitManager",
        )
        self.assertNotIn("installDir", config["msi"])
        msi_properties = {item["name"]: item["value"] for item in config["msi"]["properties"]}
        self.assertEqual(msi_properties["ALLUSERS"], "2")
        self.assertEqual(msi_properties["MSIINSTALLPERUSER"], "1")
        dependency = config["msi"]["packageDependencies"][0]
        self.assertEqual(dependency["displayName"], "Microsoft Edge WebView2 Runtime")
        self.assertEqual(config["fileSystemEntries"][0]["sourcePath"], "%GITMANAGER_APP_SOURCE%")
        self.assertEqual(config["fileSystemEntries"][0]["targetPath"], "$.installDir")
        self.assertEqual(config["msix"]["digitalSignature"]["signWith"], "GenerateTestCertificate")
        dialog = config["msi"]["installDialog"]
        self.assertIn("Git workspace manager", dialog["packageDescription"])
        self.assertEqual(dialog["publisherUrl"], "https://kastrick.vercel.app")
        self.assertEqual(dialog["primaryAccent"], "#F97316")
        self.assertEqual(dialog["promptAppsAfterInstall"][0]["file"], "$.installDir\\gitmanager.exe")

    def test_service_worker_is_packaged_but_not_registered_as_a_machine_service(self):
        desktop = json.loads(
            (ROOT / "installer" / "master-packager" / "package.json").read_text(encoding="utf-8")
        )
        store = json.loads(
            (ROOT / "installer" / "store" / "msix" / "package.json").read_text(encoding="utf-8")
        )
        build_source = (ROOT / "build.py").read_text(encoding="utf-8")
        store_source = (ROOT / "tools" / "package_windows_store.py").read_text(encoding="utf-8")
        watcher_source = (ROOT / "src-tauri" / "src" / "watcher_service.rs").read_text(encoding="utf-8")

        self.assertNotIn("services", desktop)
        self.assertNotIn("services", store)
        self.assertIn('"gitmanager.service.exe": sidecar_dir / f"gitmanager.service-{triple}.exe"', build_source)
        self.assertIn('"gitmanager.service.exe"', store_source)
        self.assertIn('"/SC", "ONLOGON"', watcher_source)
        self.assertIn("git-manager.exe", desktop["msi"]["detectRunningProcesses"])
        self.assertIn("git-manager-service.exe", desktop["msi"]["detectRunningProcesses"])
        self.assertIn("GitManager Background Service", watcher_source)
        self.assertIn("gitmanager-updater.service", watcher_source)
        self.assertIn("com.gitmanager.updater", watcher_source)
        startup = watcher_source.split("pub fn ensure_required_service", 1)[1]
        self.assertLess(
            startup.index("stop_legacy_background_service();"),
            startup.index("request_service_status()"),
        )
        self.assertIn("Windows Task Scheduler (user logon)", watcher_source)
        store_capabilities = store["msix"]["capabilities"]
        self.assertNotIn("packagedServices", store_capabilities)
        self.assertNotIn("localSystemServices", store_capabilities)

    def test_store_msix_requires_both_partner_center_identity_values(self):
        package = {"msix": {"digitalSignature": {"signWith": "GenerateTestCertificate"}}}
        with self.assertRaisesRegex(ValueError, "Set both MSIX_PUBLISHER"):
            configure_store_msix_identity(package, "CN=Publisher", "")
        with self.assertRaisesRegex(ValueError, "Set both MSIX_PUBLISHER"):
            configure_store_msix_identity(package, "", "StoreIdentity")
        self.assertEqual(
            package["msix"]["digitalSignature"]["signWith"],
            "GenerateTestCertificate",
        )

    def test_store_msix_only_removes_test_signing_when_identity_is_complete(self):
        package = {"msix": {"digitalSignature": {"signWith": "GenerateTestCertificate"}}}
        self.assertFalse(configure_store_msix_identity(package, "", ""))
        self.assertIn("digitalSignature", package["msix"])
        self.assertTrue(configure_store_msix_identity(package, "CN=Publisher", "StoreIdentity"))
        self.assertEqual(package["publisher"], "CN=Publisher")
        self.assertEqual(package["packageName"], "StoreIdentity")
        self.assertNotIn("digitalSignature", package["msix"])

    def test_store_packager_uses_mpdev_architecture_enum(self):
        source = (ROOT / "tools" / "package_windows_store.py").read_text(encoding="utf-8")
        self.assertIn('package_config["platform"] = mpdev_platform_name(args.platform)', source)

    def test_background_worker_is_not_registered_as_a_machine_service(self):
        desktop = json.loads(
            (ROOT / "installer" / "master-packager" / "package.json").read_text(encoding="utf-8")
        )
        store = json.loads(
            (ROOT / "installer" / "store" / "msix" / "package.json").read_text(encoding="utf-8")
        )
        self.assertNotIn("services", desktop)
        self.assertNotIn("services", store)
        watcher = (ROOT / "src-tauri" / "src" / "watcher_service.rs").read_text(encoding="utf-8")
        self.assertIn('args(["/Create", "/SC", "ONLOGON",', watcher)
        self.assertIn('args(["/RL", "LIMITED", "/F"])', watcher)
        self.assertIn(".config/systemd/user", watcher)
        self.assertIn("Library/LaunchAgents", watcher)

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

    def test_local_store_msix_passes_staged_source_to_mpdev(self):
        source = (ROOT / "build.py").read_text(encoding="utf-8")
        self.assertIn('environment["GITMANAGER_APP_SOURCE"] = str(', source)
        self.assertIn('str(mpdev), "build", str(manifest)', source)
        self.assertIn('env=environment, check=False', source)

    def test_windows_installer_preflights_mpdev_and_nsis_before_compiling(self):
        source = (ROOT / "build.py").read_text(encoding="utf-8")
        self.assertIn("mpdev_cli = find_master_packager_dev_cli()", source)
        self.assertIn("nsis_available = bool(shutil.which(\"makensis\"))", source)
        self.assertIn("Master Packager Dev is mandatory for Windows MSI/MSIX builds.", source)
        self.assertIn("NSIS is required as the fallback Windows setup installer.", source)

    def test_flatpak_builder_exports_the_branch_used_by_the_bundle(self):
        source = (ROOT / "tools" / "package_flatpak.py").read_text(encoding="utf-8")
        self.assertIn('"--default-branch=stable"', source)
        self.assertIn('str(repo), str(bundle), APP_ID, "stable"', source)

    def test_flatpak_stream_builds_only_the_deb_bundle(self):
        source = (ROOT / "build.py").read_text(encoding="utf-8")
        self.assertIn('bundle_formats="deb"', source)
        self.assertIn('cmd.extend(["--bundles", bundle_formats])', source)

    def test_flatpak_is_limited_to_runtime_architectures_and_checks_deb_arch(self):
        build_source = (ROOT / "build.py").read_text(encoding="utf-8")
        flatpak_source = (ROOT / "tools" / "package_flatpak.py").read_text(encoding="utf-8")
        self.assertIn('target.architecture not in ("x64", "arm64")', build_source)
        self.assertIn('expected_flatpak_arch = {"amd64": "x86_64", "arm64": "aarch64"}', flatpak_source)
        self.assertIn("if arch != expected_flatpak_arch:", flatpak_source)

    def test_local_build_script_never_dispatches_builds_to_github_actions(self):
        source = (ROOT / "build.py").read_text(encoding="utf-8").lower()
        self.assertNotIn("gh workflow run", source)
        self.assertNotIn("dispatch_build_matrix", source)
        self.assertNotIn("dispatch_store_build", source)

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

    def test_repository_scan_engine_is_enabled_only_by_the_headless_service(self):
        core_manifest = (ROOT / "crates" / "gitmanager-core" / "Cargo.toml").read_text(encoding="utf-8")
        core_library = (ROOT / "crates" / "gitmanager-core" / "src" / "lib.rs").read_text(encoding="utf-8")
        desktop_manifest = (ROOT / "src-tauri" / "Cargo.toml").read_text(encoding="utf-8")
        service_manifest = (ROOT / "crates" / "git-manager-service" / "Cargo.toml").read_text(encoding="utf-8")
        crash_helper_manifest = (ROOT / "crates" / "crash-helper" / "Cargo.toml").read_text(encoding="utf-8")
        desktop_source = (ROOT / "src-tauri" / "src" / "lib.rs").read_text(encoding="utf-8")

        self.assertIn("[features]", core_manifest)
        self.assertIn("scan-engine = []", core_manifest)
        self.assertIn('#[cfg(feature = "scan-engine")]', core_library)
        self.assertIn("pub mod scanner;", core_library)
        self.assertNotIn("scan-engine", desktop_manifest)
        self.assertIn('features = ["scan-engine"]', service_manifest)
        self.assertIn("pub use gitmanager_core::repository_index;", desktop_source)
        self.assertNotIn("pub use gitmanager_core::scanner;", desktop_source)
        self.assertNotIn("crate::scanner", desktop_source)
        self.assertIn('core-service = ["dep:gitmanager-core"]', crash_helper_manifest)
        self.assertIn('account-storage = ["core-service"]', crash_helper_manifest)
        self.assertIn('repository-data = ["core-service"]', crash_helper_manifest)
        self.assertIn('features = ["account-storage", "repository-data"]', service_manifest)
        self.assertIn("crash_helper::load_service_account_cache", desktop_source)
        self.assertIn("crash_helper::save_service_account_cache", desktop_source)
        self.assertIn("crash_helper::load_service_repository_overview", desktop_source)
        self.assertIn("crash_helper::load_service_commit_history", desktop_source)
        self.assertIn("crash_helper::load_service_commit_details", desktop_source)
        self.assertIn("crash_helper::load_service_repository_files", desktop_source)
        self.assertIn("crash_helper::read_service_repository_file", desktop_source)
        for service_call in (
            "service_save_conflict_resolution",
            "service_preview_repository_pull",
            "service_get_repository_changes",
            "service_read_repository_file_diff",
            "service_fetch_repository_remote",
            "service_stage_repository_paths",
            "service_unstage_repository_paths",
            "service_preview_repository_commit",
            "service_create_repository_commit",
            "service_delete_repository_path",
            "service_prepare_and_pull_repository_update",
            "service_list_local_repository_branches",
            "service_switch_repository_branch",
        ):
            self.assertIn(f"crash_helper::{service_call}", desktop_source)
        self.assertNotIn("repository_workspace::save_conflict_resolution(", desktop_source)
        self.assertNotIn("repository_workspace::stage_repository_paths(", desktop_source)
        self.assertNotIn("repository_workspace::create_repository_commit(", desktop_source)
        self.assertNotIn("repository_workspace::prepare_and_pull_repository_update(", desktop_source)
        self.assertNotIn("git_info::switch_repository_branch(", desktop_source)
        self.assertNotIn("extract_git_repository_overview", desktop_source)
        self.assertNotIn("git_info::load_repository_commit_history", desktop_source)
        self.assertNotIn("repository_workspace::list_repository_files", desktop_source)
        self.assertNotIn("repository_workspace::read_repository_file", desktop_source)
        self.assertNotIn("gitmanager_core::account_store::load_accounts", desktop_source)
        self.assertNotIn("gitmanager_core::account_store::save_account(", desktop_source)

        desktop_watcher = (ROOT / "src-tauri" / "src" / "watcher_service.rs").read_text(encoding="utf-8")
        service_source = (ROOT / "crates" / "git-manager-service" / "src" / "background_service.rs").read_text(encoding="utf-8")
        self.assertIn("use crate::repository_index::load_persisted_repository_index;", desktop_watcher)
        self.assertNotIn("crate::scanner", desktop_watcher)
        self.assertLess(
            service_source.index("gitmanager_core::storage::ensure_data_layout()"),
            service_source.index("let service_control = match start_service_control_server()"),
        )


if __name__ == "__main__":
    unittest.main()
