import type { GitRepository } from "./types";

export type MainSection = "repositories" | "accounts" | "settings" | "cleaner";

export type RepositoryDestination =
	| { kind: "overview" }
	| { kind: "conflicts" }
	| { kind: "commit-browser" }
	| { kind: "commit-details"; branch: string; sha: string }
	| { kind: "pull-browser" }
	| { kind: "pull-request"; number: number }
	| { kind: "pull-conflicts" };

export type InternalRoute =
	| { kind: "repositories"; path: "/r" }
	| { kind: "accounts"; path: "/a" }
	| { kind: "settings"; path: "/s" }
	| { kind: "service"; path: "/s/service" }
	| { kind: "performance"; path: "/s/performance" }
	| { kind: "cleaner"; path: "/tools/cleaner" }
	| {
			kind: "repository";
			owner: string;
			repository: string;
			destination: RepositoryDestination;
			path: string;
	  }
	| { kind: "unknown"; path: string };

const SECTION_PATHS: Record<MainSection, string> = {
	repositories: "/r",
	accounts: "/a",
	settings: "/s",
	cleaner: "/tools/cleaner",
};

function decodeSegment(value: string): string {
	try {
		return decodeURIComponent(value);
	} catch {
		return value;
	}
}

function encodeSegment(value: string): string {
	return encodeURIComponent(value.trim() || "unknown");
}

function stablePathSuffix(path: string): string {
	let hash = 0x811c9dc5;
	for (let index = 0; index < path.length; index += 1) {
		hash ^= path.charCodeAt(index);
		hash = Math.imul(hash, 0x01000193);
	}
	return (hash >>> 0).toString(16).padStart(8, "0");
}

/** Build the canonical internal destination for a locally indexed repository. */
export function repositoryRouteForItem(repository: GitRepository): string {
	const remote = repository.remotes.find((item) => item.name === "origin")
		?? repository.remotes[0];
	if (remote?.owner && remote.repo_slug) {
		return `/r/${encodeSegment(remote.owner)}/${encodeSegment(remote.repo_slug)}/`;
	}

	// Local-only repositories still receive stable, collision-resistant route identities.
	const localSlug = `${repository.name}-${stablePathSuffix(repository.path)}`;
	return `/r/local/${encodeSegment(localSlug)}/`;
}

export function sectionRoute(section: MainSection): string {
	return SECTION_PATHS[section];
}

export function parseInternalRoute(input: string): InternalRoute {
	const path = input.startsWith("/") ? input : `/${input}`;
	const normalized = path.replace(/\/{2,}/g, "/").replace(/\/$/, "") || "/";

	if (normalized === "/r") return { kind: "repositories", path: "/r" };
	if (normalized === "/a") return { kind: "accounts", path: "/a" };
	if (normalized === "/s") return { kind: "settings", path: "/s" };
	if (normalized === "/s/service") return { kind: "service", path: "/s/service" };
	if (normalized === "/s/performance") return { kind: "performance", path: "/s/performance" };
	if (normalized === "/tools/cleaner") return { kind: "cleaner", path: "/tools/cleaner" };

	const segments = normalized.split("/").slice(1);
	if (segments.length < 2 || segments[0] !== "r") {
		return { kind: "unknown", path: normalized };
	}

	const owner = decodeSegment(segments[1] ?? "");
	const repository = decodeSegment(segments[2] ?? "");
	if (!owner || !repository) return { kind: "unknown", path: normalized };

	const tail = segments.slice(3);
	if (tail.length === 0) {
		return {
			kind: "repository",
			owner,
			repository,
			destination: { kind: "overview" },
			path: `${normalized}/`,
		};
	}

	let destination: RepositoryDestination | null = null;
	if (tail.length === 2 && tail[0] === "viewer" && tail[1] === "conflict") {
		destination = { kind: "conflicts" };
	} else if (tail.length === 2 && tail[0] === "viewer" && tail[1] === "commit") {
		destination = { kind: "commit-browser" };
	} else if (
		tail.length === 4
		&& tail[0] === "viewer"
		&& tail[1] === "c"
		&& tail[2].startsWith("b_")
	) {
		destination = {
			kind: "commit-details",
			branch: decodeSegment(tail[2].slice(2)),
			sha: decodeSegment(tail[3]),
		};
	} else if (tail.length === 2 && tail[0] === "viewer" && tail[1] === "pull") {
		destination = { kind: "pull-browser" };
	} else if (tail.length === 2 && tail[0] === "viewer" && tail[1] === "pull-conflict") {
		destination = { kind: "pull-conflicts" };
	} else if (tail.length === 3 && tail[0] === "viewer" && tail[1] === "pull" && tail[2] === "conflict") {
		destination = { kind: "pull-conflicts" };
	} else if (tail.length === 2 && tail[0] === "pull" && /^\d+$/.test(tail[1])) {
		destination = { kind: "pull-request", number: Number(tail[1]) };
	}

	if (!destination) return { kind: "unknown", path: normalized };
	return { kind: "repository", owner, repository, destination, path: normalized };
}

export function sectionForRoute(route: InternalRoute): MainSection {
	if (route.kind === "accounts") return "accounts";
	if (route.kind === "settings" || route.kind === "service" || route.kind === "performance") return "settings";
	if (route.kind === "cleaner") return "cleaner";
	return "repositories";
}

export function routeLabel(route: InternalRoute): string {
	switch (route.kind) {
		case "repositories": return "Repositories";
		case "accounts": return "Accounts";
		case "settings": return "Settings";
		case "service": return "Service Manager";
		case "performance": return "Performance Center";
		case "cleaner": return "Developer Cleaner";
		case "unknown": return "Workspace";
		case "repository": {
			switch (route.destination.kind) {
				case "overview": return `${route.owner}/${route.repository}`;
				case "conflicts": return "Conflicts";
				case "commit-browser": return "Commit Graph";
				case "commit-details": return "Commit Details";
				case "pull-browser": return "Pull Requests";
				case "pull-request": return `Pull #${route.destination.number}`;
				case "pull-conflicts": return "Pull Conflicts";
			}
		}
	}
}

export function repositorySubroute(
	owner: string,
	repository: string,
	destination: RepositoryDestination,
): string {
	const base = `/r/${encodeSegment(owner)}/${encodeSegment(repository)}`;
	switch (destination.kind) {
		case "overview": return `${base}/`;
		case "conflicts": return `${base}/viewer/conflict`;
		case "commit-browser": return `${base}/viewer/commit`;
		case "commit-details":
			return `${base}/viewer/c/b_${encodeSegment(destination.branch)}/${encodeSegment(destination.sha)}`;
		case "pull-browser": return `${base}/viewer/pull`;
		case "pull-request": return `${base}/pull/${destination.number}`;
		case "pull-conflicts": return `${base}/viewer/pull/conflict`;
	}
}
