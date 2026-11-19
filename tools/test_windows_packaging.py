import importlib.util
import io
import json
import os
import sys
import tempfile
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

    def test_nsis_stops_per_user_service_after_the_ui_and_before_replacing_files(self):
        template = (ROOT / "src-tauri" / "windows" / "installer.nsi").read_text(encoding="utf-8")
        install_section = template.split("Section Install", 1)[1].split("SectionEnd", 1)[0]
        uninstall_section = template.split("Section Uninstall", 1)[1].split("SectionEnd", 1)[0]

        install_check = install_section.index('!insertmacro CheckIfAppIsRunning')
        install_cleanup = install_section.index('!insertmacro NSIS_HOOK_PREINSTALL')
        uninstall_check = uninstall_section.index('!insertmacro CheckIfAppIsRunning')
        uninstall_cleanup = uninstall_section.index('!insertmacro NSIS_HOOK_PREUNINSTALL')

        self.assertLess(install_check, install_cleanup)
        self.assertLess(uninstall_check, uninstall_cleanup)

    def test_nsis_hooks_stop_only_current_users_helper_and_service_and_verify_sidecars(self):
        hooks = (ROOT / "src-tauri" / "windows" / "hooks.nsh").read_text(encoding="utf-8")

        self.assertIn(r'USERNAME eq $%USERDOMAIN%\$%USERNAME%', hooks)
        self.assertIn('schtasks.exe" /End /TN "GitManager Core Service"', hooks)
        self.assertIn('"$R7" --stop-service', hooks)
        self.assertLess(hooks.index('"$R7" --stop-service'), hooks.index("legacy_process_cleanup:"))
        self.assertIn('taskkill.exe" /F /T', hooks)
        for image in (
            "crash_helper.exe",
            "crash_helper-x86_64-pc-windows-msvc.exe",
            "crash_helper-i686-pc-windows-msvc.exe",
            "crash_helper-aarch64-pc-windows-msvc.exe",
            "gitmanager.service.exe",
            "gitmanager.service-x86_64-pc-windows-msvc.exe",
            "gitmanager.service-i686-pc-windows-msvc.exe",
            "gitmanager.service-aarch64-pc-windows-msvc.exe",
            "gitmanager-service.exe",
            "git-manager-service.exe",
        ):
            self.assertIn(f'/IM "{image}"', hooks)
        self.assertIn('schtasks.exe" /Delete /TN "GitManager Core Service" /F', hooks)
        self.assertIn('$UpdateMode <> 1', hooks)
        self.assertIn(r'IfFileExists "$INSTDIR\crash_helper.exe"', hooks)
        self.assertIn(r'IfFileExists "$INSTDIR\gitmanager.service.exe"', hooks)
        self.assertIn(r'StrCpy $0 "$INSTDIR\resources\python\python.exe"', hooks)
        self.assertIn('IfFileExists "$0" 0 python_runtime_missing', hooks)
        self.assertIn('"$R7" --stop-service', hooks)

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
        stop_action = config["msi"]["customActions"]["exe"][0]
        self.assertEqual(stop_action["name"], "StopGitManagerCoreService")
        self.assertEqual(stop_action["filePath"], r"$.installDir\resources\crash_helper.exe")
        self.assertEqual(stop_action["arguments"], "--stop-service")
        self.assertEqual(stop_action["condition"], "Installed")
        self.assertEqual(stop_action["sequence"], "StartOfExecution")
        self.assertTrue(stop_action["continueOnError"])
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
        self.assertIn("gitmanager.exe", desktop["msi"]["detectRunningProcesses"])
        self.assertIn("crash_helper.exe", desktop["msi"]["detectRunningProcesses"])
        self.assertIn("gitmanager.service.exe", desktop["msi"]["detectRunningProcesses"])
        self.assertIn("gitmanager-service.exe", desktop["msi"]["detectRunningProcesses"])
        for sidecar in (
            "crash_helper-x86_64-pc-windows-msvc.exe",
            "crash_helper-i686-pc-windows-msvc.exe",
            "crash_helper-aarch64-pc-windows-msvc.exe",
            "gitmanager.service-x86_64-pc-windows-msvc.exe",
            "gitmanager.service-i686-pc-windows-msvc.exe",
            "gitmanager.service-aarch64-pc-windows-msvc.exe",
        ):
            self.assertIn(sidecar, desktop["msi"]["detectRunningProcesses"])
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

    def test_supervised_ui_waits_for_helper_service_instead_of_spawning_a_duplicate(self):
        watcher = (ROOT / "src-tauri" / "src" / "watcher_service.rs").read_text(encoding="utf-8")
        startup = watcher.split("pub fn ensure_required_service", 1)[1].split(
            "pub fn register_os_background_service", 1
        )[0]

        self.assertIn('var_os("GITMANAGER_CRASH_HELPER_ADDRESS")', startup)
        self.assertIn("let startup_wait = if supervisor_managed", startup)
        self.assertIn("Duration::from_secs(30)", startup)
        self.assertIn("The UI will not launch a duplicate service process", startup)
        self.assertLess(
            startup.index("if supervisor_managed {"),
            startup.index("let service_exe = resolve_background_service_executable"),
        )

    def test_debugger_attaches_to_the_real_ui_and_status_ipc_is_offloaded(self):
        helper = (ROOT / "crates" / "crash-helper" / "src" / "lib.rs").read_text(encoding="utf-8")
        main = (ROOT / "src-tauri" / "src" / "main.rs").read_text(encoding="utf-8")
        app = (ROOT / "src-tauri" / "src" / "lib.rs").read_text(encoding="utf-8")

        self.assertIn("pub fn supervisor_should_bypass()", helper)
        self.assertIn("IsDebuggerPresent", helper)
        self.assertIn("GITMANAGER_DISABLE_CRASH_SUPERVISOR", helper)
        self.assertIn("crash_helper::supervisor_should_bypass()", main)
        status_command = app.split("async fn get_core_service_status", 1)[1].split(
            "async fn inspect_repository", 1
        )[0]
        self.assertIn("spawn_blocking(crash_helper::request_service_status)", status_command)

    def test_service_diagnostics_include_crash_helper_incident_history(self):
        helper = (ROOT / "crates" / "crash-helper" / "src" / "lib.rs").read_text(encoding="utf-8")
        app = (ROOT / "src-tauri" / "src" / "lib.rs").read_text(encoding="utf-8")
        ui = (ROOT / "frontend" / "components" / "service-manager-page.tsx").read_text(encoding="utf-8")

        self.assertIn("pub fn read_service_incident_tail()", helper)
        self.assertIn("crash_helper.jsonl", helper)
        diagnostics = app.split("async fn read_service_log_tail", 1)[1].split(
            "async fn list_app_notifications", 1
        )[0]
        self.assertIn("crash_helper::read_service_incident_tail()", diagnostics)
        self.assertIn("CRASH_HELPER INCIDENT HISTORY", diagnostics)
        self.assertIn("crash_helper incident history", ui.lower())

    def test_crash_helper_replaces_obsolete_services_and_recovers_unresponsive_children(self):
        helper = (ROOT / "crates" / "crash-helper" / "src" / "lib.rs").read_text(encoding="utf-8")
        supervisor = helper.split("struct CoreServiceSupervisor", 1)[1].split(
            "fn host_target_triple", 1
        )[0]

        self.assertIn("stop_obsolete_service_process", helper)
        self.assertIn("fn restart_delay", helper)
        self.assertIn("unhealthy_since", supervisor)
        self.assertIn("core service PID", supervisor)
        self.assertIn("core_service_log_tail()", supervisor)

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
        self.assertIn('cwd=project_root, env=environment', source)
        self.assertIn('work=f"Build Microsoft Store {platform_name} MSIX"', source)

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
        self.assertIn("build_master_packager_installers(project_root, manifest)", source)
        self.assertIn("prepare_master_packager_manifest(project_root)", source)
        self.assertIn('command.extend(["tauri", "bundle", "--target", target.triple])', source)
        self.assertIn('package_config["outputTypes"] = ["msi", "msix"]', source)
        self.assertIn("MPDEV is the required", source)

    def test_workspace_libraries_and_binaries_compile_before_sidecar_staging(self):
        source = (ROOT / "build.py").read_text(encoding="utf-8")
        sidecar_builder = source.split("def build_gitm_sidecar(", 1)[1].split(
            '@tracked_work("Rust target components")', 1
        )[0]
        self.assertIn('command.extend(["build", "--workspace", "--lib", "--bins"])', sidecar_builder)
        for binary in ("gitm", "gitmanager-setup", "crash_helper", "gitmanager-service"):
            self.assertIn(binary, sidecar_builder)

    def test_installer_pipeline_compiles_every_selected_target_before_packaging(self):
        source = (ROOT / "build.py").read_text(encoding="utf-8")
        main = source.split("def main() -> int:", 1)[1]
        compile_phase = main.index("compile_results = []")
        package_phase = main.index("package_results = []")
        self.assertLess(compile_phase, package_phase)
        self.assertIn("bundle=False", main[compile_phase:package_phase])
        self.assertIn("package_local_target(", main[package_phase:])
        package_function = source.split("def package_local_target(", 1)[1].split(
            '@tracked_work("Native build pipeline")', 1
        )[0]
        self.assertIn("run_tauri_bundle(", package_function)
        bundle_helper = source.split("def run_tauri_bundle(", 1)[1].split(
            '@tracked_work("Store package build")', 1
        )[0]
        self.assertIn('command.extend(["tauri", "bundle", "--target", target.triple])', bundle_helper)
        self.assertIn("build_master_packager_installers(project_root, manifest)", package_function)
        self.assertIn("name_windows_setup_installer(project_root, release)", package_function)

    def test_windows_installer_pass_restores_target_python_before_bundling(self):
        source = (ROOT / "build.py").read_text(encoding="utf-8")
        package_function = source.split("def package_local_target(", 1)[1].split(
            '@tracked_work("Native build pipeline")', 1
        )[0]
        runtime_check = package_function.index("prepare_windows_python_runtime(project_root)")
        config_check = package_function.index("verify_windows_packaging_inputs(project_root)")
        bundle_step = package_function.index("run_tauri_bundle(")
        self.assertLess(runtime_check, bundle_step)
        self.assertLess(config_check, bundle_step)

    def test_store_pipeline_compiles_every_selected_target_before_packaging(self):
        source = (ROOT / "build.py").read_text(encoding="utf-8")
        main = source.split("def main() -> int:", 1)[1]
        store_branch = main.split("if args.store_target:", 1)[1].split(
            "if args.build_type == \"installer\":", 1
        )[0]
        compile_phase = store_branch.index("compile_results = []")
        package_phase = store_branch.index("package_results = []")
        self.assertLess(compile_phase, package_phase)
        self.assertIn("bundle=False", store_branch[compile_phase:package_phase])
        self.assertIn("already_compiled=True", store_branch[package_phase:])

    def test_store_packager_can_bundle_or_package_without_recompiling(self):
        source = (ROOT / "build.py").read_text(encoding="utf-8")
        store_function = source.split("def build_local_store_package(", 1)[1].split(
            '@tracked_work("Publish build outputs")', 1
        )[0]
        self.assertIn("already_compiled: bool = False", store_function)
        self.assertIn("run_tauri_bundle(", store_function)
        self.assertIn("publish_build_outputs(", store_function)
        self.assertIn("if not already_compiled:", store_function)

    def test_release_pipeline_can_defer_mpdev_until_after_signing(self):
        source = (ROOT / "build.py").read_text(encoding="utf-8")
        package_function = source.split("def package_local_target(", 1)[1].split(
            '@tracked_work("Native build pipeline")', 1
        )[0]
        self.assertIn('os.environ.get("GITMANAGER_DEFER_MPDEV_PACKAGING") == "1"', package_function)
        self.assertIn("release signing runs before MSI/MSIX generation", package_function)
        self.assertIn("build_master_packager_installers(project_root, manifest)", package_function)

    def test_mpdev_timestamp_server_defaults_to_microsoft_and_can_be_overridden(self):
        with patch.dict(os.environ, {}, clear=True):
            self.assertEqual(
                BUILD.resolve_mpdev_timestamp_server(),
                "http://timestamp.acs.microsoft.com",
            )
        with patch.dict(
            os.environ,
            {"GITMANAGER_MPDEV_TIMESTAMP_SERVER": "https://timestamp.example.test"},
            clear=True,
        ):
            self.assertEqual(
                BUILD.resolve_mpdev_timestamp_server(),
                "https://timestamp.example.test",
            )
        with patch.dict(
            os.environ,
            {"GITMANAGER_MPDEV_TIMESTAMP_SERVER": "file:///tmp/timestamp"},
            clear=True,
        ):
            with self.assertRaisesRegex(ValueError, "absolute HTTP or HTTPS URL"):
                BUILD.resolve_mpdev_timestamp_server()

    def test_mpdev_source_manifest_uses_the_microsoft_timestamp_authority(self):
        config = json.loads(
            (ROOT / "installer" / "master-packager" / "package.json").read_text(encoding="utf-8")
        )
        self.assertEqual(
            config["msix"]["digitalSignature"]["timestampServer"],
            "http://timestamp.acs.microsoft.com",
        )

    def test_first_run_setup_sidecar_matches_tauri_config_and_enters_setup_mode(self):
        tauri = json.loads((ROOT / "src-tauri" / "tauri.conf.json").read_text(encoding="utf-8"))
        capability = json.loads((ROOT / "src-tauri" / "capabilities" / "default.json").read_text(encoding="utf-8"))
        app = (ROOT / "src-tauri" / "src" / "lib.rs").read_text(encoding="utf-8")
        setup = (ROOT / "crates" / "gitmanager-setup" / "src" / "main.rs").read_text(encoding="utf-8")

        self.assertIn("binaries/gitmanager-setup", tauri["bundle"]["externalBin"])
        sidecar_permission = capability["permissions"][2]["allow"][0]
        self.assertEqual(sidecar_permission["name"], "binaries/gitmanager-setup")
        self.assertIn('.sidecar("binaries/gitmanager-setup")', app)
        setup_mode = app.split("fn is_setup_mode", 1)[1].split("}", 1)[0]
        self.assertIn('GITMANAGER_SETUP_MODE', setup_mode)
        self.assertIn('"--setup"', setup_mode)
        self.assertIn('.arg("--setup")', setup)
        self.assertIn('.env("GITMANAGER_SETUP_MODE", "1")', setup)

    def test_installers_launch_the_setup_companion_and_require_it(self):
        manifest = json.loads(
            (ROOT / "installer" / "master-packager" / "package.json").read_text(encoding="utf-8")
        )
        prompt = manifest["msi"]["installDialog"]["promptAppsAfterInstall"][0]
        self.assertEqual(prompt["file"], r"$.installDir\resources\gitmanager-setup.exe")

        template = (ROOT / "src-tauri" / "windows" / "installer.nsi").read_text(encoding="utf-8")
        hooks = (ROOT / "src-tauri" / "windows" / "hooks.nsh").read_text(encoding="utf-8")
        run_main = template.split("Function RunMainBinary", 1)[1].split("FunctionEnd", 1)[0]
        self.assertIn(r'$INSTDIR\gitmanager-setup.exe', run_main)
        self.assertIn(r'IfFileExists "$INSTDIR\gitmanager-setup.exe"', hooks)
        self.assertIn("setup_helper_payload_ready:", hooks)

    def test_msi_and_nsis_installers_do_not_share_the_uninstaller_directory(self):
        manifest = json.loads(
            (ROOT / "installer" / "master-packager" / "package.json").read_text(encoding="utf-8")
        )
        msi_install_dir = manifest["installDir"].replace("/", "\\\\").lower()
        self.assertIn(r"\programs\kastrick's software\gitmanager", msi_install_dir)

        template = (ROOT / "src-tauri" / "windows" / "installer.nsi").read_text(encoding="utf-8")
        self.assertIn(r"$LOCALAPPDATA\kastrick's_software\${PRODUCTNAME}", template.lower())
        self.assertNotEqual(msi_install_dir, r"%localappdata%\kastrick's_software\gitmanager")

    def test_first_run_setup_registers_the_per_user_service_task_without_duplicate_launch(self):
        app = (ROOT / "frontend" / "components" / "gitmanager-app.tsx").read_text(encoding="utf-8")
        watcher = (ROOT / "src-tauri" / "src" / "watcher_service.rs").read_text(encoding="utf-8")
        setup_dialog = app.split("function SetupDialog", 1)[1].split("const MemoRepositoryPage", 1)[0]
        windows_registration = watcher.split('#[cfg(target_os = "windows")]', 1)[1].split('#[cfg(target_os = "linux")]', 1)[0]

        self.assertIn("const [startServiceAtSignIn, setStartServiceAtSignIn] = useState(true)", setup_dialog)
        self.assertIn('invoke("control_background_service", { installOsService: true })', setup_dialog)
        self.assertIn("Start the GitManager core service when I sign in", setup_dialog)
        self.assertIn("service_control_lock_is_held()", windows_registration)
        self.assertLess(
            windows_registration.index("let service_already_running"),
            windows_registration.index('args(["/Run", "/TN", task_name])'),
        )
        self.assertIn("if !service_already_running", windows_registration)

    def test_service_lock_ownership_prevents_duplicate_supervisor_spawns(self):
        helper = (ROOT / "crates" / "crash-helper" / "src" / "lib.rs").read_text(encoding="utf-8")
        watcher = (ROOT / "src-tauri" / "src" / "watcher_service.rs").read_text(encoding="utf-8")
        self.assertIn("pub fn service_control_lock_owner_pid()", helper)
        self.assertIn("pub fn service_control_lock_is_held()", helper)
        self.assertIn("if self.child.is_none() && service_control_lock_is_held()", helper)
        self.assertIn("lock_file.write_all(format!(\\"{}\\", std::process::id()).as_bytes())", helper)
        self.assertIn("crash_helper::service_control_lock_owner_pid()", watcher)

    def test_windows_pipe_response_flushes_and_status_health_check_retries(self):
        helper = (ROOT / "crates" / "crash-helper" / "src" / "lib.rs").read_text(encoding="utf-8")
        frame_writer = helper.split("fn write_service_frame_with_timeout", 1)[1].split("\n}", 1)[0]
        status_request = helper.split("pub fn request_service_status", 1)[1].split("\n}", 1)[0]
        self.assertIn("stream.flush()", frame_writer)
        self.assertIn("for attempt in 0..3", status_request)
        self.assertIn("could not send", helper.lower())

    def test_docs_explain_msi_and_msix_service_shutdown_differences(self):
        docs = (ROOT / "docs" / "windows-packaging.md").read_text(encoding="utf-8")

        self.assertIn("MPDEV custom actions and detectRunningProcesses are MSI-only", docs)
        self.assertIn("ERROR_PACKAGES_IN_USE", docs)
        self.assertIn("crash_helper.exe", docs)
        self.assertIn("--stop-service", docs)
        self.assertIn("Add-AppxPackage -Path $packagePath -ForceTargetApplicationShutdown", docs)
        self.assertIn("do not use it while unsaved UI work is open", docs)

    def test_docs_describe_mpdev_as_mandatory_and_nsis_as_fallback(self):
        docs = (ROOT / "docs" / "windows-packaging.md").read_text(encoding="utf-8")
        self.assertIn("required Windows MSI/MSIX packager", docs)
        self.assertIn("NSIS fallback", docs)
        self.assertIn("--master-packager", docs)

    def test_repository_scan_engine_is_enabled_only_by_the_headless_service(self):
        core_manifest = (ROOT / "crates" / "gitmanager-core" / "Cargo.toml").read_text(encoding="utf-8")
        core_library = (ROOT / "crates" / "gitmanager-core" / "src" / "lib.rs").read_text(encoding="utf-8")
        core_preferences = (ROOT / "crates" / "gitmanager-core" / "src" / "app_preferences.rs").read_text(encoding="utf-8")
        desktop_manifest = (ROOT / "src-tauri" / "Cargo.toml").read_text(encoding="utf-8")
        service_manifest = (ROOT / "crates" / "git-manager-service" / "Cargo.toml").read_text(encoding="utf-8")
        archive_watcher = (ROOT / "crates" / "git-manager-service" / "src" / "archive_watcher.rs").read_text(encoding="utf-8")
        crash_helper_manifest = (ROOT / "crates" / "crash-helper" / "Cargo.toml").read_text(encoding="utf-8")
        desktop_source = (ROOT / "src-tauri" / "src" / "lib.rs").read_text(encoding="utf-8")

        self.assertIn("[features]", core_manifest)
        self.assertIn('git-engine = ["dep:chrono", "dep:git2", "dep:walkdir"]', core_manifest)
        self.assertIn('scan-engine = ["git-engine"]', core_manifest)
        self.assertIn('git2 = { version = "0.20.4", default-features = false, features = ["https", "ssh"], optional = true }', core_manifest)
        self.assertIn('#[cfg(feature = "scan-engine")]', core_library)
        self.assertIn("pub mod scanner;", core_library)
        self.assertIn('#[cfg(feature = "git-engine")]\npub mod git_info;', core_library)
        self.assertIn('#[cfg(feature = "git-engine")]\npub mod repository_workspace;', core_library)
        self.assertIn('#[cfg(feature = "git-engine")]\npub mod repository_preferences;', core_library)
        self.assertNotIn("scan-engine", desktop_manifest)
        self.assertNotIn("chrono =", desktop_manifest)
        self.assertIn("use gitmanager_core::models::GitRepositoryInfo;", desktop_source)
        self.assertNotIn("pub use gitmanager_core::git_info;", desktop_source)
        self.assertNotIn("git_info::", desktop_source)
        self.assertNotIn("gitmanager_core::repository_workspace::", desktop_source)
        self.assertNotIn('gitmanager-archive = ', core_manifest)
        self.assertIn('features = ["scan-engine"]', service_manifest)
        self.assertIn("pub use gitmanager_core::repository_index;", desktop_source)
        self.assertNotIn("pub use gitmanager_core::scanner;", desktop_source)
        self.assertNotIn("crate::scanner", desktop_source)
        self.assertIn('core-service = ["dep:gitmanager-core"]', crash_helper_manifest)
        self.assertIn('account-storage = ["core-service"]', crash_helper_manifest)
        self.assertIn('repository-data = ["core-service", "gitmanager-core/git-engine"]', crash_helper_manifest)
        self.assertIn('notification-storage = ["core-service"]', crash_helper_manifest)
        self.assertIn('preferences-storage = ["core-service"]', crash_helper_manifest)
        self.assertIn('archive-operations = ["repository-data", "dep:gitmanager-archive"]', crash_helper_manifest)
        self.assertIn('features = ["account-storage", "repository-data", "notification-storage", "preferences-storage", "archive-operations", "provider-sync"]', service_manifest)
        self.assertIn("crash_helper::load_service_account_cache", desktop_source)
        self.assertIn("crash_helper::service_connect_git_account", desktop_source)
        self.assertNotIn("gitmanager_core::account_store::save_account(", desktop_source)
        self.assertIn("crash_helper::load_service_repository_overview", desktop_source)
        self.assertIn("crash_helper::load_service_commit_history", desktop_source)
        self.assertIn("crash_helper::load_service_commit_details", desktop_source)
        self.assertIn("crash_helper::load_service_repository_files", desktop_source)
        self.assertIn("crash_helper::read_service_repository_file", desktop_source)
        self.assertIn("crash_helper::service_list_notifications", desktop_source)
        self.assertIn("crash_helper::service_mark_notification_read", desktop_source)
        self.assertIn("crash_helper::service_dismiss_notification", desktop_source)
        self.assertIn("crash_helper::service_mark_all_notifications_read", desktop_source)
        self.assertIn("crash_helper::service_load_app_preferences", desktop_source)
        self.assertIn("crash_helper::service_load_repository_preferences", desktop_source)
        self.assertIn("crash_helper::service_save_repository_preferences", desktop_source)
        self.assertNotIn("Repository::open(", desktop_source)
        self.assertNotIn("repository_settings_path(", desktop_source)
        self.assertNotIn('git2 = ', desktop_manifest)
        self.assertNotIn('walkdir = ', desktop_manifest)
        self.assertNotIn('gitmanager-archive = ', desktop_manifest)
        self.assertIn("crash_helper::service_capture_repository_archive", desktop_source)
        self.assertIn("crash_helper::service_list_repository_archives", desktop_source)
        self.assertIn("crash_helper::service_restore_repository_archive", desktop_source)
        self.assertIn("crash_helper::service_sync_archive_cloud_storage", desktop_source)
        self.assertIn("crash_helper::service_connect_git_account", desktop_source)
        self.assertIn("crash_helper::service_detect_github_identity", desktop_source)
        self.assertNotIn("fetch_remote_account_repositories", desktop_source)
        self.assertNotIn("reqwest::Client::builder", (ROOT / "src-tauri" / "src" / "remote_providers.rs").read_text(encoding="utf-8"))
        self.assertIn("gitmanager_core::account_store::save_account", (ROOT / "crates" / "crash-helper" / "src" / "lib.rs").read_text(encoding="utf-8"))
        self.assertIn("crash_helper::service_get_archive_cloud_storage_info", desktop_source)
        self.assertIn("crash_helper::service_configure_archive_cloud_storage", desktop_source)
        self.assertIn("crash_helper::service_disable_archive_cloud_storage", desktop_source)
        self.assertIn("crash_helper::with_repository_archive_lock", archive_watcher)
        self.assertNotIn("fs::create_dir_all(&candidate)", desktop_source)
        self.assertNotIn("candidate.canonicalize()", desktop_source)
        self.assertNotIn("archive::capture_repository(", desktop_source)
        self.assertNotIn("archive::restore_repository_archive(", desktop_source)
        self.assertNotIn("archive::sync_archive_cloud_storage(", desktop_source)
        self.assertIn("crash_helper::service_save_app_preferences", desktop_source)
        self.assertIn("crash_helper::service_get_repository_index_generation", desktop_source)
        self.assertIn("crash_helper::service_load_indexed_repositories", desktop_source)
        self.assertIn("crash_helper::request_service_remote_check", desktop_source)
        self.assertNotIn("load_background_preferences()", desktop_source)
        self.assertNotIn("load_persisted_repository_index", desktop_source)
        self.assertNotIn("read_app_preferences_file", desktop_source)
        self.assertNotIn("write_app_preferences_file", desktop_source)
        application_directories = desktop_source.split("fn application_directories()", 1)[1].split("\n}", 1)[0]
        self.assertNotIn("create_dir_all", application_directories)
        self.assertIn("crate::storage::ensure_data_layout()?", core_preferences)
        self.assertNotIn("check_all_repos_for_updates", desktop_source)
        self.assertNotIn("notification_center::list_notifications()", desktop_source)
        self.assertNotIn("notification_center::mark_notification_read(", desktop_source)
        self.assertNotIn("notification_center::dismiss_notification(", desktop_source)
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
        service_watcher = (ROOT / "crates" / "git-manager-service" / "src" / "watcher_service.rs").read_text(encoding="utf-8")
        self.assertIn("crash_helper::with_repository_operation_lock", service_watcher)
        self.assertIn("check_single_repo_for_remote_update_locked", service_watcher)
        self.assertIn("apply_repository_update_locked", service_watcher)
        self.assertNotIn("use crate::repository_index::load_persisted_repository_index;", desktop_watcher)
        self.assertNotIn("crate::scanner", desktop_watcher)
        self.assertNotIn("check_all_repos_for_updates", desktop_watcher)
        self.assertNotIn("apply_repository_update", desktop_watcher)
        self.assertNotIn("git2::Repository", desktop_watcher)
        self.assertLess(
            service_source.index("gitmanager_core::storage::ensure_data_layout()"),
            service_source.index("let service_control = match start_service_control_server()"),
        )


class _MemoryDownloadResponse:
    def __init__(self, payload, content_length):
        self._stream = io.BytesIO(payload)
        self.headers = {"Content-Length": str(content_length)}

    def read(self, size=-1):
        return self._stream.read(size)


class _TTYBuffer(io.StringIO):
    def isatty(self):
        return True


class BuildProgressReportingTests(unittest.TestCase):
    def setUp(self):
        BUILD._LAST_NONINTERACTIVE_PROGRESS.clear()
        BUILD._ACTIVE_PROGRESS_STREAM = None
        BUILD._ACTIVE_PROGRESS_STATE = None

    def test_mpdev_free_offline_rejection_is_classified_only_for_mpdev(self):
        message = 'mpdev: error: Offline mode is only supported with "Enterprise" license.'
        self.assertTrue(BUILD.is_mpdev_offline_mode_error(["C:/Tools/mpdev.exe"], message))
        self.assertFalse(BUILD.is_mpdev_offline_mode_error(["cargo"], message))
        self.assertFalse(BUILD.is_mpdev_offline_mode_error(["mpdev.exe"], "network connection failed"))

    def test_work_indicator_includes_progress_and_status(self):
        output = io.StringIO()
        with patch("sys.stdout", output):
            BUILD.log_work("CPython runtime", "2/6", "WORKING", "Checking the cached runtime...")
        self.assertIn("[ CPython runtime ] [2/6] [WORKING]", output.getvalue())
        self.assertIn("Checking the cached runtime...", output.getvalue())

    def test_terminal_progress_uses_real_percentage_and_one_clean_animated_row(self):
        output = _TTYBuffer()
        with patch("sys.stdout", output):
            BUILD.log_work("CPython download", "0%", "PENDING", "Queued archive...")
            BUILD.log_work("CPython download", "0%", "WORKING", "Connecting...")
            BUILD.log_work("CPython download", "30%", "WORKING", "3.2 / 10.7 MiB")
            BUILD.log_work("CPython download", "100%", "DONE", "Downloaded archive.")

        rendered = output.getvalue()
        self.assertEqual(rendered.count("\r"), 4)
        self.assertEqual(rendered.count("\n"), 1)
        self.assertNotIn("\x1b[2K", rendered)
        self.assertIn("30% [====........]", rendered)
        self.assertIn("100% [============]", rendered)
        self.assertIn("OK  CPython download", rendered)

    def test_log_lines_preserve_the_live_progress_row(self):
        output = _TTYBuffer()
        with patch("sys.stdout", output):
            BUILD.log_work("Cargo build", "1/1", "WORKING", "Compiling tauri...")
            BUILD._emit("[ Cargo build ] warning: test warning", color="yellow")
            BUILD.log_work("Cargo build", "1/1", "DONE", "Finished.")

        rendered = output.getvalue()
        self.assertIn("warning: test warning", rendered)
        self.assertIn("Compiling tauri...", rendered)
        self.assertIn("OK  Cargo build", rendered)
        self.assertEqual(rendered.count("\n"), 2)
        self.assertNotIn("\x1b[2K", rendered)
        self.assertGreaterEqual(rendered.count("\r"), 4)

    def test_tty_progress_never_wraps_at_narrow_terminal_width(self):
        output = _TTYBuffer()
        with patch("sys.stdout", output), patch.object(
            BUILD.shutil, "get_terminal_size", return_value=os.terminal_size((44, 24))
        ), patch.object(BUILD, "_supports_color", return_value=False):
            BUILD.log_work("Long build task", "45%", "WORKING", "Details " * 30)
            BUILD.log_work("Long build task", "DONE", "DONE", "Finished.")

        frames = output.getvalue().split("\r")[1:]
        self.assertTrue(frames)
        self.assertTrue(all(len(frame.split("\n", 1)[0]) <= 43 for frame in frames))

    def test_tty_spinner_falls_back_to_ascii_for_legacy_encodings(self):
        class AsciiTTY(_TTYBuffer):
            @property
            def encoding(self):
                return "ascii"

        output = AsciiTTY()
        with patch("sys.stdout", output):
            BUILD.log_work("Legacy console", "1/2", "WORKING", "Compiling...")

        rendered = output.getvalue()
        self.assertTrue(any(frame in rendered for frame in ("|", "/", "-", "\\")))
        self.assertFalse(any(frame in rendered for frame in BUILD._SPINNER_FRAMES))

    def test_cargo_status_lines_are_recognized_for_the_live_activity_row(self):
        self.assertTrue(BUILD.is_cargo_progress_line(["cargo", "build"], "   Compiling tauri v2.12.7"))
        self.assertTrue(BUILD.is_cargo_progress_line(["cargo.exe", "check"], "    Checking gitmanager-core"))
        self.assertFalse(BUILD.is_cargo_progress_line(["rustc"], "Compiling tauri"))
        self.assertFalse(BUILD.is_cargo_progress_line(["cargo", "build"], "error: could not compile"))

    def test_noninteractive_heartbeat_repeats_are_suppressed(self):
        output = io.StringIO()
        with patch("sys.stdout", output):
            BUILD.log_work("Cargo build", "1/1", "WORKING", "Compiling...")
            BUILD.log_work("Cargo build", "1/1", "WORKING", "Still compiling (5s)...")
            BUILD.log_work("Cargo build", "1/1", "WORKING", "Still compiling (10s)...")
            BUILD.log_work("Cargo build", "1/1", "DONE", "Finished.")

        rendered = output.getvalue()
        self.assertEqual(rendered.count("\n"), 2)
        self.assertNotIn("Still compiling", rendered)
        self.assertIn("[ Cargo build ] [1/1] [DONE]", rendered)

    def test_redirected_command_output_never_gets_periodic_heartbeat_spam(self):
        output = io.StringIO()
        command = [
            BUILD.sys.executable,
            "-c",
            "import time; time.sleep(0.35); print('child finished')",
        ]
        with patch("sys.stdout", output):
            result = BUILD.run_command(
                command,
                work="Quiet redirected command",
                heartbeat_seconds=0.08,
            )

        rendered = output.getvalue()
        self.assertEqual(result, 0)
        self.assertIn("child finished", rendered)
        self.assertNotIn("Waiting for process output", rendered)
        self.assertNotIn("Process is still running", rendered)

    def test_python_runtime_download_streams_bytes_and_reports_completion(self):
        payload = b"python-embed-runtime-fixture"
        response = _MemoryDownloadResponse(payload, len(payload))

        with tempfile.TemporaryDirectory(prefix="gitmanager-build-progress-test-") as temporary:
            destination = Path(temporary) / "python-embed.zip"
            output = io.StringIO()
            with patch("sys.stdout", output):
                copied = BUILD._download_response_to_file(response, destination)

            self.assertEqual(copied, len(payload))
            self.assertEqual(destination.read_bytes(), payload)
            self.assertIn("CPython download", output.getvalue())
            self.assertIn("DONE", output.getvalue())

    def test_python_runtime_download_rejects_a_truncated_http_body(self):
        response = _MemoryDownloadResponse(b"short", 100)

        with tempfile.TemporaryDirectory(prefix="gitmanager-build-progress-test-") as temporary:
            destination = Path(temporary) / "python-embed.zip"
            with patch("sys.stdout", io.StringIO()):
                with self.assertRaisesRegex(OSError, "Download ended early"):
                    BUILD._download_response_to_file(response, destination)


if __name__ == "__main__":
    unittest.main()
