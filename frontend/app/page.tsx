import { readFileSync } from "node:fs";
import path from "node:path";

function readLegacyWorkspaceMarkup(): string {
	const source = path.join(process.cwd(), "public", "gitmanager", "index.html");
	const html = readFileSync(source, "utf8");
	const body = html.match(/<body[^>]*>([\s\S]*?)<\/body>/i)?.[1];

	if (!body) {
		throw new Error("GitManager's existing workspace HTML does not contain a body.");
	}

	// Script elements are loaded from the static public directory by the layout.
	// They must not be inserted with innerHTML because browsers do not execute
	// script nodes created through this API.
	return body.replace(/<script\b[^>]*>[\s\S]*?<\/script\s*>/gi, "").trim();
}

const workspaceMarkup = readLegacyWorkspaceMarkup();

export default function HomePage() {
	return <div id="gitmanager-next-workspace" dangerouslySetInnerHTML={{ __html: workspaceMarkup }} />;
}
