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
		<main role="alert" style={{ minHeight: "100vh", display: "grid", placeContent: "center", padding: "2rem", fontFamily: "system-ui, sans-serif" }}>
			<h1>Something went wrong.</h1>
			<p>GitManager could not render this view. Your other workspace pages may still work.</p>
			<button type="button" onClick={reset} style={{ justifySelf: "start", padding: "0.6rem 1rem", cursor: "pointer" }}>Try again</button>
		</main>
	);
}
