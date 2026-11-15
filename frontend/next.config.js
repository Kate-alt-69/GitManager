const path = require("node:path");
const withEngine = require("../src/engine/plugins/enginePlugin");

/** @type {import('next').NextConfig} */
const nextConfig = {
	output: "export",
	distDir: "dist",
	reactStrictMode: true,
	images: {
		unoptimized: true,
	},
	turbopack: {
		resolveAlias: {
			"@/engine": path.resolve(__dirname, "../src/engine"),
		},
	},
};

module.exports = withEngine(nextConfig);
