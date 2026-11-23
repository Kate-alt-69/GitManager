"use client";

import { useState } from "react";
import { invoke } from "../lib/tauri";
import type { IdlePerformanceCapture } from "../lib/types";
import { Button } from "./shared-ui";

function formatBytes(bytes: number): string {
	if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KiB`;
	if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MiB`;
	return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GiB`;
}

function ProcessMetrics({ title, detail }: { title: string; detail: IdlePerformanceCapture["uiProcessTree"] }) {
	return (
		<section className="gm-panel gm-diagnostic-panel">
			<div className="gm-panel-heading"><div><h2>{title}</h2><p>{detail.processCount} process{detail.processCount === 1 ? "" : "es"} in the measured group</p></div></div>
			<dl className="gm-diagnostics-facts">
				<div><dt>Average CPU</dt><dd>{detail.averageCpuPercent.toFixed(2)}%</dd></div>
				<div><dt>Peak CPU</dt><dd>{detail.peakCpuPercent.toFixed(2)}%</dd></div>
				<div><dt>Average memory</dt><dd>{formatBytes(detail.averageMemoryBytes)}</dd></div>
				<div><dt>Peak memory</dt><dd>{formatBytes(detail.peakMemoryBytes)}</dd></div>
			</dl>
		</section>
	);
}

export function PerformancePage({
	serviceReady,
	scanning,
}: {
	serviceReady: boolean;
	scanning: boolean;
}) {
	const [capturing, setCapturing] = useState(false);
	const [capture, setCapture] = useState<IdlePerformanceCapture | null>(null);
	const [error, setError] = useState("");

	async function startCapture() {
		setCapturing(true);
		setCapture(null);
		setError("");
		try {
			setCapture(await invoke<IdlePerformanceCapture>("capture_idle_performance"));
		} catch (cause) {
			setError(cause instanceof Error ? cause.message : String(cause));
		} finally {
			setCapturing(false);
		}
	}

	const targetMet = capture !== null && capture.uiProcessTree.averageCpuPercent < 5;

	return (
		<div className="gm-page-content gm-diagnostics-page">
			<div className="gm-page-heading">
				<div>
					<div className="gm-eyebrow">RESOURCE MONITORING</div>
					<h1>Performance Center</h1>
					<p>Measure the desktop/WebView process tree separately from the required service.</p>
				</div>
				<div className="gm-performance-target">
					<strong>Under 5%</strong><span>Average UI CPU target while idle</span>
				</div>
			</div>

			<section className="gm-panel gm-performance-capture">
				<div>
					<strong>60-second idle capture</strong>
					<p>Wait until indexing and other work have settled. Start the capture, then leave GitManager untouched for the full sample window.</p>
				</div>
				<Button variant="primary" onClick={() => void startCapture()} disabled={!serviceReady || scanning || capturing}>
					{capturing ? "Capturing…" : "Start idle capture"}
				</Button>
				{capturing && <div className="gm-performance-progress" role="status">Sampling UI and service CPU/memory in the native process monitor. Keep the application idle for 60 seconds.</div>}
				{!serviceReady && <div className="gm-inline-error">The core service must be ready to measure both process groups.</div>}
				{scanning && <div className="gm-inline-status">Wait for repository indexing to settle before starting an idle capture.</div>}
			</section>

			{capture && (
				<>
					<div className={targetMet ? "gm-performance-result met" : "gm-performance-result over"}>
						<strong>{targetMet ? "UI CPU target met" : "UI CPU target not met"}</strong>
						<span>{capture.uiProcessTree.averageCpuPercent.toFixed(2)}% average over {capture.sampleDurationSeconds}s · {capture.logicalProcessors} logical processors</span>
					</div>
					<div className="gm-diagnostics-grid">
						<ProcessMetrics title="GitManager UI process tree" detail={capture.uiProcessTree} />
						{capture.serviceProcessTree
							? <ProcessMetrics title="GitManager Core Service" detail={capture.serviceProcessTree} />
							: <section className="gm-panel gm-diagnostic-panel"><div className="gm-panel-heading"><div><h2>GitManager Core Service</h2><p>Service process was not found during the capture.</p></div></div></section>}
					</div>
					<div className="gm-performance-notes">
						<small>Captured {new Date(capture.capturedAtUnixMs).toLocaleString()} · {capture.sampleCount} samples · service protocol {capture.serviceProtocolVersion} · generation {capture.serviceGeneration}</small>
						{capture.notes.map((note) => <small key={note}>{note}</small>)}
					</div>
				</>
			)}
			{error && <div className="gm-error-banner" role="alert">{error}<button type="button" onClick={() => setError("")}>Dismiss</button></div>}
		</div>
	);
}
