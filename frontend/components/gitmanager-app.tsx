"use client";

import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import dynamic from "next/dynamic";
import { EngineBox, EngineGrid } from "@/engine";
import { useGitManager } from "../lib/use-gitmanager";
import { invoke, openDialog } from "../lib/tauri";
import { Button, EmptyState, Field, Glyph, ProviderMark, RepoAvatar, Switch } from "./shared-ui";
import { parseInternalRoute, repositoryRouteForItem, repositorySubroute, routeLabel, sectionForRoute, sectionRoute } from "../lib/internal-routing";
import type { RepositoryDestination } from "../lib/internal-routing";
import type { AppPreferences, ArchiveSnapshot, AppNotification, BranchInfo, GitRepository } from "../lib/types";

const BRAND_LOGO = "/image/git-managar-orange_default-white.png";
const BRAND_RING = "/image/git-managar-orange-ring_default-white.png";
const BRAND_CENTER = "/image/git-managar-orange-center_default-white.png";

function BrandMark({ size = 36 }: { size?: number }) {
	return (
		<span className="gm-brand-mark" style={{ width: size, height: size }}>
			<img src={BRAND_LOGO} alt="" aria-hidden="true" />
		</span>
	);
}

function LoadingLogo({ size = 76 }: { size?: number }) {
	return (
		<span className="gm-loading-logo" style={{ width: size, height: size }} aria-hidden="true">
			<img className="gm-loading-logo-ring" src={BRAND_RING} alt="" />
			<img className="gm-loading-logo-center" src={BRAND_CENTER} alt="" />
		</span>
	);
}

const RepositoryDetail = dynamic(
	() => import("./repository-detail").then((module) => module.RepositoryDetail),
	{ ssr: false, loading: () => <div className="gm-workspace-loading">Loading repository details…</div> },
);
const AccountsPage = dynamic(
	() => import("./accounts-page").then((module) => module.AccountsPage),
	{ ssr: false, loading: () => <div className="gm-page-chunk-loading">Loading Accounts…</div> },
);
const SettingsPage = dynamic(
	() => import("./settings-page").then((module) => module.SettingsPage),
	{ ssr: false, loading: () => <div className="gm-page-chunk-loading">Loading Settings…</div> },
);
const ServiceManagerPage = dynamic(
	() => import("./service-manager-page").then((module) => module.ServiceManagerPage),
	{ ssr: false, loading: () => <div className="gm-page-chunk-loading">Loading Service Manager…</div> },
);
const PerformancePage = dynamic(
	() => import("./performance-page").then((module) => module.PerformancePage),
	{ ssr: false, loading: () => <div className="gm-page-chunk-loading">Loading Performance Center…</div> },
);
const CleanerPage = dynamic(
	() => import("./cleaner-page").then((module) => module.CleanerPage),
	{ ssr: false, loading: () => <div className="gm-page-chunk-loading">Loading Developer Cleaner…</div> },
);

type PageKey = "repositories" | "accounts" | "settings" | "cleaner";
type RepoFilter = "all" | "remote" | "local";
type RepoKind = "projects" | "dependencies" | "all";

const OVERVIEW_DESTINATION: RepositoryDestination = { kind: "overview" };

function relativeDate(timestamp: number): string {
	if (!timestamp) return "No commit date";
	const elapsed = Math.max(0, Date.now() - timestamp * 1000);
	const minutes = Math.floor(elapsed / 60000);
	if (minutes < 60) return `${Math.max(1, minutes)}m ago`;
	const hours = Math.floor(minutes / 60);
	if (hours < 24) return `${hours}h ago`;
	const days = Math.floor(hours / 24);
	if (days < 30) return `${days}d ago`;
	return new Date(timestamp * 1000).toLocaleDateString();
}



function Sidebar({
	page,
	setPage,
	repositoryCount,
	accountCount,
}: {
	page: PageKey;
	setPage: (page: PageKey) => void;
	repositoryCount: number;
	accountCount: number;
}) {
	const items: { key: PageKey; label: string; icon: string; count?: number }[] = [
		{ key: "repositories", label: "Repositories", icon: "▦", count: repositoryCount },
		{ key: "accounts", label: "Accounts", icon: "◎", count: accountCount },
		{ key: "settings", label: "Settings", icon: "⚙" },
		{ key: "cleaner", label: "Developer Cleaner", icon: "⌁" },
	];
	return (
		<aside className="gm-sidebar">
			<div className="gm-brand">
				<BrandMark />
				<div className="gm-brand-name">
					<strong>GitManager</strong>
					<span>By Kastrick</span>
				</div>
			</div>

			<div className="gm-sidebar-label">WORKSPACE</div>
			<nav className="gm-nav" aria-label="Workspace navigation">
				{items.map((item) => (
					<button
						key={item.key}
						className={page === item.key ? "gm-nav-item active" : "gm-nav-item"}
						onClick={() => setPage(item.key)}
						type="button"
					>
						<span className="gm-nav-icon">{item.icon}</span>
						<span>{item.label}</span>
						{typeof item.count === "number" && <span className="gm-nav-count">{item.count}</span>}
					</button>
				))}
			</nav>

			<div className="gm-sidebar-bottom">
				<div className="gm-sidebar-label">QUICK LINKS</div>
				<button className="gm-quick-link" onClick={() => setPage("repositories")}>
					<Glyph>⌕</Glyph> Browse local projects
				</button>
				<div className="gm-sidebar-footer">
					<div className="gm-user-avatar">K</div>
					<div className="gm-user-copy">
						<strong>Kastrick</strong>
						<span>Desktop workspace</span>
					</div>
					<span className="gm-online-dot" title="Application ready" />
				</div>
			</div>
		</aside>
	);
}

function TopBar({
	internalRoute,
	version,
	status,
	query,
	setQuery,
	scanning,
	serviceReady,
	remoteCheckPending,
	remoteCheckRunning,
	serviceIndexing,
	awaitingInitialIndex,
	repositoryCount,
	scanTruncated,
	notifications,
	inAppNotificationsEnabled,
	onRefreshNotifications,
	onMarkNotificationRead,
	onMarkAllNotificationsRead,
	onDismissNotification,
	onOpenNotification,
	onScan,
	onRetryService,
}: {
	internalRoute: string;
	version: string;
	status: string;
	query: string;
	setQuery: (value: string) => void;
	scanning: boolean;
	serviceReady: boolean;
	remoteCheckPending: boolean;
	remoteCheckRunning: boolean;
	serviceIndexing: boolean;
	awaitingInitialIndex: boolean;
	repositoryCount: number;
	scanTruncated: boolean;
	notifications: AppNotification[];
	inAppNotificationsEnabled: boolean;
	onRefreshNotifications: () => Promise<unknown>;
	onMarkNotificationRead: (id: string) => Promise<void>;
	onMarkAllNotificationsRead: () => Promise<number>;
	onDismissNotification: (id: string) => Promise<void>;
	onOpenNotification: (item: AppNotification) => void;
	onScan: () => void;
	onRetryService: () => void;
}) {
	const [openPanel, setOpenPanel] = useState<"tasks" | "notifications" | null>(null);
	const panelHostRef = useRef<HTMLDivElement | null>(null);
	const [serviceLog, setServiceLog] = useState<string | null>(null);
	const [serviceLogBusy, setServiceLogBusy] = useState(false);
	const [notificationFilter, setNotificationFilter] = useState<"all" | "unread">("all");
	const unreadCount = inAppNotificationsEnabled ? notifications.filter((item) => !item.read).length : 0;
	const visibleNotifications = useMemo(
		() => notificationFilter === "unread" ? notifications.filter((item) => !item.read) : notifications,
		[notificationFilter, notifications],
	);
	const currentRoute = parseInternalRoute(internalRoute);
	const label = routeLabel(currentRoute);

	useEffect(() => {
		if (openPanel === "notifications" && inAppNotificationsEnabled) {
			void onRefreshNotifications();
		}
	}, [openPanel, inAppNotificationsEnabled, onRefreshNotifications]);

	useEffect(() => {
		if (!openPanel) return;
		const closeOnEscape = (event: KeyboardEvent) => {
			if (event.key === "Escape") setOpenPanel(null);
		};
		const closeOnOutsidePointer = (event: PointerEvent) => {
			if (event.target instanceof Node && !panelHostRef.current?.contains(event.target)) {
				setOpenPanel(null);
			}
		};
		document.addEventListener("keydown", closeOnEscape);
		document.addEventListener("pointerdown", closeOnOutsidePointer);
		return () => {
			document.removeEventListener("keydown", closeOnEscape);
			document.removeEventListener("pointerdown", closeOnOutsidePointer);
		};
	}, [openPanel]);

	return (
		<header className="gm-topbar">
			<div className="gm-breadcrumb">
				<span>GitManager</span>
				<span className="gm-breadcrumb-separator">/</span>
				<strong>{label}</strong>
			</div>
			<div className="gm-topbar-actions">
				<label className="gm-global-search">
					<Glyph>⌕</Glyph>
					<input
						value={query}
						onChange={(event) => setQuery(event.target.value)}
						placeholder="Search repositories…"
						aria-label="Search repositories"
					/>
					<kbd>Ctrl K</kbd>
				</label>
				<span className="gm-version-pill">v{version}</span>
				<div className="gm-topbar-popover-host" ref={panelHostRef}>
					<div className="gm-topbar-panel-triggers">
						<button
							type="button"
							className={openPanel === "tasks" ? "gm-topbar-panel-trigger active" : "gm-topbar-panel-trigger"}
							aria-expanded={openPanel === "tasks"}
							aria-controls="gm-task-center-popover"
							onClick={() => setOpenPanel((current) => current === "tasks" ? null : "tasks")}
						>
							<span aria-hidden="true">≡</span> Tasks
							<span className={(scanning || serviceIndexing) ? "gm-panel-indicator busy" : serviceReady ? "gm-panel-indicator ready" : "gm-panel-indicator offline"} />
						</button>
						<button
							type="button"
							className={openPanel === "notifications" ? "gm-topbar-panel-trigger active" : "gm-topbar-panel-trigger"}
							aria-expanded={openPanel === "notifications"}
							aria-controls="gm-notifications-popover"
							onClick={() => setOpenPanel((current) => current === "notifications" ? null : "notifications")}
						>
							<span aria-hidden="true">♢</span> Notifications
							{unreadCount > 0 && <span className="gm-topbar-unread-count">{unreadCount > 99 ? "99+" : unreadCount}</span>}
						</button>
					</div>
					{openPanel === "tasks" && (
						<section id="gm-task-center-popover" className="gm-topbar-popover gm-task-center" role="dialog" aria-label="Task Center">
							<div className="gm-popover-heading">
								<div><strong>Task Center</strong><small>Background activity and service health</small></div>
								<button type="button" onClick={() => setOpenPanel(null)} aria-label="Close Task Center">×</button>
							</div>
							<div className="gm-task-list">
								<div className="gm-task-row">
									<span className={serviceReady ? "gm-task-state ready" : "gm-task-state offline"} />
									<div><strong>GitManager Core Service</strong><small>{serviceReady ? "Connected · required engine ready" : "Not connected · application features are unavailable"}</small></div>
									<span className="gm-task-meta">{serviceReady ? "Ready" : "Offline"}</span>
								</div>
								<div className="gm-task-row">
									<span className={(scanning || serviceIndexing) ? "gm-task-state busy" : awaitingInitialIndex ? "gm-task-state idle" : "gm-task-state ready"} />
									<div><strong>Repository index</strong><small>{(scanning || serviceIndexing) ? "Discovering repositories in the service process…" : awaitingInitialIndex ? "Waiting for the service to publish its first repository snapshot…" : scanTruncated ? "Index ready · safety limit reached" : `${repositoryCount} repositories indexed`}</small></div>
									<span className="gm-task-meta">{(scanning || serviceIndexing) ? "Running" : awaitingInitialIndex ? "Waiting" : "Ready"}</span>
								</div>
								<div className="gm-task-row">
									<span className={remoteCheckRunning ? "gm-task-state busy" : remoteCheckPending ? "gm-task-state idle" : "gm-task-state ready"} />
									<div><strong>Remote repository check</strong><small>{remoteCheckRunning ? "Checking remote branches and publishing notifications…" : remoteCheckPending ? "Queued in the core service…" : "No remote check is running"}</small></div>
									<span className="gm-task-meta">{remoteCheckRunning ? "Running" : remoteCheckPending ? "Queued" : "Idle"}</span>
								</div>
								<div className="gm-task-row">
									<span className="gm-task-state idle" />
									<div><strong>Service status</strong><small>{status}</small></div>
								</div>
							</div>
							{serviceLog !== null && <pre className="gm-service-log">{serviceLog || "No service log output yet."}</pre>}
							<div className="gm-popover-actions">
								<button type="button" onClick={onScan} disabled={!serviceReady || scanning || serviceIndexing}>{scanning || serviceIndexing ? "Indexing…" : "Refresh repository index"}</button>
								<button type="button" disabled={serviceLogBusy} onClick={async () => {
									if (serviceLog !== null) { setServiceLog(null); return; }
									setServiceLogBusy(true);
									try { setServiceLog(await invoke<string>("read_service_log_tail")); }
									catch (cause) { setServiceLog(cause instanceof Error ? cause.message : String(cause)); }
									finally { setServiceLogBusy(false); }
								}}>{serviceLogBusy ? "Loading log…" : serviceLog !== null ? "Hide service log" : "View service log"}</button>
								{!serviceReady && <button type="button" className="primary" onClick={onRetryService}>Start service</button>}
							</div>
						</section>
					)}
					{openPanel === "notifications" && (
						<section id="gm-notifications-popover" className="gm-topbar-popover gm-notifications-panel" role="dialog" aria-label="Notifications">
							<div className="gm-popover-heading">
								<div><strong>Notifications</strong><small>{unreadCount} unread · {notifications.length} saved</small></div>
								<button type="button" onClick={() => setOpenPanel(null)} aria-label="Close Notifications">×</button>
							</div>
							<div className="gm-notification-filters" role="tablist" aria-label="Notification filter">
								<button type="button" role="tab" aria-selected={notificationFilter === "all"} className={notificationFilter === "all" ? "active" : ""} onClick={() => setNotificationFilter("all")}>
									All <span>{notifications.length}</span>
								</button>
								<button type="button" role="tab" aria-selected={notificationFilter === "unread"} className={notificationFilter === "unread" ? "active" : ""} onClick={() => setNotificationFilter("unread")}>
									Unread <span>{unreadCount}</span>
								</button>
							</div>
							<div className="gm-notification-list">
								{!inAppNotificationsEnabled ? (
									<div className="gm-notification-empty"><strong>In-app notifications are disabled</strong><span>Enable them in Settings to show saved activity here. Operating system notifications remain independently configurable.</span></div>
								) : visibleNotifications.length === 0 ? (
									<div className="gm-notification-empty"><strong>{notificationFilter === "unread" ? "No unread notifications" : "You're all caught up"}</strong><span>{notificationFilter === "unread" ? "Everything has been read. Switch to All to review recent activity." : "Repository updates and GitManager activity will appear here."}</span></div>
								) : visibleNotifications.map((item) => (
									<article className={item.read ? "gm-notification-item read" : "gm-notification-item unread"} key={item.id}>
										<div className="gm-notification-copy">
											<div className="gm-notification-title">
												<strong>{item.title}</strong>
												<time title={new Date(item.createdAtUnixMs).toLocaleString()}>{new Date(item.createdAtUnixMs).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}</time>
											</div>
											<span className="gm-notification-kind">{item.kind.replace(/-/g, " ")}</span>
											<p>{item.body}</p>
										</div>
										<div className="gm-notification-actions">
											{item.repositoryPath && <button type="button" onClick={() => onOpenNotification(item)}>Open repository</button>}
											{!item.read && <button type="button" onClick={() => void onMarkNotificationRead(item.id)}>Mark read</button>}
											<button type="button" onClick={() => void onDismissNotification(item.id)}>Dismiss</button>
										</div>
									</article>
								))}
							</div>
							<div className="gm-popover-actions gm-notification-footer">
								<button type="button" onClick={() => void onMarkAllNotificationsRead()} disabled={!inAppNotificationsEnabled || unreadCount === 0}>Mark all read</button>
								<button type="button" onClick={() => void onRefreshNotifications()} disabled={!inAppNotificationsEnabled}>Refresh</button>
							</div>
						</section>
					)}
				</div>
				<Button onClick={onScan} disabled={!serviceReady || scanning} variant="secondary" className="gm-top-scan">
					<Glyph>↻</Glyph>{scanning ? "Scanning" : "Scan"}
				</Button>
			</div>
			<div className="gm-topbar-status">
				<span className={serviceReady ? "gm-status-dot" : "gm-status-dot offline"} />
				<span>{serviceReady ? status : "Core service unavailable"}</span>
			</div>
		</header>
	);
}

function RepositoryCard({
	repo,
	selected,
	dependency,
	archiveRoot,
	canSnapshot,
	onSelect,
	onSwitchBranch,
}: {
	repo: GitRepository;
	selected: boolean;
	dependency: boolean;
	archiveRoot: string;
	canSnapshot: boolean;
	onSelect: () => void;
	onSwitchBranch: (path: string, branchName: string) => Promise<GitRepository>;
}) {
	const currentBranchTip = repo.branches?.find((branch) => branch.name === repo.current_branch && !branch.is_remote)?.target_oid;
	const latest = (currentBranchTip && repo.commits?.find((commit) => commit.oid === currentBranchTip))
		|| repo.commits?.find((commit) => commit.is_head)
		|| repo.commits?.[0];
	const originRemote = repo.remotes?.find((remote) => remote.name === "origin") ?? repo.remotes?.[0];
	const remoteFullName = originRemote?.owner && originRemote.repo_slug
		? originRemote.owner + "/" + originRemote.repo_slug
		: repo.name;
	const remotePageUrl = originRemote?.web_url || repo.web_url || "";
	const [branches, setBranches] = useState<BranchInfo[]>(repo.details_loaded
		? (repo.branches ?? []).filter((branch) => !branch.is_remote)
		: []);
	const [branchesLoaded, setBranchesLoaded] = useState(Boolean(repo.details_loaded));
	const [branchPickerOpen, setBranchPickerOpen] = useState(false);
	const [branchLoading, setBranchLoading] = useState(false);
	const [branchSwitching, setBranchSwitching] = useState(false);
	const [selectedBranch, setSelectedBranch] = useState(repo.current_branch || "HEAD");
	const [branchMessage, setBranchMessage] = useState("");
	const [isSnapshotting, setIsSnapshotting] = useState(false);
	const [feedback, setFeedback] = useState("");

	useEffect(() => {
		setSelectedBranch(repo.current_branch || "HEAD");
		setBranches(repo.details_loaded
			? (repo.branches ?? []).filter((branch) => !branch.is_remote)
			: []);
		setBranchesLoaded(Boolean(repo.details_loaded));
		setBranchPickerOpen(false);
		setBranchMessage("");
	}, [repo.id, repo.current_branch, repo.details_loaded, repo.branches]);

	async function loadBranches() {
		if (branchesLoaded || branchLoading) return;
		setBranchLoading(true);
		setBranchMessage("");
		try {
			const result = await invoke<BranchInfo[]>("list_local_repository_branches", { repoPath: repo.path });
			setBranches(result ?? []);
			setBranchesLoaded(true);
		} catch (error) {
			setBranchMessage(error instanceof Error ? error.message : String(error));
		} finally {
			setBranchLoading(false);
		}
	}

	async function toggleBranchPicker() {
		const nextOpen = !branchPickerOpen;
		setBranchPickerOpen(nextOpen);
		if (nextOpen) await loadBranches();
	}

	async function handleSwitchBranch(branchName: string) {
		if (!branchName || branchName === selectedBranch || branchSwitching) return;
		setBranchSwitching(true);
		setBranchMessage("");
		try {
			const updated = await onSwitchBranch(repo.path, branchName);
			setBranches((updated.branches ?? []).filter((branch) => !branch.is_remote));
			setSelectedBranch(updated.current_branch);
			setBranchesLoaded(true);
			setBranchPickerOpen(false);
			setFeedback("Switched to " + updated.current_branch + ".");
		} catch (error) {
			setBranchMessage(error instanceof Error ? error.message : String(error));
		} finally {
			setBranchSwitching(false);
		}
	}

	async function handleCopy(value: string, label: string) {
		try {
			if (!navigator.clipboard?.writeText) throw new Error("Clipboard access is not available in this window.");
			await navigator.clipboard.writeText(value);
			setFeedback(label + " copied.");
		} catch (error) {
			setFeedback(error instanceof Error ? error.message : "Could not copy " + label.toLowerCase() + ".");
		}
	}

	async function handleSnapshot() {
		if (isSnapshotting || !canSnapshot) return;
		setIsSnapshotting(true);
		setFeedback("");
		try {
			const snapshot = await invoke<ArchiveSnapshot>("capture_repository_archive", {
				repoPath: repo.path,
				kind: "local",
				archiveRoot: archiveRoot.trim() || null,
			});
			setFeedback("Local snapshot saved · " + snapshot.fileCount + " files.");
		} catch (error) {
			setFeedback(error instanceof Error ? error.message : String(error));
		} finally {
			setIsSnapshotting(false);
		}
	}

	return (
		<article className={selected ? "gm-repo-card selected" : "gm-repo-card"}>
			<header className="gm-repo-card-header">
				<button className="gm-repo-open" onClick={onSelect} type="button">
					<RepoAvatar repo={repo} />
					<span className="gm-repo-title-copy">
						<strong>{remoteFullName}</strong>
						<small>{repo.has_remote
							? (repo.primary_provider || "Remote") + " repository"
							: (repo.owner_name || "Local repository")}</small>
					</span>
					<span className="gm-repo-open-hint">Open →</span>
				</button>
				<span className={repo.has_remote ? "gm-origin-badge dual" : "gm-origin-badge local"}>
					{repo.has_remote ? "Local / Remote" : "Local"}
				</span>
			</header>

			<p className="gm-repo-description">
				{repo.has_remote
					? "Local working copy connected to an upstream remote."
					: "Local Git repository with no remote configured."}
			</p>

			<section className="gm-commit-box" aria-label="Repository branch and latest commit">
				<div className="gm-commit-top">
					<div className="gm-commit-left">
						<button type="button" className="gm-branch-pill"
							onClick={() => void toggleBranchPicker()}
							aria-expanded={branchPickerOpen} title="Choose a local branch">
							<Glyph>⑂</Glyph>{selectedBranch}<span aria-hidden="true">⌄</span>
						</button>
						{latest && (
							<button type="button" className="gm-sha-btn"
								onClick={() => void handleCopy(latest.oid, "Commit SHA")}
								title={"Copy full commit " + latest.oid}>
								{latest.short_oid || latest.oid.slice(0, 7)}
							</button>
						)}
						<span className="gm-commit-msg" title={latest?.message || "No commit message"}>
							{latest?.summary || latest?.message.split("\n")[0] || "No commits yet"}
						</span>
					</div>
					<div className="gm-commit-right">
						<time>{latest ? relativeDate(latest.timestamp) : "No commit date"}</time>
						<span className="gm-ci-indicator" title="CI check results are not part of the local repository index yet">
							<i />CI not checked
						</span>
					</div>
				</div>

				{branchPickerOpen && (
					<div className="gm-branch-picker-panel">
						<label>
							<span>Switch local branch</span>
							<select value={selectedBranch}
								onChange={(event) => void handleSwitchBranch(event.target.value)}
								disabled={branchLoading || branchSwitching || !branchesLoaded || branches.length === 0}>
								{!branches.some((branch) => branch.name === selectedBranch) && <option value={selectedBranch}>{selectedBranch}</option>}
								{!branchesLoaded && <option value={selectedBranch}>Loading branches…</option>}
								{branches.map((branch) => (
									<option key={branch.name} value={branch.name}>
										{branch.name}{branch.is_default ? " · default" : ""}
									</option>
								))}
								{branchesLoaded && branches.length === 0 && <option value={selectedBranch}>No local branches</option>}
							</select>
						</label>
						<span>{branchLoading ? "Loading local branches…" : branchSwitching ? "Switching safely…" : branchMessage || "Uncommitted or untracked changes block branch switching."}</span>
					</div>
				)}

				<div className="gm-location-list">
					<div className="gm-location-row">
						<span className="gm-location-tag disk">DISK</span>
						<code className="gm-location-path" title={repo.path}>{repo.path}</code>
						<button type="button" className="gm-copy-btn" onClick={() => void handleCopy(repo.path, "Disk path")}>Copy</button>
					</div>
					{remotePageUrl && (
						<div className="gm-location-row">
							<span className="gm-location-tag remote">REMOTE</span>
							<code className="gm-location-path remote-path" title={remotePageUrl}>{remotePageUrl}</code>
							<button type="button" className="gm-copy-btn" onClick={() => void handleCopy(remotePageUrl, "Remote URL")}>Copy</button>
						</div>
					)}
				</div>
			</section>

			<footer className="gm-repo-card-footer">
				<div className="gm-repo-card-meta">
					<span className="gm-repo-provider"><ProviderMark provider={repo.primary_provider} />{repo.primary_provider || "local"}</span>
					{!repo.details_loaded
						? <span className="gm-status-pending"><i />Open to inspect working tree</span>
						: repo.dirty_files_count > 0
							? <span className="gm-dirty-meta"><i />{repo.dirty_files_count} modified</span>
							: <span className="gm-clean-meta"><i />Clean</span>}
					{dependency && <span className="gm-dependency-chip">Dependency</span>}
				</div>
				<div className="gm-repo-card-actions">
					{repo.has_remote && (
						<button type="button" className="gm-card-action" onClick={onSelect}>
							⇄ Review Pull
						</button>
					)}
					<button type="button" className="gm-card-action primary"
						onClick={() => void handleSnapshot()}
						disabled={isSnapshotting || !canSnapshot}
						title={canSnapshot ? "Save a local restore point" : "Local archives are disabled in Settings"}>
						{isSnapshotting ? "Saving…" : "Snapshot"}
					</button>
				</div>
			</footer>
			{feedback && <p className="gm-card-feedback" role="status" aria-live="polite">{feedback}</p>}
		</article>
	);
}
function RepositorySkeletons() {
	return (
		<div className="gm-skeleton-grid" aria-label="Loading repository data" aria-live="polite">
			{Array.from({ length: 4 }, (_, index) => (
				<div className="gm-repo-skeleton" key={index}>
					<div className="gm-skeleton-row"><i className="gm-skeleton gm-skeleton-avatar" /><span className="gm-skeleton gm-skeleton-title" /></div>
					<span className="gm-skeleton gm-skeleton-line wide" />
					<span className="gm-skeleton gm-skeleton-line" />
					<div className="gm-skeleton-row bottom"><span className="gm-skeleton gm-skeleton-chip" /><span className="gm-skeleton gm-skeleton-short" /></div>
				</div>
			))}
		</div>
	);
}

function RepositoryPage({
	repositories,
	dependencies,
	selectedRepo,
	setSelectedRepo,
	query,
	setQuery,
	filter,
	setFilter,
	kind,
	setKind,
	scanning,
	onScan,
	onQueueRemoteCheck,
	onSwitchBranch,
	preferences,
	onRescan,
	onRepositoryLoaded,
	onOpenCleaner,
	scanTruncated,
	destination,
	onNavigateDestination,
}: {
	repositories: GitRepository[];
	dependencies: string[];
	selectedRepo: GitRepository | null;
	setSelectedRepo: (repo: GitRepository | null) => void;
	destination: RepositoryDestination;
	onNavigateDestination: (destination: RepositoryDestination) => void;
	query: string;
	setQuery: (value: string) => void;
	filter: RepoFilter;
	setFilter: (value: RepoFilter) => void;
	kind: RepoKind;
	setKind: (value: RepoKind) => void;
	scanning: boolean;
	onScan: () => void;
	onQueueRemoteCheck: () => Promise<void>;
	onSwitchBranch: (path: string, branchName: string) => Promise<GitRepository>;
	preferences: AppPreferences;
	onRescan: () => void;
	onRepositoryLoaded: (repo: GitRepository) => void;
	onOpenCleaner: () => void;
	scanTruncated: boolean;
}) {
	const dependencySet = useMemo(() => new Set(dependencies), [dependencies]);
	const visible = useMemo(() => repositories.filter((repo) => {
		const text = `${repo.name} ${repo.path} ${repo.owner_name} ${repo.current_branch}`.toLowerCase();
		const queryMatch = !query.trim() || text.includes(query.toLowerCase());
		const providerMatch = filter === "all" || (filter === "remote" ? repo.has_remote : !repo.has_remote);
		const isDependency = dependencySet.has(repo.id);
		const kindMatch = kind === "all" || (kind === "dependencies" ? isDependency : !isDependency);
		return queryMatch && providerMatch && kindMatch;
	}), [repositories, query, filter, kind, dependencySet]);
	const remoteCount = repositories.filter((repo) => repo.has_remote).length;

	if (selectedRepo) {
		return (
			<RepositoryDetail
				repo={selectedRepo}
				preferences={preferences}
				destination={destination}
				onNavigateDestination={onNavigateDestination}
				onClose={() => setSelectedRepo(null)}
				onOpenWebsite={(url) => { void invoke("open_external_url", { url }).catch((error) => window.alert(String(error))); }}
				onRescan={onRescan}
				onRepositoryLoaded={onRepositoryLoaded}
				onOpenCleaner={onOpenCleaner}
			/>
		);
	}

	return (
		<div className="gm-page-content">
			<div className="gm-page-heading">
				<div>
					<div className="gm-eyebrow">YOUR DEVELOPMENT WORKSPACE</div>
					<h1>Repositories</h1>
					<p>Find, inspect and maintain the Git projects on this machine.</p>
				</div>
				<div className="gm-page-heading-actions">
					<Button onClick={onQueueRemoteCheck}><Glyph>↻</Glyph> Check remotes</Button>
					<Button variant="primary" onClick={onScan} disabled={scanning}><Glyph>⌕</Glyph>{scanning ? "Scanning…" : "Scan workspace"}</Button>
				</div>
			</div>

			<EngineGrid columns={4} gap="12px" className="gm-stat-grid">
				{scanning && repositories.length === 0 ? Array.from({ length: 4 }, (_, index) => <EngineBox className="gm-stat-card gm-stat-skeleton" key={index}><span className="gm-skeleton gm-skeleton-label" /><strong className="gm-skeleton gm-skeleton-number" /><small className="gm-skeleton gm-skeleton-label" /></EngineBox>) : <>
					<EngineBox className="gm-stat-card"><span>Repositories</span><strong>{repositories.length}</strong><small>Indexed locally</small></EngineBox>
					<EngineBox className="gm-stat-card"><span>Remote connected</span><strong>{remoteCount}</strong><small>GitHub, GitLab and more</small></EngineBox>
					<EngineBox className="gm-stat-card"><span>Working changes</span><strong>—</strong><small>Checked when a repository is opened</small></EngineBox>
					<EngineBox className="gm-stat-card"><span>Dependencies</span><strong>{dependencies.length}</strong><small>Nested or vendored projects</small></EngineBox>
				</>}
			</EngineGrid>

			{scanTruncated && <div className="gm-scan-limit-banner">Discovery reached a safety limit. Narrow your scan folders or depth to inspect the remaining workspace.</div>}

			<div className="gm-repo-toolbar">
				<label className="gm-search-field">
					<Glyph>⌕</Glyph>
					<input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Search by name, path or branch…" />
				</label>
				<div className="gm-filter-group" aria-label="Repository type">
					{(["projects", "dependencies", "all"] as RepoKind[]).map((value) => (
						<button key={value} onClick={() => setKind(value)} className={kind === value ? "active" : ""}>
							{value === "projects" ? "Projects" : value === "dependencies" ? "Dependencies" : "All"}
						</button>
					))}
				</div>
				<div className="gm-filter-group" aria-label="Remote status">
					{(["all", "remote", "local"] as RepoFilter[]).map((value) => (
						<button key={value} onClick={() => setFilter(value)} className={filter === value ? "active" : ""}>
							{value === "all" ? "All" : value === "remote" ? "Remote" : "Local"}
						</button>
					))}
				</div>
			</div>

			<div className="gm-list-heading">
				<strong>{visible.length} repositories</strong>
				<span>Click a project to inspect branches, commits and restore points.</span>
			</div>
			{visible.length === 0 && scanning ? <RepositorySkeletons /> : visible.length === 0 ? (
				<EmptyState
					icon="⌕"
					title={repositories.length ? "No matching repositories" : "Your workspace is empty"}
					body={repositories.length ? "Try a different search or filter." : "Choose a scan folder in Settings, then scan to discover your local Git projects."}
					action={!repositories.length ? <Button variant="primary" onClick={onScan} disabled={scanning}>Scan workspace</Button> : undefined}
				/>
			) : (
				<EngineGrid columns={2} gap="12px" className="gm-repo-grid">
					{visible.map((repo) => (
						<RepositoryCard
							key={repo.id}
							repo={repo}
							selected={false}
							dependency={dependencySet.has(repo.id)}
							archiveRoot={preferences.archiveRoot}
							canSnapshot={preferences.archiveLocalEnabled !== false}
							onSelect={() => setSelectedRepo(repo)}
							onSwitchBranch={onSwitchBranch}
						/>
					))}
				</EngineGrid>
			)}
		</div>
	);
}

function SetupDialog({
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
	const [error, setError] = useState("");
	function patch(values: Partial<AppPreferences>) {
		setDraft((current) => ({ ...current, ...values }));
	}
	async function chooseSetupRoot() {
		try {
			const selected = await openDialog({ title: "Choose your main projects folder", directory: true, multiple: false });
			const path = Array.isArray(selected) ? selected[0] : selected;
			if (path) patch({ scanRoots: draft.scanRoots.some((root) => root.toLowerCase() === path.toLowerCase()) ? draft.scanRoots : [...draft.scanRoots, path] });
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
			await onSave({ ...draft, firstRunComplete: true });
			onClose();
		} catch (cause) {
			setError(cause instanceof Error ? cause.message : String(cause));
		} finally {
			setSaving(false);
		}
	}
	return (
		<div className="gm-modal-backdrop">
			<div className="gm-modal gm-setup-modal">
				<div className="gm-modal-heading">
					<BrandMark size={42} />
					<div><div className="gm-eyebrow">FIRST-RUN SETUP</div><h2>Set up your workspace</h2><p>Choose where GitManager should discover your projects.</p></div>
				</div>
				<div className="gm-root-list">
					{draft.scanRoots.map((root) => <div className="gm-root-row" key={root}><Glyph>▰</Glyph><span>{root}</span><button onClick={() => patch({ scanRoots: draft.scanRoots.filter((item) => item !== root) })}>×</button></div>)}
				</div>
				<div className="gm-add-root">
					<input value={newRoot} onChange={(event) => setNewRoot(event.target.value)} onKeyDown={(event) => {
						if (event.key === "Enter" && newRoot.trim()) {
							patch({ scanRoots: [...draft.scanRoots, newRoot.trim()] });
							setNewRoot("");
						}
					}} placeholder="Add a folder path…" />
					<Button onClick={() => { if (newRoot.trim()) patch({ scanRoots: [...draft.scanRoots, newRoot.trim()] }); setNewRoot(""); }}>Add path</Button>
					<Button onClick={() => void chooseSetupRoot()}>Browse…</Button>
				</div>
				<Switch label="Show dependency repositories" checked={draft.showDependencyRepositories} onChange={(value) => patch({ showDependencyRepositories: value })} />
				<div className="gm-field">
					<span>Archive destination</span>
					<div className="gm-add-root">
						<input value={draft.archiveRoot} onChange={(event) => patch({ archiveRoot: event.target.value })} placeholder="Default application archive folder" />
						<Button onClick={() => void chooseSetupArchiveRoot()}>Browse…</Button>
					</div>
				</div>
				{error && <div className="gm-inline-error">{error}</div>}
				<div className="gm-modal-actions">
					{draft.firstRunComplete && <Button onClick={onClose}>Close</Button>}
					<Button variant="primary" onClick={() => void save()} disabled={saving || draft.scanRoots.length === 0}>{saving ? "Saving…" : "Save & start indexing"}</Button>
				</div>
			</div>
		</div>
	);
}

const MemoRepositoryPage = memo(RepositoryPage);

export function GitManagerApp() {
	const git = useGitManager();
	const [internalRoute, setInternalRoute] = useState("/r");
	const [query, setQuery] = useState("");
	const [filter, setFilter] = useState<RepoFilter>("all");
	const [kind, setKind] = useState<RepoKind>("projects");
	const [selectedId, setSelectedId] = useState<string | null>(null);
	const selectedRepo = git.repositories.find((repo) => repo.id === selectedId) ?? null;
	const parsedInternalRoute = useMemo(() => parseInternalRoute(internalRoute), [internalRoute]);
	const page: PageKey = sectionForRoute(parsedInternalRoute);
	const currentDestination: RepositoryDestination = parsedInternalRoute.kind === "repository"
		? parsedInternalRoute.destination
		: OVERVIEW_DESTINATION;

	const navigatePage = useCallback((next: PageKey) => {
		if (next === "accounts") void git.loadAccounts().catch(() => undefined);
		setSelectedId(null);
		setInternalRoute(sectionRoute(next));
	}, [git.loadAccounts]);

	const selectRepository = useCallback((repo: GitRepository | null) => {
		setSelectedId(repo?.id ?? null);
		setInternalRoute(repo ? repositoryRouteForItem(repo) : "/r");
	}, []);

	const navigateRepositoryDestination = useCallback((destination: RepositoryDestination) => {
		if (!selectedRepo) return;
		const baseRoute = parseInternalRoute(repositoryRouteForItem(selectedRepo));
		if (baseRoute.kind !== "repository") return;
		setInternalRoute(repositorySubroute(baseRoute.owner, baseRoute.repository, destination));
	}, [selectedRepo]);

	const handleScan = useCallback(() => { void git.scan(); }, [git.scan]);
	const handleRetryService = useCallback(() => { void git.retryService(); }, [git.retryService]);
	const handleOpenCleaner = useCallback(() => navigatePage("cleaner"), [navigatePage]);
	const openServiceManager = useCallback(() => setInternalRoute("/s/service"), []);
	const openPerformanceCenter = useCallback(() => setInternalRoute("/s/performance"), []);

	const openNotificationRepository = useCallback(async (item: AppNotification) => {
		if (!item.repositoryPath) return;
		let repository = git.repositories.find((candidate) => candidate.path === item.repositoryPath) ?? null;
		if (!repository) {
			const refreshed = await git.scan();
			repository = refreshed.find((candidate) => candidate.path === item.repositoryPath) ?? null;
		}
		if (repository) {
			navigatePage("repositories");
			selectRepository(repository);
		} else {
			navigatePage("repositories");
			git.setError("The repository from this notification is not currently in the saved index. Refresh its scan roots and try again.");
		}
		if (!item.read) await git.markNotificationRead(item.id).catch((cause) => {
			git.setError(cause instanceof Error ? cause.message : String(cause));
		});
	}, [git.repositories, git.scan, git.markNotificationRead, git.setError, navigatePage, selectRepository]);

	if (git.loading) {
		return (
			<div className="gm-boot-screen" role="status" aria-live="polite">
				<div className="gm-boot-card">
					<LoadingLogo />
					<div className="gm-boot-brand">
						<strong>GitManager</strong>
						<span>By Kastrick</span>
					</div>
					<span>Starting your native workspace…</span>
					<div className="gm-boot-progress"><i /></div>
				</div>
			</div>
		);
	}

	return (
		<EngineBox className="gm-app-shell">
			<Sidebar
				page={page}
				setPage={navigatePage}
				repositoryCount={git.repositories.length}
				accountCount={git.accounts.length}
			/>
			<div className="gm-main-column">
				<TopBar
					internalRoute={internalRoute}
					version={git.version}
					status={git.status}
					query={query}
					setQuery={setQuery}
					scanning={git.scanning || git.awaitingInitialIndex}
					serviceReady={git.serviceReady}
					remoteCheckPending={git.remoteCheckPending}
					remoteCheckRunning={git.remoteCheckRunning}
					serviceIndexing={git.serviceIndexing}
					awaitingInitialIndex={git.awaitingInitialIndex}
					repositoryCount={git.repositories.length}
					scanTruncated={git.scanTruncated}
					notifications={git.notifications}
					inAppNotificationsEnabled={git.preferences.inAppNotificationsEnabled !== false}
					onRefreshNotifications={git.refreshNotifications}
					onMarkNotificationRead={git.markNotificationRead}
					onMarkAllNotificationsRead={git.markAllNotificationsRead}
					onDismissNotification={git.dismissNotification}
					onOpenNotification={openNotificationRepository}
					onScan={handleScan}
					onRetryService={handleRetryService}
				/>
				<main className="gm-main">
					{(git.loading || git.scanning) && <div className="gm-loading-bar"><i /></div>}
					{git.error && <div className="gm-error-banner"><span>{git.error}</span><button onClick={() => git.setError("")}>Dismiss</button></div>}
					{page === "repositories" && (
						<MemoRepositoryPage
							repositories={git.repositories}
							dependencies={git.dependencies}
							selectedRepo={selectedRepo}
							setSelectedRepo={selectRepository}
							destination={currentDestination}
							onNavigateDestination={navigateRepositoryDestination}
							query={query}
							setQuery={setQuery}
							filter={filter}
							setFilter={setFilter}
							kind={kind}
							setKind={setKind}
							scanning={git.scanning}
							onScan={() => void git.scan()}
							scanTruncated={git.scanTruncated}
							onQueueRemoteCheck={async () => { await git.queueRemoteCheck(); }}
							onSwitchBranch={git.switchBranch}
							preferences={git.preferences}
							onRescan={handleScan}
							onRepositoryLoaded={git.updateRepository}
							onOpenCleaner={handleOpenCleaner}
						/>
					)}
					{page === "accounts" && <AccountsPage accounts={git.accounts} loading={git.accountsLoading} onConnect={git.connectAccount} />}
					{parsedInternalRoute.kind === "service" && (
						<ServiceManagerPage
							serviceReady={git.serviceReady}
							status={git.status}
							onStart={handleRetryService}
						/>
					)}
					{parsedInternalRoute.kind === "performance" && (
						<PerformancePage serviceReady={git.serviceReady} scanning={git.scanning || git.serviceIndexing} />
					)}
					{page === "settings" && parsedInternalRoute.kind === "settings" && (
						<SettingsPage
							preferences={git.preferences}
							directories={git.directories}
							onSave={git.savePreferences}
							onScan={() => void git.scan()}
							onOpenCleaner={() => navigatePage("cleaner")}
							onOpenService={openServiceManager}
							onOpenPerformance={openPerformanceCenter}
						/>
					)}
					{page === "cleaner" && <CleanerPage repositories={git.repositories} onRefresh={handleScan} initialRepoPath={selectedRepo?.path} />}
				</main>
				<footer className="gm-footer">
					<span><i /> {git.available ? "Tauri runtime connected" : "Browser preview mode"}</span>
					<span>Rust backend · Next.js · Nextjs-Engine</span>
				</footer>
			</div>
			{git.setupOpen && (
				<SetupDialog
					preferences={git.preferences}
					onSave={git.savePreferences}
					onClose={() => git.setSetupOpen(false)}
				/>
			)}
		</EngineBox>
	);
}
