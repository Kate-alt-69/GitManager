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

export interface CommitHistoryPage {
	commits: CommitNode[];
	offset: number;
	limit: number;
	hasMore: boolean;
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
	details_loaded: boolean;
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
	inAppNotificationsEnabled: boolean;
	osNotificationsEnabled: boolean;
	repositorySettings?: Record<string, unknown>;
}

export interface ApplicationDirectories {
	softwareRoot: string;
	installDir: string;
	configDir: string;
	dataDir: string;
	repoMetadataDir: string;
	gitDataDir: string;
	accountMetadataDir: string;
	accountGitDataDir: string;
	remoteRepoMetadataDir: string;
	remoteRepoGitDataDir: string;
	cacheDir: string;
}

export interface ConnectedAccount {
	provider: string;
	account_login: string;
	display_name: string;
	email: string;
	avatar_url: string;
	profile_url: string;
	session_connected: boolean;
	repositories: GitRepository[];
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

export interface CoreServiceStatus {
	suspended: boolean;
	remaining_seconds: number;
	message: string;
	service_generation: number;
	process_id: number;
	protocol_version: number;
	remote_check_pending?: boolean;
	remote_check_running?: boolean;
}

export interface ProcessUsageSummary {
	averageCpuPercent: number;
	peakCpuPercent: number;
	averageMemoryBytes: number;
	peakMemoryBytes: number;
	processCount: number;
}

export interface IdlePerformanceCapture {
	sampleDurationSeconds: number;
	sampleCount: number;
	logicalProcessors: number;
	capturedAtUnixMs: number;
	uiProcessTree: ProcessUsageSummary;
	serviceProcessTree: ProcessUsageSummary | null;
	serviceGeneration: number;
	serviceProtocolVersion: number;
	notes: string[];
}

export interface AppNotification {
	id: string;
	kind: string;
	title: string;
	body: string;
	createdAtUnixMs: number;
	read: boolean;
	repositoryPath?: string | null;
}

export interface RepositoryFileEntry {
	name: string;
	path: string;
	isDir: boolean;
	isSymlink: boolean;
	sizeBytes: number;
}

export interface WorktreeChange {
	path: string;
	indexStatus: string;
	worktreeStatus: string;
	staged: boolean;
}

export interface PullPreview {
	repoName: string;
	branch: string;
	remoteName: string;
	remoteBranch: string;
	localOid: string;
	remoteOid: string;
	commitsAhead: number;
	commitsBehind: number;
	changes: WorktreeChange[];
	statusToken: string;
	canPull: boolean;
}

export interface RepositoryCommitPreview {
	changes: WorktreeChange[];
	indexTree: string;
}

export interface PullExecutionResult {
	message: string;
	commitCreated: boolean;
	stashCreated: boolean;
	rebased: boolean;
}


export interface ScanSummary {
	repositories: GitRepository[];
	dependency_repo_ids: string[];
	scan_truncated?: boolean;
}

export interface ArchiveSnapshot {
	id: string;
	repositoryId: string;
	repositoryName: string;
	repositoryPath: string;
	kind: "local" | "remote";
	capturedAt: string;
	capturedAtUnixMs: number;
	branch: string;
	head: string | null;
	fileCount: number;
	directoryCount: number;
	sourceBytes: number;
	storedBytes: number;
	manifestSha256: string;
	manifestLogicalPath: string;
	excludedPaths: string[];
}
