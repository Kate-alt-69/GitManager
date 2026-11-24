"use client";

import { useCallback, useEffect, useState } from "react";
import { invoke } from "../lib/tauri";
import type { GitRepository } from "../lib/types";
import { Button, EmptyState } from "./shared-ui";
import { WorkbenchEditor } from "./workbench-editor";

interface WorkflowInputField {
	name: string;
	description: string;
	default_value: string;
	required: boolean;
}

interface RepoWorkflowFile {
	provider: string;
	name: string;
	file_name: string;
	relative_path: string;
	supports_dispatch: boolean;
	inputs: WorkflowInputField[];
}

interface WorkflowRunEntry {
	id: string;
	provider: string;
	workflow_name: string;
	title: string;
	status: string;
	conclusion: string;
	branch: string;
	sha: string;
	created_at: string;
	url: string;
}

interface WorkflowRunsResponse {
	provider: string;
	cli_available: boolean;
	note: string;
	runs: WorkflowRunEntry[];
}

const STARTER_GITHUB_WORKFLOW = `name: CI

on:
  push:
    branches: [main]
  workflow_dispatch:
    inputs:
      note:
        description: "Optional run note"
        required: false
        default: "Manual trigger from GitManager"

jobs:
  check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: echo "Hello from GitManager Actions!"
`;

/**
 * Unified GitHub Actions & GitLab CI Pipelines workbench panel.
 * Lists workflow files, triggers remote runs via `gh` / `glab` with
 * parsed `workflow_dispatch` inputs, streams run logs, and opens any
 * workflow in the general-purpose `WorkbenchEditor` (Actions Mode).
 */
export function ActionsPanel({
	repo,
	onOpenAccounts,
	onTriggerHappyLogo,
}: {
	repo: GitRepository;
	onOpenAccounts?: () => void;
	onTriggerHappyLogo?: () => void;
}) {
	const [workflows, setWorkflows] = useState<RepoWorkflowFile[]>([]);
	const [runsData, setRunsData] = useState<WorkflowRunsResponse | null>(null);
	const [loading, setLoading] = useState(true);
	const [editingPath, setEditingPath] = useState<string | null>(null);
	const [editingInitial, setEditingInitial] = useState<string | undefined>(undefined);
	const [dispatchTarget, setDispatchTarget] = useState<RepoWorkflowFile | null>(null);
	const [dispatchBranch, setDispatchBranch] = useState(repo.current_branch || "main");
	const [dispatchInputs, setDispatchInputs] = useState<Record<string, string>>({});
	const [activeLogRun, setActiveLogRun] = useState<WorkflowRunEntry | null>(null);
	const [logText, setLogText] = useState("");
	const [logLoading, setLogLoading] = useState(false);
	const [busy, setBusy] = useState(false);
	const [notice, setNotice] = useState("");
	const [error, setError] = useState("");

	const loadAll = useCallback(async () => {
		setLoading(true);
		setError("");
		try {
			const [wfList, runResp] = await Promise.all([
				invoke<RepoWorkflowFile[]>("list_repo_workflows", { repoPath: repo.path }),
				invoke<WorkflowRunsResponse>("list_workflow_runs", { repoPath: repo.path, limit: 15 }),
			]);
			setWorkflows(wfList);
			setRunsData(runResp);
		} catch (cause) {
			setError(cause instanceof Error ? cause.message : String(cause));
		} finally {
			setLoading(false);
		}
	}, [repo.path]);

	useEffect(() => {
		setDispatchBranch(repo.current_branch || "main");
		setEditingPath(null);
		setActiveLogRun(null);
		void loadAll();
	}, [loadAll, repo.current_branch]);

	function openDispatchForm(wf: RepoWorkflowFile) {
		const defaults: Record<string, string> = {};
		for (const field of wf.inputs) {
			defaults[field.name] = field.default_value || "";
		}
		setDispatchInputs(defaults);
		setDispatchTarget(wf);
	}

	async function runWorkflowNow(workflowFile: string, branch?: string, inputs?: Record<string, string>) {
		setBusy(true);
		setNotice("");
		setError("");
		try {
			const msg = await invoke<string>("trigger_workflow_run", {
				repoPath: repo.path,
				workflowFile,
				branch: branch || repo.current_branch || null,
				inputs: inputs || {},
			});
			setNotice(msg);
			setDispatchTarget(null);
			onTriggerHappyLogo?.();
			await loadAll();
		} catch (cause) {
			setError(cause instanceof Error ? cause.message : String(cause));
		} finally {
			setBusy(false);
		}
	}

	async function openRunLogs(run: WorkflowRunEntry) {
		setActiveLogRun(run);
		setLogLoading(true);
		setLogText("");
		try {
			const text = await invoke<string>("get_workflow_run_logs", {
				repoPath: repo.path,
				runId: run.id,
			});
			setLogText(text || "(No log output returned yet.)");
		} catch (cause) {
			setLogText(cause instanceof Error ? cause.message : String(cause));
		} finally {
			setLogLoading(false);
		}
	}

	function createNewWorkflow() {
		const isGitLab = (repo.primary_provider || "").toLowerCase().includes("gitlab");
		const relPath = isGitLab ? ".gitlab-ci.yml" : ".github/workflows/ci.yml";
		const starter = isGitLab
			? `stages:\n  - test\n\ntest-job:\n  stage: test\n  script:\n    - echo "Hello from GitManager GitLab CI!"\n`
			: STARTER_GITHUB_WORKFLOW;
		setEditingInitial(starter);
		setEditingPath(relPath);
	}

	return (
		<div className="gm-actions-root">
			<div className="gm-panel-heading">
				<div>
					<h2>Actions &amp; Pipelines — {repo.name}</h2>
					<p>Trigger remote GitHub Actions (`gh`) or GitLab CI (`glab`) runs and edit workflow files directly in-app.</p>
				</div>
				<div className="gm-panel-actions">
					<Button onClick={createNewWorkflow}>+ New Workflow</Button>
					<Button onClick={() => void loadAll()} disabled={loading}>
						{loading ? "Refreshing…" : "↻ Refresh Runs"}
					</Button>
				</div>
			</div>

			{notice ? <div className="gm-inline-success">{notice}</div> : null}
			{error ? <div className="gm-inline-error">{error}</div> : null}
			{runsData?.note ? (
				<div className="gm-inline-warning">
					<span>{runsData.note}</span>
					{onOpenAccounts ? (
						<Button onClick={onOpenAccounts}>Open Accounts CLI Login</Button>
					) : null}
				</div>
			) : null}

			{editingPath ? (
				<WorkbenchEditor
					repoPath={repo.path}
					relativePath={editingPath}
					initialContent={editingInitial}
					onClose={() => {
						setEditingPath(null);
						setEditingInitial(undefined);
					}}
					onSaved={() => {
						onTriggerHappyLogo?.();
						void loadAll();
					}}
					onSaveAndRun={async (savedPath) => {
						const fileName = savedPath.split("/").pop() || savedPath;
						await runWorkflowNow(fileName, repo.current_branch);
					}}
				/>
			) : null}

			{dispatchTarget ? (
				<section className="gm-panel gm-dispatch-panel">
					<div className="gm-panel-heading">
						<div>
							<h3>Run Workflow: {dispatchTarget.name}</h3>
							<p className="gm-mono">{dispatchTarget.relative_path}</p>
						</div>
						<Button onClick={() => setDispatchTarget(null)}>Cancel</Button>
					</div>
					<div className="gm-form-grid">
						<label className="gm-field">
							<span>Target Branch</span>
							<input
								type="text"
								value={dispatchBranch}
								onChange={(event) => setDispatchBranch(event.target.value)}
							/>
						</label>
						{dispatchTarget.inputs.map((field) => (
							<label key={field.name} className="gm-field">
								<span>
									{field.name} {field.required ? <em>*</em> : null}
								</span>
								<input
									type="text"
									value={dispatchInputs[field.name] ?? ""}
									placeholder={field.description || field.default_value || ""}
									onChange={(event) =>
										setDispatchInputs((prev) => ({ ...prev, [field.name]: event.target.value }))
									}
								/>
							</label>
						))}
					</div>
					<div className="gm-panel-actions">
						<Button
							variant="primary"
							disabled={busy}
							onClick={() =>
								void runWorkflowNow(dispatchTarget.file_name, dispatchBranch, dispatchInputs)
							}
						>
							{busy ? "Triggering…" : "▶ Start Remote Run"}
						</Button>
					</div>
				</section>
			) : null}

			<div className="gm-actions-layout">
				<section className="gm-panel">
					<div className="gm-panel-heading">
						<div>
							<h3>Workflow Definitions</h3>
							<p>Detected local YAML workflows &amp; pipelines</p>
						</div>
						<span className="gm-count-chip">{workflows.length}</span>
					</div>

					{workflows.length === 0 ? (
						<EmptyState
							icon="▶"
							title="No workflow files found"
							body="Create a starter GitHub Actions or GitLab CI file right inside the Workbench Editor."
							action={
								<Button variant="primary" onClick={createNewWorkflow}>
									+ Create Starter Workflow
								</Button>
							}
						/>
					) : (
						<div className="gm-workflow-list">
							{workflows.map((wf) => (
								<div key={wf.relative_path} className="gm-workflow-row">
									<div>
										<strong>{wf.name}</strong>
										<div className="gm-mono gm-muted">{wf.relative_path}</div>
									</div>
									<div className="gm-workflow-row-actions">
										<span className="gm-badge">{wf.provider}</span>
										<Button
											onClick={() => {
												setEditingInitial(undefined);
												setEditingPath(wf.relative_path);
											}}
										>
											Edit
										</Button>
										{wf.supports_dispatch ? (
											<Button variant="primary" onClick={() => openDispatchForm(wf)}>
												▶ Run
											</Button>
										) : null}
									</div>
								</div>
							))}
						</div>
					)}
				</section>

				<section className="gm-panel">
					<div className="gm-panel-heading">
						<div>
							<h3>Recent Remote Runs</h3>
							<p>Live status via {runsData?.provider === "gitlab" ? "glab" : "gh"} CLI</p>
						</div>
						<span className="gm-count-chip">{runsData?.runs.length ?? 0}</span>
					</div>

					{!runsData || runsData.runs.length === 0 ? (
						<p className="gm-muted-copy" style={{ padding: "12px 15px" }}>
							{loading ? "Loading remote runs…" : "No recent remote runs found for this repository."}
						</p>
					) : (
						<div className="gm-workflow-list">
							{runsData.runs.map((run) => {
								const badgeClass =
									run.conclusion === "success"
										? "gm-connected-pill"
										: run.conclusion === "failure" || run.conclusion === "failed"
											? "gm-badge danger"
											: "gm-badge warn";
								return (
									<div key={run.id} className="gm-workflow-row">
										<div>
											<strong>{run.title || run.workflow_name}</strong>
											<div className="gm-mono gm-muted">
												{run.workflow_name} · {run.branch} · {run.sha}
											</div>
										</div>
										<div className="gm-workflow-row-actions">
											<span className={badgeClass}>{run.conclusion || run.status}</span>
											<Button onClick={() => void openRunLogs(run)}>Logs</Button>
											{run.url ? (
												<Button
													onClick={() =>
														void invoke("open_external_url", { url: run.url }).catch(() => undefined)
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

			{activeLogRun ? (
				<section className="gm-panel">
					<div className="gm-panel-heading">
						<div>
							<h3>
								Run Logs — {activeLogRun.workflow_name} (#{activeLogRun.id})
							</h3>
							<p>
								{activeLogRun.branch} · {activeLogRun.sha}
							</p>
						</div>
						<Button onClick={() => setActiveLogRun(null)}>Close Logs</Button>
					</div>
					<pre className="gm-log-viewer">{logLoading ? "Fetching logs…" : logText}</pre>
				</section>
			) : null}
		</div>
	);
}
