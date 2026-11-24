"use client";

import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { usePathname } from "next/navigation";
import {
	EngineDialog,
	EnginePopover,
	useEngineTransitions,
} from "@/engine";
import { useGitManager } from "../lib/use-gitmanager";
import { invoke, openDialog } from "../lib/tauri";
import {
	applyClassicThemeMode,
	isLightTheme,
	loadStoredThemeMode,
	type ClassicThemeMode,
} from "../lib/classic-theme";
import { WorkspaceProvider } from "../lib/workspace-context";
import { AnimatedBrandLogo, type LogoMood } from "./animated-brand-logo";
import { LeftNavRail } from "./left-nav-rail";
import { ActivityDrawer, type ActivityDrawerTab, type TaskManagerSnapshot } from "./activity-drawer";
import { RingLoadingIndicator } from "./repo-ring-badge";
import { Button, Glyph, Switch } from "./shared-ui";
import type { AppNotification, AppPreferences, GitRepository } from "../lib/types";

function normalizeRoute(pathname: string | null): string {
	if (!pathname || pathname === "/") return "/";
	return pathname.replace(/\/+$/, "") || "/";
}

/**
 * Top-Bar Navigation Header shared across all Next.js App Router pages.
 * Highlights the active route and uses Next.js Engine's `transitions.push(...)`
 * for smooth per-page static route navigation.
 */
function TopNavHeader({
	activePath,
	repositoryCount,
	accountCount,
	themeMode,
	logoMood,
	scanning,
	reloadingData,
	serviceReady,
	remoteCheckPending,
	remoteCheckRunning,
	serviceIndexing,
	version,
	status,
	notifications,
	inAppNotificationsEnabled,
	onNavigate,
	onToggleTheme,
	onScan,
	onResetStorage,
	onQueueRemoteCheck,
	onRetryService,
	onOpenQuickJump,
	onRefreshNotifications,
	onMarkNotificationRead,
	onMarkAllNotificationsRead,
	onDismissNotification,
	onOpenNotification,
}: {
	activePath: string;
	repositoryCount: number;
	accountCount: number;
	themeMode: ClassicThemeMode;
	logoMood: LogoMood;
	scanning: boolean;
	reloadingData: boolean;
	serviceReady: boolean;
	remoteCheckPending: boolean;
	remoteCheckRunning: boolean;
	serviceIndexing: boolean;
	version: string;
	status: string;
	notifications: AppNotification[];
	inAppNotificationsEnabled: boolean;
	onNavigate: (href: string) => void;
	onToggleTheme: () => void;
	onScan: () => void;
	onResetStorage: () => void;
	onQueueRemoteCheck: () => void;
	onRetryService: () => void;
	onOpenQuickJump: () => void;
	onRefreshNotifications: () => Promise<unknown>;
	onMarkNotificationRead: (id: string) => Promise<void>;
	onMarkAllNotificationsRead: () => Promise<number>;
	onDismissNotification: (id: string) => Promise<void>;
	onOpenNotification: (item: AppNotification) => void;
}) {
	const unreadCount = inAppNotificationsEnabled ? notifications.filter((item) => !item.read).length : 0;

	return (
		<header className="gm-topnav">
			<div className="gm-topnav-left gm-topnav-left-hidden">
				<button
					type="button"
					className="gm-topnav-brand"
					onClick={() => onNavigate("/")}
					title="GitManager — Repositories"
				>
					<AnimatedBrandLogo size={32} mood={logoMood} themeMode={themeMode} />
					<div className="gm-topnav-brand-copy">
						<strong>GitManager</strong>
						<span>Git Workbench</span>
					</div>
				</button>

				<nav className="gm-topnav-tabs" aria-label="Primary navigation">
					<button
						type="button"
						className={activePath === "/" || activePath === "/repo" ? "gm-topnav-tab active" : "gm-topnav-tab"}
						onClick={() => onNavigate("/")}
					>
						<span className="gm-tab-icon">⎇</span>
						<span>Repositories</span>
						<span className="gm-tab-badge">{repositoryCount}</span>
					</button>
					<button
						type="button"
						className={activePath === "/actions" ? "gm-topnav-tab active" : "gm-topnav-tab"}
						onClick={() => onNavigate("/actions")}
					>
						<span className="gm-tab-icon">▶</span>
						<span>Actions</span>
					</button>
					<button
						type="button"
						className={activePath === "/accounts" ? "gm-topnav-tab active" : "gm-topnav-tab"}
						onClick={() => onNavigate("/accounts")}
					>
						<span className="gm-tab-icon">◎</span>
						<span>Accounts</span>
						{accountCount > 0 && <span className="gm-tab-badge">{accountCount}</span>}
					</button>
					<button
						type="button"
						className={
							activePath === "/settings" ||
							activePath === "/cleaner" ||
							activePath === "/service" ||
							activePath === "/performance"
								? "gm-topnav-tab active"
								: "gm-topnav-tab"
						}
						onClick={() => onNavigate("/settings")}
					>
						<span className={serviceReady ? "gm-led clean" : "gm-led offline"} />
						<span>Settings</span>
					</button>
				</nav>
			</div>

			<div className="gm-topnav-right">
				<button
					type="button"
					className="gm-quickjump-trigger"
					onClick={onOpenQuickJump}
					title="Quick Jump to any repository (Ctrl+K)"
				>
					<Glyph>⌕</Glyph>
					<span>Quick Jump</span>
					<kbd>Ctrl K</kbd>
				</button>

				<Button
					onClick={onResetStorage}
					disabled={reloadingData || scanning}
					title="Clear cached index &amp; storage and rebuild fresh from disk"
				>
					{reloadingData ? "Resetting…" : "↻ Fresh Data"}
				</Button>

				<Button
					variant="primary"
					onClick={onScan}
					disabled={scanning}
					title="Scan configured folders for Git repositories"
				>
					{scanning ? "Scanning…" : "Scan Repos"}
				</Button>

				<button
					type="button"
					className="gm-theme-toggle"
					onClick={onToggleTheme}
					title={isLightTheme(themeMode) ? "Switch to Git Dark" : "Switch to Git Light"}
				>
					{isLightTheme(themeMode) ? "☾ Dark" : "☀ Light"}
				</button>

				<EnginePopover
					triggerAsChild
					placement="bottom"
					align="end"
					offset={8}
					trigger={
						<button type="button" className="gm-status-chip" title="Core Service &amp; IPC status">
							<span className={serviceReady ? "gm-led clean" : "gm-led offline"} />
							<span>{serviceIndexing ? "Indexing" : serviceReady ? "IPC OK" : "Offline"}</span>
						</button>
					}
				>
					<div className="gm-popover-body">
						<div className="gm-popover-title">Core Service &amp; IPC</div>
						<p className="gm-popover-copy">{status}</p>
						<div className="gm-popover-meta">
							<span>State: <strong>{serviceReady ? "Connected" : "Offline"}</strong></span>
							<span>Remote check: <strong>{remoteCheckRunning ? "Running" : remoteCheckPending ? "Queued" : "Idle"}</strong></span>
							<span>Version: <code>v{version}</code></span>
						</div>
						<div className="gm-popover-actions">
							<Button onClick={onQueueRemoteCheck} disabled={!serviceReady || remoteCheckRunning}>Fetch All</Button>
							{!serviceReady && <Button variant="primary" onClick={onRetryService}>Retry</Button>}
							<Button onClick={() => onNavigate("/service")}>Service Manager</Button>
						</div>
					</div>
				</EnginePopover>

				{inAppNotificationsEnabled && (
					<EnginePopover
						triggerAsChild
						placement="bottom"
						align="end"
						offset={8}
						onOpenChange={(open) => { if (open) void onRefreshNotifications(); }}
						trigger={
							<button type="button" className="gm-status-chip" title="Repository update notifications">
								<span>Inbox</span>
								{unreadCount > 0 && <span className="gm-unread-badge">{unreadCount}</span>}
							</button>
						}
					>
						<div className="gm-popover-body gm-notif-popover">
							<div className="gm-popover-header-row">
								<span className="gm-popover-title">Notifications</span>
								{unreadCount > 0 && (
									<button
										type="button"
										className="gm-rail-inline-link"
										onClick={() => void onMarkAllNotificationsRead()}
									>
										Mark all read
									</button>
								)}
							</div>
							{notifications.length === 0 ? (
								<p className="gm-popover-copy">All caught up — no repository alerts.</p>
							) : (
								<div className="gm-notif-list">
									{notifications.slice(0, 12).map((item) => (
										<div key={item.id} className={item.read ? "gm-notif-row read" : "gm-notif-row"}>
											<button
												type="button"
												className="gm-notif-main"
												onClick={() => onOpenNotification(item)}
											>
												<strong>{item.title}</strong>
												<span>{item.body}</span>
											</button>
											<div className="gm-notif-actions">
												{!item.read && (
													<button type="button" onClick={() => void onMarkNotificationRead(item.id)}>Read</button>
												)}
												<button type="button" onClick={() => void onDismissNotification(item.id)}>×</button>
											</div>
										</div>
									))}
								</div>
							)}
						</div>
					</EnginePopover>
				)}
			</div>
		</header>
	);
}

/** Docked Bottom Settings & Storage Control Panel shared across all pages. */
function BottomSettingsDock({
	open,
	onToggleOpen,
	preferences,
	onSavePreferences,
	onResetStorage,
	onOpenFullSettings,
	onOpenServiceManager,
	themeMode,
	onToggleTheme,
	serviceReady,
	available,
	repositoryCount,
	status,
	reloadingData,
	scanning,
}: {
	open: boolean;
	onToggleOpen: () => void;
	preferences: AppPreferences;
	onSavePreferences: (next: AppPreferences) => Promise<unknown>;
	onResetStorage: () => Promise<void>;
	onOpenFullSettings: () => void;
	onOpenServiceManager: () => void;
	themeMode: ClassicThemeMode;
	onToggleTheme: () => void;
	serviceReady: boolean;
	available: boolean;
	repositoryCount: number;
	status: string;
	reloadingData: boolean;
	scanning: boolean;
}) {
	const [savingQuick, setSavingQuick] = useState(false);
	const [gitmOnPath, setGitmOnPath] = useState(false);

	useEffect(() => {
		if (!open) return;
		void invoke<boolean>("get_gitm_cli_path_status")
			.then((value) => setGitmOnPath(Boolean(value)))
			.catch(() => undefined);
	}, [open]);

	async function toggleGitmPath(next: boolean) {
		setSavingQuick(true);
		try {
			await invoke<string>("set_gitm_cli_path_integration", { enabled: next });
			setGitmOnPath(next);
		} finally {
			setSavingQuick(false);
		}
	}

	async function addScanFolder() {
		try {
			const selected = await openDialog({ title: "Add a Git projects folder", directory: true, multiple: false });
			const folder = Array.isArray(selected) ? selected[0] : selected;
			if (!folder) return;
			const exists = preferences.scanRoots.some((item) => item.toLowerCase() === folder.toLowerCase());
			if (exists) return;
			setSavingQuick(true);
			await onSavePreferences({ ...preferences, scanRoots: [...preferences.scanRoots, folder] });
		} finally {
			setSavingQuick(false);
		}
	}

	async function removeScanFolder(rootToRemove: string) {
		setSavingQuick(true);
		try {
			await onSavePreferences({
				...preferences,
				scanRoots: preferences.scanRoots.filter((item) => item !== rootToRemove),
			});
		} finally {
			setSavingQuick(false);
		}
	}

	return (
		<div className="gm-bottom-dock">
			{open && (
				<section className="gm-bottom-settings-tray" aria-label="Quick settings and storage panel">
					<div className="gm-tray-grid">
						<div className="gm-tray-col">
							<div className="gm-tray-heading">
								<strong>Scan Folders</strong>
								<Button onClick={() => void addScanFolder()} disabled={savingQuick}>+ Add folder</Button>
							</div>
							<div className="gm-tray-roots">
								{preferences.scanRoots.length === 0 ? (
									<span className="gm-muted-text">No folders configured yet — add your projects folder.</span>
								) : (
									preferences.scanRoots.map((root) => (
										<div key={root} className="gm-tray-root-chip">
											<span title={root}>{root}</span>
											<button
												type="button"
												disabled={savingQuick}
												onClick={() => void removeScanFolder(root)}
												title="Remove folder"
											>
												×
											</button>
										</div>
									))
								)}
							</div>
						</div>

						<div className="gm-tray-col">
							<div className="gm-tray-heading"><strong>Quick Preferences</strong></div>
							<div className="gm-tray-switches">
								<Switch
									label="Show dependency repositories"
									checked={preferences.showDependencyRepositories}
									onChange={(value) => { void onSavePreferences({ ...preferences, showDependencyRepositories: value }); }}
								/>
								<Switch
									label="OS desktop notifications"
									checked={preferences.osNotificationsEnabled}
									onChange={(value) => { void onSavePreferences({ ...preferences, osNotificationsEnabled: value }); }}
								/>
								<Switch
									label="Install gitm CLI on PATH"
									checked={gitmOnPath}
									onChange={(value) => { void toggleGitmPath(value); }}
								/>
							</div>
						</div>

						<div className="gm-tray-col">
							<div className="gm-tray-heading"><strong>Storage &amp; Maintenance</strong></div>
							<p className="gm-tray-note">
								Clear cached index files and rebuild GitManager&apos;s workspace data from scratch (never touches your Git repos).
							</p>
							<div className="gm-tray-actions">
								<Button
									variant="primary"
									disabled={reloadingData || scanning}
									onClick={() => void onResetStorage()}
								>
									{reloadingData ? "Refreshing Storage…" : "↻ Refresh Storage & Data"}
								</Button>
								<Button onClick={onToggleTheme}>
									{isLightTheme(themeMode) ? "☾ Git Dark" : "☀ Git Light"}
								</Button>
								<Button onClick={onOpenFullSettings}>All Settings →</Button>
							</div>
						</div>
					</div>
				</section>
			)}

			<footer className="gm-footer">
				<div className="gm-footer-left">
					<button type="button" className="gm-footer-pill" onClick={onOpenServiceManager}>
						<i className={serviceReady ? "gm-led clean" : "gm-led offline"} />
						<span>{available ? (serviceReady ? "Core Service Connected" : "Core Service Offline") : "Browser Preview"}</span>
					</button>
					<span className="gm-footer-status">{repositoryCount} repos · {status}</span>
				</div>

				<div className="gm-footer-right">
					<button
						type="button"
						className="gm-footer-pill"
						disabled={reloadingData || scanning}
						onClick={() => void onResetStorage()}
						title="Reset cached storage and re-scan repositories"
					>
						↻ Refresh Storage
					</button>
					<button
						type="button"
						className={open ? "gm-footer-pill active" : "gm-footer-pill"}
						onClick={onToggleOpen}
					>
						⚙ Quick Settings {open ? "▾" : "▴"}
					</button>
				</div>
			</footer>
		</div>
	);
}

function SetupWizardContent({
	preferences,
	onSave,
	onClose,
}: {
	preferences: AppPreferences;
	onSave: (next: AppPreferences) => Promise<unknown>;
	onClose: () => void;
}) {
	const [draft, setDraft] = useState(preferences);
	const [newRoot, setNewRoot] = useState("");
	const [saving, setSaving] = useState(false);
	const [startServiceAtSignIn, setStartServiceAtSignIn] = useState(true);
	const [installGitmToPath, setInstallGitmToPath] = useState(true);
	const [error, setError] = useState("");

	function patch(values: Partial<AppPreferences>) {
		setDraft((current) => ({ ...current, ...values }));
	}

	async function chooseSetupRoot() {
		try {
			const selected = await openDialog({ title: "Choose your main projects folder", directory: true, multiple: false });
			const path = Array.isArray(selected) ? selected[0] : selected;
			if (path) {
				patch({
					scanRoots: draft.scanRoots.some((root) => root.toLowerCase() === path.toLowerCase())
						? draft.scanRoots
						: [...draft.scanRoots, path],
				});
			}
		} catch (cause) {
			setError(cause instanceof Error ? cause.message : String(cause));
		}
	}

	async function chooseSetupArchiveRoot() {
		try {
			const selected = await openDialog({ title: "Choose an archive destination", directory: true, multiple: false });
			const path = Array.isArray(selected) ? selected[0] : selected;
			if (path) patch({ archiveRoot: path });
		} catch (cause) {
			setError(cause instanceof Error ? cause.message : String(cause));
		}
	}

	async function save() {
		setSaving(true);
		setError("");
		try {
			if (startServiceAtSignIn) {
				await invoke("control_background_service", { installOsService: true });
			}
			await invoke("set_gitm_cli_path_integration", { enabled: installGitmToPath }).catch(() => undefined);
			await onSave({ ...draft, firstRunComplete: true });
			onClose();
		} catch (cause) {
			setError(cause instanceof Error ? cause.message : String(cause));
		} finally {
			setSaving(false);
		}
	}

	return (
		<div className="gm-setup-body">
			<div className="gm-root-list">
				{draft.scanRoots.map((root) => (
					<div className="gm-root-row" key={root}>
						<Glyph>▰</Glyph>
						<span>{root}</span>
						<button type="button" onClick={() => patch({ scanRoots: draft.scanRoots.filter((item) => item !== root) })}>×</button>
					</div>
				))}
			</div>
			<div className="gm-add-root">
				<input
					value={newRoot}
					onChange={(event) => setNewRoot(event.target.value)}
					onKeyDown={(event) => {
						if (event.key === "Enter" && newRoot.trim()) {
							patch({ scanRoots: [...draft.scanRoots, newRoot.trim()] });
							setNewRoot("");
						}
					}}
					placeholder="Add a folder path…"
				/>
				<Button onClick={() => { if (newRoot.trim()) patch({ scanRoots: [...draft.scanRoots, newRoot.trim()] }); setNewRoot(""); }}>Add</Button>
				<Button onClick={() => void chooseSetupRoot()}>Browse…</Button>
			</div>
			<Switch
				label="Show dependency repositories"
				checked={draft.showDependencyRepositories}
				onChange={(value) => patch({ showDependencyRepositories: value })}
			/>
			<div className="gm-field">
				<span>Archive destination</span>
				<div className="gm-add-root">
					<input
						value={draft.archiveRoot}
						onChange={(event) => patch({ archiveRoot: event.target.value })}
						placeholder="Default application archive folder"
					/>
					<Button onClick={() => void chooseSetupArchiveRoot()}>Browse…</Button>
				</div>
			</div>
			<Switch
				label="Start GitManager Core Service automatically at sign-in"
				checked={startServiceAtSignIn}
				onChange={setStartServiceAtSignIn}
			/>
			<Switch
				label="Install gitm CLI on PATH with GitManager (run `gitm status` from any terminal)"
				checked={installGitmToPath}
				onChange={setInstallGitmToPath}
			/>
			{error && <div className="gm-inline-error">{error}</div>}
			<div className="gm-modal-actions">
				{draft.firstRunComplete && <Button onClick={onClose}>Close</Button>}
				<Button variant="primary" onClick={() => void save()} disabled={saving || draft.scanRoots.length === 0}>
					{saving ? "Saving…" : "Save & start indexing"}
				</Button>
			</div>
		</div>
	);
}

/**
 * Persistent Root Application Shell mounted by `app/layout.tsx`.
 * Keeps IPC connections, heartbeat, Top-Bar Navigation, Bottom Settings Dock,
 * and Quick Jump alive while individual Next.js pages code-split cleanly.
 */
export function GitManagerApp({ children }: { children?: ReactNode }) {
	const pathname = usePathname();
	const activePath = normalizeRoute(pathname);
	const transitions = useEngineTransitions();

	// Keep the WebView heartbeat wired to crash_helper so UI freezes are detected.
	useEffect(() => {
		let active = true;
		const reportHeartbeat = () => {
			if (active) void invoke("report_ui_heartbeat").catch(() => undefined);
		};
		reportHeartbeat();
		const heartbeat = window.setInterval(reportHeartbeat, 3000);
		return () => {
			active = false;
			window.clearInterval(heartbeat);
		};
	}, []);

	const git = useGitManager();
	const [themeMode, setThemeModeState] = useState<ClassicThemeMode>("git-dark");
	const [bottomSettingsOpen, setBottomSettingsOpen] = useState(false);
	const [quickJumpOpen, setQuickJumpOpen] = useState(false);
	const [quickJumpQuery, setQuickJumpQuery] = useState("");
	const [railExpanded, setRailExpanded] = useState(false);
	const [activityOpen, setActivityOpen] = useState(false);
	const [activityTab, setActivityTab] = useState<ActivityDrawerTab>("tasks");
	const [taskSnapshot, setTaskSnapshot] = useState<TaskManagerSnapshot>({
		paused: false,
		active_lanes: 0,
		total_running: 0,
		total_queued: 0,
		groups: [],
		history: [],
	});
	const [savingPulse, setSavingPulse] = useState(false);
	const [happyCelebration, setHappyCelebration] = useState(false);
	const happyTimerRef = useRef<number | null>(null);
	const quickJumpInputRef = useRef<HTMLInputElement | null>(null);

	const triggerHappyLogo = useCallback(() => {
		if (happyTimerRef.current !== null) {
			window.clearTimeout(happyTimerRef.current);
		}
		setHappyCelebration(true);
		happyTimerRef.current = window.setTimeout(() => {
			setHappyCelebration(false);
			happyTimerRef.current = null;
		}, 900);
	}, []);

	useEffect(() => {
		setThemeMode(loadStoredThemeMode());
		return () => {
			if (happyTimerRef.current !== null) window.clearTimeout(happyTimerRef.current);
		};
	}, []);

	useEffect(() => {
		const onKeyDown = (event: KeyboardEvent) => {
			if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "k") {
				event.preventDefault();
				setQuickJumpOpen((open) => !open);
				setQuickJumpQuery("");
			}
		};
		window.addEventListener("keydown", onKeyDown);
		return () => window.removeEventListener("keydown", onKeyDown);
	}, []);

	const logoMood: LogoMood = happyCelebration
		? "happy"
		: savingPulse || git.reloadingData
			? "saving"
			: git.loading || git.scanning || git.serviceIndexing
				? "loading"
				: "idle";

	const setThemeMode = useCallback((next: ClassicThemeMode) => {
		setThemeModeState(next);
		applyClassicThemeMode(next, true);
	}, []);

	const toggleTheme = useCallback(() => {
		setThemeModeState((current) => {
			const next: ClassicThemeMode = isLightTheme(current) ? "git-dark" : "git-light";
			applyClassicThemeMode(next, true);
			return next;
		});
	}, []);

	const navigateTo = useCallback((href: string, preset: "fade" | "layout" = "fade") => {
		if (href === "/accounts") void git.loadAccounts().catch(() => undefined);
		void transitions.push(href, preset);
	}, [git.loadAccounts, transitions]);

	const openRepository = useCallback((repo: GitRepository, view = "overview") => {
		const params = new URLSearchParams({ id: repo.id, view });
		void transitions.push(`/repo/?${params.toString()}`, "layout");
	}, [transitions]);

	const handleScan = useCallback(() => {
		void git.scan().then(() => triggerHappyLogo());
	}, [git.scan, triggerHappyLogo]);

	const handleResetStorage = useCallback(async () => {
		setSavingPulse(true);
		try {
			await git.resetStorageAndRescan();
			triggerHappyLogo();
		} finally {
			setSavingPulse(false);
		}
	}, [git.resetStorageAndRescan, triggerHappyLogo]);

	const handleSavePreferences = useCallback(async (next: AppPreferences) => {
		setSavingPulse(true);
		try {
			const saved = await git.savePreferences(next);
			triggerHappyLogo();
			return saved;
		} finally {
			setSavingPulse(false);
		}
	}, [git.savePreferences, triggerHappyLogo]);

	const handleRetryService = useCallback(() => {
		void git.retryService().then(() => triggerHappyLogo());
	}, [git.retryService, triggerHappyLogo]);

	const handleQueueRemoteCheck = useCallback(() => {
		void git.queueRemoteCheck();
	}, [git.queueRemoteCheck]);

	const openNotificationRepository = useCallback(async (item: AppNotification) => {
		if (!item.repositoryPath) return;
		let repository = git.repositories.find((candidate) => candidate.path === item.repositoryPath) ?? null;
		if (!repository) {
			const refreshed = await git.scan();
			repository = refreshed.find((candidate) => candidate.path === item.repositoryPath) ?? null;
		}
		if (repository) {
			openRepository(repository);
		} else {
			navigateTo("/");
			git.setError("The repository from this notification is not currently in the saved index.");
		}
		if (!item.read) {
			await git.markNotificationRead(item.id).catch(() => undefined);
		}
	}, [git.repositories, git.scan, git.markNotificationRead, git.setError, navigateTo, openRepository]);

	const quickJumpMatches = useMemo(() => {
		const needle = quickJumpQuery.trim().toLowerCase();
		if (!needle) return git.repositories.slice(0, 10);
		return git.repositories
			.filter(
				(repo) =>
					repo.name.toLowerCase().includes(needle)
					|| repo.current_branch.toLowerCase().includes(needle)
					|| repo.path.toLowerCase().includes(needle),
			)
			.slice(0, 12);
	}, [git.repositories, quickJumpQuery]);

	const workspaceValue = useMemo(() => ({
		git,
		themeMode,
		logoMood,
		toggleTheme,
		setThemeMode,
		triggerHappyLogo,
		handleScan,
		handleResetStorage,
		handleSavePreferences,
		handleRetryService,
		handleQueueRemoteCheck,
		navigateTo,
		openRepository,
	}), [
		git,
		themeMode,
		logoMood,
		toggleTheme,
		setThemeMode,
		triggerHappyLogo,
		handleScan,
		handleResetStorage,
		handleSavePreferences,
		handleRetryService,
		handleQueueRemoteCheck,
		navigateTo,
		openRepository,
	]);

	if (git.loading) {
		return (
			<div className="gm-boot-screen" role="status" aria-live="polite">
				<div className="gm-boot-card">
					<AnimatedBrandLogo size={76} mood="loading" themeMode={themeMode} />
					<div className="gm-boot-brand">
						<strong>GitManager</strong>
						<span>Classic Developer Workbench</span>
					</div>
					<RingLoadingIndicator label="Connecting to core service…" size={28} themeMode={themeMode} />
					<div className="gm-boot-progress"><i /></div>
				</div>
			</div>
		);
	}

	const unreadNotificationCount =
		git.preferences.inAppNotificationsEnabled !== false
			? git.notifications.filter((item) => !item.read).length
			: 0;

	return (
		<WorkspaceProvider value={workspaceValue}>
			<div className="gm-browser-frame">
				<LeftNavRail
					expanded={railExpanded}
					onToggleExpanded={() => setRailExpanded((v) => !v)}
					activePath={activePath}
					themeMode={themeMode}
					repositoryCount={git.repositories.length}
					accountCount={git.accounts.length}
					unreadNotifications={unreadNotificationCount}
					serviceReady={git.serviceReady}
					activityOpen={activityOpen}
					activityTab={activityTab}
					taskSnapshot={taskSnapshot}
					onNavigate={(href) => navigateTo(href)}
					onToggleTasks={() => {
						if (activityOpen && activityTab === "tasks") {
							setActivityOpen(false);
						} else {
							setActivityOpen(true);
							setActivityTab("tasks");
						}
					}}
					onToggleInbox={() => {
						if (activityOpen && activityTab === "notifications") {
							setActivityOpen(false);
						} else {
							setActivityOpen(true);
							setActivityTab("notifications");
						}
					}}
				/>

				<ActivityDrawer
					open={activityOpen}
					activeTab={activityTab}
					onChangeTab={setActivityTab}
					onClose={() => setActivityOpen(false)}
					themeMode={themeMode}
					serviceReady={git.serviceReady}
					onRestartBgw={handleRetryService}
					notifications={git.notifications}
					onMarkNotificationRead={git.markNotificationRead}
					onDismissNotification={git.dismissNotification}
					onOpenNotification={openNotificationRepository}
					onTriggerHappyLogo={triggerHappyLogo}
					onSnapshotChange={setTaskSnapshot}
				/>

				<div className="gm-app-shell">
					<TopNavHeader
					activePath={activePath}
					repositoryCount={git.repositories.length}
					accountCount={git.accounts.length}
					themeMode={themeMode}
					logoMood={logoMood}
					scanning={git.scanning || git.awaitingInitialIndex}
					reloadingData={git.reloadingData}
					serviceReady={git.serviceReady}
					remoteCheckPending={git.remoteCheckPending}
					remoteCheckRunning={git.remoteCheckRunning}
					serviceIndexing={git.serviceIndexing}
					version={git.version}
					status={git.status}
					notifications={git.notifications}
					inAppNotificationsEnabled={git.preferences.inAppNotificationsEnabled !== false}
					onNavigate={(href) => navigateTo(href)}
					onToggleTheme={toggleTheme}
					onScan={handleScan}
					onResetStorage={() => { void handleResetStorage(); }}
					onQueueRemoteCheck={handleQueueRemoteCheck}
					onRetryService={handleRetryService}
					onOpenQuickJump={() => { setQuickJumpOpen(true); setQuickJumpQuery(""); }}
					onRefreshNotifications={git.refreshNotifications}
					onMarkNotificationRead={git.markNotificationRead}
					onMarkAllNotificationsRead={git.markAllNotificationsRead}
					onDismissNotification={git.dismissNotification}
					onOpenNotification={openNotificationRepository}
				/>

				<main className="gm-main">
					{(git.loading || git.scanning || git.serviceIndexing) && <div className="gm-loading-bar"><i /></div>}
					{git.error && (
						<div className="gm-error-banner" role="alert">
							<span>{git.error}</span>
							<button type="button" onClick={() => git.setError("")}>Dismiss</button>
						</div>
					)}
					{children}
				</main>

				<BottomSettingsDock
					open={bottomSettingsOpen}
					onToggleOpen={() => {
						void transitions.run(() => setBottomSettingsOpen((value) => !value), "layout");
					}}
					preferences={git.preferences}
					onSavePreferences={handleSavePreferences}
					onResetStorage={handleResetStorage}
					onOpenFullSettings={() => {
						setBottomSettingsOpen(false);
						navigateTo("/settings");
					}}
					onOpenServiceManager={() => navigateTo("/service")}
					themeMode={themeMode}
					onToggleTheme={toggleTheme}
					serviceReady={git.serviceReady}
					available={git.available}
					repositoryCount={git.repositories.length}
					status={git.status}
					reloadingData={git.reloadingData}
					scanning={git.scanning}
				/>

				<EngineDialog
					open={quickJumpOpen}
					onOpenChange={setQuickJumpOpen}
					title="Quick Jump"
					description="Jump directly to any local Git repository."
					duration={160}
				>
					<div className="gm-quickjump-body">
						<input
							ref={quickJumpInputRef}
							autoFocus
							className="gm-quickjump-input"
							value={quickJumpQuery}
							onChange={(event) => setQuickJumpQuery(event.target.value)}
							placeholder="Type a repository name, branch, or path…"
							onKeyDown={(event) => {
								if (event.key === "Enter" && quickJumpMatches[0]) {
									openRepository(quickJumpMatches[0]);
									setQuickJumpOpen(false);
								}
							}}
						/>
						<div className="gm-quickjump-list">
							{quickJumpMatches.map((repo) => (
								<button
									key={repo.id}
									type="button"
									className="gm-quickjump-item"
									onClick={() => {
										openRepository(repo);
										setQuickJumpOpen(false);
									}}
								>
									<span className={repo.dirty_files_count > 0 ? "gm-led dirty" : "gm-led clean"} />
									<strong>{repo.name}</strong>
									<code>{repo.current_branch || "HEAD"}</code>
									<small>{repo.path}</small>
								</button>
							))}
							{quickJumpMatches.length === 0 && (
								<div className="gm-rail-empty">No repositories match &ldquo;{quickJumpQuery}&rdquo;</div>
							)}
						</div>
					</div>
				</EngineDialog>

				<EngineDialog
					open={git.setupOpen}
					onOpenChange={git.setSetupOpen}
					title="Workspace Setup"
					description="Choose where GitManager should discover your Git repositories."
					duration={180}
				>
					<SetupWizardContent
						preferences={git.preferences}
						onSave={handleSavePreferences}
						onClose={() => git.setSetupOpen(false)}
					/>
				</EngineDialog>
				</div>
			</div>
		</WorkspaceProvider>
	);
}
