import type { Metadata } from "next";
import type { ReactNode } from "react";
import { GitManagerRuntime } from "../lib/runtime-bridge";
import "./globals.css";

export const metadata: Metadata = {
  title: "GitManager",
  description: "GitManager desktop application",
  icons: {
    icon: "/image/git-managar-orange_default-black.png",
  },
};

export default function RootLayout({ children }: { children: ReactNode }) {
  return (
    <html lang="en">
      <body>
        <GitManagerRuntime>{children}</GitManagerRuntime>
      </body>
    </html>
  );
}
