"use client";

import type { ReactNode } from "react";
import type { GitRepository } from "../lib/types";

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
