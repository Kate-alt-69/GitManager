"use client";

import { useCallback, useEffect, useState } from "react";
import { invoke } from "../lib/tauri";
import { Button } from "./shared-ui";

export interface ProviderCliStatus {
	provider: string;
	binary: string;
	installed: boolean;
	version: string;
	authenticated: boolean;
	account_summary: string;
}

/**
 * Official GitHub CLI (`gh`) and GitLab CLI (`glab`) Authentication Card.
 * Lets users sign in via browser OAuth (`--web`) or Personal Access Token
 * piped safely over stdin into the OS keyring — no browser cookies required.
 */
export function CliAuthCard({ onAuthChanged }: { onAuthChanged?: () => void }) {
	const [statuses, setStatuses] = useState<ProviderCliStatus[]>([]);
	const [loading, setLoading] = useState(true);
	const [activeTokenProvider, setActiveTokenProvider] = useState<string | null>(null);
	const [hostname, setHostname] = useState("");
	const [token, setToken] = useState("");
	const [busy, setBusy] = useState(false);
	const [notice, setNotice] = useState("");
	const [error, setError] = useState("");

	const refresh = useCallback(async () => {
		setLoading(true);
		try {
			const next = await invoke<ProviderCliStatus[]>("get_cli_auth_status");
			setStatuses(next);
		} catch (cause) {
			setError(cause instanceof Error ? cause.message : String(cause));
		} finally {
			setLoading(false);
		}
	}, []);

	useEffect(() => {
		void refresh();
	}, [refresh]);

	async function startWebLogin(provider: string) {
		setBusy(true);
		setNotice("");
		setError("");
		try {
			const message = await invoke<string>("cli_auth_start_web_login", {
				provider,
				hostname: hostname.trim() || null,
			});
			setNotice(message);
		} catch (cause) {
			setError(cause instanceof Error ? cause.message : String(cause));
		} finally {
			setBusy(false);
		}
	}

	async function submitTokenLogin(provider: string) {
		setBusy(true);
		setNotice("");
		setError("");
		try {
			const message = await invoke<string>("cli_auth_login_token", {
				provider,
				hostname: hostname.trim() || null,
				token,
			});
			setToken("");
			setActiveTokenProvider(null);
			setNotice(message);
			await refresh();
			onAuthChanged?.();
		} catch (cause) {
			setError(cause instanceof Error ? cause.message : String(cause));
		} finally {
			setBusy(false);
		}
	}

	return (
		<section className="gm-panel gm-cli-auth-panel">
			<div className="gm-panel-heading">
				<div>
					<h2>Official Provider CLIs (<code>gh</code> &amp; <code>glab</code>)</h2>
					<p>Sign in with browser OAuth or an Access Token stored in your OS keyring — powers Actions, Pipelines, Releases, and remote workflows without cookies.</p>
				</div>
				<Button onClick={() => void refresh()} disabled={loading || busy}>
					{loading ? "Checking…" : "↻ Refresh CLI Status"}
				</Button>
			</div>

			{notice ? <div className="gm-inline-success">{notice}</div> : null}
			{error ? <div className="gm-inline-error">{error}</div> : null}

			<div className="gm-cli-auth-grid">
				{statuses.map((item) => {
					const label = item.provider === "github" ? "GitHub CLI (gh)" : "GitLab CLI (glab)";
					const defaultHost = item.provider === "github" ? "github.com" : "gitlab.com";
					const tokenOpen = activeTokenProvider === item.provider;

					return (
						<div key={item.provider} className="gm-cli-card">
							<div className="gm-cli-card-head">
								<div>
									<strong>{label}</strong>
									<small className="gm-mono gm-muted">
										{item.installed ? item.version : `Install \`${item.binary}\` to enable`}
									</small>
								</div>
								<span
									className={
										item.authenticated
											? "gm-connected-pill"
											: item.installed
												? "gm-badge warn"
												: "gm-badge"
									}
								>
									{item.authenticated
										? "Signed in"
										: item.installed
											? "Not signed in"
											: "Not installed"}
								</span>
							</div>

							<p className="gm-muted-copy">{item.account_summary}</p>

							<div className="gm-panel-actions">
								<Button
									variant="primary"
									disabled={!item.installed || busy}
									onClick={() => void startWebLogin(item.provider)}
								>
									Login via Browser
								</Button>
								<Button
									disabled={!item.installed || busy}
									onClick={() => {
										setActiveTokenProvider(tokenOpen ? null : item.provider);
										setHostname(defaultHost);
										setToken("");
									}}
								>
									{tokenOpen ? "Cancel Token" : "Login with Access Token"}
								</Button>
							</div>

							{tokenOpen ? (
								<div className="gm-cli-token-form">
									<label className="gm-field">
										<span>Hostname</span>
										<input
											type="text"
											value={hostname}
											onChange={(event) => setHostname(event.target.value)}
											placeholder={defaultHost}
										/>
									</label>
									<label className="gm-field">
										<span>Personal Access Token (piped via stdin to OS keyring)</span>
										<input
											type="password"
											value={token}
											onChange={(event) => setToken(event.target.value)}
											placeholder={`Paste ${item.provider} token…`}
											autoComplete="off"
										/>
									</label>
									<Button
										variant="primary"
										disabled={busy || !token.trim()}
										onClick={() => void submitTokenLogin(item.provider)}
									>
										{busy ? "Signing in…" : `Authenticate ${item.binary}`}
									</Button>
								</div>
							) : null}
						</div>
					);
				})}
			</div>
		</section>
	);
}
