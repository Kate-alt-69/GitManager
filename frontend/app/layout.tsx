import type { Metadata } from "next";
import type { ReactNode } from "react";
import { GitManagerProviders } from "./providers";
import { GitManagerApp } from "../components/gitmanager-app";
import "./globals.css";

export const metadata: Metadata = {
	title: "GitManager",
	description: "GitManager by Kastrick — Classic Cozy Developer Git Workbench",
	icons: {
		icon: "/image/git-managar-orange_default-black.png",
	},
};

export default function RootLayout({ children }: { children: ReactNode }) {
	return (
		<html lang="en">
			<body>
				<GitManagerProviders>
					<GitManagerApp>{children}</GitManagerApp>
				</GitManagerProviders>
			</body>
		</html>
	);
}
