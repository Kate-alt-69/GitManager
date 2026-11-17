"use client";

import { useState } from "react";
import { invoke } from "../lib/tauri";
import type { ConnectedAccount } from "../lib/types";
import { Button, EmptyState, Field, ProviderMark, RepoAvatar } from "./shared-ui";

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
	const [backgroundServiceBusy, setBackgroundServiceBusy] = useState(false);
	const [backgroundServiceMessage, setBackgroundServiceMessage] = useState("");

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
