const withEngine = require("./src/engine/plugins/enginePlugin");

/** @type {import("next").NextConfig} */
const nextConfig = {
	output: "export",
	trailingSlash: true,
	reactStrictMode: true,
	images: {
		unoptimized: true,
	},
	turbopack: {
		resolveAlias: {
			fs: { browser: "./app/__turbopack_stub__.ts" },
			"fs/promises": { browser: "./app/__turbopack_stub__.ts" },
			path: { browser: "./app/__turbopack_stub__.ts" },
			os: { browser: "./app/__turbopack_stub__.ts" },
			crypto: { browser: "./app/__turbopack_stub__.ts" },
		},
	},
};

module.exports = withEngine(nextConfig);
