"use client";

import { createTheme, coordinateEngineViewTransition } from "@/engine";

export type ClassicThemeMode = "git-dark" | "git-light" | "cozy-dark" | "midnight";

export const THEME_OPTIONS: Array<{
	id: ClassicThemeMode;
	label: string;
	description: string;
	isLight: boolean;
}> = [
	{
		id: "git-dark",
		label: "Git Dark",
		description: "Neutral obsidian graphite with Git ember-orange accents",
		isLight: false,
	},
	{
		id: "git-light",
		label: "Git Light",
		description: "Crisp paper light workspace with Git ember-orange accents",
		isLight: true,
	},
	{
		id: "cozy-dark",
		label: "Cozy Dark",
		description: "Warm espresso-charcoal tones for long late-night sessions",
		isLight: false,
	},
	{
		id: "midnight",
		label: "Midnight OLED",
		description: "Pure black contrast canvas with sharp orange highlights",
		isLight: false,
	},
];

const STORAGE_KEY = "gitmanager.theme.mode";

/**
 * Validated Next.js Engine Gen 3 theme definition for GitManager.
 * Uses neutral obsidian-graphite surfaces paired with Git's signature ember orange.
 */
export const gitmanagerClassicTheme = createTheme({
	id: "gitmanager-classic",
	scope: "global",
	name: "GitManager Theme",
	tokens: {
		colors: {
			canvas: "#111214",
			surface: "#181a1d",
			surfaceRaised: "#202328",
			border: "#2c3038",
			text: "#eef0f3",
			muted: "#959ca8",
			accent: "#f05032",
			added: "#4ade80",
			modified: "#f59e0b",
			deleted: "#f87171",
		},
	},
	markdown: {
		defaults: {
			codeBackground: "#1b1e23",
			codeColor: "#eef0f3",
		},
		styles: {
			codeBlock: {
				borderColor: "#2c3038",
			},
		},
		variants: {
			light: {
				defaults: {
					codeBackground: "#f2f3f5",
					codeColor: "#181a1d",
				},
			},
		},
	},
});

export function isLightTheme(mode: ClassicThemeMode): boolean {
	return mode === "git-light";
}

/** Reads the user's persisted theme preference, defaulting to Git Dark. */
export function loadStoredThemeMode(): ClassicThemeMode {
	if (typeof window === "undefined") return "git-dark";
	try {
		const saved = window.localStorage.getItem(STORAGE_KEY) || window.localStorage.getItem("gitmanager.classic.theme");
		if (saved === "light" || saved === "git-light") return "git-light";
		if (saved === "cozy-dark") return "cozy-dark";
		if (saved === "midnight") return "midnight";
		return "git-dark";
	} catch {
		return "git-dark";
	}
}

/**
 * Applies the chosen theme preset through Next.js Engine's shared View Transition
 * coordinator so theme changes animate smoothly without clashing with route transitions.
 */
export function applyClassicThemeMode(mode: ClassicThemeMode, animate = true): void {
	if (typeof document === "undefined") return;
	const scheme = isLightTheme(mode) ? "light" : "dark";
	const apply = () => {
		document.documentElement.dataset.gmTheme = mode;
		document.documentElement.style.colorScheme = scheme;
		try {
			window.localStorage.setItem(STORAGE_KEY, mode);
		} catch {
			// Ignore storage quota / private-mode restrictions.
		}
	};

	if (animate) {
		void coordinateEngineViewTransition(apply);
	} else {
		apply();
	}
}
