"use client";

import { isLightTheme, type ClassicThemeMode } from "../lib/classic-theme";

export type LogoMood = "idle" | "loading" | "saving" | "happy";

/**
 * Layered GitManager brand logo built from separate ring and center assets so
 * it can animate smoothly during indexing/loading, saving, and celebrate with
 * a bouncy saturated pop ("happy") when an operation succeeds.
 */
export function AnimatedBrandLogo({
	size = 32,
	mood = "idle",
	themeMode = "git-dark",
	label = "",
}: {
	size?: number;
	mood?: LogoMood;
	themeMode?: ClassicThemeMode;
	label?: string;
}) {
	const variant = isLightTheme(themeMode) ? "black" : "white";
	const ringSrc = `/image/git-managar-orange-ring_default-${variant}.png`;
	const centerSrc = `/image/git-managar-orange-center_default-${variant}.png`;

	return (
		<span
			className={`gm-anim-logo gm-anim-logo-${mood}`}
			style={{ width: size, height: size }}
			aria-label={label || undefined}
			aria-hidden={!label}
		>
			<img className="gm-anim-logo-ring" src={ringSrc} alt="" draggable={false} />
			<img className="gm-anim-logo-center" src={centerSrc} alt="" draggable={false} />
		</span>
	);
}
