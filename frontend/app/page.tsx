"use client";

import { useDeferredValue, useMemo, useState } from "react";
import { useWorkspace } from "../lib/workspace-context";
import { Button, EmptyState, Glyph, ProviderMark } from "../components/shared-ui";

/**
 * Classic Dense Repository Index page (`/`).
 * Renders the searchable, filterable repository table with branch, provider,
 * working-tree change counts, and local/remote state. Clicking any row
 * navigates to the dedicated `/repo?id=...` static route.
 */
export default function RepositoriesIndexPage() {
	const { git, handleScan, openRepository, navigateTo } = useWorkspace();
	const [searchQuery, setSearchQuery] = useState("");
	const deferredQuery = useDeferredValue(searchQuery);
	const [filterMode, setFilterMode] = useState<"all" | "dirty" | "remote" | "local">("all");

	const filteredRepositories = useMemo(() => {
		const term = deferredQuery.trim().toLowerCase();
		return git.repositories.filter((repo) => {
			if (filterMode === "dirty" && repo.dirty_files_count === 0) {
				return false;
			}
			if (filterMode === "remote" && !repo.has_remote) {
				return false;
			}
			if (filterMode === "local" && repo.has_remote) {
				return false;
			}
			if (!term) return true;
			return (
				repo.name.toLowerCase().includes(term) ||
				repo.path.toLowerCase().includes(term) ||
				repo.current_branch.toLowerCase().includes(term) ||
				(repo.primary_provider || "").toLowerCase().includes(term)
			);
		});
	}, [deferredQuery, filterMode, git.repositories]);

	const dirtyCount = useMemo(
		() => git.repositories.filter((repo) => repo.dirty_files_count > 0).length,
		[git.repositories],
	);
	const remoteCount = useMemo(
		() => git.repositories.filter((repo) => repo.has_remote).length,
		[git.repositories],
	);
	const localOnlyCount = useMemo(
		() => git.repositories.filter((repo) => !repo.has_remote).length,
		[git.repositories],
	);

	return (
		<section className="gm-page gm-repo-index-page">
			<div className="gm-page-header">
				<div>
					<h1>Repositories</h1>
					<p>
						{git.repositories.length} indexed across {git.preferences.scanRoots.length} scan root
						{git.preferences.scanRoots.length === 1 ? "" : "s"}
					</p>
				</div>
				<div className="gm-page-header-actions">
					<Button onClick={handleScan} disabled={git.scanning}>
						<Glyph>↻</Glyph> {git.scanning ? "Scanning…" : "Scan Roots"}
					</Button>
					<Button onClick={() => navigateTo("/settings")}>
						<Glyph>⚙</Glyph> Scan Roots
					</Button>
				</div>
			</div>

			<div className="gm-toolbar-row">
				<input
					type="search"
					className="gm-input gm-search-input"
					value={searchQuery}
					onChange={(event) => setSearchQuery(event.target.value)}
					placeholder="Filter repositories by name, branch, path, or provider…"
					aria-label="Filter repositories"
				/>
				<div className="gm-filter-pills" role="group" aria-label="Repository status filters">
					<button
						type="button"
						className={filterMode === "all" ? "gm-pill active" : "gm-pill"}
						onClick={() => setFilterMode("all")}
					>
						All ({git.repositories.length})
					</button>
					<button
						type="button"
						className={filterMode === "dirty" ? "gm-pill active" : "gm-pill"}
						onClick={() => setFilterMode("dirty")}
					>
						Modified ({dirtyCount})
					</button>
					<button
						type="button"
						className={filterMode === "remote" ? "gm-pill active" : "gm-pill"}
						onClick={() => setFilterMode("remote")}
					>
						Remote ({remoteCount})
					</button>
					{localOnlyCount > 0 ? (
						<button
							type="button"
							className={filterMode === "local" ? "gm-pill active" : "gm-pill"}
							onClick={() => setFilterMode("local")}
						>
							Local Only ({localOnlyCount})
						</button>
					) : null}
				</div>
			</div>

			{filteredRepositories.length === 0 ? (
				<EmptyState
					icon="⎇"
					title={git.repositories.length === 0 ? "No repositories found yet" : "No repositories match your filter"}
					body={
						git.repositories.length === 0
							? "Configure scan roots in Settings or click Scan Roots to index local Git repositories."
							: "Clear the search box or switch the filter tab to see all indexed repositories."
					}
					action={
						git.repositories.length === 0 ? (
							<Button variant="primary" onClick={handleScan} disabled={git.scanning}>
								Scan Repositories
							</Button>
						) : (
							<Button
								onClick={() => {
									setSearchQuery("");
									setFilterMode("all");
								}}
							>
								Reset Filters
							</Button>
						)
					}
				/>
			) : (
				<div className="gm-panel gm-table-panel">
					<table className="gm-repo-table">
						<thead>
							<tr>
								<th>Repository</th>
								<th>Branch</th>
								<th>Remote</th>
								<th>Working Tree</th>
								<th>Path</th>
							</tr>
						</thead>
						<tbody>
							{filteredRepositories.map((repo) => (
								<tr
									key={repo.id}
									className="gm-repo-row"
									onClick={() => openRepository(repo)}
									tabIndex={0}
									onKeyDown={(event) => {
										if (event.key === "Enter" || event.key === " ") {
											event.preventDefault();
											openRepository(repo);
										}
									}}
								>
									<td>
										<div className="gm-repo-name-cell">
											<ProviderMark provider={repo.primary_provider} />
											<div>
												<strong>{repo.name}</strong>
											</div>
										</div>
									</td>
									<td>
										<code className="gm-mono">{repo.current_branch || "HEAD"}</code>
									</td>
									<td>
										{repo.has_remote ? (
											<span className="gm-badge info">{repo.primary_provider || "Remote"}</span>
										) : (
											<span className="gm-muted">Local only</span>
										)}
									</td>
									<td>
										{repo.dirty_files_count > 0 ? (
											<span className="gm-badge warn">{repo.dirty_files_count} changed</span>
										) : (
											<span className="gm-muted">Clean</span>
										)}
									</td>
									<td className="gm-path-cell">
										<span className="gm-mono gm-muted">{repo.path}</span>
									</td>
								</tr>
							))}
						</tbody>
					</table>
				</div>
			)}
		</section>
	);
}
