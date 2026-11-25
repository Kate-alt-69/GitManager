"use client";

import { useCallback, useMemo, useState } from "react";
import { EngineBox, EngineStack, useEngineTransitions } from "@/engine";
import { LeftNavRail, type NavigationView } from "./left-nav-rail";
import { useGitManagerRuntime } from "../lib/runtime-bridge";
import type { TaskScene } from "./task-lens";

const viewContent: Record<NavigationView, { title: string; description: string }> = {
  repositories: {
    title: "Repositories",
    description: "The navigation foundation is ready. Repository cards and workspace content will be built next.",
  },
  actions: {
    title: "Actions",
    description: "The Actions workspace is reserved for the next UI layer.",
  },
  accounts: {
    title: "Accounts",
    description: "Connected accounts will be surfaced here in the next UI layer.",
  },
  settings: {
    title: "Settings",
    description: "Application settings will be added after the workspace foundation.",
  },
  notifications: {
    title: "Notifications",
    description: "Notification history and actions will be added in the Inbox layer.",
  },
  tasks: {
    title: "Tasks",
    description: "The task queue view will be built on top of the navigation task-scene contract.",
  },
};

export function GitManagerApp() {
  const runtime = useGitManagerRuntime();
  const transitions = useEngineTransitions();
  const [activeView, setActiveView] = useState<NavigationView>("repositories");
  const [navExpanded, setNavExpanded] = useState(false);
  const [inboxOpen, setInboxOpen] = useState(false);

  const taskScenes = useMemo<TaskScene[]>(() => {
    const scenes: TaskScene[] = [];
    if (runtime.scanning || runtime.serviceIndexing) {
      scenes.push({
        id: "repository-index",
        repositoryName: "Workspace",
        label: "Indexing repositories",
        state: "running",
      });
    }
    if (runtime.remoteCheckRunning) {
      scenes.push({
        id: "remote-status",
        repositoryName: "Workspace",
        label: "Refreshing remote status",
        state: "running",
      });
    }
    if (runtime.remoteCheckPending) {
      scenes.push({
        id: "remote-status-queued",
        repositoryName: "Workspace",
        label: "Remote status queued",
        state: "queued",
      });
    }
    return scenes;
  }, [runtime.scanning, runtime.serviceIndexing, runtime.remoteCheckRunning, runtime.remoteCheckPending]);

  const unreadCount = runtime.notifications.reduce((count, notification) => count + (notification.read ? 0 : 1), 0);

  const navigate = useCallback((view: NavigationView) => {
    if (view === activeView) return;
    void transitions.run(
      () => setActiveView(view),
      { type: "fade", duration: 190 },
    ).catch(() => setActiveView(view));
  }, [activeView, transitions]);

  const collapseFromWorkspace = () => {
    if (navExpanded) setNavExpanded(false);
    if (inboxOpen) setInboxOpen(false);
  };

  const serviceLabel = runtime.error
    ? "Service issue"
    : runtime.loading
      ? "Connecting..."
      : runtime.serviceReady
        ? (taskScenes.length > 0 ? "Working" : "Service ready")
        : runtime.available
          ? "Service unavailable"
          : "Desktop app";

  const content = viewContent[activeView];

  return (
    <EngineBox className="gm-app-shell">
      <LeftNavRail
        expanded={navExpanded}
        onExpandedChange={setNavExpanded}
        activeView={activeView}
        onNavigate={navigate}
        inboxOpen={inboxOpen}
        onInboxOpenChange={setInboxOpen}
        unreadCount={unreadCount}
        taskScenes={taskScenes}
        serviceLabel={serviceLabel}
      />

      <main className="gm-workspace" onPointerDown={collapseFromWorkspace}>
        <EngineBox className="gm-workspace__content">
          <EngineStack direction="vertical" className="gm-workspace__stack">
            <section className="gm-workspace__heading" key={activeView} aria-live="polite">
              <span className="gm-workspace__eyebrow">GITMANAGER</span>
              <h1>{content.title}</h1>
              <p>{content.description}</p>
            </section>
          </EngineStack>
        </EngineBox>
      </main>
    </EngineBox>
  );
}
