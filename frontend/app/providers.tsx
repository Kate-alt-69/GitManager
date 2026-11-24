"use client";

import { useEffect } from "react";
import type { ReactNode } from "react";
import { EngineProvider, EngineCollectedStyles } from "../src/engine/providers/EngineProvider";
import { applyClassicThemeMode, gitmanagerClassicTheme, loadStoredThemeMode } from "../lib/classic-theme";

/**
 * Root client provider wiring Next.js Engine Gen 3 (`EngineProvider`,
 * `EngineCollectedStyles`, and the global `gitmanagerClassicTheme` tokens)
 * with persisted Cozy Dark / Classic Warm Light mode hydration.
 */
export function GitManagerProviders({ children }: { children: ReactNode }) {
	useEffect(() => {
		// Reference the validated global EngineTheme so bundlers keep registration intact.
		void gitmanagerClassicTheme.id;
		applyClassicThemeMode(loadStoredThemeMode(), false);
	}, []);

	return (
		<EngineProvider config={{ contentMaxWidth: "100%", gapBase: "0.75rem" }}>
			{children}
			<EngineCollectedStyles id="__gitmanager_engine_styles__" />
		</EngineProvider>
	);
}
