"use client";

import { useEffect, useMemo, useState } from "react";
import dynamic from "next/dynamic";
import { EngineBox, EngineGrid } from "@/engine";
import { useGitManager } from "../lib/use-gitmanager";
import { invoke, openDialog } from "../lib/tauri";
import { Button, EmptyState, Field, Glyph, ProviderMark, RepoAvatar, Switch } from "./shared-ui";
import { parseInternalRoute, repositoryRouteForItem, routeLabel, sectionRoute } from "../lib/internal-routing";
import type { AppPreferences, ArchiveSnapshot, AppNotification, BranchInfo, GitRepository, UpdateAlert } from "../lib/types";

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
import { invoke, openDialog } from "../lib/tauri";
import { Button, EmptyState, Field, Glyph, ProviderMark, RepoAvatar, Switch } from "./shared-ui";

const RepositoryWorkspace = dynamic(
	() => import("./repository-workspace").then((module) => module.RepositoryWorkspace),
	{ ssr: false, loading: () => <div className="gm-workspace-loading">Loading repository workspace…</div> },
);
const AccountsPage = dynamic(
	() => import("./accounts-page").then((module) => module.AccountsPage),
	{ ssr: false, loading: () => <div className="gm-page-chunk-loading">Loading Accounts…</div> },
);
const SettingsPage = dynamic(
	() => import("./settings-page").then((module) => module.SettingsPage),
	{ ssr: false, loading: () => <div className="gm-page-chunk-loading">Loading Settings…</div> },
);
const CleanerPage = dynamic(
	() => import("./cleaner-page").then((module) => module.CleanerPage),
	{ ssr: false, loading: () => <div className="gm-page-chunk-loading">Loading Developer Cleaner…</div> },
);
import {
	parseInternalRoute,
	repositoryRouteForItem,
	routeLabel,
	sectionRoute,
} from "../lib/internal-routing";
import type {
	AppPreferences,
	ArchiveSnapshot,
	AppNotification,
	BranchInfo,
	GitRepository,
	UpdateAlert,
} from "../lib/types";

type PageKey = "repositories" | "accounts" | "settings" | "cleaner";
type RepoFilter = "all" | "remote" | "local";
type RepoKind = "projects" | "dependencies" | "all";

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
	repositoryCount,
	scanTruncated,
	notifications,
	inAppNotificationsEnabled,
	onRefreshNotifications,
	onMarkNotificationRead,
	onDismissNotification,
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
	repositoryCount: number;
	scanTruncated: boolean;
	notifications: AppNotification[];
	inAppNotificationsEnabled: boolean;
	onRefreshNotifications: () => Promise<unknown>;
	onMarkNotificationRead: (id: string) => Promise<void>;
	onDismissNotification: (id: string) => Promise<void>;
	onScan: () => void;
	onRetryService: () => void;
}) {
	const [openPanel, setOpenPanel] = useState<"tasks" | "notifications" | null>(null);
	const unreadCount = inAppNotificationsEnabled ? notifications.filter((item) => !item.read).length : 0;
	const currentRoute = parseInternalRoute(internalRoute);
	const label = routeLabel(currentRoute);

	useEffect(() => {
		if (openPanel !== "notifications" || !inAppNotificationsEnabled) return;
		void onRefreshNotifications();
		const timer = window.setInterval(() => void onRefreshNotifications(), 30000);
		return () => window.clearInterval(timer);
	}, [openPanel, onRefreshNotifications]);

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
				<div className="gm-topbar-popover-host">
					<div className="gm-topbar-panel-triggers">
						<button
							type="button"
							className={openPanel === "tasks" ? "gm-topbar-panel-trigger active" : "gm-topbar-panel-trigger"}
							aria-expanded={openPanel === "tasks"}
							aria-controls="gm-task-center-popover"
							onClick={() => setOpenPanel((current) => current === "tasks" ? null : "tasks")}
						>
							<span aria-hidden="true">≡</span> Tasks
							<span className={scanning ? "gm-panel-indicator busy" : serviceReady ? "gm-panel-indicator ready" : "gm-panel-indicator offline"} />
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
									<span className={scanning ? "gm-task-state busy" : "gm-task-state ready"} />
									<div><strong>Repository index</strong><small>{scanning ? "Discovering repositories in the service process…" : scanTruncated ? "Index ready · safety limit reached" : `${repositoryCount} repositories indexed`}</small></div>
									<span className="gm-task-meta">{scanning ? "Running" : "Idle"}</span>
								</div>
								<div className="gm-task-row">
									<span className="gm-task-state idle" />
									<div><strong>Service status</strong><small>{status}</small></div>
								</div>
							</div>
							<div className="gm-popover-actions">
								<button type="button" onClick={onScan} disabled={!serviceReady || scanning}>{scanning ? "Indexing…" : "Refresh repository index"}</button>
								{!serviceReady && <button type="button" className="primary" onClick={onRetryService}>Start service</button>}
							</div>
						</section>
					)}
					{openPanel === "notifications" && (
						<section id="gm-notifications-popover" className="gm-topbar-popover gm-notifications-panel" role="dialog" aria-label="Notifications">
							<div className="gm-popover-heading">
								<div><strong>Notifications</strong><small>{unreadCount} unread · latest activity</small></div>
								<button type="button" onClick={() => setOpenPanel(null)} aria-label="Close Notifications">×</button>
							</div>
							<div className="gm-notification-list">
								{!inAppNotificationsEnabled ? (
									<div className="gm-notification-empty"><strong>In-app notifications are disabled</strong><span>Enable them in Settings to show GitManager activity here. OS notifications can be configured separately.</span></div>
								) : notifications.length === 0 ? (
									<div className="gm-notification-empty"><strong>You're all caught up</strong><span>Repository updates and GitManager activity will appear here.</span></div>
								) : notifications.map((item) => (
									<article className={item.read ? "gm-notification-item read" : "gm-notification-item unread"} key={item.id}>
										<div className="gm-notification-copy">
											<div className="gm-notification-title"><strong>{item.title}</strong><time>{new Date(item.createdAtUnixMs).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}</time></div>
											<p>{item.body}</p>
										</div>
										<div className="gm-notification-actions">
											{!item.read && <button type="button" onClick={() => void onMarkNotificationRead(item.id)}>Mark read</button>}
											<button type="button" onClick={() => void onDismissNotification(item.id)}>Dismiss</button>
										</div>
									</article>
								))}
							</div>
							<div className="gm-popover-actions">
								<button type="button" onClick={() => void onRefreshNotifications()} disabled={!inAppNotificationsEnabled}>Refresh notifications</button>
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
function RepositoryDetail({
	repo,
	preferences,
	onClose,
	onOpenWebsite,
	onRescan,
	onRepositoryLoaded,
	onPullCompleted,
	onOpenCleaner,
}: {
	repo: GitRepository;
	preferences: AppPreferences;
	onClose: () => void;
	onOpenWebsite: (url: string) => void;
	onRescan: () => void;
	onRepositoryLoaded: (repo: GitRepository) => void;
	onPullCompleted: (repoPath: string) => void;
	onOpenCleaner: () => void;
}) {
	const [archives, setArchives] = useState<ArchiveSnapshot[]>([]);
	const [repositoryDetails, setRepositoryDetails] = useState<GitRepository | null>(repo.details_loaded ? repo : null);
	const [detailsLoading, setDetailsLoading] = useState(!repo.details_loaded);
	const [archiveRoot, setArchiveRoot] = useState(preferences.archiveRoot || "");
	const [archiveStatus, setArchiveStatus] = useState("");
	const [selectedCommit, setSelectedCommit] = useState<string | null>(null);
	const [commitDetail, setCommitDetail] = useState<unknown>(null);
	const [busy, setBusy] = useState(false);

	async function refreshArchives() {
		if (!archiveRoot) {
			try {
				const root = await invoke<string>("get_default_archive_directory");
				setArchiveRoot(root);
				const rows = await invoke<ArchiveSnapshot[]>("list_repository_archives", {
					repoPath: repo.path, kind: null, archiveRoot: root,
				});
				setArchives(rows ?? []);
			} catch (error) {
				setArchiveStatus(String(error));
			}
			return;
		}
		try {
			const rows = await invoke<ArchiveSnapshot[]>("list_repository_archives", {
				repoPath: repo.path, kind: null, archiveRoot,
			});
			setArchives(rows ?? []);
		} catch (error) {
			setArchiveStatus(String(error));
		}
	}

	async function capture(kind: "local" | "remote") {
		setBusy(true);
		setArchiveStatus(`Capturing ${kind} archive…`);
		try {
			const result = await invoke<ArchiveSnapshot>("capture_repository_archive", {
				repoPath: repo.path, kind, archiveRoot: archiveRoot || null,
			});
			setArchiveStatus(`Saved ${kind} restore point · ${result.fileCount} files`);
			await refreshArchives();
		} catch (error) {
			setArchiveStatus(String(error));
		} finally {
			setBusy(false);
		}
	}

	async function restore(snapshot: ArchiveSnapshot) {
		const accepted = window.confirm(
			`Restore ${snapshot.kind} snapshot ${snapshot.id}? Existing destination files will be preserved first.`,
		);
		if (!accepted) return;
		setBusy(true);
		setArchiveStatus("Verifying and restoring snapshot…");
		try {
			await invoke("restore_repository_archive", {
				repoPath: repo.path,
				destinationPath: repo.path,
				snapshotId: snapshot.id,
				kind: snapshot.kind,
				archiveRoot: archiveRoot || null,
				confirmReplace: true,
			});
			setArchiveStatus("Restore completed. Refreshing repository state…");
			onRescan();
			await refreshArchives();
		} catch (error) {
			setArchiveStatus(String(error));
		} finally {
			setBusy(false);
		}
	}

	async function showCommit(oid: string) {
		setSelectedCommit(oid);
		setCommitDetail(null);
		try {
			const details = await invoke("get_commit_details", { repoPath: repo.path, oid });
			setCommitDetail(details);
		} catch (error) {
			setCommitDetail({ error: String(error) });
		}
	}

	useEffect(() => {
		void refreshArchives();
		// Repository path defines the archive index scope.
		// eslint-disable-next-line react-hooks/exhaustive-deps
	}, [repo.path]);

	useEffect(() => {
		let active = true;
		if (repo.details_loaded) {
			setRepositoryDetails(repo);
			setDetailsLoading(false);
			onRepositoryLoaded(repo);
			return () => { active = false; };
		}
		setRepositoryDetails(null);
		setDetailsLoading(true);
		void invoke<GitRepository>("inspect_repository", { repoPath: repo.path }).then((loaded) => {
			if (active) {
				setRepositoryDetails(loaded);
				onRepositoryLoaded(loaded);
			}
		}).catch((error) => {
			if (active) setArchiveStatus(`Could not load Git Tree details: ${String(error)}`);
		}).finally(() => {
			if (active) setDetailsLoading(false);
		});
		return () => { active = false; };
	}, [repo.path, repo.details_loaded, onRepositoryLoaded]);

	const displayRepo = repositoryDetails ?? repo;
	const branches = (displayRepo.branches ?? []).filter((branch) => !branch.is_remote);
	const commits = displayRepo.commits ?? [];

	return (
		<section className="gm-detail-view">
			<div className="gm-detail-heading">
				<div className="gm-detail-heading-main">
					<button className="gm-back-link" onClick={onClose} type="button"><Glyph>‹</Glyph> All repositories</button>
					<div className="gm-detail-title-row">
						<RepoAvatar repo={repo} />
						<div>
							<div className="gm-eyebrow">{repo.primary_provider || "LOCAL"} REPOSITORY</div>
							<h1>{repo.name}</h1>
						</div>
					</div>
					<p className="gm-detail-path">{repo.path}</p>
				</div>
				<div className="gm-detail-actions">
					{repo.web_url && <Button onClick={() => onOpenWebsite(repo.web_url!)}><Glyph>↗</Glyph> Open remote</Button>}
					<Button variant="primary" onClick={onRescan}><Glyph>↻</Glyph> Refresh</Button>
				</div>
			</div>

			<div className="gm-detail-stats">
				<div><span>Current branch</span><strong><Glyph>⑂</Glyph>{displayRepo.current_branch}</strong></div>
				<div><span>Commits indexed</span><strong>{displayRepo.total_commits_loaded}</strong></div>
				<div><span>Working tree</span><strong>{displayRepo.dirty_files_count ? `${displayRepo.dirty_files_count} changed` : "Clean"}</strong></div>
				<div><span>Remotes</span><strong>{displayRepo.remotes?.length ?? 0}</strong></div>
			</div>

			<RepositoryWorkspace
				key={repo.path}
				repo={displayRepo}
				onRepositoryUpdated={onRepositoryLoaded}
				onPullCompleted={onPullCompleted}
				onOpenCleaner={onOpenCleaner}
			/>

			<div className="gm-detail-columns">
				<div className="gm-panel gm-commit-panel">
					<div className="gm-panel-heading">
						<div><h2>Commit history</h2><p>Recent commits across local and remote branches.</p></div>
						<span className="gm-count-chip">{commits.length} commits</span>
					</div>
					{detailsLoading ? <div className="gm-panel-empty">Loading Git Tree, branches and repository status…</div> : commits.length === 0 ? <div className="gm-panel-empty">No commits were found for this repository.</div> : (
						<div className="gm-commit-list">
							{commits.map((commit, index) => (
								<button
									key={commit.oid}
									className={selectedCommit === commit.oid ? "gm-commit-row selected" : "gm-commit-row"}
									onClick={() => void showCommit(commit.oid)}
									type="button"
								>
									<span className={index === 0 ? "gm-commit-node head" : "gm-commit-node"} />
									<span className="gm-commit-copy">
										<strong>{commit.summary || commit.message.split("\n")[0]}</strong>
										<small>{commit.author_name} · {relativeDate(commit.timestamp)}</small>
										{commit.tags?.length > 0 && <span className="gm-commit-tags">{commit.tags.map((tag) => <i key={tag}>{tag}</i>)}</span>}
									</span>
									<code>{commit.short_oid}</code>
								</button>
							))}
						</div>
					)}
					{selectedCommit && (
						<div className="gm-commit-inspector">
							<div className="gm-commit-inspector-heading">
								<strong>Commit details</strong>
								<button onClick={() => { setSelectedCommit(null); setCommitDetail(null); }} type="button">×</button>
							</div>
							<pre>{commitDetail ? JSON.stringify(commitDetail, null, 2) : "Loading commit details…"}</pre>
						</div>
					)}
				</div>

				<div className="gm-detail-side">
					<div className="gm-panel">
						<div className="gm-panel-heading"><div><h2>Branches</h2><p>Local branch references</p></div><span className="gm-count-chip">{branches.length}</span></div>
						<div className="gm-branch-list">
							{branches.slice(0, 12).map((branch) => (
								<div className="gm-branch-row" key={branch.name}>
									<Glyph>⑂</Glyph>
									<strong>{branch.name}</strong>
									{branch.is_head && <span className="gm-current-chip">Current</span>}
									{branch.is_default && <span className="gm-default-chip">Default</span>}
								</div>
							))}
							{branches.length === 0 && <p className="gm-muted-copy">No local branches reported.</p>}
						</div>
					</div>

					<div className="gm-panel gm-archive-panel">
						<div className="gm-panel-heading">
							<div><h2>Restore points</h2><p>Verified snapshots for this repository.</p></div>
							<Button onClick={() => void refreshArchives()} disabled={busy}><Glyph>↻</Glyph></Button>
						</div>
						<div className="gm-archive-actions">
							<Button variant="primary" disabled={busy || preferences.archiveLocalEnabled === false} onClick={() => void capture("local")}>Save Local</Button>
							<Button disabled={busy || !repo.has_remote || preferences.archiveRemoteEnabled === false} onClick={() => void capture("remote")}>Save Remote</Button>
						</div>
						{archiveStatus && <p className="gm-inline-status">{archiveStatus}</p>}
						<div className="gm-snapshot-list">
							{archives.length === 0 ? <p className="gm-muted-copy">No restore points yet. Capture a Local or Remote Archive to start.</p> :
								archives.slice(0, 5).map((snapshot) => (
									<div className="gm-snapshot-row" key={snapshot.id}>
										<div>
											<strong>{snapshot.kind === "remote" ? "Remote archive" : "Local archive"}</strong>
											<small>{snapshot.capturedAt ? new Date(snapshot.capturedAt).toLocaleString() : snapshot.id}</small>
											<small>{snapshot.fileCount} files · {Number(snapshot.storedBytes || 0).toLocaleString()} bytes</small>
										</div>
										<Button disabled={busy} variant="ghost" onClick={() => void restore(snapshot)}>Restore</Button>
									</div>
								))}
						</div>
						<small className="gm-archive-root">Archive folder: {archiveRoot || "Resolving…"}</small>
					</div>
				</div>
			</div>
		</section>
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
	onCheckUpdates,
	updates,
	onSwitchBranch,
	preferences,
	onRescan,
	onRepositoryLoaded,
	onPullCompleted,
	onOpenCleaner,
	scanTruncated,
}: {
	repositories: GitRepository[];
	dependencies: string[];
	selectedRepo: GitRepository | null;
	setSelectedRepo: (repo: GitRepository | null) => void;
	query: string;
	setQuery: (value: string) => void;
	filter: RepoFilter;
	setFilter: (value: RepoFilter) => void;
	kind: RepoKind;
	setKind: (value: RepoKind) => void;
	scanning: boolean;
	onScan: () => void;
	onCheckUpdates: () => Promise<UpdateAlert[]>;
	updates: UpdateAlert[];
	onSwitchBranch: (path: string, branchName: string) => Promise<GitRepository>;
	preferences: AppPreferences;
	onRescan: () => void;
	onRepositoryLoaded: (repo: GitRepository) => void;
	onPullCompleted: (repoPath: string) => void;
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
				onClose={() => setSelectedRepo(null)}
				onOpenWebsite={(url) => { void invoke("open_external_url", { url }).catch((error) => window.alert(String(error))); }}
				onRescan={onRescan}
				onRepositoryLoaded={onRepositoryLoaded}
				onPullCompleted={onPullCompleted}
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
					<Button onClick={onCheckUpdates}><Glyph>↻</Glyph> Check remotes</Button>
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
			{updates.length > 0 && (
				<div className="gm-update-banner">
					<div className="gm-update-banner-icon">↑</div>
					<div className="gm-update-banner-copy">
						<strong>{updates.length} remote update{updates.length === 1 ? "" : "s"} available</strong>
						<span>Review each repository, choose how to protect local changes, and confirm every Pull.</span>
					</div>
					<div className="gm-update-list">
						{updates.slice(0, 3).map((update) => (
							<div key={update.repo_path}>
								<span>{update.repo_name} · {update.commits_behind} behind</span>
								<Button variant="primary" onClick={() => {
									const target = repositories.find((item) => item.path === update.repo_path);
									if (target) setSelectedRepo(target);
								}}>Review Pull</Button>
							</div>
						))}
					</div>
				</div>
			)}

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

export function GitManagerApp() {
	const git = useGitManager();
	const [page, setPage] = useState<PageKey>("repositories");
	const [internalRoute, setInternalRoute] = useState("/r");
	const [query, setQuery] = useState("");
	const [filter, setFilter] = useState<RepoFilter>("all");
	const [kind, setKind] = useState<RepoKind>("projects");
	const [selectedId, setSelectedId] = useState<string | null>(null);
	const selectedRepo = git.repositories.find((repo) => repo.id === selectedId) ?? null;

	function navigatePage(next: PageKey) {
		setPage(next);
		setSelectedId(null);
		setInternalRoute(sectionRoute(next));
	}

	function selectRepository(repo: GitRepository | null) {
		setSelectedId(repo?.id ?? null);
		setInternalRoute(repo ? repositoryRouteForItem(repo) : "/r");
	}

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
					scanning={git.scanning}
					serviceReady={git.serviceReady}
					repositoryCount={git.repositories.length}
					scanTruncated={git.scanTruncated}
					notifications={git.notifications}
					inAppNotificationsEnabled={git.preferences.inAppNotificationsEnabled !== false}
					onRefreshNotifications={git.refreshNotifications}
					onMarkNotificationRead={git.markNotificationRead}
					onDismissNotification={git.dismissNotification}
					onScan={() => void git.scan()}
					onRetryService={() => void git.retryService()}
				/>
				<main className="gm-main">
					{(git.loading || git.scanning) && <div className="gm-loading-bar"><i /></div>}
					{git.error && <div className="gm-error-banner"><span>{git.error}</span><button onClick={() => git.setError("")}>Dismiss</button></div>}
					{page === "repositories" && (
						<RepositoryPage
							repositories={git.repositories}
							dependencies={git.dependencies}
							selectedRepo={selectedRepo}
							setSelectedRepo={selectRepository}
							query={query}
							setQuery={setQuery}
							filter={filter}
							setFilter={setFilter}
							kind={kind}
							setKind={setKind}
							scanning={git.scanning}
							onScan={() => void git.scan()}
							scanTruncated={git.scanTruncated}
							onCheckUpdates={() => git.checkUpdates()}
							updates={git.updates}
							onSwitchBranch={git.switchBranch}
							preferences={git.preferences}
							onRescan={() => void git.scan()}
							onRepositoryLoaded={git.updateRepository}
							onPullCompleted={git.clearUpdate}
							onOpenCleaner={() => navigatePage("cleaner")}
						/>
					)}
					{page === "accounts" && <AccountsPage accounts={git.accounts} onConnect={git.connectAccount} />}
					{page === "settings" && (
						<SettingsPage
							preferences={git.preferences}
							directories={git.directories}
							onSave={git.savePreferences}
							onScan={() => void git.scan()}
							onOpenCleaner={() => navigatePage("cleaner")}
						/>
					)}
					{page === "cleaner" && <CleanerPage repositories={git.repositories} onRefresh={() => void git.scan()} initialRepoPath={selectedRepo?.path} />}
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
