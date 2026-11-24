"use client";

import { RingLoadingIndicator } from "./repo-ring-badge";

import { useEffect, useMemo, useState } from "react";
import { invoke } from "../lib/tauri";
import { Button } from "./shared-ui";

const ACTION_SNIPPETS: Array<{ label: string; provider: "github" | "gitlab"; body: string }> = [
	{
		label: "Manual Trigger (workflow_dispatch)",
		provider: "github",
		body: `on:\n  workflow_dispatch:\n    inputs:\n      environment:\n        description: "Target environment"\n        required: false\n        default: "staging"\n`,
	},
	{
		label: "Rust Build & Test Job",
		provider: "github",
		body: `  rust-check:\n    runs-on: ubuntu-latest\n    steps:\n      - uses: actions/checkout@v4\n      - uses: dtolnay/rust-toolchain@stable\n      - run: cargo test --workspace\n`,
	},
	{
		label: "Node.js Build Job",
		provider: "github",
		body: `  web-build:\n    runs-on: ubuntu-latest\n    steps:\n      - uses: actions/checkout@v4\n      - uses: actions/setup-node@v4\n        with:\n          node-version: 22\n      - run: npm ci && npm run build\n`,
	},
	{
		label: "GitLab CI Stage Template",
		provider: "gitlab",
		body: `stages:\n  - test\n  - build\n\ntest-job:\n  stage: test\n  script:\n    - echo "Running tests"\n`,
	},
];

function isWorkflowFile(relativePath: string): boolean {
	const norm = relativePath.replace(/\\/g, "/").toLowerCase();
	return (
		norm.startsWith(".github/workflows/") ||
		norm.startsWith(".forgejo/workflows/") ||
		norm.startsWith(".gitea/workflows/") ||
		norm === ".gitlab-ci.yml"
	);
}

function validateWorkflowYaml(content: string, relativePath: string): string[] {
	const warnings: string[] = [];
	const lines = content.split("\n");
	lines.forEach((line, idx) => {
		if (line.includes("\t")) {
			warnings.push(`Line ${idx + 1}: YAML forbids raw tab characters — use spaces.`);
		}
	});
	const norm = relativePath.replace(/\\/g, "/").toLowerCase();
	if (norm === ".gitlab-ci.yml") {
		if (!/^\s*stages\s*:/m.test(content) && !/^\s*script\s*:/m.test(content)) {
			warnings.push("GitLab CI files usually define `stages:` or at least one job with `script:`.");
		}
	} else {
		if (!/^\s*on\s*:/m.test(content)) {
			warnings.push("Missing top-level `on:` trigger block.");
		}
		if (!/^\s*jobs\s*:/m.test(content)) {
			warnings.push("Missing top-level `jobs:` block.");
		}
	}
	return warnings.slice(0, 6);
}

/**
 * General-Purpose In-App Code Editor (`WorkbenchEditor`).
 * Works on any file inside a repository with line numbers, Find bar,
 * Diff Preview vs HEAD, and safe write-back. Automatically enables
 * Actions Mode (YAML validator, workflow snippets, Save & Run) when
 * editing `.github/workflows/*.yml`, `.gitlab-ci.yml`, or Forgejo/Gitea workflows.
 */
export function WorkbenchEditor({
	repoPath,
	relativePath,
	initialContent,
	onClose,
	onSaved,
	onSaveAndRun,
}: {
	repoPath: string;
	relativePath: string;
	initialContent?: string;
	onClose?: () => void;
	onSaved?: (relativePath: string) => void;
	onSaveAndRun?: (relativePath: string) => Promise<void> | void;
}) {
	const [content, setContent] = useState(initialContent ?? "");
	const [savedContent, setSavedContent] = useState(initialContent ?? "");
	const [loading, setLoading] = useState(initialContent === undefined);
	const [saving, setSaving] = useState(false);
	const [showDiff, setShowDiff] = useState(false);
	const [diffText, setDiffText] = useState("");
	const [findQuery, setFindQuery] = useState("");
	const [notice, setNotice] = useState("");
	const [error, setError] = useState("");

	const actionsMode = useMemo(() => isWorkflowFile(relativePath), [relativePath]);
	const yamlWarnings = useMemo(
		() => (actionsMode ? validateWorkflowYaml(content, relativePath) : []),
		[actionsMode, content, relativePath],
	);
	const isDirty = content !== savedContent;
	const lineCount = useMemo(() => Math.max(1, content.split("\n").length), [content]);

	useEffect(() => {
		if (initialContent !== undefined) {
			setContent(initialContent);
			setSavedContent(initialContent);
			setLoading(false);
			return;
		}
		let active = true;
		setLoading(true);
		setError("");
		void invoke<string>("read_repository_file", {
			repoPath,
			relativePath,
		})
			.then((loaded) => {
				if (!active) return;
				setContent(loaded);
				setSavedContent(loaded);
			})
			.catch((cause) => {
				if (!active) return;
				setError(cause instanceof Error ? cause.message : String(cause));
			})
			.finally(() => {
				if (active) setLoading(false);
			});
		return () => {
			active = false;
		};
	}, [initialContent, relativePath, repoPath]);

	async function handleSave(): Promise<boolean> {
		setSaving(true);
		setNotice("");
		setError("");
		try {
			const msg = await invoke<string>("write_repository_file", {
				repoPath,
				relativePath,
				content,
			});
			setSavedContent(content);
			setNotice(msg);
			onSaved?.(relativePath);
			return true;
		} catch (cause) {
			setError(cause instanceof Error ? cause.message : String(cause));
			return false;
		} finally {
			setSaving(false);
		}
	}

	async function handleToggleDiff() {
		if (showDiff) {
			setShowDiff(false);
			return;
		}
		try {
			const rawDiff = await invoke<string>("read_repository_file_diff", {
				repoPath,
				relativePath,
				staged: false,
			});
			setDiffText(rawDiff || "No unstaged diff against HEAD yet (save your edits first).");
			setShowDiff(true);
		} catch (cause) {
			setError(cause instanceof Error ? cause.message : String(cause));
		}
	}

	function insertSnippet(snippet: string) {
		setContent((current) => (current.endsWith("\n") || !current ? `${current}${snippet}` : `${current}\n${snippet}`));
	}

	return (
		<div className="gm-workbench-editor">
			<div className="gm-editor-toolbar">
				<div className="gm-editor-file-meta">
					<code className="gm-mono">{relativePath}</code>
					{actionsMode ? <span className="gm-badge info">Actions Mode</span> : <span className="gm-badge">General Editor</span>}
					{isDirty ? <span className="gm-badge warn">Unsaved</span> : <span className="gm-muted">Saved</span>}
				</div>
				<div className="gm-editor-toolbar-actions">
					<input
						type="search"
						className="gm-input gm-editor-find"
						value={findQuery}
						onChange={(event) => setFindQuery(event.target.value)}
						placeholder="Find in file…"
						aria-label="Find in file"
					/>
					<Button onClick={() => void handleToggleDiff()}>
						{showDiff ? "Edit File" : "Diff vs HEAD"}
					</Button>
					<Button variant="primary" disabled={saving || loading} onClick={() => void handleSave()}>
						{saving ? "Saving…" : "Save File"}
					</Button>
					{actionsMode && onSaveAndRun ? (
						<Button
							disabled={saving || loading}
							onClick={async () => {
								const ok = await handleSave();
								if (ok) await onSaveAndRun(relativePath);
							}}
						>
							▶ Save &amp; Run
						</Button>
					) : null}
					{onClose ? <Button onClick={onClose}>Close</Button> : null}
				</div>
			</div>

			{notice ? <div className="gm-inline-success">{notice}</div> : null}
			{error ? <div className="gm-inline-error">{error}</div> : null}

			{findQuery.trim() ? (
				<div className="gm-muted-copy" style={{ padding: "4px 12px" }}>
					Matches for <code>{findQuery}</code>:{" "}
					{content.toLowerCase().split(findQuery.trim().toLowerCase()).length - 1}
				</div>
			) : null}

			<div className={actionsMode ? "gm-editor-split" : "gm-editor-single"}>
				<div className="gm-editor-pane">
					{loading ? (
						<RingLoadingIndicator label={`Loading ${relativePath}…`} />
					) : showDiff ? (
						<pre className="gm-log-viewer">{diffText}</pre>
					) : (
						<div className="gm-code-surface">
							<pre className="gm-line-gutter" aria-hidden="true">
								{Array.from({ length: lineCount }, (_, idx) => idx + 1).join("\n")}
							</pre>
							<textarea
								className="gm-code-textarea"
								value={content}
								spellCheck={false}
								onChange={(event) => setContent(event.target.value)}
								onKeyDown={(event) => {
									if (event.key === "Tab") {
										event.preventDefault();
										const el = event.currentTarget;
										const start = el.selectionStart;
										const end = el.selectionEnd;
										const indent = actionsMode ? "  " : "\t";
										const next = `${content.slice(0, start)}${indent}${content.slice(end)}`;
										setContent(next);
										requestAnimationFrame(() => {
											el.selectionStart = el.selectionEnd = start + indent.length;
										});
									}
									if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "s") {
										event.preventDefault();
										void handleSave();
									}
								}}
							/>
						</div>
					)}
				</div>

				{actionsMode ? (
					<aside className="gm-editor-helper">
						<h3>Workflow Helper</h3>
						<p className="gm-muted-copy">
							YAML uses spaces for indentation (Tab inserts 2 spaces automatically here).
						</p>

						{yamlWarnings.length > 0 ? (
							<div className="gm-yaml-warnings">
								<strong>YAML Checks ({yamlWarnings.length})</strong>
								<ul>
									{yamlWarnings.map((warning) => (
										<li key={warning}>{warning}</li>
									))}
								</ul>
							</div>
						) : (
							<div className="gm-inline-success">✓ Basic YAML structure looks clean</div>
						)}

						<div className="gm-snippet-list">
							<strong>Quick-Insert Snippets</strong>
							{ACTION_SNIPPETS.map((snippet) => (
								<button
									key={snippet.label}
									type="button"
									className="gm-snippet-btn"
									onClick={() => insertSnippet(snippet.body)}
								>
									<span>+ {snippet.label}</span>
									<small>{snippet.provider}</small>
								</button>
							))}
						</div>
					</aside>
				) : null}
			</div>
		</div>
	);
}
