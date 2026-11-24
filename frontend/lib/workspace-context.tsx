"use client";

import { createContext, useContext } from "react";
import type { ClassicThemeMode } from "./classic-theme";
import type { LogoMood } from "../components/animated-brand-logo";
import type { useGitManager } from "./use-gitmanager";
import type { AppPreferences, GitRepository } from "./types";

export type GitManagerState = ReturnType<typeof useGitManager>;

export interface WorkspaceContextValue {
	git: GitManagerState;
	themeMode: ClassicThemeMode;
	logoMood: LogoMood;
	toggleTheme: () => void;
	setThemeMode: (mode: ClassicThemeMode) => void;
	triggerHappyLogo: () => void;
	handleScan: () => void;
	handleResetStorage: () => Promise<void>;
	handleSavePreferences: (next: AppPreferences) => Promise<AppPreferences>;
	handleRetryService: () => void;
	handleQueueRemoteCheck: () => void;
	navigateTo: (href: string, preset?: "fade" | "layout") => void;
	openRepository: (repo: GitRepository, view?: string) => void;
}

const WorkspaceContext = createContext<WorkspaceContextValue | null>(null);

export const WorkspaceProvider = WorkspaceContext.Provider;

/** Accesses shared GitManager IPC state and navigation actions from any route page. */
export function useWorkspace(): WorkspaceContextValue {
	const value = useContext(WorkspaceContext);
	if (!value) {
		throw new Error("useWorkspace() must be called inside GitManagerApp.");
	}
	return value;
}
