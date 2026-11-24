"use client";

import { RingLoadingIndicator } from "../../components/repo-ring-badge";

import { Suspense, useCallback, useEffect, useMemo, useState } from "react";
import { useSearchParams } from "next/navigation";
import { CustomSelect } from "@/engine";
import { invoke } from "../../lib/tauri";
import type { RepositoryDestination } from "../../lib/internal-routing";
import { useWorkspace } from "../../lib/workspace-context";
import { RepositoryDetail } from "../../components/repository-detail";
import { ActionsPanel } from "../../components/actions-panel";
import { ReleasesPanel } from "../../components/releases-panel";
import { IssuesPanel } from "../../components/issues-panel";
import { VariablesPanel } from "../../components/variables-panel";
import { Button, EmptyState } from "../../components/shared-ui";

type ExtraRepoTab = "none" | "actions" | "releases" | "issues" | "variables";

function destinationFromViewParam(viewParam: string | null): RepositoryDestination {
	switch (viewParam) {
		case "commits":
			return { kind: "commit-browser" };
		case "pulls":
			return { kind: "pull-browser" };
		case "conflicts":
			return { kind: "conflicts" };
		case "pull-conflicts":
			return { kind: "pull-conflicts" };
		default:
			return { kind: "overview" };
	}
}

function extraTabFromViewParam(viewParam: string | null): ExtraRepoTab {
	switch (viewParam) {
		case "actions":
		case "releases":
		case "issues":
		case "variables":
			return viewParam;
		default:
			return "none";
	}
}

/**
 * Inner repository viewer content reading `?id=...&view=...` from the URL.
 * Wrapped in `<Suspense>` so Next.js static export prerenders `/repo/` cleanly.
 */
function RepoPageContent() {
	const searchParams = useSearchParams();
	const { git, handleScan, navigateTo, triggerHappyLogo } = useWorkspace();
	const repoId = searchParams.get("id") || "";
	const viewParam = searchParams.get("view");

	const selectedRepository = useMemo(() => {
		if (!repoId) return git.repositories[0] ?? null;
		return (
			git.repositories.find((item) => item.id === repoId || item.path === repoId) ??
			null
		);
	}, [git.repositories, repoId]);

	const [extraTab, setExtraTab] = useState<ExtraRepoTab>(() => extraTabFromViewParam(viewParam));
	const [destination, setDestination] = useState<RepositoryDestination>(() =>
		destinationFromViewParam(viewParam),
	);
	const [switchingBranch, setSwitchingBranch] = useState(false);
	const [branchProtected, setBranchProtected] = useState<{ protected: boolean; summary: string }>({
		protected: false,
		summary: "",
	});

	useEffect(() => {
		setExtraTab(extraTabFromViewParam(viewParam));
		setDestination(destinationFromViewParam(viewParam));
	}, [viewParam, repoId]);

	useEffect(() => {
		if (!selectedRepository?.path || !selectedRepository.current_branch) {
			setBranchProtected({ protected: false, summary: "" });
			return;
		}
		let active = true;
		void invoke<{ branch: string; protected: boolean; summary: string }>(
			"get_branch_protection_status",
			{
				repoPath: selectedRepository.path,
				branch: selectedRepository.current_branch,
			},
		)
			.then((res) => {
				if (active) setBranchProtected({ protected: res.protected, summary: res.summary });
			})
			.catch(() => {
				if (active) setBranchProtected({ protected: false, summary: "" });
			});
		return () => {
			active = false;
		};
	}, [selectedRepository?.path, selectedRepository?.current_branch]);

	const handleBranchChange = useCallback(
		async (nextBranch: string) => {
			if (
				switchingBranch ||
				!selectedRepository ||
				!nextBranch ||
				nextBranch === selectedRepository.current_branch
			) {
				return;
			}
			setSwitchingBranch(true);
			try {
				await git.switchBranch(selectedRepository.path, nextBranch);
				triggerHappyLogo();
			} finally {
				setSwitchingBranch(false);
			}
		},
		[git, selectedRepository, switchingBranch, triggerHappyLogo],
	);

	const handleOpenWebsite = useCallback((url: string) => {
		void invoke("open_external_url", { url }).catch(() => {
			if (typeof window !== "undefined") {
				window.open(url, "_blank", "noopener,noreferrer");
			}
		});
	}, []);

	const branchOptions = useMemo(() => {
		if (!selectedRepository) return [];
		const localNames = selectedRepository.branches
			.filter((branch) => !branch.is_remote)
			.map((branch) => branch.name);
		const names = localNames.length > 0 ? localNames : [selectedRepository.current_branch || "main"];
		return names.map((branchName) => ({
			value: branchName,
			label: branchName,
		}));
	}, [selectedRepository]);

	if (!selectedRepository) {
		return (
			<section className="gm-page">
				<EmptyState
					icon="⎇"
					title="Repository not found"
					body="The selected repository is not currently indexed. Return to the Repositories index or run a scan."
					action={
						<Button variant="primary" onClick={() => navigateTo("/")}>
							Back to Repositories
						</Button>
					}
				/>
			</section>
		);
	}

	return (
		<section className="gm-page gm-repo-detail-page">
			<div className="gm-repo-subbar">
				<div className="gm-repo-subbar-left">
					<Button onClick={() => navigateTo("/")}>← Repositories</Button>
					<div className="gm-repo-subbar-title">
						<strong>{selectedRepository.name}</strong>
						<span className="gm-mono gm-muted">{selectedRepository.path}</span>
					</div>
				</div>

				<div className="gm-repo-subbar-controls">
					<div className="gm-branch-select-wrap" style={{ display: "flex", alignItems: "center", gap: 8 }}>
						<CustomSelect
							name="active-branch"
							ariaLabel="Active branch"
							value={selectedRepository.current_branch || "main"}
							options={branchOptions}
							onChange={(nextValue) => void handleBranchChange(nextValue)}
						/>
						{branchProtected.protected ? (
							<span className="gm-badge warn" title={branchProtected.summary}>
								🔒 Protected
							</span>
						) : null}
					</div>

					<nav className="gm-subtabs" aria-label="Repository sections">
						<button
							type="button"
							className={extraTab === "none" && destination.kind === "overview" ? "gm-subtab active" : "gm-subtab"}
							onClick={() => {
								setExtraTab("none");
								setDestination({ kind: "overview" });
							}}
						>
							Overview
						</button>
						<button
							type="button"
							className={
								extraTab === "none" &&
								(destination.kind === "commit-browser" || destination.kind === "commit-details")
									? "gm-subtab active"
									: "gm-subtab"
							}
							onClick={() => {
								setExtraTab("none");
								setDestination({ kind: "commit-browser" });
							}}
						>
							Commits
						</button>
						<button
							type="button"
							className={
								extraTab === "none" &&
								(destination.kind === "pull-browser" || destination.kind === "pull-request")
									? "gm-subtab active"
									: "gm-subtab"
							}
							onClick={() => {
								setExtraTab("none");
								setDestination({ kind: "pull-browser" });
							}}
						>
							Pulls
						</button>
						<button
							type="button"
							className={extraTab === "actions" ? "gm-subtab active" : "gm-subtab"}
							onClick={() => setExtraTab("actions")}
						>
							▶ Actions
						</button>
						<button
							type="button"
							className={extraTab === "issues" ? "gm-subtab active" : "gm-subtab"}
							onClick={() => setExtraTab("issues")}
						>
							● Issues
						</button>
						<button
							type="button"
							className={extraTab === "releases" ? "gm-subtab active" : "gm-subtab"}
							onClick={() => setExtraTab("releases")}
						>
							🏷 Releases
						</button>
						<button
							type="button"
							className={extraTab === "variables" ? "gm-subtab active" : "gm-subtab"}
							onClick={() => setExtraTab("variables")}
						>
							🔑 Variables
						</button>
						<button
							type="button"
							className={extraTab === "none" && destination.kind === "conflicts" ? "gm-subtab active" : "gm-subtab"}
							onClick={() => {
								setExtraTab("none");
								setDestination({ kind: "conflicts" });
							}}
						>
							Conflicts
						</button>
					</nav>
				</div>
			</div>

			{extraTab === "actions" ? (
				<ActionsPanel
					repo={selectedRepository}
					onOpenAccounts={() => navigateTo("/accounts")}
					onTriggerHappyLogo={triggerHappyLogo}
				/>
			) : extraTab === "issues" ? (
				<IssuesPanel
					repo={selectedRepository}
					onBranchCreated={handleScan}
					onOpenAccounts={() => navigateTo("/accounts")}
					onTriggerHappyLogo={triggerHappyLogo}
				/>
			) : extraTab === "releases" ? (
				<ReleasesPanel
					repo={selectedRepository}
					onOpenAccounts={() => navigateTo("/accounts")}
					onTriggerHappyLogo={triggerHappyLogo}
				/>
			) : extraTab === "variables" ? (
				<VariablesPanel
					repo={selectedRepository}
					onOpenAccounts={() => navigateTo("/accounts")}
					onTriggerHappyLogo={triggerHappyLogo}
				/>
			) : (
				<RepositoryDetail
					repo={selectedRepository}
					preferences={git.preferences}
					destination={destination}
					onNavigateDestination={setDestination}
					onClose={() => navigateTo("/")}
					onOpenWebsite={handleOpenWebsite}
					onRescan={handleScan}
					onRepositoryLoaded={git.updateRepository}
					onOpenCleaner={() =>
						navigateTo(`/settings?tab=cleaner&repo=${encodeURIComponent(selectedRepository.path)}`)
					}
				/>
			)}
		</section>
	);
}

/**
 * Custom Repository Viewer static route (`/repo/`).
 */
export default function RepositoryViewerPage() {
	return (
		<Suspense fallback={<RingLoadingIndicator label="Loading repository viewer…" />}>
			<RepoPageContent />
		</Suspense>
	);
}
