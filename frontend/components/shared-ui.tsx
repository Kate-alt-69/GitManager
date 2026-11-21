"use client";

import { useEffect, useRef, useState, type ReactNode } from "react";
import type { GitRepository } from "../lib/types";

export type ContextMenuAction = {
	label: string;
	onSelect: () => void;
	disabled?: boolean;
	danger?: boolean;
};

export function ContextMenu({
	x,
	y,
	items,
	onClose,
}: {
	x: number;
	y: number;
	items: ContextMenuAction[];
	onClose: () => void;
}) {
	const menuRef = useRef<HTMLDivElement | null>(null);
	const [position, setPosition] = useState({ left: x, top: y });

	useEffect(() => {
		const closeOnOutsidePointer = (event: PointerEvent) => {
			if (event.target instanceof Node && !menuRef.current?.contains(event.target)) onClose();
		};
		const closeOnEscape = (event: KeyboardEvent) => {
			if (event.key === "Escape") {
				event.preventDefault();
				onClose();
			}
		};
		const closeOnViewportChange = () => onClose();
		document.addEventListener("pointerdown", closeOnOutsidePointer);
		document.addEventListener("keydown", closeOnEscape);
		window.addEventListener("resize", closeOnViewportChange);
		const frame = window.requestAnimationFrame(() => {
			const bounds = menuRef.current?.getBoundingClientRect();
			if (!bounds) return;
			setPosition({
				left: Math.max(8, Math.min(x, window.innerWidth - bounds.width - 8)),
				top: Math.max(8, Math.min(y, window.innerHeight - bounds.height - 8)),
			});
			menuRef.current?.querySelector<HTMLButtonElement>("button:not(:disabled)")?.focus();
		});
		return () => {
			window.cancelAnimationFrame(frame);
			document.removeEventListener("pointerdown", closeOnOutsidePointer);
			document.removeEventListener("keydown", closeOnEscape);
			window.removeEventListener("resize", closeOnViewportChange);
		};
	}, [x, y, onClose]);

	return (
		<div
			ref={menuRef}
			className="gm-context-menu"
			role="menu"
			aria-label="Context actions"
			style={{ left: position.left, top: position.top }}
			onPointerDown={(event) => event.stopPropagation()}
			onContextMenu={(event) => event.preventDefault()}
		>
			{items.map((item) => (
				<button
					key={item.label}
					type="button"
					role="menuitem"
					className={item.danger ? "danger" : ""}
					disabled={item.disabled}
					onClick={() => { onClose(); item.onSelect(); }}
				>
					{item.label}
				</button>
			))}
		</div>
	);
}
export function Glyph({ children }: { children: string }) {
	return <span className="gm-glyph" aria-hidden="true">{children}</span>;
}

export function Button({
	children,
	onClick,
	variant = "secondary",
	disabled = false,
	className = "",
	type = "button",
}: {
	children: ReactNode;
	onClick?: () => void;
	variant?: "primary" | "secondary" | "ghost" | "danger";
	disabled?: boolean;
	className?: string;
	type?: "button" | "submit";
}) {
	return (
		<button
			type={type}
			className={`gm-button gm-button-${variant} ${className}`}
			onClick={onClick}
			disabled={disabled}
		>
			{children}
		</button>
	);
}

export function Field({
	label,
	value,
	onChange,
	placeholder,
	type = "text",
}: {
	label: string;
	value: string | number;
	onChange: (value: string) => void;
	placeholder?: string;
	type?: string;
}) {
	return (
		<label className="gm-field">
			<span>{label}</span>
			<input
				type={type}
				value={value}
				onChange={(event) => onChange(event.target.value)}
				placeholder={placeholder}
			/>
		</label>
	);
}

export function Switch({
	label,
	checked,
	onChange,
	description,
}: {
	label: string;
	checked: boolean;
	onChange: (checked: boolean) => void;
	description?: string;
}) {
	return (
		<label className="gm-switch-row">
			<span className="gm-switch-copy">
				<strong>{label}</strong>
				{description && <small>{description}</small>}
			</span>
			<input
				type="checkbox"
				checked={checked}
				onChange={(event) => onChange(event.target.checked)}
			/>
			<span className="gm-switch-track" aria-hidden="true"><i /></span>
		</label>
	);
}

export function ProviderMark({ provider }: { provider: string }) {
	const value = provider.toLowerCase();
	const label = value === "github" ? "GH" : value === "gitlab" ? "GL" : value === "local" ? "L" : "↗";
	return <span className={`gm-provider-mark gm-provider-${value}`}>{label}</span>;
}

export function RepoAvatar({ repo }: { repo: GitRepository }) {
	const letter = (repo.name?.trim().charAt(0) || "R").toUpperCase();
	let image = "";
	try {
		const url = new URL(repo.icon_url || "");
		const allowed = ["github.com", "gitlab.com", "avatars.githubusercontent.com"];
		if (url.protocol === "https:" && allowed.includes(url.hostname.toLowerCase())) image = url.href;
	} catch {
		image = "";
	}
	return (
		<span className="gm-repo-avatar">
			{image ? <img src={image} alt="" /> : <span>{letter}</span>}
		</span>
	);
}

export function EmptyState({
	icon,
	title,
	body,
	action,
}: {
	icon: string;
	title: string;
	body: string;
	action?: ReactNode;
}) {
	return (
		<div className="gm-empty-state">
			<div className="gm-empty-icon">{icon}</div>
			<h3>{title}</h3>
			<p>{body}</p>
			{action}
		</div>
	);
}
