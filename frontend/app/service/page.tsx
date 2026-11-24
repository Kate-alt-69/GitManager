"use client";

import { useWorkspace } from "../../lib/workspace-context";
import { ServiceManagerPage } from "../../components/service-manager-page";

/**
 * Background Service & OS Registration diagnostics route (`/service/`).
 */
export default function ServiceRoutePage() {
	const { git, handleRetryService } = useWorkspace();

	return (
		<ServiceManagerPage
			serviceReady={git.serviceReady}
			status={git.status}
			onStart={handleRetryService}
		/>
	);
}
