"use client";

import { useCallback, useEffect, useState } from "react";
import { invoke } from "../lib/tauri";
import type { GitRepository } from "../lib/types";
import { Button, EmptyState, Switch } from "./shared-ui";

interface CiVariableEntry {
	name: string;
	kind: string;
	scope: string;
	updated_at: string;
	value_preview: string;
}

interface CiVariablesResponse {
	provider: string;
	note: string;
	items: CiVariableEntry[];
}

/**
 * CI Secrets & Variables Manager for GitHub Actions and GitLab CI.
 * Secret values stay masked; new values are piped over stdin directly to `gh`/`glab`.
 */
export function VariablesPanel({
	repo,
	onOpenAccounts,
	onTriggerHappyLogo,
}: {
	repo: GitRepository;
	onOpenAccounts?: () => void;
	onTriggerHappyLogo?: () => void;
}) {
	const [data, setData] = useState<CiVariablesResponse | null>(null);
	const [loading, setLoading] = useState(true);
	const [name, setName] = useState("");
	const [value, setValue] = useState("");
	const [isSecret, setIsSecret] = useState(true);
	const [busy, setBusy] = useState(false);
	const [notice, setNotice] = useState("");
	const [error, setError] = useState("");

	const loadItems = useCallback(async () => {
		setLoading(true);
		setError("");
		try {
			const resp = await invoke<CiVariablesResponse>("list_ci_variables", { repoPath: repo.path });
			setData(resp);
		} catch (cause) {
			setError(cause instanceof Error ? cause.message : String(cause));
		} finally {
			setLoading(false);
		}
	}, [repo.path]);

	useEffect(() => {
		void loadItems();
	}, [loadItems]);

	async function handleSaveVariable() {
		setBusy(true);
		setNotice("");
		setError("");
		try {
			const msg = await invoke<string>("set_ci_variable", {
				repoPath: repo.path,
				name,
				value,
				isSecret,
			});
			setNotice(msg);
			setName("");
			setValue("");
			onTriggerHappyLogo?.();
			await loadItems();
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
					<h2>CI Secrets &amp; Variables — {repo.name}</h2>
					<p>Inspect configured variable keys and safely add or rotate masked secrets via stdin.</p>
				</div>
				<Button onClick={() => void loadItems()} disabled={loading}>
					{loading ? "Refreshing…" : "↻ Refresh"}
				</Button>
			</div>

			{notice ? <div className="gm-inline-success">{notice}</div> : null}
			{error ? <div className="gm-inline-error">{error}</div> : null}
			{data?.note ? (
				<div className="gm-inline-warning">
					<span>{data.note}</span>
					{onOpenAccounts ? <Button onClick={onOpenAccounts}>Open Accounts</Button> : null}
				</div>
			) : null}

			<section className="gm-panel">
				<div className="gm-panel-heading">
					<div>
						<h3>Add or Update Key</h3>
						<p>Values are piped over stdin directly to {data?.provider === "gitlab" ? "glab" : "gh"} and never stored in GitManager.</p>
					</div>
				</div>
				<div className="gm-form-grid">
					<label className="gm-field">
						<span>Key Name</span>
						<input
							type="text"
							value={name}
							onChange={(event) => setName(event.target.value.toUpperCase())}
							placeholder="DEPLOY_TOKEN"
						/>
					</label>
					<label className="gm-field">
						<span>Value</span>
						<input
							type={isSecret ? "password" : "text"}
							value={value}
							onChange={(event) => setValue(event.target.value)}
							placeholder={isSecret ? "Masked secret value…" : "Plain variable value…"}
							autoComplete="off"
						/>
					</label>
				</div>
				<div className="gm-panel-actions" style={{ justifyContent: "space-between", flexWrap: "wrap" }}>
					<Switch checked={isSecret} onChange={setIsSecret} label="Store as masked secret" />
					<Button
						variant="primary"
						disabled={busy || !name.trim() || !value}
						onClick={() => void handleSaveVariable()}
					>
						{busy ? "Saving…" : isSecret ? "Save Secret" : "Save Variable"}
					</Button>
				</div>
			</section>

			<section className="gm-panel">
				<div className="gm-panel-heading">
					<div>
						<h3>Configured Keys</h3>
						<p>Repository secrets &amp; environment variables</p>
					</div>
					<span className="gm-count-chip">{data?.items.length ?? 0}</span>
				</div>
				{!data || data.items.length === 0 ? (
					<EmptyState
						icon="🔑"
						title={loading ? "Loading keys…" : "No secrets or variables listed"}
						body="Add a new secret or variable above to configure CI/CD environments."
					/>
				) : (
					<div className="gm-workflow-list">
						{data.items.map((item) => (
							<div key={item.kind + item.name + item.scope} className="gm-workflow-row">
								<div>
									<strong className="gm-mono">{item.name}</strong>
									<div className="gm-mono gm-muted">
										{item.value_preview} · scope: {item.scope}
									</div>
								</div>
								<span className={item.kind === "secret" ? "gm-badge warn" : "gm-badge info"}>
									{item.kind}
								</span>
							</div>
						))}
					</div>
				)}
			</section>
		</div>
	);
}
