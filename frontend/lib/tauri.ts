export type TauriEvent<T> = { payload: T };
export type Unlisten = () => void;

type TauriGlobal = {
	core?: {
		invoke?: <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
	};
	event?: {
		listen?: <T>(event: string, handler: (event: TauriEvent<T>) => void) => Promise<Unlisten>;
	};
};

declare global {
	interface Window {
		__TAURI__?: TauriGlobal;
		__TAURI_INTERNALS__?: {
			invoke?: <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
		};
	}
}

export function isTauriRuntime(): boolean {
	return typeof window !== "undefined" && Boolean(
		window.__TAURI__?.core?.invoke || window.__TAURI_INTERNALS__?.invoke,
	);
}

export async function invoke<T>(
	command: string,
	args?: Record<string, unknown>,
): Promise<T> {
	if (typeof window === "undefined") {
		throw new Error("Tauri commands are only available in the desktop runtime.");
	}
	const call = window.__TAURI__?.core?.invoke ?? window.__TAURI_INTERNALS__?.invoke;
	if (!call) {
		throw new Error("GitManager's Tauri runtime was not detected.");
	}
	return call<T>(command, args);
}

export async function listen<T>(
	eventName: string,
	handler: (payload: T) => void,
): Promise<Unlisten> {
	const subscribe = window.__TAURI__?.event?.listen;
	if (!subscribe) return () => undefined;
	return subscribe<T>(eventName, (event) => handler(event.payload));
}
