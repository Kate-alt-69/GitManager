"use client";

import { useEffect, useRef } from "react";
import { invoke } from "../../lib/tauri";
import { useWorkspace } from "../../lib/workspace-context";
import { AccountsPage } from "../../components/accounts-page";
import type { ProviderCliStatus } from "../../components/cli-auth-card";

/**
 * Connected Git Provider Accounts route (`/accounts/`).
 * Loads cached accounts and automatically syncs any already-authenticated
 * `gh` (GitHub CLI) or `glab` (GitLab CLI) session so signed-in users appear
 * under Connected Identities immediately without clicking Connect manually.
 */
export default function AccountsRoutePage() {
	const { git, triggerHappyLogo } = useWorkspace();
	const autoSyncedRef = useRef(false);

	useEffect(() => {
		void git.loadAccounts();
	}, [git.loadAccounts]);

	useEffect(() => {
		if (autoSyncedRef.current || git.accountsLoading) return;
		autoSyncedRef.current = true;
		let active = true;

		void invoke<ProviderCliStatus[]>("get_cli_auth_status")
			.then(async (statuses) => {
				if (!active) return;
				for (const item of statuses) {
					if (!item.authenticated) continue;
					const alreadyConnected = git.accounts.some(
						(acc) => acc.provider.toLowerCase() === item.provider.toLowerCase() && acc.session_connected,
					);
					if (!alreadyConnected) {
						try {
							await git.connectAccount(item.provider, "", "");
						} catch {
							// Best-effort auto-sync; user can still reconnect manually.
						}
					}
				}
			})
			.catch(() => undefined);

		return () => {
			active = false;
		};
	}, [git]);

	return (
		<AccountsPage
			accounts={git.accounts}
			loading={git.accountsLoading}
			onConnect={async (provider, username, token) => {
				const account = await git.connectAccount(provider, username, token);
				triggerHappyLogo();
				return account;
			}}
		/>
	);
}
