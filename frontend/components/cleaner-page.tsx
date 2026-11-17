"use client";

import { useEffect, useState } from "react";
import { invoke } from "../lib/tauri";
import type { GitRepository } from "../lib/types";
import { Button, EmptyState, Field, Switch } from "./shared-ui";

type CleanerScope = "repository" | "system" | "remote";

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
