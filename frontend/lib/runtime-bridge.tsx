"use client";

import { useEffect, type ReactNode } from "react";
import { invoke } from "./tauri";
import { useGitManager } from "./use-gitmanager";

/**
 * Keeps the existing UI-to-backend integration alive without rendering or
 * depending on any of the retired application UI.
 *
 * The three-second heartbeat is part of the native crash-supervisor contract.
 */
export function GitManagerRuntime({ children }: { children: ReactNode }) {
  useGitManager();

  useEffect(() => {
    let active = true;
    const reportHeartbeat = () => {
      if (active) void invoke("report_ui_heartbeat").catch(() => undefined);
    };

    reportHeartbeat();
    const heartbeat = window.setInterval(reportHeartbeat, 3000);

    return () => {
      active = false;
      window.clearInterval(heartbeat);
    };
  }, []);

  return <>{children}</>;
}
