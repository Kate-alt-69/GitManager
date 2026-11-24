"use client";

import { isLightTheme, type ClassicThemeMode } from "../lib/classic-theme";
import { RepoRingBadge } from "./repo-ring-badge";
import type { TaskManagerSnapshot } from "./activity-drawer";

/**
 * Browser-style collapsible Left Navigation Rail.
 * - Collapsed (68px): shows ONLY the GitManager 1:1 logo icon, primary nav icons,
 *   the Mini Task widget (with RepoRingBadge), and the Inbox button.
 * - Expanded (228px): reveals "GitManager" + "by Kastrick" and full labels.
 */
export function LeftNavRail({
	expanded,
	onToggleExpanded,
	activePath,
	themeMode,
	repositoryCount,
	accountCount,
	unreadNotifications,
	serviceReady,
	activityOpen,
	activityTab,
	taskSnapshot,
	onNavigate,
	onToggleTasks,
	onToggleInbox,
}: {
	expanded: boolean;
	onToggleExpanded: () => void;
	activePath: string;
	themeMode: ClassicThemeMode;
	repositoryCount: number;
	accountCount: number;
	unreadNotifications: number;
	serviceReady: boolean;
	activityOpen: boolean;
	activityTab: string;
	taskSnapshot: TaskManagerSnapshot;
	onNavigate: (href: string) => void;
	onToggleTasks: () => void;
	onToggleInbox: () => void;
}) {
	const variant = isLightTheme(themeMode) ? "black" : "white";
	const logoSrc = `/static/image/git-managar-orange_default-${variant}.png`;
	const isWorking = !taskSnapshot.paused && taskSnapshot.total_running > 0;
	const leadGroup = taskSnapshot.groups[0];
	const leadTask = leadGroup?.tasks[0];

	return (
		<aside
			className={expanded ? "gm-left-rail expanded" : "gm-left-rail"}
			aria-label="Primary sidebar"
		>
			<div className="gm-rail-top">
				<button
					type="button"
					className="gm-rail-brand"
					onClick={onToggleExpanded}
					title={expanded ? "Collapse sidebar" : "Expand sidebar"}
				>
					<img src={logoSrc} alt="GitManager" className="gm-rail-logo-img" draggable={false} />
					<div className="gm-rail-brand-meta">
						<strong>GitManager</strong>
						<span>by Kastrick</span>
					</div>
				</button>

				<div className="gm-rail-divider" />

				<nav className="gm-rail-nav" aria-label="Primary navigation">
					<button
						type="button"
						className={activePath === "/" || activePath === "/repo" ? "gm-rail-item active" : "gm-rail-item"}
						onClick={() => onNavigate("/")}
						title="Repositories"
					>
						<span className="gm-rail-icon">⎇</span>
						<span className="gm-rail-label">Repositories</span>
						{expanded && repositoryCount > 0 ? (
							<span className="gm-rail-count">{repositoryCount}</span>
						) : null}
					</button>
					<button
						type="button"
						className={activePath === "/actions" ? "gm-rail-item active" : "gm-rail-item"}
						onClick={() => onNavigate("/actions")}
						title="Actions"
					>
						<span className="gm-rail-icon">▶</span>
						<span className="gm-rail-label">Actions</span>
					</button>
					<button
						type="button"
						className={activePath === "/accounts" ? "gm-rail-item active" : "gm-rail-item"}
						onClick={() => onNavigate("/accounts")}
						title="Accounts"
					>
						<span className="gm-rail-icon">◎</span>
						<span className="gm-rail-label">Accounts</span>
						{expanded && accountCount > 0 ? (
							<span className="gm-rail-count">{accountCount}</span>
						) : null}
					</button>
					<button
						type="button"
						className={
							activePath === "/settings" ||
							activePath === "/cleaner" ||
							activePath === "/service" ||
							activePath === "/performance"
								? "gm-rail-item active"
								: "gm-rail-item"
						}
						onClick={() => onNavigate("/settings")}
						title="Settings"
					>
						<span className="gm-rail-icon">
							<span className={serviceReady ? "gm-led clean" : "gm-led offline"} />
						</span>
						<span className="gm-rail-label">Settings</span>
					</button>
				</nav>
			</div>

			<div className="gm-rail-bottom">
				<button
					type="button"
					className={activityOpen && activityTab === "tasks" ? "gm-mini-task-btn open" : "gm-mini-task-btn"}
					onClick={onToggleTasks}
					title="TaskManager & Activity"
				>
					<RepoRingBadge
						label={leadGroup?.repo_name || "GM"}
						working={isWorking}
						size={34}
						themeMode={themeMode}
					/>
					<div className="gm-mini-task-copy">
						<strong>
							{isWorking
								? `${taskSnapshot.groups.length} repo${taskSnapshot.groups.length === 1 ? "" : "s"} active`
								: "Tasks idle"}
						</strong>
						<small>
							{isWorking && leadTask ? `${leadTask.progress}% · ${leadTask.kind}` : "Click for queue"}
						</small>
					</div>
				</button>

				<button
					type="button"
					className={activityOpen && activityTab === "notifications" ? "gm-rail-item active" : "gm-rail-item"}
					onClick={onToggleInbox}
					title="Inbox & Notifications"
				>
					<span className="gm-rail-icon">🔔</span>
					<span className="gm-rail-label">Inbox</span>
					{unreadNotifications > 0 ? (
						<span className="gm-rail-pill">{unreadNotifications}</span>
					) : null}
				</button>

				<button
					type="button"
					className="gm-rail-item"
					onClick={onToggleExpanded}
					title={expanded ? "Collapse sidebar" : "Expand sidebar"}
				>
					<span className="gm-rail-icon">{expanded ? "⇤" : "⇥"}</span>
					<span className="gm-rail-label">Collapse</span>
				</button>
			</div>
		</aside>
	);
}
