import type { Metadata } from "next";
import type { ReactNode } from "react";
import { GitManagerProviders } from "./providers";
import "./globals.css";

export const metadata: Metadata = {
	title: "GitManager",
	description: "A focused workspace for your Git repositories",
};

export default function RootLayout({ children }: { children: ReactNode }) {
	return (
		<html lang="en">
			<body>
				<GitManagerProviders>{children}</GitManagerProviders>
			</body>
		</html>
	);
}
