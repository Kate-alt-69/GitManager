export interface CommitNode {
	oid: string;
	short_oid: string;
	message: string;
	summary: string;
	author_name: string;
	author_email: string;
	timestamp: number;
	date_iso: string;
	parents: string[];
	branches: string[];
	tags: string[];
	is_head: boolean;
	lane: number;
}

export interface BranchInfo {
	name: string;
	is_head: boolean;
	is_remote: boolean;
	target_oid: string;
	upstream?: string | null;
	is_default: boolean;
}

export interface RemoteEndpoint {
	name: string;
	url: string;
	provider: string;
	host: string;
	owner?: string | null;
	repo_slug?: string | null;
	web_url?: string | null;
	icon_url: string;
}

export interface GitRepository {
	id: string;
	name: string;
	path: string;
	git_dir: string;
	parent_folder: string;
	owner_name: string;
	owner_email: string;
	current_branch: string;
	has_remote: boolean;
	primary_provider: string;
	primary_remote_url?: string | null;
	web_url?: string | null;
	icon_url: string;
	remotes: RemoteEndpoint[];
	branches: BranchInfo[];
	commits: CommitNode[];
	dirty_files_count: number;
	total_commits_loaded: number;
	source_kind: string;
}

export interface AppPreferences {
	scanRoots: string[];
	maxScanDepth: number;
	includeHiddenDirs: boolean;
	indexDependencyRepositories: boolean;
	showDependencyRepositories: boolean;
	firstRunComplete: boolean;
	archiveRoot: string;
	archiveLocalEnabled: boolean;
	archiveRemoteEnabled: boolean;
	archiveTrigger: string;
	archiveQuietMs: number;
	cloudNodeAddress: string;
	repositorySettings?: Record<string, unknown>;
}

export interface ApplicationDirectories {
	softwareRoot: string;
	configDir: string;
	dataDir: string;
}

export interface ConnectedAccount {
	provider: string;
	account_login: string;
	display_name?: string;
	avatar_url?: string;
	repository_count?: number;
	repositories?: GitRepository[];
}

export interface UpdateAlert {
	repo_path: string;
	repo_name: string;
	branch: string;
	commits_behind: number;
	remote_oid?: string;
	commits_ahead?: number;
	working_tree_clean?: boolean;
	failure_message?: string;
	prompt_message?: string;
}

export interface ScanSummary {
	repositories: GitRepository[];
	dependencyRepoIds: string[];
}

export interface ArchiveSnapshot {
	id: string;
	kind: string;
	capturedAt?: string;
	fileCount: number;
	storedBytes: number;
	excludedPaths?: string[];
}
