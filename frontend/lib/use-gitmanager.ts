"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { invoke, isTauriRuntime, listen } from "./tauri";
import type {
	AppPreferences,
	ApplicationDirectories,
	ConnectedAccount,
	AppNotification,
	GitRepository,
	ScanSummary,
	UpdateAlert,
} from "./types";

const DEFAULT_PREFERENCES: AppPreferences = {
	scanRoots: [],
	maxScanDepth: 8,
	includeHiddenDirs: false,
	indexDependencyRepositories: true,
	showDependencyRepositories: true,
	firstRunComplete: false,
	archiveRoot: "",
	archiveLocalEnabled: true,
	archiveRemoteEnabled: true,
	archiveTrigger: "manual",
	archiveQuietMs: 1500,
	cloudNodeAddress: "",
	inAppNotificationsEnabled: true,
	osNotificationsEnabled: true,
};

export function useGitManager() {
	const scanInFlight = useRef(false);
	const [available, setAvailable] = useState(false);
	const [serviceReady, setServiceReady] = useState(false);
	const [loading, setLoading] = useState(true);
	const [scanning, setScanning] = useState(false);
	const [scanTruncated, setScanTruncated] = useState(false);
	const [indexReady, setIndexReady] = useState(false);
	const indexGeneration = useRef("0");
	const [version, setVersion] = useState("—");
	const [preferences, setPreferences] = useState<AppPreferences>(DEFAULT_PREFERENCES);
	const [directories, setDirectories] = useState<ApplicationDirectories | null>(null);
	const [repositories, setRepositories] = useState<GitRepository[]>([]);
	const [dependencies, setDependencies] = useState<string[]>([]);
	const [accounts, setAccounts] = useState<ConnectedAccount[]>([]);
	const [updates, setUpdates] = useState<UpdateAlert[]>([]);
	const [notifications, setNotifications] = useState<AppNotification[]>([]);
	const [setupOpen, setSetupOpen] = useState(false);
	const [status, setStatus] = useState("Ready");
	const [error, setError] = useState("");

	const performScan = useCallback(async (currentPreferences: AppPreferences, customRoot?: string) => {
		if (!isTauriRuntime()) {
			setStatus("Open GitManager as a desktop app to access local repositories.");
			return [];
		}
		if (scanInFlight.current) return [];
		scanInFlight.current = true;
		setScanning(true);
		setStatus("Indexing repositories…");
		setError("");
		try {
			const rootPaths = customRoot?.trim()
				? [customRoot.trim()]
				: currentPreferences.scanRoots;
			const summary = await invoke<ScanSummary>("scan_machine_repositories", {
				rootPaths,
				maxDepth: currentPreferences.maxScanDepth || 8,
				includeHiddenDirs: Boolean(currentPreferences.includeHiddenDirs),
				indexDependencyRepositories: currentPreferences.indexDependencyRepositories !== false,
				filterMode: "grouped",
			});
			const local = Array.isArray(summary.repositories)
				? summary.repositories.filter((repo) => repo.source_kind === "local_scan")
				: [];
			setRepositories(local);
			setDependencies(summary.dependency_repo_ids ?? []);
			setScanTruncated(Boolean(summary.scan_truncated));
			setIndexReady(true);
			setStatus(summary.scan_truncated
				? `Indexed ${local.length} repositories · stopped at a safety limit`
				: `Indexed ${local.length} repositories`);
			return local;
		} catch (cause) {
			const message = cause instanceof Error ? cause.message : String(cause);
			setError(message);
			setStatus("Repository scan failed");
			return [];
		} finally {
			scanInFlight.current = false;
			setScanning(false);
		}
	}, []);

	const scan = useCallback((customRoot?: string) => performScan(preferences, customRoot), [performScan, preferences]);

	const retryService = useCallback(async () => {
		setError("");
		setStatus("Starting GitManager core service…");
		try {
			await invoke<string>("ensure_required_service");
			setServiceReady(true);
			setStatus("GitManager core service ready");
			if (preferences.firstRunComplete) await performScan(preferences);
		} catch (cause) {
			const message = cause instanceof Error ? cause.message : String(cause);
			setServiceReady(false);
			setError(message);
			setStatus("Core service unavailable");
		}
	}, [performScan, preferences]);

	const updateRepository = useCallback((updated: GitRepository) => {
		setRepositories((current) => current.map((repo) => repo.id === updated.id ? updated : repo));
	}, []);

	const clearUpdate = useCallback((repoPath: string) => {
		setUpdates((current) => current.filter((item) => item.repo_path !== repoPath));
	}, []);

	const refreshNotifications = useCallback(async () => {
		if (!isTauriRuntime()) return [];
		try {
			const current = await invoke<AppNotification[]>("list_app_notifications");
			setNotifications(current ?? []);
			return current ?? [];
		} catch {
			return [];
		}
	}, []);

	// The independent service cannot emit through this Tauri process. Refresh
	// the small bounded store periodically so the unread badge stays current.
	useEffect(() => {
		if (!serviceReady || preferences.inAppNotificationsEnabled === false) return;
		const timer = window.setInterval(() => void refreshNotifications(), 15000);
		return () => window.clearInterval(timer);
	}, [serviceReady, preferences.inAppNotificationsEnabled, refreshNotifications]);


	const markNotificationRead = useCallback(async (id: string) => {
		await invoke("mark_app_notification_read", { id });
		setNotifications((current) => current.map((item) => item.id === id ? { ...item, read: true } : item));
	}, []);

	const dismissNotification = useCallback(async (id: string) => {
		await invoke("dismiss_app_notification", { id });
		setNotifications((current) => current.filter((item) => item.id !== id));
	}, []);


	// Initial discovery is owned by gitmanager.service. Poll only the tiny index
	// generation stamp until the service publishes its first index; then read the
	// full inventory exactly once. This avoids a duplicate startup directory walk.
	useEffect(() => {
		if (!serviceReady || !preferences.firstRunComplete || indexReady) return;
		let active = true;
		const refreshInitialIndex = async () => {
			try {
				const generation = await invoke<string>("get_repository_index_generation");
				if (!active || generation === "0" || generation === indexGeneration.current) return;
				const summary = await invoke<ScanSummary>("load_indexed_repositories", { filterMode: "grouped" });
				if (!active) return;
				indexGeneration.current = generation;
				const local = Array.isArray(summary.repositories)
					? summary.repositories.filter((repo) => repo.source_kind === "local_scan")
					: [];
				setRepositories(local);
				setDependencies(summary.dependency_repo_ids ?? []);
				setScanTruncated(Boolean(summary.scan_truncated));
				setIndexReady(true);
				setStatus(summary.scan_truncated
					? `Indexed ${local.length} repositories · stopped at a safety limit`
					: `Indexed ${local.length} repositories`);
			} catch {
				// A missing index is normal while the service performs first-run discovery.
			}
		};
		void refreshInitialIndex();
		const timer = window.setInterval(() => void refreshInitialIndex(), 1500);
		return () => {
			active = false;
			window.clearInterval(timer);
		};
	}, [serviceReady, preferences.firstRunComplete, indexReady]);

	// Report only after the desktop hook is ready. The independent crash_helper
	// treats a missing heartbeat as an unresponsive UI.
	useEffect(() => {
		if (!available || loading || !isTauriRuntime()) return;
		const heartbeat = () => {
			void invoke("report_ui_heartbeat").catch(() => undefined);
		};
		heartbeat();
		const timer = window.setInterval(heartbeat, 3000);
		return () => window.clearInterval(timer);
	}, [available, loading]);

	const savePreferences = useCallback(async (next: AppPreferences) => {
		const saved = await invoke<AppPreferences>("save_app_preferences", {
			preferences: { ...next, firstRunComplete: true },
		});
		setPreferences(saved);
		setSetupOpen(false);
		setStatus("Preferences saved");
		await performScan(saved);
		return saved;
	}, [performScan]);

	const checkUpdates = useCallback(async () => {
		setStatus("Checking remote branches…");
		try {
			const found = await invoke<UpdateAlert[]>("check_repo_updates_now", {
				rootPaths: preferences.scanRoots,
			});
			setUpdates(found ?? []);
			setStatus(found?.length ? `${found.length} update available` : "All repositories are up to date");
			return found ?? [];
		} catch (cause) {
			const message = cause instanceof Error ? cause.message : String(cause);
			setError(message);
			setStatus("Remote check failed");
			return [];
		}
	}, [preferences.scanRoots]);

	const switchBranch = useCallback(async (repoPath: string, branchName: string) => {
		setStatus("Switching local branch safely…");
		try {
			const updated = await invoke<GitRepository>("switch_repository_branch", {
				repoPath,
				branchName,
			});
			setRepositories((current) => current.map((repo) => repo.id === updated.id ? updated : repo));
			setStatus("Switched to " + updated.current_branch);
			return updated;
		} catch (cause) {
			const message = cause instanceof Error ? cause.message : String(cause);
			setError(message);
			setStatus("Branch switch stopped safely");
			throw cause;
		}
	}, []);

	const connectAccount = useCallback(async (
		provider: string,
		usernameOrHost: string,
		accessToken: string,
	) => {
		const account = await invoke<ConnectedAccount>("connect_git_account", {
			provider,
			usernameOrHost,
			accessToken: accessToken.trim() ? accessToken.trim() : null,
		});
		setAccounts((current) => [
			...current.filter((item) => !(
				item.provider === account.provider &&
				item.account_login.toLowerCase() === account.account_login.toLowerCase()
			)),
			account,
		]);
		setStatus(`Connected ${account.account_login}`);
		return account;
	}, []);

	useEffect(() => {
		let active = true;
		let unlistenUpdate: (() => void) | undefined;
		let unlistenState: (() => void) | undefined;
		let unlistenNotifications: (() => void) | undefined;

		async function initialize() {
			setAvailable(isTauriRuntime());
			if (!isTauriRuntime()) {
				setLoading(false);
				setStatus("Desktop runtime not detected");
				return;
			}
			try {
				setStatus("Connecting to GitManager core service…");
				await invoke<string>("ensure_required_service");
				if (!active) return;
				setServiceReady(true);
				setStatus("Core service connected");
				void refreshNotifications();

				const cliTools = await invoke<{ git: boolean; github: boolean }>("check_cli_tools");
				if (!cliTools.git && !cliTools.github) {
					const flatpak = await invoke<{ isFlatpak: boolean }>("get_flatpak_cli_integration_status")
						.catch(() => ({ isFlatpak: false }));
					const environmentNote = flatpak.isFlatpak
						? "Flatpak isolates host executables, so Git or GitHub CLI installed outside this sandbox may not be visible here. "
						: "";
					const proceed = window.confirm(
						"Git CLI and GitHub CLI were not detected.\n\n" +
						environmentNote +
						"GitManager has its own built-in Git engine, so it can still scan and manage local repositories. " +
						"However, features that launch external Git commands and GitHub CLI account integration will be unavailable. " +
						(flatpak.isFlatpak ? "The optional gitm terminal command can be enabled later in Settings.\n\n" : "\n\n") +
						"Would you like to continue with the repository scan?\n\nChoose OK to continue scanning or Cancel to close GitManager."
					);
					if (!proceed) {
						await invoke("exit_application");
						return;
					}
				}
				const [appVersion, paths, saved] = await Promise.all([
					invoke<string>("get_app_version"),
					invoke<ApplicationDirectories>("get_application_directories"),
					invoke<AppPreferences>("load_app_preferences"),
				]);
				if (!active) return;
				setVersion(appVersion || "—");
				setDirectories(paths);
				let resolved = { ...DEFAULT_PREFERENCES, ...saved };
				if (!resolved.scanRoots?.length) {
					const suggested = await invoke<string[]>("get_default_scan_directories").catch(() => []);
					resolved = { ...resolved, scanRoots: suggested ?? [] };
				}
				setPreferences(resolved);

				// Hydrate the last published inventory before requesting any refresh.
				// This is a read of the service-owned index, never a filesystem scan.
				const [cachedIndex, generation] = await Promise.all([
					invoke<ScanSummary>("load_indexed_repositories", { filterMode: "grouped" }).catch(() => null),
					invoke<string>("get_repository_index_generation").catch(() => "0"),
				]);
				indexGeneration.current = generation;
				if (active && generation !== "0") setIndexReady(true);
				if (active && cachedIndex) {
					const local = Array.isArray(cachedIndex.repositories)
						? cachedIndex.repositories.filter((repo) => repo.source_kind === "local_scan")
						: [];
					setRepositories(local);
					setDependencies(cachedIndex.dependency_repo_ids ?? []);
					setScanTruncated(Boolean(cachedIndex.scan_truncated));
				}

				const setupMode = await invoke<boolean>("is_setup_mode").catch(() => false);
				if (setupMode) {
					setSetupOpen(true);
				} else if (!resolved.firstRunComplete) {
					try {
						await invoke("launch_first_run_setup");
						return;
					} catch {
						setSetupOpen(true);
					}
				} else {
					// The service owns first-run discovery. Render cached data immediately
					// and let the lightweight generation poll hydrate the first new index.
					setLoading(false);
					setStatus(indexGeneration.current === "0"
						? "GitManager Core Service is indexing repositories…"
						: "Repository index loaded");
				}

				// GitHub CLI detection is intentionally deferred to Accounts.
				// The explicit Connect provider action uses the existing gh session
				// when its username and token fields are left blank.
			} catch (cause) {
				const message = cause instanceof Error ? cause.message : String(cause);
				if (active) {
					setError(message);
					setStatus("GitManager could not initialize");
				}
			} finally {
				if (active) setLoading(false);
			}
		}

		void initialize();
		void listen<UpdateAlert>("gitmanager://repo-update-available", (payload) => {
			if (active) setUpdates((current) => [
				...current.filter((item) => item.repo_path !== payload.repo_path),
				payload,
			]);
		}).then((unlisten) => { unlistenUpdate = unlisten; });
		void listen<{ checking?: boolean; status?: string }>("gitmanager://update-check-state", (payload) => {
			if (active && payload.status) setStatus(payload.status);
		}).then((unlisten) => { unlistenState = unlisten; });
		void listen("gitmanager://notifications-updated", () => {
			if (active) void refreshNotifications();
		}).then((unlisten) => { unlistenNotifications = unlisten; });

		return () => {
			active = false;
			unlistenUpdate?.();
			unlistenState?.();
			unlistenNotifications?.();
		};
	}, [performScan, refreshNotifications]);

	return {
		available, serviceReady, loading, scanning, scanTruncated, indexReady,
		awaitingInitialIndex: serviceReady && preferences.firstRunComplete && !indexReady,
		version, preferences, setPreferences, directories,
		repositories, dependencies, accounts, updates, notifications, setupOpen, setSetupOpen,
		status, error, setError, scan, retryService, savePreferences, checkUpdates, switchBranch, updateRepository, clearUpdate, connectAccount,
		refreshNotifications, markNotificationRead, dismissNotification,
	};
}
