"use client";

import { useEffect, useMemo, useRef, useState } from "react";
import dynamic from "next/dynamic";
import { invoke } from "../lib/tauri";
import type { AppPreferences, ArchiveSnapshot, CommitHistoryPage, GitRepository } from "../lib/types";
import type { RepositoryDestination } from "../lib/internal-routing";
import { Button, Glyph, RepoAvatar } from "./shared-ui";

const RepositoryWorkspace = dynamic(
	() => import("./repository-workspace").then((module) => module.RepositoryWorkspace),
	{ ssr: false, loading: () => <div className="gm-workspace-loading">Loading repository workspace…</div> },
);

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

function pullRequestUrl(repository: GitRepository, number: number): string | null {
	const remote = repository.remotes.find((item) => item.name === "origin") ?? repository.remotes[0];
	const base = (repository.web_url ?? remote?.web_url ?? "").replace(/\/+$/, "");
	const provider = (repository.primary_provider || remote?.provider || "").toLowerCase();
	if (!base) return null;
	if (provider.includes("github")) return `${base}/pull/${number}`;
	if (provider.includes("gitlab")) return `${base}/-/merge_requests/${number}`;
	if (provider.includes("bitbucket")) return `${base}/pull-requests/${number}`;
	return null;
}

function pullRequestListUrl(repository: GitRepository): string | null {
	const remote = repository.remotes.find((item) => item.name === "origin") ?? repository.remotes[0];
	const base = (repository.web_url ?? remote?.web_url ?? "").replace(/\/+$/, "");
	const provider = (repository.primary_provider || remote?.provider || "").toLowerCase();
	if (!base) return null;
	if (provider.includes("github")) return base + "/pulls";
	if (provider.includes("gitlab")) return base + "/-/merge_requests";
	if (provider.includes("bitbucket")) return base + "/pull-requests";
	return null;
}

export function RepositoryDetail({
	repo,
	preferences,
	destination,
	onNavigateDestination,
	onClose,
	onOpenWebsite,
	onRescan,
	onRepositoryLoaded,
	onPullCompleted,
	onOpenCleaner,
}: {
	repo: GitRepository;
	preferences: AppPreferences;
	destination: RepositoryDestination;
	onNavigateDestination: (destination: RepositoryDestination) => void;
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
	const [commitQuery, setCommitQuery] = useState("");
	const [commitBranchFilter, setCommitBranchFilter] = useState("");
	const [historyPage, setHistoryPage] = useState<CommitHistoryPage | null>(null);
	const [historyLoading, setHistoryLoading] = useState(false);
	const [historyError, setHistoryError] = useState("");
	const [debouncedCommitQuery, setDebouncedCommitQuery] = useState("");
	const historyRequestKeyRef = useRef("");
	const [busy, setBusy] = useState(false);
	const isHistoryRoute = destination.kind === "commit-browser"
		|| destination.kind === "commit-details"
		|| destination.kind === "pull-browser"
		|| destination.kind === "pull-request";
	const historyRequestKey = JSON.stringify([
		repo.path,
		isHistoryRoute,
		commitBranchFilter,
		debouncedCommitQuery,
	]);
	const commitRouteSha = destination.kind === "commit-details" ? destination.sha : null;

	useEffect(() => {
		const timer = window.setTimeout(() => setDebouncedCommitQuery(commitQuery), 250);
		return () => window.clearTimeout(timer);
	}, [commitQuery]);

	useEffect(() => {
		if (!isHistoryRoute) {
			historyRequestKeyRef.current = "";
			setHistoryPage(null);
			setHistoryError("");
			setHistoryLoading(false);
			return;
		}
		let active = true;
		historyRequestKeyRef.current = historyRequestKey;
		setHistoryPage(null);
		setHistoryError("");
		setHistoryLoading(true);
		void invoke<CommitHistoryPage>("load_repository_commit_history", {
			repoPath: repo.path,
			offset: 0,
			limit: 50,
			branchFilter: commitBranchFilter.trim() || null,
			searchQuery: debouncedCommitQuery.trim() || null,
		}).then((page) => {
			if (active && historyRequestKeyRef.current === historyRequestKey) {
				setHistoryPage(page);
			}
		}).catch((cause) => {
			if (active && historyRequestKeyRef.current === historyRequestKey) {
				setHistoryError(cause instanceof Error ? cause.message : String(cause));
			}
		}).finally(() => {
			if (active && historyRequestKeyRef.current === historyRequestKey) {
				setHistoryLoading(false);
			}
		});
		return () => { active = false; };
	}, [historyRequestKey, isHistoryRoute, repo.path, commitBranchFilter, debouncedCommitQuery]);

	async function loadMoreCommitHistory() {
		const page = historyPage;
		const requestKey = historyRequestKey;
		if (!page?.hasMore || historyLoading) return;
		setHistoryLoading(true);
		setHistoryError("");
		try {
			const next = await invoke<CommitHistoryPage>("load_repository_commit_history", {
				repoPath: repo.path,
				offset: page.offset + page.commits.length,
				limit: 50,
				branchFilter: commitBranchFilter.trim() || null,
				searchQuery: debouncedCommitQuery.trim() || null,
			});
			if (historyRequestKeyRef.current === requestKey) {
				setHistoryPage((current) => current ? {
					...next,
					offset: current.offset,
					commits: [...current.commits, ...next.commits],
				} : next);
			}
		} catch (cause) {
			if (historyRequestKeyRef.current === requestKey) {
				setHistoryError(cause instanceof Error ? cause.message : String(cause));
			}
		} finally {
			if (historyRequestKeyRef.current === requestKey) setHistoryLoading(false);
		}
	}

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

	useEffect(() => {
		void refreshArchives();
		// Repository path defines the archive index scope.
		// eslint-disable-next-line react-hooks/exhaustive-deps
	}, [repo.path]);

	useEffect(() => {
		if (!commitRouteSha) {
			setSelectedCommit(null);
			setCommitDetail(null);
			return;
		}
		let active = true;
		setSelectedCommit(commitRouteSha);
		setCommitDetail(null);
		void invoke("get_commit_details", { repoPath: repo.path, oid: commitRouteSha })
			.then((details) => { if (active) setCommitDetail(details); })
			.catch((error) => { if (active) setCommitDetail({ error: String(error) }); });
		return () => { active = false; };
	}, [repo.path, commitRouteSha]);

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
	const isPullRoute = destination.kind === "pull-browser" || destination.kind === "pull-request";
	const branches = (displayRepo.branches ?? []).filter((branch) => isPullRoute ? branch.is_remote : !branch.is_remote);
	const commits = isHistoryRoute ? (historyPage?.commits ?? []) : (displayRepo.commits ?? []);
	const commitBranchOptions = useMemo(() => Array.from(new Set([
		...(displayRepo.branches ?? []).map((branch) => branch.name),
		...commits.flatMap((commit) => commit.branches ?? []),
	])).filter(Boolean).sort((left, right) => left.localeCompare(right)), [displayRepo.branches, commits]);
	const filteredCommits = commits;
	const isCommitRoute = destination.kind === "commit-browser" || destination.kind === "commit-details";
	const showWorkspace = destination.kind === "overview"
		|| destination.kind === "conflicts"
		|| destination.kind === "pull-browser"
		|| destination.kind === "pull-conflicts";
	const showCommitHistory = destination.kind === "overview" || isCommitRoute || destination.kind === "pull-request" || destination.kind === "pull-browser";
	const activeDestination = destination.kind === "commit-details"
		? "commit-browser"
		: destination.kind === "pull-request"
			? "pull-browser"
			: destination.kind;
	const remotePullRequestUrl = destination.kind === "pull-request"
		? pullRequestUrl(displayRepo, destination.number)
		: null;
	const repositoryRouteTabs: Array<{ label: string; destination: RepositoryDestination }> = [
		{ label: "Overview", destination: { kind: "overview" } },
		{ label: "Conflicts", destination: { kind: "conflicts" } },
		{ label: "Commits", destination: { kind: "commit-browser" } },
		{ label: "Pull requests", destination: { kind: "pull-browser" } },
		{ label: "Pull conflicts", destination: { kind: "pull-conflicts" } },
	];

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

			<nav className="gm-repository-route-nav" aria-label="Repository views">
				{repositoryRouteTabs.map((item) => (
					<button
						key={item.destination.kind}
						type="button"
						className={activeDestination === item.destination.kind ? "active" : ""}
						aria-current={activeDestination === item.destination.kind ? "page" : undefined}
						onClick={() => onNavigateDestination(item.destination)}
					>
						{item.label}
					</button>
				))}
			</nav>

			{destination.kind === "conflicts" && (
				<div className="gm-route-notice">
					<strong>Local conflict workspace</strong>
					<span>Only paths marked unmerged by Git are shown. Open a file in your editor, resolve its contents, then return here and stage the resolved file.</span>
				</div>
			)}
			{destination.kind === "pull-conflicts" && (
				<div className="gm-route-notice">
					<strong>Pull conflict resolver</strong>
					<span>These are local Git conflicts, not a GitHub or GitLab web view. Resolve the working-copy files, stage them, then review the Pull again.</span>
				</div>
			)}
			{destination.kind === "pull-browser" && (
				<div className="gm-route-notice">
					<strong>Remote pull requests</strong>
					<span>Pull requests are provider metadata. GitManager keeps branch and commit navigation local; open the provider to browse its pull-request list.</span>
					{repo.web_url && <Button onClick={() => onOpenWebsite(pullRequestListUrl(displayRepo) ?? repo.web_url!)}>Open pull requests</Button>}
				</div>
			)}
			{destination.kind === "pull-request" && (
				<div className="gm-route-notice">
					<strong>Pull request #{destination.number}</strong>
					<span>GitManager treats pull requests as remote decorations. The branch and commit history remain the local Git data shown below.</span>
					{remotePullRequestUrl
						? <Button onClick={() => onOpenWebsite(remotePullRequestUrl)}>Open pull request</Button>
						: <span>This local repository does not expose a supported provider URL for pull request #{destination.number}.</span>}
				</div>
			)}
			{showWorkspace && (
				<RepositoryWorkspace
					key={repo.path}
					repo={displayRepo}
					focus={destination.kind === "conflicts" || destination.kind === "pull-conflicts" ? "conflicts" : "all"}
					onRepositoryUpdated={onRepositoryLoaded}
					onPullCompleted={onPullCompleted}
					onOpenCleaner={onOpenCleaner}
				/>
			)}

			{showCommitHistory && <div className="gm-detail-columns">
				<div className="gm-panel gm-commit-panel">
					<div className="gm-panel-heading">
						<div>
							<h2>{isHistoryRoute ? "Commit history" : "Latest commit"}</h2>
							<p>{isHistoryRoute ? "Browse all reachable commits with branch, message, author or SHA filters." : "Most recent commit on the current repository HEAD."}</p>
						</div>
						<span className="gm-count-chip">{isHistoryRoute ? `${commits.length} loaded${historyPage?.hasMore ? "+" : ""}` : `${commits.length} latest`}</span>
					</div>
					{isHistoryRoute && <div className="gm-commit-filters">
						<input
							value={commitQuery}
							onChange={(event) => setCommitQuery(event.target.value)}
							placeholder="Filter message, author or SHA…"
							aria-label="Filter commits by message, author or SHA"
						/>
						<select
							value={commitBranchFilter}
							onChange={(event) => setCommitBranchFilter(event.target.value)}
							aria-label="Filter commits by branch"
						>
							<option value="">All branches</option>
							{commitBranchOptions.map((branch) => <option value={branch} key={branch}>{branch}</option>)}
						</select>
					</div>}
					{detailsLoading ? <div className="gm-panel-empty">Loading repository overview…</div>
						: (isHistoryRoute && historyLoading && commits.length === 0) ? <div className="gm-panel-empty">Loading commit history…</div>
						: commits.length === 0 ? <div className="gm-panel-empty">{isHistoryRoute ? "No commits match these filters." : "No commits were found for this repository."}</div>
						: (
						<div className="gm-commit-list">
							{filteredCommits.map((commit, index) => (
								<button
									key={commit.oid}
									className={selectedCommit === commit.oid ? "gm-commit-row selected" : "gm-commit-row"}
									onClick={() => onNavigateDestination({
										kind: "commit-details",
										branch: commit.branches?.[0] || displayRepo.current_branch,
										sha: commit.oid,
									})}
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
					{isHistoryRoute && historyPage?.hasMore && <div className="gm-commit-more">
						<button type="button" onClick={() => void loadMoreCommitHistory()} disabled={historyLoading}>
							{historyLoading ? "Loading…" : "Load more commits"}
						</button>
					</div>}
					{historyError && <div className="gm-route-notice" role="alert">{historyError}</div>}
					{selectedCommit && (
						<div className="gm-commit-inspector">
							<div className="gm-commit-inspector-heading">
								<strong>Commit details</strong>
								<button onClick={() => onNavigateDestination({ kind: "commit-browser" })} type="button">×</button>
							</div>
							<pre>{commitDetail ? JSON.stringify(commitDetail, null, 2) : "Loading commit details…"}</pre>
						</div>
					)}
				</div>

				<div className="gm-detail-side">
					<div className="gm-panel">
						<div className="gm-panel-heading"><div><h2>{isPullRoute ? "Remote branches" : "Local branches"}</h2><p>{isPullRoute ? "Fetched remote refs associated with this repository" : "Local branch references"}</p></div><span className="gm-count-chip">{branches.length}</span></div>
						<div className="gm-branch-list">
							{branches.slice(0, 12).map((branch) => (
								<div className="gm-branch-row" key={branch.name}>
									<Glyph>⑂</Glyph>
									<strong>{branch.name}</strong>
									{branch.is_head && <span className="gm-current-chip">Current</span>}
									{branch.is_default && <span className="gm-default-chip">Default</span>}
								</div>
							))}
							{branches.length === 0 && <p className="gm-muted-copy">{isPullRoute ? "No fetched remote branches reported." : "No local branches reported."}</p>}
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
				</div>}
			</section>
	);
}
