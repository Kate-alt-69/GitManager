"use client";

import { useCallback, useEffect, useState } from "react";
import { invoke } from "../lib/tauri";
import type { GitRepository } from "../lib/types";
import { Button, EmptyState } from "./shared-ui";

interface RepoIssueEntry {
	number: string;
	title: string;
	state: string;
	author: string;
	labels: string[];
	updated_at: string;
	url: string;
}

interface RepoIssuesResponse {
	provider: string;
	note: string;
	issues: RepoIssueEntry[];
}

/**
 * Lightweight Issues Panel for GitHub (`gh issue`) & GitLab (`glab issue`).
 * Supports filtering open/closed issues, creating/closing issues, and
 * one-click "Create Branch from Issue" (`issue/<num>-<slug>`).
 */
export function IssuesPanel({
	repo,
	onBranchCreated,
	onOpenAccounts,
	onTriggerHappyLogo,
}: {
	repo: GitRepository;
	onBranchCreated?: () => void;
	onOpenAccounts?: () => void;
	onTriggerHappyLogo?: () => void;
}) {
	const [stateFilter, setStateFilter] = useState<"open" | "closed" | "all">("open");
	const [data, setData] = useState<RepoIssuesResponse | null>(null);
	const [loading, setLoading] = useState(true);
	const [creating, setCreating] = useState(false);
	const [title, setTitle] = useState("");
	const [body, setBody] = useState("");
	const [labels, setLabels] = useState("");
	const [busy, setBusy] = useState(false);
	const [notice, setNotice] = useState("");
	const [error, setError] = useState("");

	const loadIssues = useCallback(async () => {
		setLoading(true);
		setError("");
		try {
			const resp = await invoke<RepoIssuesResponse>("list_repo_issues", {
				repoPath: repo.path,
				state: stateFilter,
			});
			setData(resp);
		} catch (cause) {
			setError(cause instanceof Error ? cause.message : String(cause));
		} finally {
			setLoading(false);
		}
	}, [repo.path, stateFilter]);

	useEffect(() => {
		void loadIssues();
	}, [loadIssues]);

	async function handleCreateIssue() {
		setBusy(true);
		setNotice("");
		setError("");
		try {
			const out = await invoke<string>("create_repo_issue", {
				repoPath: repo.path,
				title,
				body,
				labels: labels.trim() || null,
			});
			setNotice(out || "Issue created.");
			setTitle("");
			setBody("");
			setLabels("");
			setCreating(false);
			onTriggerHappyLogo?.();
			await loadIssues();
		} catch (cause) {
			setError(cause instanceof Error ? cause.message : String(cause));
		} finally {
			setBusy(false);
		}
	}

	async function handleToggleState(issue: RepoIssueEntry) {
		const isOpen = issue.state.toLowerCase().startsWith("open");
		setBusy(true);
		setNotice("");
		setError("");
		try {
			const msg = await invoke<string>("set_repo_issue_state", {
				repoPath: repo.path,
				issueNumber: issue.number,
				close: isOpen,
			});
			setNotice(msg);
			await loadIssues();
		} catch (cause) {
			setError(cause instanceof Error ? cause.message : String(cause));
		} finally {
			setBusy(false);
		}
	}

	async function handleCreateBranch(issue: RepoIssueEntry) {
		setBusy(true);
		setNotice("");
		setError("");
		try {
			const branchName = await invoke<string>("create_branch_from_issue", {
				repoPath: repo.path,
				issueNumber: issue.number,
				issueTitle: issue.title,
			});
			setNotice(`Checked out new branch \`${branchName}\`.`);
			onTriggerHappyLogo?.();
			onBranchCreated?.();
		} catch (cause) {
			setError(cause instanceof Error ? cause.message : String(cause));
		} finally {
			setBusy(false);
		}
	}

	return (
		<div className="gm-actions-root">
			<div className="gm-panel-heading">
				<div>
					<h2>Issues — {repo.name}</h2>
					<p>Track issues and spin up a local feature branch directly from any ticket.</p>
				</div>
				<div className="gm-panel-actions">
					<div className="gm-filter-pills">
						{(["open", "closed", "all"] as const).map((st) => (
							<button
								key={st}
								type="button"
								className={stateFilter === st ? "gm-pill active" : "gm-pill"}
								onClick={() => setStateFilter(st)}
							>
								{st.charAt(0).toUpperCase() + st.slice(1)}
							</button>
						))}
					</div>
					<Button variant="primary" onClick={() => setCreating((v) => !v)}>
						{creating ? "Cancel" : "+ New Issue"}
					</Button>
				</div>
			</div>

			{notice ? <div className="gm-inline-success">{notice}</div> : null}
			{error ? <div className="gm-inline-error">{error}</div> : null}
			{data?.note ? (
				<div className="gm-inline-warning">
					<span>{data.note}</span>
					{onOpenAccounts ? <Button onClick={onOpenAccounts}>Open Accounts</Button> : null}
				</div>
			) : null}

			{creating ? (
				<section className="gm-panel">
					<div className="gm-form-grid">
						<label className="gm-field gm-field-wide">
							<span>Issue Title</span>
							<input
								type="text"
								value={title}
								onChange={(event) => setTitle(event.target.value)}
								placeholder="Short summary of the bug or feature…"
							/>
						</label>
						<label className="gm-field gm-field-wide">
							<span>Labels (comma-separated, optional)</span>
							<input
								type="text"
								value={labels}
								onChange={(event) => setLabels(event.target.value)}
								placeholder="bug, enhancement"
							/>
						</label>
						<label className="gm-field gm-field-wide">
							<span>Description</span>
							<textarea
								rows={4}
								className="gm-code-textarea"
								style={{ minHeight: 100, border: "1px solid var(--gm-border, #2c313a)", borderRadius: 8 }}
								value={body}
								onChange={(event) => setBody(event.target.value)}
								placeholder="Details, repro steps, or acceptance criteria…"
							/>
						</label>
					</div>
					<div className="gm-panel-actions">
						<Button
							variant="primary"
							disabled={busy || !title.trim()}
							onClick={() => void handleCreateIssue()}
						>
							{busy ? "Creating…" : "Create Issue"}
						</Button>
					</div>
				</section>
			) : null}

			<section className="gm-panel">
				<div className="gm-panel-heading">
					<div>
						<h3>{stateFilter.toUpperCase()} Issues</h3>
						<p>Loaded via {data?.provider === "gitlab" ? "glab" : "gh"} CLI</p>
					</div>
					<span className="gm-count-chip">{data?.issues.length ?? 0}</span>
				</div>

				{!data || data.issues.length === 0 ? (
					<EmptyState
						icon="●"
						title={loading ? "Loading issues…" : `No ${stateFilter} issues`}
						body="Issues from your remote repository will appear here."
					/>
				) : (
					<div className="gm-workflow-list">
						{data.issues.map((issue) => {
							const isOpen = issue.state.toLowerCase().startsWith("open");
							return (
								<div key={issue.number} className="gm-workflow-row">
									<div>
										<strong>
											#{issue.number} {issue.title}
										</strong>
										<div className="gm-mono gm-muted">
											by {issue.author || "unknown"}
											{issue.labels.length > 0 ? ` · ${issue.labels.join(", ")}` : ""}
										</div>
									</div>
									<div className="gm-workflow-row-actions">
										<span className={isOpen ? "gm-connected-pill" : "gm-badge"}>{issue.state}</span>
										<Button disabled={busy} onClick={() => void handleCreateBranch(issue)}>
											⎇ Create Branch
										</Button>
										<Button disabled={busy} onClick={() => void handleToggleState(issue)}>
											{isOpen ? "Close" : "Reopen"}
										</Button>
										{issue.url ? (
											<Button
												onClick={() =>
													void invoke("open_external_url", { url: issue.url }).catch(() => undefined)
												}
											>
												↗
											</Button>
										) : null}
									</div>
								</div>
							);
						})}
					</div>
				)}
			</section>
		</div>
	);
}
