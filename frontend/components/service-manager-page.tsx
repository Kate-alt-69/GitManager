"use client";

import { useCallback, useEffect, useState } from "react";
import { invoke, listen } from "../lib/tauri";
import type { CoreServiceStatus } from "../lib/types";
import { Button } from "./shared-ui";

export function ServiceManagerPage({
	serviceReady,
	status,
	onStart,
}: {
	serviceReady: boolean;
	status: string;
	onStart: () => void;
}) {
	const [details, setDetails] = useState<CoreServiceStatus | null>(null);
	const [loading, setLoading] = useState(false);
	const [error, setError] = useState("");
	const [logs, setLogs] = useState<string | null>(null);
	const [logsLoading, setLogsLoading] = useState(false);

	const refresh = useCallback(async () => {
		setLoading(true);
		setError("");
		try {
			setDetails(await invoke<CoreServiceStatus>("get_core_service_status"));
		} catch (cause) {
			setDetails(null);
			setError(cause instanceof Error ? cause.message : String(cause));
		} finally {
			setLoading(false);
		}
	}, []);

	useEffect(() => {
		let active = true;
		let unlisten: (() => void) | undefined;
		void listen<{
			ready: boolean;
			generation: number;
			processId: number;
			protocolVersion: number;
			remoteCheckPending?: boolean;
			remoteCheckRunning?: boolean;
			indexing?: boolean;
			message: string;
		}>(
			"gitmanager://service-health",
			(payload) => {
				if (!active) return;
				setDetails((current) => ({
					suspended: current?.suspended ?? false,
					remaining_seconds: current?.remaining_seconds ?? 0,
					message: payload.message,
					service_generation: payload.generation,
					process_id: payload.processId,
					protocol_version: payload.protocolVersion,
					remote_check_pending: Boolean(payload.remoteCheckPending),
					remote_check_running: Boolean(payload.remoteCheckRunning),
					indexing: Boolean(payload.indexing),
				}));
				if (payload.ready) setError("");
				else setError(payload.message || "GitManager Core Service is unavailable.");
			},
		).then((stop) => {
			if (!active) { stop(); return; }
			unlisten = stop;
			void refresh();
		});
		return () => {
			active = false;
			unlisten?.();
		};
	}, [refresh]);

	async function toggleLogs() {
		if (logs !== null) {
			setLogs(null);
			return;
		}
		setLogsLoading(true);
		setError("");
		try {
			setLogs(await invoke<string>("read_service_log_tail"));
		} catch (cause) {
			setError(cause instanceof Error ? cause.message : String(cause));
		} finally {
			setLogsLoading(false);
		}
	}

	return (
		<div className="gm-page-content gm-diagnostics-page">
			<div className="gm-page-heading">
				<div>
					<div className="gm-eyebrow">SYSTEM DIAGNOSTICS</div>
					<h1>Service Manager</h1>
					<p>Inspect the required per-user data engine, its authenticated protocol and recent diagnostics.</p>
				</div>
				<div className={serviceReady ? "gm-diagnostics-state ready" : "gm-diagnostics-state offline"}>
					<i /> {serviceReady ? "Core service ready" : "Core service offline"}
				</div>
			</div>

			<div className="gm-diagnostics-grid">
				<section className="gm-panel gm-diagnostic-panel">
					<div className="gm-panel-heading"><div><h2>Service process</h2><p>Runs under the current user's permissions, separately from the WebView.</p></div></div>
					<dl className="gm-diagnostics-facts">
						<div><dt>Process ID</dt><dd>{details?.process_id || "—"}</dd></div>
						<div><dt>Protocol version</dt><dd>{details?.protocol_version || "—"}</dd></div>
						<div><dt>Service generation</dt><dd>{details?.service_generation || "—"}</dd></div>
						<div><dt>Repository indexing</dt><dd>{details ? details.indexing ? "Running" : "Idle" : "Unknown"}</dd></div>
						<div><dt>Background work</dt><dd>{details?.suspended ? `Paused · ${details.remaining_seconds}s remaining` : details ? "Active" : "Unknown"}</dd></div>
						<div><dt>Remote check</dt><dd>{details?.remote_check_running ? "Running" : details?.remote_check_pending ? "Queued" : "Idle"}</dd></div>
					</dl>
					<div className="gm-diagnostic-message">{details?.message || status}</div>
					<div className="gm-panel-actions">
						<Button onClick={() => void refresh()} disabled={loading}>{loading ? "Checking…" : "Refresh status"}</Button>
						{!serviceReady && <Button variant="primary" onClick={onStart}>Start / retry service</Button>}
					</div>
				</section>

				<section className="gm-panel gm-diagnostic-panel">
					<div className="gm-panel-heading"><div><h2>Service diagnostics</h2><p>Read the latest bounded service log output.</p></div></div>
					<div className="gm-diagnostic-message">
						The service owns repository discovery, indexing and persistent account/cache operations. The UI does not run a fallback scan when the service is unavailable.
					</div>
					<div className="gm-panel-actions">
						<Button onClick={() => void toggleLogs()} disabled={logsLoading}>{logsLoading ? "Loading…" : logs !== null ? "Hide service log" : "View service log"}</Button>
					</div>
					{logs !== null && <pre className="gm-diagnostic-log">{logs || "No service log output yet."}</pre>}
				</section>
			</div>
			{error && <div className="gm-error-banner" role="alert">{error}<button type="button" onClick={() => setError("")}>Dismiss</button></div>}
		</div>
	);
}
