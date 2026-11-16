"use client";

import { useEffect } from "react";

type GitManagerRouteErrorProps = {
	error: Error & { digest?: string };
	reset: () => void;
};

export default function GitManagerRouteError({ error, reset }: GitManagerRouteErrorProps) {
	useEffect(() => {
		console.error("[GitManager] Route rendering failed", {
			message: error.message,
			digest: error.digest,
		});
	}, [error]);

	return (
		<main role="alert" style={{ minHeight: "100vh", display: "grid", placeContent: "center", justifyItems: "start", padding: "2rem", fontFamily: "system-ui, sans-serif" }}>
			<img src="/image/git-managar-orange_default-white.png" alt="" width="64" height="64" style={{ objectFit: "contain" }} />
			<div style={{ display: "grid", gap: "0.15rem", marginTop: "0.5rem" }}>
				<strong>GitManager</strong>
				<span>By Kastrick</span>
			</div>
			<h1>Something went wrong.</h1>
			<p>GitManager could not render this view. Your other workspace pages may still work.</p>
			<button type="button" onClick={reset} style={{ justifySelf: "start", padding: "0.6rem 1rem", cursor: "pointer" }}>Try again</button>
		</main>
	);
}
