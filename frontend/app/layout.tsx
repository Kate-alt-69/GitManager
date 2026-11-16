import type { Metadata } from "next";
import type { ReactNode } from "react";
import { GitManagerProviders } from "./providers";
import "./globals.css";

export const metadata: Metadata = {
	title: "GitManager",
	description: "GitManager by Kastrick — a focused workspace for your Git repositories",
	icons: {
		icon: "/image/git-managar-orange_default-black.png",
	},
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
