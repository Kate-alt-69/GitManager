"use client";

import { RingLoadingIndicator } from "../../components/repo-ring-badge";

import { Suspense, useEffect, useState } from "react";
import { useSearchParams } from "next/navigation";
import { useWorkspace } from "../../lib/workspace-context";
import { SettingsPage } from "../../components/settings-page";
import { CleanerPage } from "../../components/cleaner-page";
import { ServiceManagerPage } from "../../components/service-manager-page";
import { PerformancePage } from "../../components/performance-page";
import { TroubleshootPanel } from "../../components/troubleshoot-panel";

type SettingsSection = "preferences" | "cleaner" | "service" | "performance" | "troubleshoot";

function normalizeSection(param: string | null): SettingsSection {
	switch (param) {
		case "cleaner":
		case "service":
		case "performance":
		case "troubleshoot":
			return param;
		default:
			return "preferences";
	}
}

/**
 * Consolidated Settings Hub (`/settings/`).
 * Houses General/Cloud/About Preferences alongside Cleaner, Service, Performance,
 * and Troubleshoot so the top navigation bar stays clean and focused.
 */
function SettingsHubContent() {
	const searchParams = useSearchParams();
	const {
		git,
		handleSavePreferences,
		handleScan,
		handleRetryService,
		handleResetStorage,
		triggerHappyLogo,
		themeMode,
		setThemeMode,
	} = useWorkspace();
	const tabParam = searchParams.get("tab");
	const repoParam = searchParams.get("repo") || undefined;
	const [section, setSection] = useState<SettingsSection>(() => normalizeSection(tabParam));

	useEffect(() => {
		if (tabParam) {
			setSection(normalizeSection(tabParam));
		}
	}, [tabParam]);

	return (
		<section className="gm-page gm-settings-hub">
			<nav className="gm-settings-subnav" aria-label="Settings sections">
				<button
					type="button"
					className={section === "preferences" ? "gm-subtab active" : "gm-subtab"}
					onClick={() => setSection("preferences")}
				>
					⚙ General &amp; Cloud
				</button>
				<button
					type="button"
					className={section === "cleaner" ? "gm-subtab active" : "gm-subtab"}
					onClick={() => setSection("cleaner")}
				>
					⌁ Cleaner
				</button>
				<button
					type="button"
					className={section === "service" ? "gm-subtab active" : "gm-subtab"}
					onClick={() => setSection("service")}
				>
					<span className={git.serviceReady ? "gm-led clean" : "gm-led offline"} /> Service
				</button>
				<button
					type="button"
					className={section === "performance" ? "gm-subtab active" : "gm-subtab"}
					onClick={() => setSection("performance")}
				>
					▤ Performance
				</button>
				<button
					type="button"
					className={section === "troubleshoot" ? "gm-subtab active" : "gm-subtab"}
					onClick={() => setSection("troubleshoot")}
				>
					🛠 Troubleshoot
				</button>
			</nav>

			{section === "preferences" ? (
				<SettingsPage
					preferences={git.preferences}
					directories={git.directories}
					onSave={handleSavePreferences}
					onScan={handleScan}
					onOpenCleaner={() => setSection("cleaner")}
					onOpenService={() => setSection("service")}
					onOpenPerformance={() => setSection("performance")}
					themeMode={themeMode}
					onChangeTheme={setThemeMode}
				/>
			) : null}

			{section === "cleaner" ? (
				<CleanerPage
					repositories={git.repositories}
					onRefresh={handleScan}
					initialRepoPath={repoParam}
				/>
			) : null}

			{section === "service" ? (
				<ServiceManagerPage
					serviceReady={git.serviceReady}
					status={git.status}
					onStart={handleRetryService}
				/>
			) : null}

			{section === "performance" ? (
				<PerformancePage serviceReady={git.serviceReady} scanning={git.scanning} />
			) : null}

			{section === "troubleshoot" ? (
				<TroubleshootPanel
					serviceReady={git.serviceReady}
					status={git.status}
					onRestartService={handleRetryService}
					onResetStorage={handleResetStorage}
					onTriggerHappyLogo={triggerHappyLogo}
				/>
			) : null}
		</section>
	);
}

export default function SettingsRoutePage() {
	return (
		<Suspense fallback={<RingLoadingIndicator label="Loading settings…" />}>
			<SettingsHubContent />
		</Suspense>
	);
}
