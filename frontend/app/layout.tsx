import type { ReactNode } from "react";
import { GitManagerProviders } from "./providers";

export const metadata = {
	title: "GitManager",
	description: "Your Git workspace",
};

export default function RootLayout({ children }: { children: ReactNode }) {
	return (
		<html lang="en">
			<head>
				<link rel="stylesheet" href="/gitmanager/styles.css" />
			</head>
			<body>
				<GitManagerProviders>{children}</GitManagerProviders>
				<script src="/gitmanager/app.js" defer />
				<script src="/gitmanager/cleaner.js" defer />
			</body>
		</html>
	);
}
