"use client";

import { useRef, useState, type KeyboardEvent, type MouseEvent, type PointerEvent } from "react";
import { EngineBox, EngineStack } from "@/engine";
import { AnimatedBrandLogo } from "./animated-brand-logo";
import { TaskLens, type TaskScene } from "./task-lens";

export type NavigationView = "repositories" | "actions" | "accounts" | "settings" | "notifications" | "tasks";
type PrimaryView = Extract<NavigationView, "repositories" | "actions" | "accounts" | "settings">;
const primaryItems: Array<{ id: PrimaryView; label: string; icon: "repository" | "actions" | "accounts" | "settings" }> = [
  { id: "repositories", label: "Repositories", icon: "repository" },
  { id: "actions", label: "Actions", icon: "actions" },
  { id: "accounts", label: "Accounts", icon: "accounts" },
  { id: "settings", label: "Settings", icon: "settings" },
];

function NavigationIcon({ name }: { name: "repository" | "actions" | "accounts" | "settings" | "inbox" | "notifications" | "tasks" }) {
  const common = { className: "gm-icon", viewBox: "0 0 24 24", fill: "none", stroke: "currentColor", strokeWidth: 1.7, strokeLinecap: "round" as const, strokeLinejoin: "round" as const, "aria-hidden": true as const };
  switch (name) {
    case "repository": return <svg {...common}><path d="M5.25 4.5h13.5A1.25 1.25 0 0 1 20 5.75v12.5a1.25 1.25 0 0 1-1.25 1.25H5.25A1.25 1.25 0 0 1 4 18.25V5.75A1.25 1.25 0 0 1 5.25 4.5Z" /><path d="M8 4.5v15M11 9h5.25M11 13h5.25" /></svg>;
    case "actions": return <svg {...common}><path d="m13.2 2.8-8 11h5.9l-.3 7.4 8-11.3h-5.8l.2-7.1Z" /></svg>;
    case "accounts": return <svg {...common}><path d="M16 20v-1.4a3.6 3.6 0 0 0-3.6-3.6H7.6A3.6 3.6 0 0 0 4 18.6V20" /><circle cx="10" cy="7.5" r="3.5" /><path d="M16 4.4a3.5 3.5 0 0 1 0 6.8M17.5 14.8a3.6 3.6 0 0 1 2.5 3.4V20" /></svg>;
    case "settings": return <svg {...common}><path d="M12 8.2a3.8 3.8 0 1 0 0 7.6 3.8 3.8 0 0 0 0-7.6Z" /><path d="m19.4 13.5 1.1.9-1.6 2.8-1.4-.5a7.9 7.9 0 0 1-1.7 1l-.2 1.5h-3.2l-.3-1.5a7.9 7.9 0 0 1-1.7-1l-1.4.5-1.6-2.8 1.1-.9a7.7 7.7 0 0 1 0-2l-1.1-.9 1.6-2.8 1.4.5a7.9 7.9 0 0 1 1.7-1l.3-1.5h3.2l.2 1.5a7.9 7.9 0 0 1 1.7 1l1.4-.5 1.6 2.8-1.1.9a7.7 7.7 0 0 1 0 2Z" /></svg>;
    case "inbox": return <svg {...common}><path d="M4.2 4.5h15.6v15H4.2z" /><path d="M4.2 13.2h4.2l1.5 2h4.2l1.5-2h4.2" /></svg>;
    case "notifications": return <svg {...common}><path d="M18 9a6 6 0 0 0-12 0c0 7-2.3 7-2.3 8.6h16.6C20.3 16 18 16 18 9Z" /><path d="M10 21h4" /></svg>;
    case "tasks": return <svg {...common}><path d="m4 7 1.5 1.5L8 6" /><path d="M11 7h9M4 13l1.5 1.5L8 12" /><path d="M11 13h9M4 19l1.5 1.5L8 18" /><path d="M11 19h9" /></svg>;
  }
}

export function LeftNavRail({ expanded, onExpandedChange, activeView, onNavigate, inboxOpen, onInboxOpenChange, unreadCount, taskScenes, serviceLabel }: {
  expanded: boolean; onExpandedChange: (expanded: boolean) => void; activeView: NavigationView; onNavigate: (view: NavigationView) => void;
  inboxOpen: boolean; onInboxOpenChange: (open: boolean) => void; unreadCount: number; taskScenes: TaskScene[]; serviceLabel: string;
}) {
  const [expandedWidth, setExpandedWidth] = useState(252);
  const drag = useRef<{ pointerId: number; startX: number; startWidth: number } | null>(null);
  const activate = (view: NavigationView) => { onNavigate(view); onInboxOpenChange(false); };
  const handleRailClick = (event: MouseEvent<HTMLElement>) => {
    const target = event.target as HTMLElement;
    if (target.closest("button, a, [role='separator']")) return;
    if (!expanded) onExpandedChange(true);
  };
  const handleResizeStart = (event: PointerEvent<HTMLDivElement>) => {
    event.preventDefault(); event.stopPropagation();
    drag.current = { pointerId: event.pointerId, startX: event.clientX, startWidth: expandedWidth };
    onExpandedChange(true); event.currentTarget.setPointerCapture(event.pointerId);
  };
  const handleResizeMove = (event: PointerEvent<HTMLDivElement>) => {
    if (!drag.current || drag.current.pointerId !== event.pointerId) return;
    setExpandedWidth(Math.max(216, Math.min(340, drag.current.startWidth + event.clientX - drag.current.startX)));
  };
  const handleResizeEnd = (event: PointerEvent<HTMLDivElement>) => {
    if (!drag.current || drag.current.pointerId !== event.pointerId) return;
    drag.current = null;
    if (event.currentTarget.hasPointerCapture(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId);
  };
  const handleResizeKey = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
    event.preventDefault(); onExpandedChange(true);
    setExpandedWidth((current) => Math.max(216, Math.min(340, current + (event.key === "ArrowRight" ? 12 : -12))));
  };
  const unreadLabel = unreadCount > 99 ? "99+" : String(unreadCount);
  return (
    <aside className="gm-rail" data-expanded={expanded ? "true" : "false"} style={{ width: expanded ? expandedWidth : 72 }} aria-label="GitManager navigation" onClick={handleRailClick}>
      <EngineBox className="gm-rail__surface">
        <EngineBox className="gm-rail__brand"><AnimatedBrandLogo expanded={expanded} /><span className="gm-rail__brand-copy"><strong>GitManager</strong><span className="gm-rail__service"><i className={serviceLabel === "Service ready" ? "gm-service-dot is-ready" : "gm-service-dot"} />{serviceLabel}</span></span></EngineBox>
        <EngineStack direction="vertical" className="gm-primary-nav">
          {primaryItems.map((item) => <button key={item.id} className={"gm-nav-item" + (activeView === item.id ? " is-active" : "")} type="button" aria-current={activeView === item.id ? "page" : undefined} aria-label={item.label} title={expanded ? undefined : item.label} onClick={(event) => { event.stopPropagation(); activate(item.id); }}>
            <span className="gm-nav-item__icon"><NavigationIcon name={item.icon} /></span><span className="gm-nav-item__label">{item.label}</span>
          </button>)}
        </EngineStack>
        <EngineBox className="gm-rail__bottom">
          <div className={"gm-inbox-flyout" + (inboxOpen ? " is-open" : "")} aria-hidden={!inboxOpen} id="gm-inbox-flyout">
            <button type="button" className={"gm-nav-item gm-inbox-flyout__item" + (activeView === "notifications" ? " is-active" : "")} aria-current={activeView === "notifications" ? "page" : undefined} tabIndex={inboxOpen ? 0 : -1} onClick={(event) => { event.stopPropagation(); activate("notifications"); }}>
              <span className="gm-nav-item__icon"><NavigationIcon name="notifications" /></span><span className="gm-nav-item__label">Notifications</span><span className="gm-nav-count"><span>{unreadCount > 0 ? unreadLabel + " new" : "All read"}</span></span>
            </button>
            <button type="button" className={"gm-nav-item gm-inbox-flyout__item" + (activeView === "tasks" ? " is-active" : "")} aria-current={activeView === "tasks" ? "page" : undefined} tabIndex={inboxOpen ? 0 : -1} onClick={(event) => { event.stopPropagation(); activate("tasks"); }}>
              <span className="gm-nav-item__icon"><NavigationIcon name="tasks" /></span><span className="gm-nav-item__label">Tasks</span><span className="gm-nav-count"><span>{taskScenes.length > 0 ? taskScenes.length + " active" : "No active tasks"}</span></span>
            </button>
            <EngineBox className="gm-inbox-flyout__task-preview"><TaskLens scenes={taskScenes} /></EngineBox>
          </div>
          <button type="button" className={"gm-nav-item gm-inbox-trigger" + (inboxOpen ? " is-open" : "") + (activeView === "notifications" || activeView === "tasks" ? " is-active" : "")} aria-expanded={inboxOpen} aria-controls="gm-inbox-flyout" aria-label="Inbox" title={expanded ? undefined : "Inbox"} onClick={(event) => { event.stopPropagation(); onInboxOpenChange(!inboxOpen); }}>
            <span className="gm-nav-item__icon"><NavigationIcon name="inbox" /></span><span className="gm-nav-item__label">Inbox</span><span className="gm-inbox-trigger__trailing">{unreadCount > 0 ? <span className="gm-unread-badge">{unreadLabel}</span> : null}{taskScenes.length > 0 ? <TaskLens scenes={taskScenes} compact /> : null}</span>
          </button>
        </EngineBox>
      </EngineBox>
      <div className="gm-rail__resize" role="separator" aria-label="Resize navigation" aria-orientation="vertical" aria-valuemin={216} aria-valuemax={340} aria-valuenow={expandedWidth} tabIndex={0} onPointerDown={handleResizeStart} onPointerMove={handleResizeMove} onPointerUp={handleResizeEnd} onPointerCancel={handleResizeEnd} onKeyDown={handleResizeKey} />
    </aside>
  );
}
