"use client";

import { createContext, useContext, useEffect, type ReactNode } from "react";
import { invoke } from "./tauri";
import { useGitManager } from "./use-gitmanager";

type GitManagerRuntimeState = ReturnType<typeof useGitManager>;

const GitManagerRuntimeContext = createContext<GitManagerRuntimeState | null>(null);

export function useGitManagerRuntime(): GitManagerRuntimeState {
  const value = useContext(GitManagerRuntimeContext);
  if (!value) {
    throw new Error("useGitManagerRuntime must be used inside GitManagerRuntime.");
  }
  return value;
}

/**
 * Owns the existing UI-to-backend hook once and shares its snapshot with the
 * UI. The native watchdog heartbeat remains unchanged.
 */
export function GitManagerRuntime({ children }: { children: ReactNode }) {
  const runtime = useGitManager();

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

  return (
    <GitManagerRuntimeContext.Provider value={runtime}>
      {children}
    </GitManagerRuntimeContext.Provider>
  );
}
