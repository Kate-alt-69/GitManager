"use client";

import type { ReactNode } from "react";
import { EngineProvider, EngineCollectedStyles } from "../src/engine/providers/EngineProvider";

export function GitManagerProviders({ children }: { children: ReactNode }) {
	return (
		<EngineProvider config={{ contentMaxWidth: "1440px", gapBase: "1rem" }}>
			{children}
			<EngineCollectedStyles id="__gitmanager_engine_styles__" />
		</EngineProvider>
	);
}
