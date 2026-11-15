"use client";

import { useCallback, useEffect, useState } from "react";
import { invoke, isTauriRuntime, listen } from "./tauri";
import type {
	AppPreferences,
	ApplicationDirectories,
	ConnectedAccount,
	GitRepository,
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
};

export function useGitManager() {
	const [available, setAvailable] = useState(false);
	const [loading, setLoading] = useState(true);
	const [scanning, setScanning] = useState(false);
	const [version, setVersion] = useState("—");
	const [preferences, setPreferences] = useState<AppPreferences>(DEFAULT_PREFERENCES);
	const [directories, setDirectories] = useState<ApplicationDirectories | null>(null);
	const [repositories, setRepositories] = useState<GitRepository[]>([]);
	const [dependencies, setDependencies] = useState<string[]>([]);
	const [accounts, setAccounts] = useState<ConnectedAccount[]>([]);
	const [updates, setUpdates] = useState<UpdateAlert[]>([]);
	const [setupOpen, setSetupOpen] = useState(false);
	const [status, setStatus] = useState("Ready");
	const [error, setError] = useState("");

	const performScan = useCallback(async (currentPreferences: AppPreferences, customRoot?: string) => {
		if (!isTauriRuntime()) {
			setStatus("Open GitManager as a desktop app to access local repositories.");
			return [];
		}
		setScanning(true);
		setStatus("Indexing repositories…");
		setError("");
		try {
			const rootPaths = customRoot?.trim()
				? [customRoot.trim()]
				: currentPreferences.scanRoots;
			const summary = await invoke<{
				repositories?: GitRepository[];
				dependencyRepoIds?: string[];
			}>("scan_machine_repositories", {
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
			setDependencies(summary.dependency_repo_ids ?? summary.dependencyRepoIds ?? []);
			setStatus(`Indexed ${local.length} repositories`);
			return local;
		} catch (cause) {
			const message = cause instanceof Error ? cause.message : String(cause);
			setError(message);
			setStatus("Repository scan failed");
			return [];
		} finally {
			setScanning(false);
		}
	}, []);

	const scan = useCallback((customRoot?: string) => performScan(preferences, customRoot), [performScan, preferences]);

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

	const pullUpdate = useCallback(async (repoPath: string) => {
		setStatus("Pulling verified fast-forward update…");
		try {
			await invoke("pull_repository_update", { repoPath });
			setUpdates((current) => current.filter((item) => item.repo_path !== repoPath));
			await performScan(preferences);
			setStatus("Repository updated");
		} catch (cause) {
			const message = cause instanceof Error ? cause.message : String(cause);
			setError(message);
			setStatus("Update stopped safely");
		}
	}, [performScan, preferences]);

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

		async function initialize() {
			setAvailable(isTauriRuntime());
			if (!isTauriRuntime()) {
				setLoading(false);
				setStatus("Desktop runtime not detected");
				return;
			}
			try {
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
					await performScan(resolved);
				}

				try {
					const identity = await invoke<{ login?: string }>("detect_github_identity");
					if (identity?.login) {
						const account = await invoke<ConnectedAccount>("connect_git_account", {
							provider: "github",
							usernameOrHost: "",
							accessToken: null,
						});
						if (active) setAccounts((current) => [
							...current.filter((item) => item.account_login !== account.account_login),
							account,
						]);
					}
				} catch {
					// Missing GitHub CLI authentication is normal; manual connection remains available.
				}
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

		return () => {
			active = false;
			unlistenUpdate?.();
			unlistenState?.();
		};
	}, [scan]);

	return {
		available, loading, scanning, version, preferences, setPreferences, directories,
		repositories, dependencies, accounts, updates, setupOpen, setSetupOpen,
		status, error, setError, scan, savePreferences, checkUpdates, pullUpdate, connectAccount,
	};
}
