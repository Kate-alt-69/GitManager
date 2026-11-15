"use client";

import type { ReactNode } from "react";
import {
	EngineCollectedStyles,
	EngineProvider,
} from "@/engine/providers/EngineProvider";

/**
 * Establish the Generation 3 Engine context for the desktop UI.
 *
 * Existing screens remain mounted as a compatibility layer while individual
 * surfaces are migrated to Engine components. New Engine components share
 * the same provider and collected-style boundary.
 */
export function GitManagerProviders({ children }: { children: ReactNode }) {
	return (
		<EngineProvider>
			{children}
			<EngineCollectedStyles />
		</EngineProvider>
	);
}
