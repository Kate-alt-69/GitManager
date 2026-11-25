"use client";

import { useEffect, useMemo, useRef, useState } from "react";

export type TaskSceneState = "running" | "queued" | "idle";
export interface TaskScene {
  id: string;
  repositoryName: string;
  label: string;
  state: TaskSceneState;
  iconUrl?: string | null;
}

function RepositoryGlyph() {
  return <svg viewBox="0 0 24 24" aria-hidden="true" className="gm-repo-glyph"><path d="M5.25 4.5h13.5a1.25 1.25 0 0 1 1.25 1.25v12.5a1.25 1.25 0 0 1-1.25 1.25H5.25A1.25 1.25 0 0 1 4 18.25V5.75A1.25 1.25 0 0 1 5.25 4.5Z" /><path d="M8 4.5v15M11 9h5.25M11 13h5.25" /></svg>;
}
function SceneMark({ scene, className }: { scene: TaskScene; className: string }) {
  return <span className={className}>{scene.iconUrl ? <img className="gm-task-lens__repo-image" src={scene.iconUrl} alt="" draggable={false} /> : <RepositoryGlyph />}</span>;
}

export function TaskLens({ scenes, compact = false }: { scenes: TaskScene[]; compact?: boolean }) {
  const sceneKey = useMemo(() => scenes.map((scene) => scene.id).join("|"), [scenes]);
  const [activeId, setActiveId] = useState<string | null>(scenes[0]?.id ?? null);
  const [outgoingScene, setOutgoingScene] = useState<TaskScene | null>(null);
  const activeIndex = useRef(0);
  const activeScene = scenes.find((scene) => scene.id === activeId) ?? scenes[0] ?? null;
  const isRunning = scenes.some((scene) => scene.state === "running");
  const displayedScene: TaskScene = activeScene ?? { id: "idle", repositoryName: "Workspace", label: "No active tasks", state: "idle" };

  useEffect(() => {
    activeIndex.current = 0;
    setActiveId(scenes[0]?.id ?? null);
    setOutgoingScene(null);
  }, [sceneKey, scenes]);

  useEffect(() => {
    if (scenes.length < 2) return;
    let clearOutgoing: number | undefined;
    const interval = window.setInterval(() => {
      const currentIndex = activeIndex.current % scenes.length;
      const nextIndex = (currentIndex + 1) % scenes.length;
      setOutgoingScene(scenes[currentIndex]);
      setActiveId(scenes[nextIndex].id);
      activeIndex.current = nextIndex;
      if (clearOutgoing !== undefined) window.clearTimeout(clearOutgoing);
      clearOutgoing = window.setTimeout(() => setOutgoingScene(null), 560);
    }, 3300);
    return () => {
      window.clearInterval(interval);
      if (clearOutgoing !== undefined) window.clearTimeout(clearOutgoing);
    };
  }, [sceneKey, scenes]);

  return (
    <span className={"gm-task-lens" + (compact ? " gm-task-lens--compact" : "")} data-running={isRunning ? "true" : "false"} role={compact ? undefined : "status"} aria-live={compact ? "off" : "polite"} aria-label={compact ? undefined : displayedScene.repositoryName + ": " + displayedScene.label}>
      <span className="gm-task-lens__mark" aria-hidden="true">
        <img className="gm-task-lens__ring" src="/image/git-managar-orange-ring_default-white.png" alt="" draggable={false} />
        {outgoingScene && outgoingScene.id !== displayedScene.id ? <SceneMark key={"out-" + outgoingScene.id} scene={outgoingScene} className="gm-task-lens__scene gm-task-lens__scene--outgoing" /> : null}
        <SceneMark key={"in-" + displayedScene.id} scene={displayedScene} className="gm-task-lens__scene gm-task-lens__scene--incoming" />
      </span>
      {!compact ? <span className="gm-task-lens__copy"><span className="gm-task-lens__repository">{displayedScene.repositoryName}</span><span className="gm-task-lens__label">{displayedScene.label}</span></span> : null}
    </span>
  );
}
