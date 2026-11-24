"use client";

import { useState } from "react";
import { invoke } from "../lib/tauri";
import { Button } from "./shared-ui";

/**
 * Dedicated Troubleshooting & Recovery panel under Settings -> Troubleshoot.
 * Centralizes restarting the Core Service, wiping/rebuilding cached storage,
 * repairing OS background service & `gitm` PATH registrations, and exporting a
 * plain-text diagnostics report.
 */
export function TroubleshootPanel({
	serviceReady,
	status,
	onRestartService,
	onResetStorage,
	onTriggerHappyLogo,
}: {
	serviceReady: boolean;
	status: string;
	onRestartService: () => void;
	onResetStorage: () => Promise<void>;
	onTriggerHappyLogo: () => void;
}) {
	const [busyAction, setBusyAction] = useState<string>("");
	const [notice, setNotice] = useState<string>("");
	const [error, setError] = useState<string>("");

	async function runAction(id: string, fn: () => Promise<string>) {
		setBusyAction(id);
		setNotice("");
		setError("");
		try {
			const result = await fn();
			setNotice(result);
			onTriggerHappyLogo();
		} catch (cause) {
			setError(cause instanceof Error ? cause.message : String(cause));
		} finally {
			setBusyAction("");
		}
	}

	return (
		<div className="gm-page-content">
			<div className="gm-page-heading">
				<div>
					<div className="gm-eyebrow">RECOVERY &amp; DIAGNOSTICS</div>
					<h1>Troubleshoot</h1>
					<p>Restart the background service, reset corrupted caches, repair OS integrations, or export a diagnostics bundle.</p>
				</div>
				<span className={serviceReady ? "gm-connected-pill" : "gm-badge warn"}>
					{serviceReady ? "Core Service Ready" : status || "Service Offline"}
				</span>
			</div>

			{notice ? <div className="gm-inline-success">{notice}</div> : null}
			{error ? <div className="gm-inline-error">{error}</div> : null}

			<div className="gm-troubleshoot-grid">
				<section className="gm-panel gm-diagnostic-panel">
					<div className="gm-panel-heading">
						<div>
							<h2>Restart Core Service</h2>
							<p>Reconnects the local IPC pipe and respawns <code>gitmanager-service</code> if scans or background checks stall.</p>
						</div>
					</div>
					<div className="gm-panel-actions">
						<Button
							variant="primary"
							disabled={busyAction !== ""}
							onClick={() =>
								void runAction("restart", async () => {
									onRestartService();
									await invoke("ensure_required_service");
									return "Core Service restarted and IPC connection verified.";
								})
							}
						>
							{busyAction === "restart" ? "Restarting…" : "Restart Core Service"}
						</Button>
					</div>
				</section>

				<section className="gm-panel gm-diagnostic-panel">
					<div className="gm-panel-heading">
						<div>
							<h2>Reset Storage &amp; Fresh Rescan</h2>
							<p>Clears cached repository metadata and rebuilds the workspace index from your configured scan roots.</p>
						</div>
					</div>
					<div className="gm-panel-actions">
						<Button
							disabled={busyAction !== ""}
							onClick={() =>
								void runAction("reset", async () => {
									await onResetStorage();
									return "Workspace storage cleared and fresh scan completed.";
								})
							}
						>
							{busyAction === "reset" ? "Resetting…" : "↻ Fresh Data & Rescan"}
						</Button>
					</div>
				</section>

				<section className="gm-panel gm-diagnostic-panel">
					<div className="gm-panel-heading">
						<div>
							<h2>Repair OS Sign-In Service</h2>
							<p>Re-registers the background watcher with Windows Task Scheduler / Registry Run, systemd, or launchd.</p>
						</div>
					</div>
					<div className="gm-panel-actions">
						<Button
							disabled={busyAction !== ""}
							onClick={() =>
								void runAction("os-service", async () => {
									await invoke("control_background_service", { enableInApp: true, installOsService: true });
									return "OS background service registration refreshed.";
								})
							}
						>
							{busyAction === "os-service" ? "Repairing…" : "Repair OS Registration"}
						</Button>
					</div>
				</section>

				<section className="gm-panel gm-diagnostic-panel">
					<div className="gm-panel-heading">
						<div>
							<h2>Repair <code>gitm</code> CLI on PATH</h2>
							<p>Ensures the bundled <code>gitm</code> command-line tool is registered on your user PATH for terminal use.</p>
						</div>
					</div>
					<div className="gm-panel-actions">
						<Button
							disabled={busyAction !== ""}
							onClick={() =>
								void runAction("gitm-path", async () => {
									const msg = await invoke<string>("set_gitm_cli_path_integration", { enabled: true });
									return msg || "Verified gitm CLI on user PATH.";
								})
							}
						>
							{busyAction === "gitm-path" ? "Updating PATH…" : "Register gitm on PATH"}
						</Button>
					</div>
				</section>

				<section className="gm-panel gm-diagnostic-panel">
					<div className="gm-panel-heading">
						<div>
							<h2>Export Diagnostics Report</h2>
							<p>Writes OS info, app version, and CLI versions (<code>git</code>, <code>gitm</code>, <code>gh</code>, <code>glab</code>) to a text file in your Downloads folder.</p>
						</div>
					</div>
					<div className="gm-panel-actions">
						<Button
							disabled={busyAction !== ""}
							onClick={() =>
								void runAction("export", async () => {
									const savedPath = await invoke<string>("export_troubleshoot_diagnostics");
									return `Diagnostics report saved to ${savedPath}`;
								})
							}
						>
							{busyAction === "export" ? "Exporting…" : "Export Diagnostics (.txt)"}
						</Button>
					</div>
				</section>
			</div>
		</div>
	);
}
