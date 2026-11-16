"use client";

import { useEffect } from "react";

type GitManagerGlobalErrorProps = {
	error: Error & { digest?: string };
	unstable_retry: () => void;
};

export default function GitManagerGlobalError({ error, unstable_retry }: GitManagerGlobalErrorProps) {
	useEffect(() => {
		console.error("[GitManager] Application rendering failed", {
			message: error.message,
			digest: error.digest,
		});
	}, [error]);

	return (
		<html lang="en">
			<body style={{ minHeight: "100vh", margin: 0, display: "grid", placeContent: "center", padding: "2rem", fontFamily: "system-ui, sans-serif" }}>
				<main role="alert">
					<img src="/image/git-managar-orange_default-black.png" alt="GitManager" width="64" height="64" style={{ objectFit: "contain" }} />
					<p><strong>By Kastrick</strong></p>
					<h1>GitManager encountered an application error.</h1>
					<p>Try again to reload the affected application tree.</p>
					<button type="button" onClick={unstable_retry} style={{ padding: "0.6rem 1rem", cursor: "pointer" }}>Try again</button>
				</main>
			</body>
		</html>
	);
}
