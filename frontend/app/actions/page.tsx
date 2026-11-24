"use client";

import { useEffect, useState } from "react";
import { useWorkspace } from "../../lib/workspace-context";
import { ActionsPanel } from "../../components/actions-panel";
import { Button, EmptyState } from "../../components/shared-ui";

/**
 * Global Actions & Pipelines static route (`/actions/`).
 * Lets users pick any indexed repository to inspect workflows, trigger remote
 * runs via `gh` / `glab`, view live logs, and edit workflow files in-app.
 */
export default function ActionsRoutePage() {
	const { git, navigateTo, triggerHappyLogo } = useWorkspace();
	const [selectedRepoId, setSelectedRepoId] = useState<string>(git.repositories[0]?.id || "");

	useEffect(() => {
		if (!selectedRepoId && git.repositories[0]) {
			setSelectedRepoId(git.repositories[0].id);
		}
	}, [git.repositories, selectedRepoId]);

	const activeRepo =
		git.repositories.find((item) => item.id === selectedRepoId) ?? git.repositories[0] ?? null;

	return (
		<section className="gm-page gm-actions-page">
			<div className="gm-page-header">
				<div>
					<h1>Actions &amp; Pipelines</h1>
					<p>Run and edit GitHub Actions &amp; GitLab CI workflows across your repositories.</p>
				</div>
				{git.repositories.length > 0 ? (
					<label className="gm-field gm-repo-picker">
						<span>Repository</span>
						<select
							value={activeRepo?.id || ""}
							onChange={(event) => setSelectedRepoId(event.target.value)}
						>
							{git.repositories.map((repo) => (
								<option key={repo.id} value={repo.id}>
									{repo.name} ({repo.current_branch || "HEAD"})
								</option>
							))}
						</select>
					</label>
				) : null}
			</div>

			{!activeRepo ? (
				<EmptyState
					icon="▶"
					title="No repositories indexed yet"
					body="Scan your local repositories first to inspect and trigger Actions or Pipelines."
					action={
						<Button variant="primary" onClick={() => navigateTo("/")}>
							Go to Repositories
						</Button>
					}
				/>
			) : (
				<ActionsPanel
					repo={activeRepo}
					onOpenAccounts={() => navigateTo("/accounts")}
					onTriggerHappyLogo={triggerHappyLogo}
				/>
			)}
		</section>
	);
}
