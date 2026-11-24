"use client";

import { RingLoadingIndicator } from "../../components/repo-ring-badge";

import { Suspense } from "react";
import { useSearchParams } from "next/navigation";
import { useWorkspace } from "../../lib/workspace-context";
import { CleanerPage } from "../../components/cleaner-page";

function CleanerRouteContent() {
	const searchParams = useSearchParams();
	const { git, handleScan } = useWorkspace();
	const initialRepoPath = searchParams.get("repo") || undefined;

	return (
		<CleanerPage
			repositories={git.repositories}
			onRefresh={handleScan}
			initialRepoPath={initialRepoPath}
		/>
	);
}

/**
 * Repository & System Cleaner route (`/cleaner/`).
 */
export default function CleanerRoutePage() {
	return (
		<Suspense fallback={<RingLoadingIndicator label="Loading cleaner…" />}>
			<CleanerRouteContent />
		</Suspense>
	);
}
