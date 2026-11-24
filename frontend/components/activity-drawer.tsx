"use client";

import { useCallback, useEffect, useState } from "react";
import { invoke, openDialog } from "../lib/tauri";
import type { ClassicThemeMode } from "../lib/classic-theme";
import type { AppNotification } from "../lib/types";
import { Button } from "./shared-ui";
import { RepoRingBadge } from "./repo-ring-badge";

export interface QueuedTaskItem {
	id: number;
	repo_key: string;
	repo_name: string;
	disk_path: string;
	kind: string;
	title: string;
	command_preview: string;
	status: string;
	progress: number;
	phase: string;
	reversible: boolean;
	logs: string[];
}

export interface RepoTaskGroup {
	repo_key: string;
	repo_name: string;
	disk_path: string;
	running_count: number;
	queued_count: number;
	tasks: QueuedTaskItem[];
}

export interface TaskHistoryEntry {
	id: number;
	repo_key: string;
	repo_name: string;
	disk_path: string;
	kind: string;
	title: string;
	finished_at: number;
	reversible: boolean;
	status: string;
	overwritten: boolean;
	revert_note: string;
}

export interface TaskManagerSnapshot {
	paused: boolean;
	active_lanes: number;
	total_running: number;
	total_queued: number;
	groups: RepoTaskGroup[];
	history: TaskHistoryEntry[];
}

export type ActivityDrawerTab = "tasks" | "history" | "notifications";

/**
 * Left-side slide-out Activity & Inbox Drawer (browser side-panel style).
 * Snuggles per-repository TaskManager queues, reversible Undo/Redo history,
 * BGW (`gitmanager.service.exe`) controls, and Notifications together on the left
 * while keeping the main workspace view open on the right.
 */
export function ActivityDrawer({
	open,
	activeTab,
	onChangeTab,
	onClose,
	themeMode,
	serviceReady,
	onRestartBgw,
	notifications,
	onMarkNotificationRead,
	onDismissNotification,
	onOpenNotification,
	onTriggerHappyLogo,
	onSnapshotChange,
}: {
	open: boolean;
	activeTab: ActivityDrawerTab;
	onChangeTab: (tab: ActivityDrawerTab) => void;
	onClose: () => void;
	themeMode: ClassicThemeMode;
	serviceReady: boolean;
	onRestartBgw: () => void;
	notifications: AppNotification[];
	onMarkNotificationRead: (id: string) => Promise<void>;
	onDismissNotification: (id: string) => Promise<void>;
	onOpenNotification: (item: AppNotification) => void;
	onTriggerHappyLogo: () => void;
	onSnapshotChange?: (snap: TaskManagerSnapshot) => void;
}) {
	const [snapshot, setSnapshot] = useState<TaskManagerSnapshot>({
		paused: false,
		active_lanes: 0,
		total_running: 0,
		total_queued: 0,
		groups: [],
		history: [],
	});
	const [expandedRepoKey, setExpandedRepoKey] = useState<string | null>(null);
	const [cloneOpen, setCloneOpen] = useState(false);
	const [cloneUrl, setCloneUrl] = useState("");
	const [cloneDest, setCloneDest] = useState("");
	const [notice, setNotice] = useState("");
	const [error, setError] = useState("");

	const pollSnapshot = useCallback(async () => {
		try {
			const snap = await invoke<TaskManagerSnapshot>("get_task_manager_snapshot");
			setSnapshot(snap);
			onSnapshotChange?.(snap);
		} catch {
			// Ignore transient poll error if Tauri runtime is booting
		}
	}, [onSnapshotChange]);

	useEffect(() => {
		void pollSnapshot();
		const intervalMs = open || snapshot.total_running > 0 ? 1000 : 3500;
		const timer = window.setInterval(() => void pollSnapshot(), intervalMs);
		return () => window.clearInterval(timer);
	}, [open, pollSnapshot, snapshot.total_running]);

	async function handleTogglePause() {
		try {
			await invoke("set_task_queue_paused", { paused: !snapshot.paused });
			await pollSnapshot();
		} catch (cause) {
			setError(cause instanceof Error ? cause.message : String(cause));
		}
	}

	async function handleEnqueueClone() {
		if (!cloneUrl.trim() || !cloneDest.trim()) return;
		setError("");
		setNotice("");
		try {
			const repoName = cloneUrl.trim().split("/").pop()?.replace(/\.git$/i, "") || "repository";
			await invoke("enqueue_repo_task", {
				request: {
					repoName,
					diskPath: cloneDest.trim(),
					kind: "clone",
					cloneUrl: cloneUrl.trim(),
				},
			});
			setCloneUrl("");
			setCloneDest("");
			setCloneOpen(false);
			setNotice(`Queued clone for ${repoName}`);
			await pollSnapshot();
		} catch (cause) {
			setError(cause instanceof Error ? cause.message : String(cause));
		}
	}

	async function handlePickCloneFolder() {
		try {
			const selected = await openDialog({ title: "Choose destination folder for clone", directory: true, multiple: false });
			const path = Array.isArray(selected) ? selected[0] : selected;
			if (path) {
				const repoSlug = cloneUrl.trim().split("/").pop()?.replace(/\.git$/i, "") || "repo";
				const sep = path.includes("\\") ? "\\" : "/";
				setCloneDest(`${path.replace(/[\\/]+$/, "")}${sep}${repoSlug}`);
			}
		} catch {
			// Cancelled
		}
	}

	async function handleRevert(taskId: number) {
		setError("");
		setNotice("");
		try {
			const msg = await invoke<string>("revert_history_task", { taskId });
			setNotice(msg);
			onTriggerHappyLogo();
			await pollSnapshot();
		} catch (cause) {
			setError(cause instanceof Error ? cause.message : String(cause));
		}
	}

	async function handleRedo(taskId: number) {
		setError("");
		setNotice("");
		try {
			const msg = await invoke<string>("redo_history_task", { taskId });
			setNotice(msg);
			onTriggerHappyLogo();
			await pollSnapshot();
		} catch (cause) {
			setError(cause instanceof Error ? cause.message : String(cause));
		}
	}

	const unreadCount = notifications.filter((n) => !n.read).length;

	return (
		<aside
			className={open ? "gm-left-activity-drawer" : "gm-left-activity-drawer closed"}
			aria-label="Activity and Inbox"
		>
			<div className="gm-drawer-head">
				<strong>Activity &amp; Inbox</strong>
				<div style={{ display: "flex", gap: 6 }}>
					<Button onClick={() => setCloneOpen((v) => !v)}>+ Clone</Button>
					<Button onClick={onClose} title="Close side panel">✕</Button>
				</div>
			</div>

			<div className="gm-bgw-strip">
				<div className="gm-bgw-meta">
					<span className={serviceReady ? "gm-led clean" : "gm-led offline"} />
					<strong className="gm-mono">gitmanager.service.exe</strong>
					<span className="gm-muted">
						{snapshot.paused ? "Paused" : `${snapshot.active_lanes} lane${snapshot.active_lanes === 1 ? "" : "s"}`}
					</span>
				</div>
				<div style={{ display: "flex", gap: 5 }}>
					<Button onClick={() => void handleTogglePause()}>{snapshot.paused ? "Resume" : "Pause"}</Button>
					<Button onClick={onRestartBgw} title="Restart BGW">↻</Button>
				</div>
			</div>

			{cloneOpen ? (
				<div className="gm-clone-quickform">
					<input
						type="text"
						className="gm-input"
						value={cloneUrl}
						onChange={(event) => setCloneUrl(event.target.value)}
						placeholder="github:owner/repo or https://…"
					/>
					<div style={{ display: "flex", gap: 6 }}>
						<input
							type="text"
							className="gm-input"
							style={{ flex: 1 }}
							value={cloneDest}
							onChange={(event) => setCloneDest(event.target.value)}
							placeholder="Destination disk path…"
						/>
						<Button onClick={() => void handlePickCloneFolder()}>…</Button>
					</div>
					<Button variant="primary" disabled={!cloneUrl.trim() || !cloneDest.trim()} onClick={() => void handleEnqueueClone()}>
						Queue gitm clone
					</Button>
				</div>
			) : null}

			{notice ? <div className="gm-inline-success" style={{ margin: "8px 12px 0" }}>{notice}</div> : null}
			{error ? <div className="gm-inline-error" style={{ margin: "8px 12px 0" }}>{error}</div> : null}

			<div className="gm-drawer-tabs" role="tablist">
				<button
					type="button"
					className={activeTab === "tasks" ? "gm-drawer-tab active" : "gm-drawer-tab"}
					onClick={() => onChangeTab("tasks")}
				>
					Tasks <span className="gm-badge">{snapshot.groups.length}</span>
				</button>
				<button
					type="button"
					className={activeTab === "history" ? "gm-drawer-tab active" : "gm-drawer-tab"}
					onClick={() => onChangeTab("history")}
				>
					History <span className="gm-badge">{snapshot.history.length}</span>
				</button>
				<button
					type="button"
					className={activeTab === "notifications" ? "gm-drawer-tab active" : "gm-drawer-tab"}
					onClick={() => onChangeTab("notifications")}
				>
					Inbox <span className={unreadCount > 0 ? "gm-badge warn" : "gm-badge"}>{unreadCount}</span>
				</button>
			</div>

			<div className="gm-drawer-body">
				{activeTab === "tasks" ? (
					snapshot.groups.length === 0 ? (
						<div className="gm-drawer-empty">
							All repository queues are idle. Click <strong>+ Clone</strong> above or run Git actions to watch parallel per-repo lanes.
						</div>
					) : (
						snapshot.groups.map((group) => {
							const lead = group.tasks.find((t) => t.status === "running") ?? group.tasks[0];
							const isWorking = !snapshot.paused && group.running_count > 0;
							const isExpanded = expandedRepoKey === group.repo_key;
							return (
								<div
									key={group.repo_key}
									className={isExpanded ? "gm-repo-task-card selected" : "gm-repo-task-card"}
								>
									<div
										className="gm-repo-task-head"
										onClick={() => setExpandedRepoKey(isExpanded ? null : group.repo_key)}
									>
										<RepoRingBadge
											label={group.repo_name}
											working={isWorking}
											size={38}
											themeMode={themeMode}
										/>
										<div style={{ minWidth: 0, flex: 1 }}>
											<strong>{group.repo_name}</strong>
											<small className="gm-mono gm-muted">{group.disk_path}</small>
										</div>
										<span className="gm-badge info">
											{group.running_count} + {group.queued_count} left
										</span>
									</div>

									<div className="gm-repo-task-line">
										<span>{lead.title}</span>
										<strong>{lead.progress}%</strong>
									</div>
									<div className="gm-progress-track">
										<div className="gm-progress-fill" style={{ width: `${lead.progress}%` }} />
									</div>
									<div className="gm-repo-task-foot">
										<span className="gm-mono gm-muted">{lead.phase}</span>
										<button
											type="button"
											className="gm-link-btn"
											onClick={() => setExpandedRepoKey(isExpanded ? null : group.repo_key)}
										>
											{isExpanded ? "Hide queue ▾" : `Order #1–#${group.tasks.length} ▸`}
										</button>
									</div>

									{isExpanded ? (
										<div className="gm-ordered-queue">
											{group.tasks.map((task, idx) => (
												<div key={task.id} className="gm-ordered-step">
													<div className="gm-ordered-step-row">
														<span className="gm-order-num">#{idx + 1}</span>
														<div style={{ minWidth: 0, flex: 1 }}>
															<strong>{task.title}</strong>
															<div className="gm-mono gm-muted">{task.command_preview}</div>
														</div>
														{task.status === "queued" && idx > 1 ? (
															<Button
																onClick={() =>
																	void invoke("reorder_repo_task", {
																		repoKey: group.repo_key,
																		taskId: task.id,
																	}).then(() => pollSnapshot())
																}
															>
																↑
															</Button>
														) : null}
														{task.status === "queued" ? (
															<Button
																onClick={() =>
																	void invoke("cancel_repo_task", { taskId: task.id }).then(() => pollSnapshot())
																}
															>
																✕
															</Button>
														) : null}
													</div>
													{task.logs.length > 0 ? (
														<pre className="gm-mini-log">{task.logs.slice(-4).join("\n")}</pre>
													) : null}
												</div>
											))}
										</div>
									) : null}
								</div>
							);
						})
					)
				) : null}

				{activeTab === "history" ? (
					snapshot.history.length === 0 ? (
						<div className="gm-drawer-empty">Completed tasks appear here with safe Revert &amp; Go Forward controls.</div>
					) : (
						snapshot.history.map((item) => (
							<div
								key={item.id}
								className={item.status === "reverted" ? "gm-history-card reverted" : "gm-history-card"}
							>
								<div className="gm-repo-task-head">
									<RepoRingBadge label={item.repo_name} working={false} size={32} themeMode={themeMode} />
									<div style={{ minWidth: 0, flex: 1 }}>
										<strong>{item.title}</strong>
										<small className="gm-mono gm-muted">{item.repo_name}</small>
									</div>
									{item.overwritten ? (
										<span className="gm-badge danger">Overwritten</span>
									) : item.status === "reverted" ? (
										<span className="gm-badge warn">Reverted</span>
									) : item.status === "failed" ? (
										<span className="gm-badge danger">Failed</span>
									) : (
										<span className="gm-badge">Done</span>
									)}
								</div>
								<div className="gm-repo-task-foot">
									<span className="gm-muted">{item.revert_note}</span>
									{item.reversible && item.status === "applied" ? (
										<Button onClick={() => void handleRevert(item.id)}>↩ Revert</Button>
									) : item.reversible && item.status === "reverted" && !item.overwritten ? (
										<Button variant="primary" onClick={() => void handleRedo(item.id)}>↪ Forward</Button>
									) : null}
								</div>
							</div>
						))
					)
				) : null}

				{activeTab === "notifications" ? (
					notifications.length === 0 ? (
						<div className="gm-drawer-empty">No notifications right now.</div>
					) : (
						notifications.map((item) => (
							<div key={item.id} className="gm-history-card">
								<div className="gm-repo-task-head">
									<strong style={{ flex: 1 }}>{item.title}</strong>
									{!item.read ? <span className="gm-badge warn">New</span> : null}
								</div>
								<div className="gm-muted" style={{ fontSize: "0.8rem" }}>{item.body}</div>
								<div className="gm-repo-task-foot">
									<Button onClick={() => onOpenNotification(item)}>Open</Button>
									<div style={{ display: "flex", gap: 6 }}>
										{!item.read ? (
											<Button onClick={() => void onMarkNotificationRead(item.id)}>Read</Button>
										) : null}
										<Button onClick={() => void onDismissNotification(item.id)}>Dismiss</Button>
									</div>
								</div>
							</div>
						))
					)
				) : null}
			</div>
		</aside>
	);
}
