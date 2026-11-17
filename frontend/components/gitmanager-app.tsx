"use client";

import { useEffect, useMemo, useState, type ReactNode } from "react";
import {
	EngineBox,
	EngineGrid,
} from "@/engine";
import { useGitManager } from "../lib/use-gitmanager";
import { invoke, openDialog } from "../lib/tauri";
import { RepositoryWorkspace } from "./repository-workspace";
import type {
	ApplicationDirectories,
	AppPreferences,
	ArchiveSnapshot,
	BranchInfo,
	ConnectedAccount,
	GitRepository,
	UpdateAlert,
} from "../lib/types";

type PageKey = "repositories" | "accounts" | "settings" | "cleaner";
type RepoFilter = "all" | "remote" | "local";
type RepoKind = "projects" | "dependencies" | "all";
type SettingsTab = "general" | "cloud" | "about";
type CleanerScope = "repository" | "system" | "remote";

function Glyph({ children }: { children: string }) {
	return <span className="gm-glyph" aria-hidden="true">{children}</span>;
}

function Button({
	children,
	onClick,
	variant = "secondary",
	disabled = false,
	className = "",
	type = "button",
}: {
	children: ReactNode;
	onClick?: () => void;
	variant?: "primary" | "secondary" | "ghost" | "danger";
	disabled?: boolean;
	className?: string;
	type?: "button" | "submit";
}) {
	return (
		<button
			type={type}
			className={`gm-button gm-button-${variant} ${className}`}
			onClick={onClick}
			disabled={disabled}
		>
			{children}
		</button>
	);
}

function Field({
	label,
	value,
	onChange,
	placeholder,
	type = "text",
}: {
	label: string;
	value: string | number;
	onChange: (value: string) => void;
	placeholder?: string;
	type?: string;
}) {
	return (
		<label className="gm-field">
			<span>{label}</span>
			<input
				type={type}
				value={value}
				onChange={(event) => onChange(event.target.value)}
				placeholder={placeholder}
			/>
		</label>
	);
}

function Switch({
	label,
	checked,
	onChange,
	description,
}: {
	label: string;
	checked: boolean;
	onChange: (checked: boolean) => void;
	description?: string;
}) {
	return (
		<label className="gm-switch-row">
			<span className="gm-switch-copy">
				<strong>{label}</strong>
				{description && <small>{description}</small>}
			</span>
			<input
				type="checkbox"
				checked={checked}
				onChange={(event) => onChange(event.target.checked)}
			/>
			<span className="gm-switch-track" aria-hidden="true"><i /></span>
		</label>
	);
}

function ProviderMark({ provider }: { provider: string }) {
	const value = provider.toLowerCase();
	const label = value === "github" ? "GH" : value === "gitlab" ? "GL" : value === "local" ? "L" : "↗";
	return <span className={`gm-provider-mark gm-provider-${value}`}>{label}</span>;
}

function RepoAvatar({ repo }: { repo: GitRepository }) {
	const letter = (repo.name?.trim().charAt(0) || "R").toUpperCase();
	let image = "";
	try {
		const url = new URL(repo.icon_url || "");
		const allowed = ["github.com", "gitlab.com", "avatars.githubusercontent.com"];
		if (url.protocol === "https:" && allowed.includes(url.hostname.toLowerCase())) image = url.href;
	} catch {
		image = "";
	}
	return (
		<span className="gm-repo-avatar">
			{image ? <img src={image} alt="" /> : <span>{letter}</span>}
		</span>
	);
}

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

function EmptyState({
	icon,
	title,
	body,
	action,
}: {
	icon: string;
	title: string;
	body: string;
	action?: ReactNode;
}) {
	return (
		<div className="gm-empty-state">
			<div className="gm-empty-icon">{icon}</div>
			<h3>{title}</h3>
			<p>{body}</p>
			{action}
		</div>
	);
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
				<div className="gm-brand-mark">
					<svg viewBox="0 0 32 32" role="img" aria-label="GitManager">
						<path d="M8 7.5 16 3l8 4.5v9L16 21l-8-4.5z" />
						<path d="M16 21v8M8 16.5l8 4.5 8-4.5M16 12v9" />
						<circle cx="8" cy="7.5" r="2.2" />
						<circle cx="24" cy="7.5" r="2.2" />
						<circle cx="16" cy="12" r="2.2" />
					</svg>
				</div>
				<div className="gm-brand-name">
					<strong>GitManager</strong>
					<span>by Kastrick</span>
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
	page,
	version,
	status,
	query,
	setQuery,
	scanning,
	onScan,
}: {
	page: PageKey;
	version: string;
	status: string;
	query: string;
	setQuery: (value: string) => void;
	scanning: boolean;
	onScan: () => void;
}) {
	const label = page === "repositories" ? "Repositories"
		: page === "accounts" ? "Accounts"
		: page === "cleaner" ? "Developer Cleaner" : "Settings";
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
				<Button onClick={onScan} disabled={scanning} variant="secondary" className="gm-top-scan">
					<Glyph>↻</Glyph>{scanning ? "Scanning" : "Scan"}
				</Button>
			</div>
			<div className="gm-topbar-status">
				<span className="gm-status-dot" />
				<span>{status}</span>
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

function AccountsPage({
	accounts,
	onConnect,
}: {
	accounts: ConnectedAccount[];
	onConnect: (provider: string, username: string, token: string) => Promise<unknown>;
}) {
	const [provider, setProvider] = useState("github");
	const [username, setUsername] = useState("");
	const [token, setToken] = useState("");
	const [busy, setBusy] = useState(false);
	const [error, setError] = useState("");
	const [message, setMessage] = useState("");

	async function connect() {
		setBusy(true);
		setError("");
		setMessage("");
		try {
			const account = await onConnect(provider, username, token);
			setToken("");
			setMessage(`Connected ${(account as ConnectedAccount).account_login}.`);
		} catch (cause) {
			setError(cause instanceof Error ? cause.message : String(cause));
		} finally {
			setBusy(false);
		}
	}

	const remoteRepositories = accounts.flatMap((account) => account.repositories ?? []);

	return (
		<div className="gm-page-content">
			<div className="gm-page-heading">
				<div><div className="gm-eyebrow">REMOTE PROVIDERS</div><h1>Accounts</h1><p>Connect provider identities to browse repositories outside your local scan roots.</p></div>
			</div>
			<div className="gm-account-layout">
				<div className="gm-panel gm-connect-panel">
					<div className="gm-panel-heading"><div><h2>Connect an account</h2><p>GitManager keeps provider credentials in the active app session.</p></div></div>
					<div className="gm-form-grid">
						<label className="gm-field">
							<span>Provider</span>
							<select value={provider} onChange={(event) => setProvider(event.target.value)}>
								<option value="github">GitHub</option>
								<option value="gitlab">GitLab</option>
							</select>
						</label>
						<Field label="Username or organization" value={username} onChange={setUsername} placeholder={provider === "github" ? "Optional for GitHub CLI sign-in" : "Username or GitLab host"} />
						<label className="gm-field gm-field-wide">
							<span>Personal access token <em>Optional for GitHub CLI sign-in</em></span>
							<input type="password" value={token} onChange={(event) => setToken(event.target.value)} placeholder="Paste token for private repositories" autoComplete="off" />
						</label>
					</div>
					{error && <div className="gm-inline-error">{error}</div>}
					{message && <div className="gm-inline-success">{message}</div>}
					<div className="gm-panel-actions">
						<Button variant="primary" onClick={() => void connect()} disabled={busy}>{busy ? "Connecting…" : "Connect provider"}</Button>
						{provider === "github" && <span className="gm-muted-copy">Leave both fields blank to use the active <code>gh auth login</code> session.</span>}
					</div>
				</div>
				<div className="gm-panel gm-connected-panel">
					<div className="gm-panel-heading"><div><h2>Connected identities</h2><p>Provider sessions available in this app.</p></div><span className="gm-count-chip">{accounts.length}</span></div>
					{accounts.length === 0 ? (
						<EmptyState icon="◎" title="No accounts connected" body="Connect GitHub or GitLab to browse remote repositories." />
					) : accounts.map((account) => (
						<div className="gm-account-row" key={account.provider + account.account_login}>
							<ProviderMark provider={account.provider} />
							<div className="gm-account-copy"><strong>{account.display_name || account.account_login}</strong><small>{account.account_login} · {account.provider}</small></div>
							<span className="gm-connected-pill"><i /> Connected</span>
							{account.repositories?.length > 0 && <span className="gm-count-chip">{account.repositories.length} repos</span>}
						</div>
					))}
				</div>
			</div>
			<section className="gm-panel gm-connected-panel">
				<div className="gm-panel-heading"><div><h2>Remote repositories</h2><p>Repositories returned by your connected provider identities.</p></div><span className="gm-count-chip">{remoteRepositories.length}</span></div>
				{remoteRepositories.length === 0 ? <p className="gm-muted-copy" style={{ padding: "0 15px" }}>Connect a provider account to load its remote repository list.</p> : remoteRepositories.map((repo) => (
					<button className="gm-account-row gm-remote-repo-row" type="button" key={repo.id} onClick={() => repo.web_url && void invoke("open_external_url", { url: repo.web_url }).catch((error) => window.alert(String(error)))}>
						<RepoAvatar repo={repo} />
						<div className="gm-account-copy"><strong>{repo.owner_name ? repo.owner_name + "/" : ""}{repo.name}</strong><small>{repo.current_branch || "default branch"} · {repo.primary_provider || "remote"}</small></div>
						<span className="gm-connected-pill">↗ Open</span>
					</button>
				))}
			</section>
		</div>
	);
}

function SettingsPage({
	preferences,
	directories,
	onSave,
	onScan,
	onOpenCleaner,
}: {
	preferences: AppPreferences;
	directories: ApplicationDirectories | null;
	onSave: (next: AppPreferences) => Promise<unknown>;
	onScan: () => void;
	onOpenCleaner: () => void;
}) {
	const [tab, setTab] = useState<SettingsTab>("general");
	const [draft, setDraft] = useState(preferences);
	useEffect(() => setDraft(preferences), [preferences]);
	const [newRoot, setNewRoot] = useState("");
	const [saving, setSaving] = useState(false);
	const [message, setMessage] = useState("");
	const [cloudJson, setCloudJson] = useState("");
	const [cloudInfo, setCloudInfo] = useState<Record<string, unknown> | null>(null);
	const [cloudStatus, setCloudStatus] = useState("");
	const [cloudNodeAddress, setCloudNodeAddress] = useState(preferences.cloudNodeAddress || "");
	const [cloudBusy, setCloudBusy] = useState(false);
	const [certificates, setCertificates] = useState<Array<{ thumbprint: string; subject: string; notAfter: string }>>([]);
	const [selectedThumbprint, setSelectedThumbprint] = useState("");
	const [installerPath, setInstallerPath] = useState("");
	const [signingStatus, setSigningStatus] = useState("Windows Authenticode signing is available when a valid CurrentUser Code Signing certificate is installed.");
	const [signingBusy, setSigningBusy] = useState(false);
	const [flatpakCli, setFlatpakCli] = useState<{ isFlatpak: boolean; installed: boolean; wrapperPath: string | null }>({ isFlatpak: false, installed: false, wrapperPath: null });
	const [flatpakCliBusy, setFlatpakCliBusy] = useState(false);
	const [flatpakCliMessage, setFlatpakCliMessage] = useState("");

	async function refreshFlatpakCli() {
		try {
			const status = await invoke<{ isFlatpak: boolean; installed: boolean; wrapperPath: string | null }>("get_flatpak_cli_integration_status");
			setFlatpakCli(status);
		} catch {
			setFlatpakCli({ isFlatpak: false, installed: false, wrapperPath: null });
		}
	}

	useEffect(() => {
		let active = true;
		void invoke<{ isFlatpak: boolean; installed: boolean; wrapperPath: string | null }>("get_flatpak_cli_integration_status")
			.then((status) => { if (active) setFlatpakCli(status); })
			.catch(() => undefined);
		return () => { active = false; };
	}, []);

	async function installFlatpakCli() {
		setFlatpakCliBusy(true);
		setFlatpakCliMessage("");
		try {
			const path = await invoke<string>("install_flatpak_gitm_cli");
			await refreshFlatpakCli();
			setFlatpakCliMessage("The gitm terminal command was installed at " + path + ". Make sure ~/.local/bin is in your PATH.");
		} catch (cause) {
			setFlatpakCliMessage(cause instanceof Error ? cause.message : String(cause));
		} finally {
			setFlatpakCliBusy(false);
		}
	}

	async function removeFlatpakCli() {
		setFlatpakCliBusy(true);
		setFlatpakCliMessage("");
		try {
			await invoke("remove_flatpak_gitm_cli");
			await refreshFlatpakCli();
			setFlatpakCliMessage("The GitManager-managed gitm wrapper was removed.");
		} catch (cause) {
			setFlatpakCliMessage(cause instanceof Error ? cause.message : String(cause));
		} finally {
			setFlatpakCliBusy(false);
		}
	}

	function patch(values: Partial<AppPreferences>) {
		setDraft((current) => ({ ...current, ...values }));
	}

	function addRoot() {
		const value = newRoot.trim();
		if (!value || draft.scanRoots.some((root) => root.toLowerCase() === value.toLowerCase())) return;
		patch({ scanRoots: [...draft.scanRoots, value] });
		setNewRoot("");
	}

	async function chooseScanRoot() {
		try {
			const selected = await openDialog({ title: "Choose a repository scan folder", directory: true, multiple: false });
			const path = Array.isArray(selected) ? selected[0] : selected;
			if (path) {
				patch({ scanRoots: draft.scanRoots.some((root) => root.toLowerCase() === path.toLowerCase()) ? draft.scanRoots : [...draft.scanRoots, path] });
			}
		} catch (cause) {
			setMessage(cause instanceof Error ? cause.message : String(cause));
		}
	}

	async function chooseArchiveRoot() {
		try {
			const selected = await openDialog({ title: "Choose an archive destination", directory: true, multiple: false });
			const path = Array.isArray(selected) ? selected[0] : selected;
			if (path) patch({ archiveRoot: path });
		} catch (cause) {
			setMessage(cause instanceof Error ? cause.message : String(cause));
		}
	}

	async function refreshCertificates() {
		setSigningBusy(true);
		setSigningStatus("Reading CurrentUser Code Signing certificates…");
		try {
			const found = await invoke<Array<{ thumbprint: string; subject: string; notAfter: string }>>("list_windows_signing_certificates");
			setCertificates(found ?? []);
			setSelectedThumbprint(found?.[0]?.thumbprint ?? "");
			setSigningStatus(found?.length
				? found.length + " usable Code Signing certificate(s) found."
				: "No valid Code Signing certificate with a private key was found in CurrentUser\\My.");
		} catch (cause) {
			setCertificates([]);
			setSelectedThumbprint("");
			setSigningStatus(cause instanceof Error ? cause.message : String(cause));
		} finally {
			setSigningBusy(false);
		}
	}

	async function chooseInstaller() {
		try {
			const selected = await openDialog({
				title: "Choose the GitManager Windows installer",
				directory: false,
				multiple: false,
				filters: [{ name: "Windows installer", extensions: ["exe", "msi"] }],
			});
			const path = Array.isArray(selected) ? selected[0] : selected;
			if (path) setInstallerPath(path);
		} catch (cause) {
			setSigningStatus(cause instanceof Error ? cause.message : String(cause));
		}
	}

	async function signInstaller() {
		if (!selectedThumbprint || !installerPath.trim()) {
			setSigningStatus("Choose a certificate and Windows .exe or .msi installer first.");
			return;
		}
		setSigningBusy(true);
		setSigningStatus("Creating a SHA-256 Authenticode signature and timestamp…");
		try {
			const result = await invoke<{ chainTrusted?: boolean }>("sign_windows_installer", {
				installerPath: installerPath.trim(),
				thumbprint: selectedThumbprint,
			});
			setSigningStatus(result.chainTrusted
				? "Signature created and trust chain verified. SmartScreen reputation may still build over time."
				: "Signature created, but Windows could not verify the trust chain. This may be a self-signed or untrusted certificate.");
		} catch (cause) {
			setSigningStatus("Signing failed: " + (cause instanceof Error ? cause.message : String(cause)));
		} finally {
			setSigningBusy(false);
		}
	}

	async function save() {
		setSaving(true);
		setMessage("");
		try {
			await onSave(draft);
			setMessage("Settings saved. Repository indexing has been refreshed.");
			setDraft({ ...draft, firstRunComplete: true });
		} catch (cause) {
			setMessage(cause instanceof Error ? cause.message : String(cause));
		} finally {
			setSaving(false);
		}
	}

	async function refreshCloud() {
		setCloudBusy(true);
		try {
			const root = draft.archiveRoot || await invoke<string>("get_default_archive_directory");
			if (!draft.archiveRoot) patch({ archiveRoot: root });
			const result = await invoke<Record<string, unknown>>("get_archive_cloud_storage_info", { archiveRoot: root });
			setCloudInfo(result);
			setCloudStatus(result.configured ? `Connected · ${String(result.provider || "provider")}` : "No cloud provider configured. Local archives continue to work.");
		} catch (cause) {
			setCloudStatus(cause instanceof Error ? cause.message : String(cause));
		} finally {
			setCloudBusy(false);
		}
	}

	async function configureCloud() {
		setCloudBusy(true);
		try {
			const result = await invoke<Record<string, unknown>>("configure_archive_cloud_storage", {
				archiveRoot: draft.archiveRoot,
				providerJson: cloudJson,
			});
			setCloudInfo(result);
			setCloudStatus("Cloud provider configuration saved.");
		} catch (cause) {
			setCloudStatus(cause instanceof Error ? cause.message : String(cause));
		} finally {
			setCloudBusy(false);
		}
	}

	async function disableCloud() {
		if (!window.confirm("Disable cloud synchronization? Existing provider objects will not be deleted.")) return;
		setCloudBusy(true);
		try {
			const result = await invoke<Record<string, unknown>>("disable_archive_cloud_storage", { archiveRoot: draft.archiveRoot });
			setCloudInfo(result);
			setCloudStatus("Cloud synchronization disabled.");
		} catch (cause) {
			setCloudStatus(cause instanceof Error ? cause.message : String(cause));
		} finally {
			setCloudBusy(false);
		}
	}

	async function syncCloud() {
		setCloudBusy(true);
		try {
			const result = await invoke<string>("sync_archive_cloud_storage", { archiveRoot: draft.archiveRoot });
			setCloudStatus(`Synchronization completed · ${result}`);
		} catch (cause) {
			setCloudStatus(cause instanceof Error ? cause.message : String(cause));
		} finally {
			setCloudBusy(false);
		}
	}

	async function checkNode() {
		setCloudBusy(true);
		try {
			const result = await invoke<{ address: string; reachable: boolean; message: string }>("check_rbe_node_status", { address: cloudNodeAddress });
			const saved = await invoke<string>("save_cloud_node_address", { address: result.address || cloudNodeAddress });
			setCloudNodeAddress(saved);
			patch({ cloudNodeAddress: saved });
			setCloudStatus(result.message || (result.reachable ? "NAS backend reachable." : "NAS backend not confirmed reachable."));
		} catch (cause) {
			setCloudStatus(cause instanceof Error ? cause.message : String(cause));
		} finally {
			setCloudBusy(false);
		}
	}

	return (
		<div className="gm-page-content">
			<div className="gm-page-heading">
				<div><div className="gm-eyebrow">PREFERENCES & MAINTENANCE</div><h1>Settings</h1><p>Configure scanning, archive behaviour, cloud storage and application locations.</p></div>
				<Button variant="primary" onClick={() => void save()} disabled={saving}>{saving ? "Saving…" : "Save changes"}</Button>
			</div>
			<div className="gm-settings-tabs">
				{([
					["general", "General"],
					["cloud", "Cloud storage"],
					["about", "About"],
				] as [SettingsTab, string][]).map(([value, label]) => (
					<button key={value} className={tab === value ? "active" : ""} onClick={() => setTab(value)}>{label}</button>
				))}
			</div>

			{tab === "general" && (
				<div className="gm-settings-grid">
					{flatpakCli.isFlatpak && (
						<section className="gm-panel gm-settings-card">
							<div className="gm-panel-heading"><div><h2>Terminal integration</h2><p>Optionally expose GitManager's bundled gitm command to your Linux terminal.</p></div><span className="gm-panel-icon">⌘</span></div>
							<p className="gm-muted-copy">GitManager creates or removes only its own ~/.local/bin/gitm wrapper. It never edits .bashrc, .zshrc or your shell PATH. Ensure ~/.local/bin is already on your PATH.</p>
							{flatpakCli.wrapperPath && <p className="gm-muted-copy">Wrapper: {flatpakCli.wrapperPath}</p>}
							<div className="gm-add-root">
								<Button variant="primary" onClick={() => void installFlatpakCli()} disabled={flatpakCliBusy}>{flatpakCliBusy ? "Working…" : flatpakCli.installed ? "Refresh gitm command" : "Install gitm command"}</Button>
								{flatpakCli.installed && <Button variant="danger" onClick={() => void removeFlatpakCli()} disabled={flatpakCliBusy}>Remove wrapper</Button>}
							</div>
							{flatpakCliMessage && <p className="gm-muted-copy" role="status">{flatpakCliMessage}</p>}
						</section>
					)}
					<section className="gm-panel gm-settings-card">
						<div className="gm-panel-heading"><div><h2>Repository discovery</h2><p>Choose which directories GitManager scans for local repositories.</p></div><span className="gm-panel-icon">⌕</span></div>
						<div className="gm-root-list">
							{draft.scanRoots.map((root) => (
								<div className="gm-root-row" key={root}><Glyph>▰</Glyph><span title={root}>{root}</span><button onClick={() => patch({ scanRoots: draft.scanRoots.filter((item) => item !== root) })} aria-label={`Remove ${root}`}>×</button></div>
							))}
							{draft.scanRoots.length === 0 && <p className="gm-muted-copy">No scan folders configured. Add a directory below.</p>}
						</div>
						<div className="gm-add-root">
							<input value={newRoot} onChange={(event) => setNewRoot(event.target.value)} onKeyDown={(event) => { if (event.key === "Enter") addRoot(); }} placeholder="C:\\Users\\you\\Projects or /home/you/projects" />
							<Button onClick={addRoot}>Add path</Button>
							<Button onClick={() => void chooseScanRoot()}>Browse…</Button>
						</div>
						<div className="gm-form-grid">
							<Field label="Maximum scan depth" value={draft.maxScanDepth} onChange={(value) => patch({ maxScanDepth: Math.min(16, Math.max(2, Number(value) || 8)) })} type="number" />
						</div>
						<Switch label="Include hidden directories" checked={draft.includeHiddenDirs} onChange={(value) => patch({ includeHiddenDirs: value })} description="Include folders such as .config or hidden workspaces." />
						<Switch label="Index dependency repositories" checked={draft.indexDependencyRepositories} onChange={(value) => patch({ indexDependencyRepositories: value })} description="Only search node_modules, vendor and build folders when no regular repositories are found." />
						<Switch label="Show dependency repositories" checked={draft.showDependencyRepositories} onChange={(value) => patch({ showDependencyRepositories: value })} description="Include nested dependencies in the default repository list." />
						<div className="gm-panel-actions"><Button onClick={onScan}><Glyph>↻</Glyph> Scan now</Button></div>
					</section>

					<section className="gm-panel gm-settings-card">
						<div className="gm-panel-heading"><div><h2>Automatic archives</h2><p>Control restore-point capture and retention through GitManager's archive engine.</p></div><span className="gm-panel-icon">▤</span></div>
						<div className="gm-add-root">
							<input aria-label="Archive folder" value={draft.archiveRoot} onChange={(event) => patch({ archiveRoot: event.target.value })} placeholder="Default application archive directory" />
							<Button onClick={() => void chooseArchiveRoot()}>Browse…</Button>
						</div>
						<Switch label="Local archives" checked={draft.archiveLocalEnabled} onChange={(value) => patch({ archiveLocalEnabled: value })} description="Capture working-tree files, including uncommitted changes." />
						<Switch label="Remote archives" checked={draft.archiveRemoteEnabled} onChange={(value) => patch({ archiveRemoteEnabled: value })} description="Capture a self-contained archive of Git refs and history." />
						<label className="gm-field">
							<span>Capture trigger</span>
							<select value={draft.archiveTrigger} onChange={(event) => patch({ archiveTrigger: event.target.value })}>
								<option value="manual">Manual only</option>
								<option value="commit">After Git commits</option>
								<option value="change">After filesystem changes settle</option>
							</select>
						</label>
						<Field label="Quiet period (milliseconds)" value={draft.archiveQuietMs} onChange={(value) => patch({ archiveQuietMs: Math.min(60000, Math.max(250, Number(value) || 1500)) })} type="number" />
						<div className="gm-panel-actions"><Button onClick={() => void save()} variant="primary" disabled={saving}>Save archive settings</Button></div>
					</section>

					<section className="gm-panel gm-settings-card">
						<div className="gm-panel-heading"><div><h2>Background updates</h2><p>Check remote branches and register the per-user background worker.</p></div><span className="gm-panel-icon">↻</span></div>
						<p className="gm-muted-copy">Updates use a safe fast-forward-only strategy. GitManager does not overwrite local commits or pull over a dirty working tree.</p>
						<div className="gm-panel-actions">
							<Button onClick={() => void invoke("check_repo_updates_now", { rootPaths: draft.scanRoots })}>Check remotes</Button>
							<Button variant="primary" onClick={() => void invoke("control_background_service", { enableInApp: true, installOsService: true })}>Enable background service</Button>
						</div>
					</section>

					<section className="gm-panel gm-settings-card">
						<div className="gm-panel-heading"><div><h2>Production signing</h2><p>Sign a Windows installer with a certificate installed for this Windows user.</p></div><span className="gm-panel-icon">✓</span></div>
						<label className="gm-field">
							<span>Code Signing certificate</span>
							<select value={selectedThumbprint} onChange={(event) => setSelectedThumbprint(event.target.value)} disabled={signingBusy || certificates.length === 0}>
								<option value="">Choose a certificate…</option>
								{certificates.map((certificate) => <option key={certificate.thumbprint} value={certificate.thumbprint}>{certificate.subject} · expires {certificate.notAfter}</option>)}
							</select>
						</label>
						<div className="gm-add-root">
							<input value={installerPath} onChange={(event) => setInstallerPath(event.target.value)} placeholder="Choose a GitManager-Setup.exe or .msi installer…" />
							<Button onClick={() => void chooseInstaller()} disabled={signingBusy}>Choose file…</Button>
						</div>
						<div className="gm-panel-actions">
							<Button onClick={() => void refreshCertificates()} disabled={signingBusy}>Refresh certificates</Button>
							<Button variant="primary" onClick={() => void signInstaller()} disabled={signingBusy || !selectedThumbprint || !installerPath.trim()}>Sign installer</Button>
						</div>
						<p className="gm-muted-copy" style={{ padding: "0 15px" }}>{signingStatus}</p>
						<p className="gm-muted-copy" style={{ padding: "0 15px" }}>The signing operation runs in the native Rust backend. The certificate's private key never leaves the Windows Certificate Store.</p>
					</section>

					<section className="gm-panel gm-settings-card">
						<div className="gm-panel-heading"><div><h2>Developer tools</h2><p>Preview generated files and developer caches before cleaning them.</p></div><span className="gm-panel-icon">⌁</span></div>
						<p className="gm-muted-copy">Nothing is removed until you inspect a cleanup preview and explicitly confirm the action.</p>
						<div className="gm-panel-actions"><Button onClick={onOpenCleaner}>Open Developer Cleaner</Button></div>
					</section>
				</div>
			)}

			{tab === "cloud" && (
				<div className="gm-settings-grid">
					<section className="gm-panel gm-settings-card">
						<div className="gm-panel-heading"><div><h2>Object storage</h2><p>Configure an archive provider supported by the RBE Cloud Node storage engine.</p></div><span className="gm-panel-icon">☁</span></div>
						<Field label="Archive folder" value={draft.archiveRoot} onChange={(value) => patch({ archiveRoot: value })} />
						<label className="gm-field gm-field-wide">
							<span>Provider configuration (JSON)</span>
							<textarea
								rows={9}
								value={cloudJson}
								onChange={(event) => setCloudJson(event.target.value)}
								placeholder={'Paste a provider settings JSON document. Credentials should be supplied through environment variables, not embedded here.'}
							/>
						</label>
						{cloudStatus && <div className="gm-inline-status">{cloudStatus}</div>}
						<div className="gm-panel-actions">
							<Button onClick={() => void refreshCloud()} disabled={cloudBusy}>Refresh status</Button>
							<Button onClick={() => void syncCloud()} disabled={cloudBusy || !Boolean(cloudInfo?.configured)}>Sync now</Button>
							<Button variant="primary" onClick={() => void configureCloud()} disabled={cloudBusy || !cloudJson.trim()}>Save provider</Button>
							<Button variant="danger" onClick={() => void disableCloud()} disabled={cloudBusy || !Boolean(cloudInfo?.configured)}>Disable</Button>
						</div>
					</section>
					<section className="gm-panel gm-settings-card">
						<div className="gm-panel-heading"><div><h2>RBE Cloud Node</h2><p>Check the health endpoints on a NAS or another computer on your LAN.</p></div><span className="gm-panel-icon">⌂</span></div>
						<Field label="NAS address" value={cloudNodeAddress} onChange={(value) => setCloudNodeAddress(value)} placeholder="192.168.1.20:8080" />
						<p className="gm-muted-copy">This checks backend health and saves the address. It does not establish peer authentication or pairing.</p>
						<div className="gm-panel-actions"><Button variant="primary" onClick={() => void checkNode()} disabled={cloudBusy}>Check NAS health</Button></div>
						{cloudStatus && <div className="gm-inline-status">{cloudStatus}</div>}
					</section>
				</div>
			)}

			{tab === "about" && (
				<div className="gm-panel gm-about-panel">
					<div className="gm-about-mark">GM</div>
					<h2>GitManager</h2>
					<p>A native Git workspace manager powered by Rust, Tauri, React and Next.js Engine.</p>
					<div className="gm-about-paths">
						<div><span>Application install</span><code>{directories?.installDir || "Unavailable"}</code></div>
						<div><span>Software root</span><code>{directories?.softwareRoot || "Unavailable"}</code></div>
						<div><span>Configuration</span><code>{directories?.configDir || "Unavailable"}</code></div>
						<div><span>Application data</span><code>{directories?.dataDir || "Unavailable"}</code></div>
					</div>
				</div>
			)}
			{message && <div className="gm-inline-status">{message}</div>}
		</div>
	);
}

function CleanerPage({ repositories, onRefresh, initialRepoPath }: { repositories: GitRepository[]; onRefresh: () => void; initialRepoPath?: string }) {
	const [scope, setScope] = useState<CleanerScope>("repository");
	const [repoPath, setRepoPath] = useState(initialRepoPath || repositories[0]?.path || "");
	useEffect(() => { if (initialRepoPath) setRepoPath(initialRepoPath); }, [initialRepoPath]);
	const [remoteUrl, setRemoteUrl] = useState("");
	const [branch, setBranch] = useState("");
	const [includeDependencies, setIncludeDependencies] = useState(false);
	const [includeVirtualenvs, setIncludeVirtualenvs] = useState(false);
	const [includeWindowsJunk, setIncludeWindowsJunk] = useState(false);
	const [confirmation, setConfirmation] = useState("");
	const [preview, setPreview] = useState<Record<string, unknown> | null>(null);
	const [status, setStatus] = useState("");
	const [busy, setBusy] = useState(false);

	const args = {
		scope,
		repoPath: repoPath.trim() || null,
		remoteUrl: remoteUrl.trim() || null,
		branch: branch.trim() || null,
		includeDependencies,
		includeVirtualenvs,
		includeWindowsJunk,
	};

	async function runPreview() {
		setBusy(true);
		setPreview(null);
		setStatus("Building a safe cleanup plan…");
		setConfirmation("");
		try {
			const result = await invoke<Record<string, unknown>>("cleaner_preview", args);
			setPreview(result);
			setStatus("Preview ready — nothing has been deleted.");
		} catch (cause) {
			setStatus(cause instanceof Error ? cause.message : String(cause));
		} finally {
			setBusy(false);
		}
	}

	async function applyCleanup() {
		if (!preview || Number(preview.candidate_count) < 1 || confirmation !== "YES") return;
		if (!window.confirm("Delete only the paths listed in this cleanup preview?")) return;
		setBusy(true);
		setStatus("Applying cleanup…");
		try {
			const result = await invoke<Record<string, unknown>>("cleaner_apply", { ...args, confirmText: "YES" });
			setPreview(result);
			setStatus(result.pushed ? `Cleanup branch pushed: ${String(result.branch)}` : "Cleanup completed.");
			if (scope === "repository") onRefresh();
		} catch (cause) {
			setStatus(cause instanceof Error ? cause.message : String(cause));
		} finally {
			setBusy(false);
		}
	}

	const candidates = Array.isArray(preview?.candidates) ? preview.candidates as Record<string, unknown>[] : [];
	return (
		<div className="gm-page-content">
			<div className="gm-page-heading">
				<div><div className="gm-eyebrow">MAINTENANCE</div><h1>Developer Cleaner</h1><p>Review generated files and development caches before removing anything.</p></div>
			</div>
			<div className="gm-cleaner-layout">
				<section className="gm-panel gm-settings-card">
					<div className="gm-panel-heading"><div><h2>Cleanup scope</h2><p>Choose what should be inspected. Preview always runs before apply.</p></div></div>
					<label className="gm-field"><span>Scope</span><select value={scope} onChange={(event) => { setScope(event.target.value as CleanerScope); setPreview(null); }}>
						<option value="repository">Local repository</option>
						<option value="system">Current user caches</option>
						<option value="remote">Remote repository</option>
					</select></label>
					{scope === "repository" && <label className="gm-field"><span>Repository</span><select value={repoPath} onChange={(event) => setRepoPath(event.target.value)}>
						{repositories.map((repo) => <option key={repo.id} value={repo.path}>{repo.name} — {repo.path}</option>)}
					</select></label>}
					{scope === "remote" && <>
						<Field label="Remote URL" value={remoteUrl} onChange={setRemoteUrl} placeholder="https://github.com/owner/repository.git" />
						<Field label="Branch (optional)" value={branch} onChange={setBranch} placeholder="Leave blank to use the remote default" />
					</>}
					<Switch label="Include dependency folders" checked={includeDependencies} onChange={setIncludeDependencies} description="Include package and build dependency directories." />
					<Switch label="Include virtual environments" checked={includeVirtualenvs} onChange={setIncludeVirtualenvs} description="Include Python and other virtual environment folders." />
					{scope === "system" && <Switch label="Include Windows junk files" checked={includeWindowsJunk} onChange={setIncludeWindowsJunk} />}
					<div className="gm-panel-actions"><Button variant="primary" onClick={() => void runPreview()} disabled={busy || (scope === "repository" && !repoPath) || (scope === "remote" && !remoteUrl.trim())}>{busy ? "Scanning…" : "Preview cleanup"}</Button></div>
				</section>
				<section className="gm-panel gm-cleaner-results">
					<div className="gm-panel-heading"><div><h2>Preview results</h2><p>Nothing is removed by the preview operation.</p></div></div>
					{preview ? (
						<>
							<div className="gm-cleaner-summary">
								<div><span>Candidates</span><strong>{String(preview.candidate_count ?? candidates.length)}</strong></div>
								<div><span>Estimated space</span><strong>{String(preview.total_bytes ?? preview.freed_bytes_estimate ?? "—")}</strong></div>
							</div>
							<div className="gm-cleaner-candidates">
								{candidates.slice(0, 50).map((item, index) => <div key={String(item.path ?? item.relative_path ?? index)}>
									<div><strong>{String(item.label ?? item.category ?? "Cleanup item")}</strong><small>{String(item.relative_path ?? item.path ?? "")}</small></div>
									<code>{String(item.size_bytes ?? "")} B</code>
								</div>)}
								{candidates.length === 0 && <p className="gm-muted-copy">No cleanup candidates found.</p>}
							</div>
							{Number(preview.candidate_count) > 0 && <>
								<Field label="Type YES to confirm apply" value={confirmation} onChange={setConfirmation} placeholder="YES" />
								<Button variant="danger" onClick={() => void applyCleanup()} disabled={busy || confirmation !== "YES"}>Apply reviewed cleanup</Button>
							</>}
						</>
					) : <EmptyState icon="⌁" title="No preview yet" body="Choose a scope and run a preview. GitManager will show every candidate before an apply action is enabled." />}
					{status && <div className="gm-inline-status">{status}</div>}
				</section>
			</div>
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
					<div className="gm-brand-mark small">GM</div>
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
	const [query, setQuery] = useState("");
	const [filter, setFilter] = useState<RepoFilter>("all");
	const [kind, setKind] = useState<RepoKind>("projects");
	const [selectedId, setSelectedId] = useState<string | null>(null);
	const selectedRepo = git.repositories.find((repo) => repo.id === selectedId) ?? null;

	if (git.loading) {
		return (
			<div className="gm-boot-screen" role="status" aria-live="polite">
				<div className="gm-boot-card">
					<div className="gm-boot-mark">GM</div>
					<strong>Loading GitManager</strong>
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
				setPage={setPage}
				repositoryCount={git.repositories.length}
				accountCount={git.accounts.length}
			/>
			<div className="gm-main-column">
				<TopBar
					page={page}
					version={git.version}
					status={git.status}
					query={query}
					setQuery={setQuery}
					scanning={git.scanning}
					onScan={() => void git.scan()}
				/>
				<main className="gm-main">
					{(git.loading || git.scanning) && <div className="gm-loading-bar"><i /></div>}
					{git.error && <div className="gm-error-banner"><span>{git.error}</span><button onClick={() => git.setError("")}>Dismiss</button></div>}
					{page === "repositories" && (
						<RepositoryPage
							repositories={git.repositories}
							dependencies={git.dependencies}
							selectedRepo={selectedRepo}
							setSelectedRepo={(repo) => setSelectedId(repo?.id ?? null)}
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
							onOpenCleaner={() => setPage("cleaner")}
						/>
					)}
					{page === "accounts" && <AccountsPage accounts={git.accounts} onConnect={git.connectAccount} />}
					{page === "settings" && (
						<SettingsPage
							preferences={git.preferences}
							directories={git.directories}
							onSave={git.savePreferences}
							onScan={() => void git.scan()}
							onOpenCleaner={() => setPage("cleaner")}
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
