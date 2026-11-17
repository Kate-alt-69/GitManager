"use client";

import { useEffect, useState } from "react";
import { invoke, openDialog } from "../lib/tauri";
import type { AppPreferences, ApplicationDirectories } from "../lib/types";
import { Button, Field, Glyph, Switch } from "./shared-ui";

type SettingsTab = "general" | "cloud" | "about";

export function SettingsPage({
	preferences,
	directories,
	onSave,
	onScan,
	onOpenCleaner,
}: {
	preferences: AppPreferences;
	directories: ApplicationDirectories | null;
	onSave: (next: AppPreferences) => Promise<unknown>;
	onScan: () => void;
	onOpenCleaner: () => void;
}) {
	const [tab, setTab] = useState<SettingsTab>("general");
	const [draft, setDraft] = useState(preferences);
	useEffect(() => setDraft(preferences), [preferences]);
	const [newRoot, setNewRoot] = useState("");
	const [saving, setSaving] = useState(false);
	const [message, setMessage] = useState("");
	const [cloudJson, setCloudJson] = useState("");
	const [cloudInfo, setCloudInfo] = useState<Record<string, unknown> | null>(null);
	const [cloudStatus, setCloudStatus] = useState("");
	const [cloudNodeAddress, setCloudNodeAddress] = useState(preferences.cloudNodeAddress || "");
	const [cloudBusy, setCloudBusy] = useState(false);
	const [certificates, setCertificates] = useState<Array<{ thumbprint: string; subject: string; notAfter: string }>>([]);
	const [selectedThumbprint, setSelectedThumbprint] = useState("");
	const [installerPath, setInstallerPath] = useState("");
	const [signingStatus, setSigningStatus] = useState("Windows Authenticode signing is available when a valid CurrentUser Code Signing certificate is installed.");
	const [signingBusy, setSigningBusy] = useState(false);
	const [flatpakCli, setFlatpakCli] = useState<{ isFlatpak: boolean; installed: boolean; wrapperPath: string | null }>({ isFlatpak: false, installed: false, wrapperPath: null });
	const [flatpakCliBusy, setFlatpakCliBusy] = useState(false);
	const [flatpakCliMessage, setFlatpakCliMessage] = useState("");

	async function refreshFlatpakCli() {
		try {
			const status = await invoke<{ isFlatpak: boolean; installed: boolean; wrapperPath: string | null }>("get_flatpak_cli_integration_status");
			setFlatpakCli(status);
		} catch {
			setFlatpakCli({ isFlatpak: false, installed: false, wrapperPath: null });
		}
	}

	useEffect(() => {
		let active = true;
		void invoke<{ isFlatpak: boolean; installed: boolean; wrapperPath: string | null }>("get_flatpak_cli_integration_status")
			.then((status) => { if (active) setFlatpakCli(status); })
			.catch(() => undefined);
		return () => { active = false; };
	}, []);

	async function installFlatpakCli() {
		setFlatpakCliBusy(true);
		setFlatpakCliMessage("");
		try {
			const path = await invoke<string>("install_flatpak_gitm_cli");
			await refreshFlatpakCli();
			setFlatpakCliMessage("The gitm terminal command was installed at " + path + ". Make sure ~/.local/bin is in your PATH.");
		} catch (cause) {
			setFlatpakCliMessage(cause instanceof Error ? cause.message : String(cause));
		} finally {
			setFlatpakCliBusy(false);
		}
	}

	async function removeFlatpakCli() {
		setFlatpakCliBusy(true);
		setFlatpakCliMessage("");
		try {
			await invoke("remove_flatpak_gitm_cli");
			await refreshFlatpakCli();
			setFlatpakCliMessage("The GitManager-managed gitm wrapper was removed.");
		} catch (cause) {
			setFlatpakCliMessage(cause instanceof Error ? cause.message : String(cause));
		} finally {
			setFlatpakCliBusy(false);
		}
	}

	function patch(values: Partial<AppPreferences>) {
		setDraft((current) => ({ ...current, ...values }));
	}

	function addRoot() {
		const value = newRoot.trim();
		if (!value || draft.scanRoots.some((root) => root.toLowerCase() === value.toLowerCase())) return;
		patch({ scanRoots: [...draft.scanRoots, value] });
		setNewRoot("");
	}

	async function chooseScanRoot() {
		try {
			const selected = await openDialog({ title: "Choose a repository scan folder", directory: true, multiple: false });
			const path = Array.isArray(selected) ? selected[0] : selected;
			if (path) {
				patch({ scanRoots: draft.scanRoots.some((root) => root.toLowerCase() === path.toLowerCase()) ? draft.scanRoots : [...draft.scanRoots, path] });
			}
		} catch (cause) {
			setMessage(cause instanceof Error ? cause.message : String(cause));
		}
	}

	async function chooseArchiveRoot() {
		try {
			const selected = await openDialog({ title: "Choose an archive destination", directory: true, multiple: false });
			const path = Array.isArray(selected) ? selected[0] : selected;
			if (path) patch({ archiveRoot: path });
		} catch (cause) {
			setMessage(cause instanceof Error ? cause.message : String(cause));
		}
	}

	async function refreshCertificates() {
		setSigningBusy(true);
		setSigningStatus("Reading CurrentUser Code Signing certificates…");
		try {
			const found = await invoke<Array<{ thumbprint: string; subject: string; notAfter: string }>>("list_windows_signing_certificates");
			setCertificates(found ?? []);
			setSelectedThumbprint(found?.[0]?.thumbprint ?? "");
			setSigningStatus(found?.length
				? found.length + " usable Code Signing certificate(s) found."
				: "No valid Code Signing certificate with a private key was found in CurrentUser\\My.");
		} catch (cause) {
			setCertificates([]);
			setSelectedThumbprint("");
			setSigningStatus(cause instanceof Error ? cause.message : String(cause));
		} finally {
			setSigningBusy(false);
		}
	}

	async function chooseInstaller() {
		try {
			const selected = await openDialog({
				title: "Choose the GitManager Windows installer",
				directory: false,
				multiple: false,
				filters: [{ name: "Windows installer", extensions: ["exe", "msi"] }],
			});
			const path = Array.isArray(selected) ? selected[0] : selected;
			if (path) setInstallerPath(path);
		} catch (cause) {
			setSigningStatus(cause instanceof Error ? cause.message : String(cause));
		}
	}

	async function signInstaller() {
		if (!selectedThumbprint || !installerPath.trim()) {
			setSigningStatus("Choose a certificate and Windows .exe or .msi installer first.");
			return;
		}
		setSigningBusy(true);
		setSigningStatus("Creating a SHA-256 Authenticode signature and timestamp…");
		try {
			const result = await invoke<{ chainTrusted?: boolean }>("sign_windows_installer", {
				installerPath: installerPath.trim(),
				thumbprint: selectedThumbprint,
			});
			setSigningStatus(result.chainTrusted
				? "Signature created and trust chain verified. SmartScreen reputation may still build over time."
				: "Signature created, but Windows could not verify the trust chain. This may be a self-signed or untrusted certificate.");
		} catch (cause) {
			setSigningStatus("Signing failed: " + (cause instanceof Error ? cause.message : String(cause)));
		} finally {
			setSigningBusy(false);
		}
	}

	async function enableBackgroundService() {
		setBackgroundServiceBusy(true);
		setBackgroundServiceMessage("");
		try {
			await invoke("control_background_service", { installOsService: true });
			setBackgroundServiceMessage(
				"GitManager Core Service is registered to start at sign-in. GitManager will also launch it on demand whenever the desktop app starts. On Windows, find gitmanager.service.exe in Task Manager → Details."
			);
		} catch (cause) {
			setBackgroundServiceMessage(
				"Could not register or start the background worker: " +
				(cause instanceof Error ? cause.message : String(cause))
			);
		} finally {
			setBackgroundServiceBusy(false);
		}
	}

	async function save() {
		setSaving(true);
		setMessage("");
		try {
			await onSave(draft);
			setMessage("Settings saved. Repository indexing has been refreshed.");
			setDraft({ ...draft, firstRunComplete: true });
		} catch (cause) {
			setMessage(cause instanceof Error ? cause.message : String(cause));
		} finally {
			setSaving(false);
		}
	}

	async function refreshCloud() {
		setCloudBusy(true);
		try {
			const root = draft.archiveRoot || await invoke<string>("get_default_archive_directory");
			if (!draft.archiveRoot) patch({ archiveRoot: root });
			const result = await invoke<Record<string, unknown>>("get_archive_cloud_storage_info", { archiveRoot: root });
			setCloudInfo(result);
			setCloudStatus(result.configured ? `Connected · ${String(result.provider || "provider")}` : "No cloud provider configured. Local archives continue to work.");
		} catch (cause) {
			setCloudStatus(cause instanceof Error ? cause.message : String(cause));
		} finally {
			setCloudBusy(false);
		}
	}

	async function configureCloud() {
		setCloudBusy(true);
		try {
			const result = await invoke<Record<string, unknown>>("configure_archive_cloud_storage", {
				archiveRoot: draft.archiveRoot,
				providerJson: cloudJson,
			});
			setCloudInfo(result);
			setCloudStatus("Cloud provider configuration saved.");
		} catch (cause) {
			setCloudStatus(cause instanceof Error ? cause.message : String(cause));
		} finally {
			setCloudBusy(false);
		}
	}

	async function disableCloud() {
		if (!window.confirm("Disable cloud synchronization? Existing provider objects will not be deleted.")) return;
		setCloudBusy(true);
		try {
			const result = await invoke<Record<string, unknown>>("disable_archive_cloud_storage", { archiveRoot: draft.archiveRoot });
			setCloudInfo(result);
			setCloudStatus("Cloud synchronization disabled.");
		} catch (cause) {
			setCloudStatus(cause instanceof Error ? cause.message : String(cause));
		} finally {
			setCloudBusy(false);
		}
	}

	async function syncCloud() {
		setCloudBusy(true);
		try {
			const result = await invoke<string>("sync_archive_cloud_storage", { archiveRoot: draft.archiveRoot });
			setCloudStatus(`Synchronization completed · ${result}`);
		} catch (cause) {
			setCloudStatus(cause instanceof Error ? cause.message : String(cause));
		} finally {
			setCloudBusy(false);
		}
	}

	async function checkNode() {
		setCloudBusy(true);
		try {
			const result = await invoke<{ address: string; reachable: boolean; message: string }>("check_rbe_node_status", { address: cloudNodeAddress });
			const saved = await invoke<string>("save_cloud_node_address", { address: result.address || cloudNodeAddress });
			setCloudNodeAddress(saved);
			patch({ cloudNodeAddress: saved });
			setCloudStatus(result.message || (result.reachable ? "NAS backend reachable." : "NAS backend not confirmed reachable."));
		} catch (cause) {
			setCloudStatus(cause instanceof Error ? cause.message : String(cause));
		} finally {
			setCloudBusy(false);
		}
	}

	return (
		<div className="gm-page-content">
			<div className="gm-page-heading">
				<div><div className="gm-eyebrow">PREFERENCES & MAINTENANCE</div><h1>Settings</h1><p>Configure scanning, archive behaviour, cloud storage and application locations.</p></div>
				<Button variant="primary" onClick={() => void save()} disabled={saving}>{saving ? "Saving…" : "Save changes"}</Button>
			</div>
			<div className="gm-settings-tabs">
				{([
					["general", "General"],
					["cloud", "Cloud storage"],
					["about", "About"],
				] as [SettingsTab, string][]).map(([value, label]) => (
					<button key={value} className={tab === value ? "active" : ""} onClick={() => setTab(value)}>{label}</button>
				))}
			</div>

			{tab === "general" && (
				<div className="gm-settings-grid">
					{flatpakCli.isFlatpak && (
						<section className="gm-panel gm-settings-card">
							<div className="gm-panel-heading"><div><h2>Terminal integration</h2><p>Optionally expose GitManager's bundled gitm command to your Linux terminal.</p></div><span className="gm-panel-icon">⌘</span></div>
							<p className="gm-muted-copy">GitManager creates or removes only its own ~/.local/bin/gitm wrapper. It never edits .bashrc, .zshrc or your shell PATH. Ensure ~/.local/bin is already on your PATH.</p>
							{flatpakCli.wrapperPath && <p className="gm-muted-copy">Wrapper: {flatpakCli.wrapperPath}</p>}
							<div className="gm-add-root">
								<Button variant="primary" onClick={() => void installFlatpakCli()} disabled={flatpakCliBusy}>{flatpakCliBusy ? "Working…" : flatpakCli.installed ? "Refresh gitm command" : "Install gitm command"}</Button>
								{flatpakCli.installed && <Button variant="danger" onClick={() => void removeFlatpakCli()} disabled={flatpakCliBusy}>Remove wrapper</Button>}
							</div>
							{flatpakCliMessage && <p className="gm-muted-copy" role="status">{flatpakCliMessage}</p>}
						</section>
					)}
					<section className="gm-panel gm-settings-card">
						<div className="gm-panel-heading"><div><h2>Repository discovery</h2><p>Choose which directories GitManager scans for local repositories.</p></div><span className="gm-panel-icon">⌕</span></div>
						<div className="gm-root-list">
							{draft.scanRoots.map((root) => (
								<div className="gm-root-row" key={root}><Glyph>▰</Glyph><span title={root}>{root}</span><button onClick={() => patch({ scanRoots: draft.scanRoots.filter((item) => item !== root) })} aria-label={`Remove ${root}`}>×</button></div>
							))}
							{draft.scanRoots.length === 0 && <p className="gm-muted-copy">No scan folders configured. Add a directory below.</p>}
						</div>
						<div className="gm-add-root">
							<input value={newRoot} onChange={(event) => setNewRoot(event.target.value)} onKeyDown={(event) => { if (event.key === "Enter") addRoot(); }} placeholder="C:\\Users\\you\\Projects or /home/you/projects" />
							<Button onClick={addRoot}>Add path</Button>
							<Button onClick={() => void chooseScanRoot()}>Browse…</Button>
						</div>
						<div className="gm-form-grid">
							<Field label="Maximum scan depth" value={draft.maxScanDepth} onChange={(value) => patch({ maxScanDepth: Math.min(16, Math.max(2, Number(value) || 8)) })} type="number" />
						</div>
						<Switch label="Include hidden directories" checked={draft.includeHiddenDirs} onChange={(value) => patch({ includeHiddenDirs: value })} description="Include folders such as .config or hidden workspaces." />
						<Switch label="Index dependency repositories" checked={draft.indexDependencyRepositories} onChange={(value) => patch({ indexDependencyRepositories: value })} description="Only search node_modules, vendor and build folders when no regular repositories are found." />
						<Switch label="Show dependency repositories" checked={draft.showDependencyRepositories} onChange={(value) => patch({ showDependencyRepositories: value })} description="Include nested dependencies in the default repository list." />
						<div className="gm-panel-actions"><Button onClick={onScan}><Glyph>↻</Glyph> Scan now</Button></div>
					</section>

					<section className="gm-panel gm-settings-card">
						<div className="gm-panel-heading"><div><h2>Automatic archives</h2><p>Control restore-point capture and retention through GitManager's archive engine.</p></div><span className="gm-panel-icon">▤</span></div>
						<div className="gm-add-root">
							<input aria-label="Archive folder" value={draft.archiveRoot} onChange={(event) => patch({ archiveRoot: event.target.value })} placeholder="Default application archive directory" />
							<Button onClick={() => void chooseArchiveRoot()}>Browse…</Button>
						</div>
						<Switch label="Local archives" checked={draft.archiveLocalEnabled} onChange={(value) => patch({ archiveLocalEnabled: value })} description="Capture working-tree files, including uncommitted changes." />
						<Switch label="Remote archives" checked={draft.archiveRemoteEnabled} onChange={(value) => patch({ archiveRemoteEnabled: value })} description="Capture a self-contained archive of Git refs and history." />
						<label className="gm-field">
							<span>Capture trigger</span>
							<select value={draft.archiveTrigger} onChange={(event) => patch({ archiveTrigger: event.target.value })}>
								<option value="manual">Manual only</option>
								<option value="commit">After Git commits</option>
								<option value="change">After filesystem changes settle</option>
							</select>
						</label>
						<Field label="Quiet period (milliseconds)" value={draft.archiveQuietMs} onChange={(value) => patch({ archiveQuietMs: Math.min(60000, Math.max(250, Number(value) || 1500)) })} type="number" />
						<div className="gm-panel-actions"><Button onClick={() => void save()} variant="primary" disabled={saving}>Save archive settings</Button></div>
					</section>

					<section className="gm-panel gm-settings-card">
						<div className="gm-panel-heading"><div><h2>GitManager Core Service</h2><p>The required data engine for repository discovery, indexing and remote checks.</p></div><span className="gm-panel-icon">⚙</span></div>
						<p className="gm-muted-copy">The core service starts automatically when GitManager launches. Registering it here also starts it when you sign in, so indexing and background checks can continue before opening the desktop UI.</p>
						<p className="gm-muted-copy">The service runs as your current user. On Windows it uses Task Scheduler, not the machine-wide Services manager. Its process is named gitmanager.service.exe.</p>
						<div className="gm-panel-actions">
							<Button onClick={() => void invoke("check_repo_updates_now", { rootPaths: draft.scanRoots })}>Check remotes</Button>
							<Button variant="primary" onClick={() => void enableBackgroundService()} disabled={backgroundServiceBusy}>{backgroundServiceBusy ? "Registering…" : "Start service at sign-in"}</Button>
						</div>
						{backgroundServiceMessage && <p className="gm-muted-copy" role="status">{backgroundServiceMessage}</p>}
					</section>
					<section className="gm-panel gm-settings-card">
						<div className="gm-panel-heading"><div><h2>Notifications</h2><p>Configure where GitManager reports repository activity.</p></div><span className="gm-panel-icon">♢</span></div>
						<Switch label="In-app notifications" checked={draft.inAppNotificationsEnabled !== false} onChange={(value) => patch({ inAppNotificationsEnabled: value })} description="Keep activity in the top-right Notifications panel." />
						<Switch label="Operating system notifications" checked={draft.osNotificationsEnabled !== false} onChange={(value) => patch({ osNotificationsEnabled: value })} description="Show native desktop notifications when repository updates are found." />
					</section>

					<section className="gm-panel gm-settings-card">
						<div className="gm-panel-heading"><div><h2>Production signing</h2><p>Sign a Windows installer with a certificate installed for this Windows user.</p></div><span className="gm-panel-icon">✓</span></div>
						<label className="gm-field">
							<span>Code Signing certificate</span>
							<select value={selectedThumbprint} onChange={(event) => setSelectedThumbprint(event.target.value)} disabled={signingBusy || certificates.length === 0}>
								<option value="">Choose a certificate…</option>
								{certificates.map((certificate) => <option key={certificate.thumbprint} value={certificate.thumbprint}>{certificate.subject} · expires {certificate.notAfter}</option>)}
							</select>
						</label>
						<div className="gm-add-root">
							<input value={installerPath} onChange={(event) => setInstallerPath(event.target.value)} placeholder="Choose a GitManager-Setup.exe or .msi installer…" />
							<Button onClick={() => void chooseInstaller()} disabled={signingBusy}>Choose file…</Button>
						</div>
						<div className="gm-panel-actions">
							<Button onClick={() => void refreshCertificates()} disabled={signingBusy}>Refresh certificates</Button>
							<Button variant="primary" onClick={() => void signInstaller()} disabled={signingBusy || !selectedThumbprint || !installerPath.trim()}>Sign installer</Button>
						</div>
						<p className="gm-muted-copy" style={{ padding: "0 15px" }}>{signingStatus}</p>
						<p className="gm-muted-copy" style={{ padding: "0 15px" }}>The signing operation runs in the native Rust backend. The certificate's private key never leaves the Windows Certificate Store.</p>
					</section>

					<section className="gm-panel gm-settings-card">
						<div className="gm-panel-heading"><div><h2>Developer tools</h2><p>Preview generated files and developer caches before cleaning them.</p></div><span className="gm-panel-icon">⌁</span></div>
						<p className="gm-muted-copy">Nothing is removed until you inspect a cleanup preview and explicitly confirm the action.</p>
						<div className="gm-panel-actions"><Button onClick={onOpenCleaner}>Open Developer Cleaner</Button></div>
					</section>
				</div>
			)}

			{tab === "cloud" && (
				<div className="gm-settings-grid">
					<section className="gm-panel gm-settings-card">
						<div className="gm-panel-heading"><div><h2>Object storage</h2><p>Configure an archive provider supported by the RBE Cloud Node storage engine.</p></div><span className="gm-panel-icon">☁</span></div>
						<Field label="Archive folder" value={draft.archiveRoot} onChange={(value) => patch({ archiveRoot: value })} />
						<label className="gm-field gm-field-wide">
							<span>Provider configuration (JSON)</span>
							<textarea
								rows={9}
								value={cloudJson}
								onChange={(event) => setCloudJson(event.target.value)}
								placeholder={'Paste a provider settings JSON document. Credentials should be supplied through environment variables, not embedded here.'}
							/>
						</label>
						{cloudStatus && <div className="gm-inline-status">{cloudStatus}</div>}
						<div className="gm-panel-actions">
							<Button onClick={() => void refreshCloud()} disabled={cloudBusy}>Refresh status</Button>
							<Button onClick={() => void syncCloud()} disabled={cloudBusy || !Boolean(cloudInfo?.configured)}>Sync now</Button>
							<Button variant="primary" onClick={() => void configureCloud()} disabled={cloudBusy || !cloudJson.trim()}>Save provider</Button>
							<Button variant="danger" onClick={() => void disableCloud()} disabled={cloudBusy || !Boolean(cloudInfo?.configured)}>Disable</Button>
						</div>
					</section>
					<section className="gm-panel gm-settings-card">
						<div className="gm-panel-heading"><div><h2>RBE Cloud Node</h2><p>Check the health endpoints on a NAS or another computer on your LAN.</p></div><span className="gm-panel-icon">⌂</span></div>
						<Field label="NAS address" value={cloudNodeAddress} onChange={(value) => setCloudNodeAddress(value)} placeholder="192.168.1.20:8080" />
						<p className="gm-muted-copy">This checks backend health and saves the address. It does not establish peer authentication or pairing.</p>
						<div className="gm-panel-actions"><Button variant="primary" onClick={() => void checkNode()} disabled={cloudBusy}>Check NAS health</Button></div>
						{cloudStatus && <div className="gm-inline-status">{cloudStatus}</div>}
					</section>
				</div>
			)}

			{tab === "about" && (
				<div className="gm-panel gm-about-panel">
					<div className="gm-about-mark">GM</div>
					<h2>GitManager</h2>
					<p>A native Git workspace manager powered by Rust, Tauri, React and Next.js Engine.</p>
					<div className="gm-about-paths">
						<div><span>Application install</span><code>{directories?.installDir || "Unavailable"}</code></div>
						<div><span>Software root</span><code>{directories?.softwareRoot || "Unavailable"}</code></div>
						<div><span>Configuration</span><code>{directories?.configDir || "Unavailable"}</code></div>
						<div><span>Application data</span><code>{directories?.dataDir || "Unavailable"}</code></div>
					</div>
				</div>
			)}
			{message && <div className="gm-inline-status">{message}</div>}
		</div>
	);
}
