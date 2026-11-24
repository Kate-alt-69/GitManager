"use client";

import { useWorkspace } from "../../lib/workspace-context";
import { PerformancePage } from "../../components/performance-page";

/**
 * Idle CPU & Memory Performance diagnostics route (`/performance/`).
 */
export default function PerformanceRoutePage() {
	const { git } = useWorkspace();

	return (
		<PerformancePage
			serviceReady={git.serviceReady}
			scanning={git.scanning}
		/>
	);
}
