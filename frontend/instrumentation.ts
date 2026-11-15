/**
 * GitManager is exported as a static Tauri WebView application. It does not ship
 * a Next.js server runtime, so no Node-only process monitors or request hooks
 * are installed in the generated workspace.
 */
export function register(): void {
	// The native Rust process owns desktop lifecycle and diagnostics.
}
