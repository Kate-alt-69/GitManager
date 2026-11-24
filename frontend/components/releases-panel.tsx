"use client";

import { useCallback, useEffect, useState } from "react";
import { invoke, openDialog } from "../lib/tauri";
import type { GitRepository } from "../lib/types";
import { Button, EmptyState, Switch } from "./shared-ui";

interface RepoReleaseEntry {
	tag_name: string;
	name: string;
	is_draft: boolean;
	is_prerelease: boolean;
	published_at: string;
	url: string;
}

interface RepoReleasesResponse {
	provider: string;
	note: string;
	releases: RepoReleaseEntry[];
}

/**
 * Releases & Tag Builder panel for GitHub Releases (`gh`) and GitLab Releases (`glab`).
 * Includes one-click changelog auto-generation from commits since the last tag
 * and optional build asset attachment.
 */
export function ReleasesPanel({
	repo,
	onOpenAccounts,
	onTriggerHappyLogo,
}: {
	repo: GitRepository;
	onOpenAccounts?: () => void;
	onTriggerHappyLogo?: () => void;
}) {
	const [data, setData] = useState<RepoReleasesResponse | null>(null);
	const [loading, setLoading] = useState(true);
	const [creating, setCreating] = useState(false);
	const [tagName, setTagName] = useState("");
	const [title, setTitle] = useState("");
	const [notes, setNotes] = useState("");
	const [isDraft, setIsDraft] = useState(false);
	const [isPrerelease, setIsPrerelease] = useState(false);
	const [assetPath, setAssetPath] = useState("");
	const [busy, setBusy] = useState(false);
	const [notice, setNotice] = useState("");
	const [error, setError] = useState("");

	const loadReleases = useCallback(async () => {
		setLoading(true);
		setError("");
		try {
			const resp = await invoke<RepoReleasesResponse>("list_repo_releases", { repoPath: repo.path });
			setData(resp);
		} catch (cause) {
			setError(cause instanceof Error ? cause.message : String(cause));
		} finally {
			setLoading(false);
		}
	}, [repo.path]);

	useEffect(() => {
		void loadReleases();
	}, [loadReleases]);

	async function handleAutoNotes() {
		setBusy(true);
		setError("");
		try {
			const generated = await invoke<string>("generate_release_notes", { repoPath: repo.path });
			setNotes(generated);
		} catch (cause) {
			setError(cause instanceof Error ? cause.message : String(cause));
		} finally {
			setBusy(false);
		}
	}

	async function handlePickAsset() {
		try {
			const chosen = await openDialog({ title: "Select release asset file", multiple: false });
			const path = Array.isArray(chosen) ? chosen[0] : chosen;
			if (path) setAssetPath(path);
		} catch {
			// User cancelled file dialog
		}
	}

	async function handleCreateRelease() {
		setBusy(true);
		setNotice("");
		setError("");
		try {
			const msg = await invoke<string>("create_repo_release", {
				repoPath: repo.path,
				tagName,
				title,
				notes,
				isDraft,
				isPrerelease,
				assetPath: assetPath.trim() || null,
			});
			setNotice(msg);
			setTagName("");
			setTitle("");
			setNotes("");
			setAssetPath("");
			setCreating(false);
			onTriggerHappyLogo?.();
			await loadReleases();
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
					<h2>Releases &amp; Tags — {repo.name}</h2>
					<p>Publish versioned releases with auto-generated changelogs and build artifacts.</p>
				</div>
				<div className="gm-panel-actions">
					<Button variant="primary" onClick={() => setCreating((v) => !v)}>
						{creating ? "Cancel" : "+ New Release"}
					</Button>
					<Button onClick={() => void loadReleases()} disabled={loading}>
						{loading ? "Refreshing…" : "↻ Refresh"}
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
					<div className="gm-panel-heading">
						<div>
							<h3>Create New Release</h3>
							<p>Tag a commit and publish release notes via {data?.provider === "gitlab" ? "glab" : "gh"}.</p>
						</div>
						<Button onClick={() => void handleAutoNotes()} disabled={busy}>
							✨ Auto-Generate Notes from Commits
						</Button>
					</div>
					<div className="gm-form-grid">
						<label className="gm-field">
							<span>Tag Name</span>
							<input
								type="text"
								value={tagName}
								onChange={(event) => setTagName(event.target.value)}
								placeholder="v1.0.0"
							/>
						</label>
						<label className="gm-field">
							<span>Release Title</span>
							<input
								type="text"
								value={title}
								onChange={(event) => setTitle(event.target.value)}
								placeholder="Optional (defaults to tag name)"
							/>
						</label>
						<label className="gm-field gm-field-wide">
							<span>Release Notes (Markdown)</span>
							<textarea
								rows={6}
								className="gm-code-textarea"
								style={{ minHeight: 130, border: "1px solid var(--gm-border, #2c313a)", borderRadius: 8 }}
								value={notes}
								onChange={(event) => setNotes(event.target.value)}
								placeholder="Write release notes or click Auto-Generate Notes…"
							/>
						</label>
						<label className="gm-field gm-field-wide">
							<span>Optional Binary / Archive Asset</span>
							<div style={{ display: "flex", gap: 8 }}>
								<input
									type="text"
									value={assetPath}
									onChange={(event) => setAssetPath(event.target.value)}
									placeholder="Path to installer or zip asset…"
								/>
								<Button onClick={() => void handlePickAsset()}>Browse…</Button>
							</div>
						</label>
					</div>
					<div className="gm-panel-actions" style={{ justifyContent: "space-between", flexWrap: "wrap" }}>
						<div style={{ display: "flex", gap: 16 }}>
							<Switch checked={isDraft} onChange={setIsDraft} label="Save as draft" />
							<Switch checked={isPrerelease} onChange={setIsPrerelease} label="Pre-release" />
						</div>
						<Button
							variant="primary"
							disabled={busy || !tagName.trim()}
							onClick={() => void handleCreateRelease()}
						>
							{busy ? "Publishing…" : "Publish Release"}
						</Button>
					</div>
				</section>
			) : null}

			<section className="gm-panel">
				<div className="gm-panel-heading">
					<div>
						<h3>Published Releases</h3>
						<p>Tags and releases reported by {data?.provider === "gitlab" ? "GitLab" : "GitHub"}</p>
					</div>
					<span className="gm-count-chip">{data?.releases.length ?? 0}</span>
				</div>
				{!data || data.releases.length === 0 ? (
					<EmptyState
						icon="🏷"
						title="No releases yet"
						body="Create your first tagged release with auto-generated notes from your commit history."
						action={
							<Button variant="primary" onClick={() => setCreating(true)}>
								+ Create Release
							</Button>
						}
					/>
				) : (
					<div className="gm-workflow-list">
						{data.releases.map((rel) => (
							<div key={rel.tag_name + rel.name} className="gm-workflow-row">
								<div>
									<strong>{rel.name || rel.tag_name}</strong>
									<div className="gm-mono gm-muted">
										{rel.tag_name}
										{rel.published_at ? ` · ${rel.published_at}` : ""}
									</div>
								</div>
								<div className="gm-workflow-row-actions">
									{rel.is_draft ? <span className="gm-badge warn">Draft</span> : null}
									{rel.is_prerelease ? <span className="gm-badge info">Pre-release</span> : null}
									{!rel.is_draft && !rel.is_prerelease ? (
										<span className="gm-connected-pill">Published</span>
									) : null}
								</div>
							</div>
						))}
					</div>
				)}
			</section>
		</div>
	);
}
