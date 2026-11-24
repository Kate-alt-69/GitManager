"use client";

import { useEffect, useState } from "react";
import { isLightTheme, type ClassicThemeMode } from "../lib/classic-theme";

const REST_ANGLES = [90, 180, 240, 360] as const;

function pickRestAngle(seed: string): number {
	let hash = 0;
	for (let i = 0; i < seed.length; i++) {
		hash = (hash * 31 + seed.charCodeAt(i)) | 0;
	}
	return REST_ANGLES[Math.abs(hash) % REST_ANGLES.length];
}

/**
 * 1:1 Square Repository Icon wrapped by GitManager's real Ring PNG asset.
 * - While `working` is true: the 1:1 repo icon scales slightly inward while the
 *   GitManager ring scales out around it and spins continuously.
 * - When `working` turns false: the ring snaps to 90°/180°/240°/360° and zooms
 *   back behind the 1:1 repo icon while the icon scales back to full size.
 */
export function RepoRingBadge({
	label,
	working,
	size = 38,
	themeMode = "git-dark",
}: {
	label: string;
	working: boolean;
	size?: number;
	themeMode?: ClassicThemeMode;
}) {
	const [restAngle, setRestAngle] = useState<number>(() => pickRestAngle(label));
	const variant = isLightTheme(themeMode) ? "black" : "white";
	const ringSrc = `/static/image/git-managar-orange-ring_default-${variant}.png`;
	const initials = (label.replace(/[^a-zA-Z0-9]/g, "").slice(0, 2) || "GM").toUpperCase();

	useEffect(() => {
		if (!working) {
			setRestAngle(pickRestAngle(`${label}-${Date.now()}`));
		}
	}, [label, working]);

	return (
		<div
			className={working ? "gm-ring-badge working" : "gm-ring-badge idle"}
			style={{ width: size, height: size }}
			aria-hidden="true"
		>
			<img
				src={ringSrc}
				alt=""
				className={working ? "gm-ring-layer spinning" : "gm-ring-layer tucked"}
				style={working ? undefined : { transform: `rotate(${restAngle}deg) scale(0.48)` }}
				draggable={false}
			/>
			<div className="gm-ring-square-icon">{initials}</div>
		</div>
	);
}

/**
 * Global GitManager Ring Loading Indicator for route transitions, workspace
 * loading states, and async panels.
 */
export function RingLoadingIndicator({
	label = "Loading…",
	size = 36,
	themeMode = "git-dark",
}: {
	label?: string;
	size?: number;
	themeMode?: ClassicThemeMode;
}) {
	const variant = isLightTheme(themeMode) ? "black" : "white";
	const ringSrc = `/static/image/git-managar-orange-ring_default-${variant}.png`;
	const centerSrc = `/static/image/git-managar-orange-center_default-${variant}.png`;

	return (
		<div className="gm-ring-loader" role="status" aria-live="polite">
			<div className="gm-ring-badge working" style={{ width: size, height: size }}>
				<img src={ringSrc} alt="" className="gm-ring-layer spinning" draggable={false} />
				<img src={centerSrc} alt="" className="gm-ring-center-img" draggable={false} />
			</div>
			{label ? <span className="gm-muted">{label}</span> : null}
		</div>
	);
}
