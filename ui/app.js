// Pure-Rust Tauri v2 Global IPC Bridge (withGlobalTauri: true — Zero Node.js dependencies)

const LANE_COLORS = [
  "#58a6ff", // Blue (main)
  "#3fb950", // Green (feature)
  "#d2a8ff", // Purple (release)
  "#f78166", // Coral/Orange (hotfix)
  "#f2cc60", // Gold (experiment)
  "#79c0ff", // Cyan
  "#ff7b72"  // Red
];

const DEMO_REPOSITORIES = [
  {
    id: "local::/Users/dev/Projects/git-manager-core",
    name: "git-manager-core",
    path: "/Users/dev/Projects/git-manager-core",
    git_dir: "/Users/dev/Projects/git-manager-core/.git",
    parent_folder: "/Users/dev/Projects",
    owner_name: "Torvalds & Contributors",
    owner_email: "maintainers@gitmanager.dev",
    current_branch: "main",
    has_remote: true,
    primary_provider: "github",
    primary_remote_url: "https://github.com/tauri-apps/tauri.git",
    web_url: "https://github.com/tauri-apps/tauri",
    icon_url: "https://github.com/tauri-apps.png?size=128",
    remotes: [
      {
        name: "origin",
        url: "https://github.com/tauri-apps/tauri.git",
        provider: "github",
        host: "github.com",
        owner: "tauri-apps",
        repo_slug: "tauri",
        web_url: "https://github.com/tauri-apps/tauri",
        icon_url: "https://github.com/tauri-apps.png?size=128"
      }
    ],
    branches: [
      { name: "main", is_head: true, is_remote: false, target_oid: "c100001", upstream: "origin/main" },
      { name: "feat/gitlab-oauth", is_head: false, is_remote: false, target_oid: "c100002", upstream: "origin/feat/gitlab-oauth" },
      { name: "origin/main", is_head: false, is_remote: true, target_oid: "c100001", upstream: null }
    ],
    commits: [
      {
        oid: "c100001a9f8e7d6c5b4a3",
        short_oid: "c100001",
        message: "Merge branch 'feat/gitlab-oauth' into main\n\nAdds multi-account GitHub and GitLab synchronization and remote avatar resolution.",
        summary: "Merge branch 'feat/gitlab-oauth' into main",
        author_name: "Alex Rivera",
        author_email: "alex@gitmanager.dev",
        timestamp: 1762950000,
        date_iso: "2026-11-12 14:20",
        parents: ["c100003a9f8e7d6c5b4a3", "c100002a9f8e7d6c5b4a3"],
        branches: ["main", "origin/main"],
        tags: ["v1.0.0"],
        is_head: true,
        lane: 0
      },
      {
        oid: "c100002a9f8e7d6c5b4a3",
        short_oid: "c100002",
        message: "feat(providers): add GitLab v4 personal access token & public project indexer",
        summary: "feat(providers): add GitLab v4 personal access token & public project indexer",
        author_name: "Priya Nair",
        author_email: "priya@gitmanager.dev",
        timestamp: 1762941000,
        date_iso: "2026-11-12 11:50",
        parents: ["c100004a9f8e7d6c5b4a3"],
        branches: ["feat/gitlab-oauth"],
        tags: [],
        is_head: false,
        lane: 1
      },
      {
        oid: "c100003a9f8e7d6c5b4a3",
        short_oid: "c100003",
        message: "perf(scanner): index .git directories across Windows 10/11, Linux, and macOS",
        summary: "perf(scanner): index .git directories across Windows 10/11, Linux, and macOS",
        author_name: "Alex Rivera",
        author_email: "alex@gitmanager.dev",
        timestamp: 1762932000,
        date_iso: "2026-11-12 09:15",
        parents: ["c100004a9f8e7d6c5b4a3"],
        branches: [],
        tags: [],
        is_head: false,
        lane: 0
      },
      {
        oid: "c100004a9f8e7d6c5b4a3",
        short_oid: "c100004",
        message: "feat(ui): implement topological SVG Git Tree visualizer and Remote/Local smart filter",
        summary: "feat(ui): implement topological SVG Git Tree visualizer and Remote/Local smart filter",
        author_name: "Alex Rivera",
        author_email: "alex@gitmanager.dev",
        timestamp: 1762880000,
        date_iso: "2026-11-11 18:40",
        parents: ["c100005a9f8e7d6c5b4a3"],
        branches: [],
        tags: ["v0.9.0-rc1"],
        is_head: false,
        lane: 0
      },
      {
        oid: "c100005a9f8e7d6c5b4a3",
        short_oid: "c100005",
        message: "chore: initial Pure-Rust Tauri v2 workspace bootstrap",
        summary: "chore: initial Pure-Rust Tauri v2 workspace bootstrap",
        author_name: "Alex Rivera",
        author_email: "alex@gitmanager.dev",
        timestamp: 1762800000,
        date_iso: "2026-11-10 20:00",
        parents: [],
        branches: [],
        tags: [],
        is_head: false,
        lane: 0
      }
    ],
    dirty_files_count: 2,
    total_commits_loaded: 5,
    source_kind: "local_scan"
  },
  {
    id: "local::/Users/dev/Projects/rust-embedded-dsp",
    name: "rust-embedded-dsp",
    path: "/Users/dev/Projects/rust-embedded-dsp",
    git_dir: "/Users/dev/Projects/rust-embedded-dsp/.git",
    parent_folder: "/Users/dev/Projects",
    owner_name: "Samir Patel",
    owner_email: "samir.patel@embedded.local",
    current_branch: "develop",
    has_remote: false,
    primary_provider: "local",
    primary_remote_url: null,
    web_url: null,
    icon_url: "local://git-icon",
    remotes: [],
    branches: [
      { name: "develop", is_head: true, is_remote: false, target_oid: "b200001", upstream: null },
      { name: "simd-fir-filter", is_head: false, is_remote: false, target_oid: "b200002", upstream: null }
    ],
    commits: [
      {
        oid: "b200001ff0011223344",
        short_oid: "b200001",
        message: "feat(dsp): zero-allocation biquad cascade on Cortex-M",
        summary: "feat(dsp): zero-allocation biquad cascade on Cortex-M",
        author_name: "Samir Patel",
        author_email: "samir.patel@embedded.local",
        timestamp: 1762920000,
        date_iso: "2026-11-12 06:00",
        parents: ["b200002ff0011223344"],
        branches: ["develop"],
        tags: ["local-draft"],
        is_head: true,
        lane: 0
      },
      {
        oid: "b200002ff0011223344",
        short_oid: "b200002",
        message: "init: local-only repository initialized via git init",
        summary: "init: local-only repository initialized via git init",
        author_name: "Samir Patel",
        author_email: "samir.patel@embedded.local",
        timestamp: 1762850000,
        date_iso: "2026-11-11 10:30",
        parents: [],
        branches: ["simd-fir-filter"],
        tags: [],
        is_head: false,
        lane: 0
      }
    ],
    dirty_files_count: 0,
    total_commits_loaded: 2,
    source_kind: "local_scan"
  }
];

let allRepositories = [];
let connectedAccounts = [];
let pendingUpdateAlerts = [];
let pullingRepoPaths = new Set();
let selectedRepoId = null;
let repositoryScanInProgress = false;
let selectedCommitOid = null;
let requestedFilter = "grouped";
let indexGroupingMode = "folder";
let searchQuery = "";
let appPreferences = null;
let dependencyRepoIds = new Set();
let repositoryKindFilter = "all";
let activePage = "repositories";
let applicationDirectories = null;
let selectedBranchName = null;
let branchFilterMode = "all";
let branchSearchQuery = "";
let branchStateRepoId = null;
let githubIdentity = null;
let activeCommitHoverKey = null;
const ciStatusCache = new Map();
const repositorySettingsCache = new Map();
const repositorySettingsLoading = new Map();
const commitDetailsCache = new Map();

async function tauriInvoke(cmd, args) {
  if (window.__TAURI__ && window.__TAURI__.core && window.__TAURI__.core.invoke) {
    return window.__TAURI__.core.invoke(cmd, args);
  }
  if (window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke) {
    return window.__TAURI_INTERNALS__.invoke(cmd, args);
  }
  throw new Error("Tauri runtime not detected");
}

function escapeHtml(raw) {
  return String(raw || "")
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

function fallbackRepoIconDataUri(provider, repoName) {
  const initial = ((repoName || "G").trim()[0] || "G").toUpperCase();
  const bg =
    provider === "github"
      ? "#238636"
      : provider === "gitlab"
      ? "#e24329"
      : "#1f6feb";
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64" viewBox="0 0 64 64">
    <rect width="64" height="64" rx="14" fill="${bg}"/>
    <text x="50%" y="54%" dominant-baseline="middle" text-anchor="middle" fill="#ffffff" font-family="system-ui, sans-serif" font-weight="700" font-size="28">${initial}</text>
  </svg>`;
  return `data:image/svg+xml;utf8,${encodeURIComponent(svg)}`;
}

function resolveRepoIconUrl(repo) {
  if (repo.icon_url && repo.icon_url.startsWith("https://")) {
    return repo.icon_url;
  }
  return fallbackRepoIconDataUri(repo.primary_provider, repo.name);
}

/**
 * Renders the clear animated SVG loading spinner and background service update prompts:
 * "Hay you have <X-REPO_NAME> update do you want to update it?"
 */
function renderUpdateNotificationsTray() {
  const tray = document.getElementById("update-notifications-tray");
  if (!tray) return;

  if (pendingUpdateAlerts.length === 0) {
    tray.innerHTML = "";
    return;
  }

  tray.innerHTML = pendingUpdateAlerts
    .map((upd) => {
      const isPulling = pullingRepoPaths.has(upd.repo_path);
      const promptText =
        upd.prompt_message ||
        `Hay you have ${upd.repo_name} update do you want to update it?`;

      return `
        <div class="update-alert-card" data-repo-path="${escapeHtml(upd.repo_path)}">
          <div class="update-alert-left">
            <svg class="loading-spinner active-spin" width="20" height="20" viewBox="0 0 24 24" fill="none">
              <circle cx="12" cy="12" r="9" stroke="currentColor" stroke-opacity="0.25" stroke-width="3"></circle>
              <path d="M12 3a9 9 0 0 1 9 9" stroke="#58a6ff" stroke-width="3" stroke-linecap="round"></path>
            </svg>
            <div class="update-alert-copy">
              <strong>${escapeHtml(promptText)}</strong>
              <span>Branch <code>${escapeHtml(upd.branch)}</code> is ${upd.commits_behind} commit(s) behind remote (${escapeHtml((upd.remote_oid || "").slice(0, 7))})</span>
              ${upd.failure_message ? `<span class="update-alert-error">${escapeHtml(upd.automatic_attempt ? "Auto Update stopped safely: " : "Update failed: ")}${escapeHtml(upd.failure_message)}</span>` : ""}
              ${upd.working_tree_clean === false ? '<span class="update-alert-note">Working tree has local changes; automatic update is disabled for this repository.</span>' : ""}
              ${Number(upd.commits_ahead) > 0 ? '<span class="update-alert-note">Local branch has commits not on origin; update requires manual review.</span>' : ""}
            </div>
          </div>
          <div class="update-alert-actions">
            <button class="btn btn-primary btn-accept-update" data-repo-path="${escapeHtml(upd.repo_path)}" ${isPulling ? "disabled" : ""}>
              ${
                isPulling
                  ? `<span class="inline-btn-spinner">
                       <svg class="loading-spinner active-spin" width="14" height="14" viewBox="0 0 24 24" fill="none">
                         <circle cx="12" cy="12" r="9" stroke="#ffffff" stroke-opacity="0.3" stroke-width="3"></circle>
                         <path d="M12 3a9 9 0 0 1 9 9" stroke="#ffffff" stroke-width="3" stroke-linecap="round"></path>
                       </svg>
                       Updating ${escapeHtml(upd.repo_name)}...
                     </span>`
                  : "Yes, Update Now"
              }
            </button>
            <button class="btn btn-secondary btn-dismiss-update" data-repo-path="${escapeHtml(upd.repo_path)}" ${isPulling ? "disabled" : ""}>
              Later
            </button>
          </div>
        </div>
      `;
    })
    .join("");

  tray.querySelectorAll(".btn-accept-update").forEach((btn) => {
    btn.addEventListener("click", async () => {
      const repoPath = btn.getAttribute("data-repo-path");
      if (!repoPath) return;
      await acceptAndPullRepoUpdate(repoPath);
    });
  });

  tray.querySelectorAll(".btn-dismiss-update").forEach((btn) => {
    btn.addEventListener("click", () => {
      const repoPath = btn.getAttribute("data-repo-path");
      pendingUpdateAlerts = pendingUpdateAlerts.filter((u) => u.repo_path !== repoPath);
      renderUpdateNotificationsTray();
    });
  });
}

async function acceptAndPullRepoUpdate(repoPath, automatic = false) {
  pullingRepoPaths.add(repoPath);
  setServiceCheckingVisual(true, automatic ? `Auto-updating ${repoPath}...` : `Updating ${repoPath}...`);
  renderUpdateNotificationsTray();

  try {
    await tauriInvoke("pull_repository_update", { repoPath });
    pendingUpdateAlerts = pendingUpdateAlerts.filter((u) => u.repo_path !== repoPath);
    await runMachineScan();
  } catch (err) {
    const alert = pendingUpdateAlerts.find((item) => item.repo_path === repoPath);
    if (alert) {
      alert.failure_message = String((err && err.message) || err || "Update failed");
      alert.automatic_attempt = automatic;
    }
  } finally {
    pullingRepoPaths.delete(repoPath);
    setServiceCheckingVisual(false, "Svc: Watching Remotes");
    renderUpdateNotificationsTray();
  }
}

function setServiceCheckingVisual(isChecking, text) {
  const spinner = document.getElementById("service-spinner-icon");
  const label = document.getElementById("service-status-text");
  if (spinner) {
    spinner.classList.toggle("active-spin", true);
    spinner.style.opacity = isChecking ? "1" : "0.75";
  }
  if (label && text) {
    label.textContent = text;
  }
}

function pushRepoUpdateAlert(upd) {
  if (!upd || !upd.repo_path || pullingRepoPaths.has(upd.repo_path)) return;
  const repo = allRepositories.find((item) => item.id === upd.repo_id);
  const settings = repo ? repositoryAutomationSettings(repo) : null;
  const safeForAutoUpdate =
    repo &&
    repo.source_kind === "local_scan" &&
    settings &&
    settings.autoUpdate === true &&
    upd.working_tree_clean === true &&
    Number(upd.commits_ahead) === 0;

  const existing = pendingUpdateAlerts.find((item) => item.repo_path === upd.repo_path);
  if (existing) {
    Object.assign(existing, upd);
  } else {
    pendingUpdateAlerts.push(upd);
  }

  if (safeForAutoUpdate) {
    void acceptAndPullRepoUpdate(upd.repo_path, true);
    return;
  }

  renderUpdateNotificationsTray();
}

function branchesForRepository(repo) {
  return (repo.branches || []).slice().sort((a, b) => {
    if (a.is_head !== b.is_head) return a.is_head ? -1 : 1;
    if (a.is_remote !== b.is_remote) return a.is_remote ? 1 : -1;
    return a.name.localeCompare(b.name);
  });
}

function renderBranchNavigation(repo) {
  const container = document.getElementById("git-branch-toolbar");
  if (!container) return;
  if (branchStateRepoId !== repo.id) {
    branchStateRepoId = repo.id;
    selectedBranchName = null;
    branchFilterMode = "all";
    branchSearchQuery = "";
  }

  const allBranches = branchesForRepository(repo);
  const localCount = allBranches.filter((branch) => !branch.is_remote).length;
  const remoteCount = allBranches.filter((branch) => branch.is_remote).length;
  const query = branchSearchQuery.trim().toLowerCase();
  const visibleBranches = allBranches.filter((branch) => {
    if (branchFilterMode === "local" && branch.is_remote) return false;
    if (branchFilterMode === "remote" && !branch.is_remote) return false;
    return !query || branch.name.toLowerCase().includes(query) || String(branch.upstream || "").toLowerCase().includes(query);
  });

  if (!allBranches.length) {
    container.innerHTML = '<div class="branch-toolbar-empty">No branch references are available for this repository.</div>';
    return;
  }

  const filters = [
    ["all", "All", allBranches.length],
    ["local", "Local", localCount],
    ["remote", "Remote", remoteCount]
  ].map(([mode, label, count]) =>
    '<button type="button" class="branch-filter-button ' + (branchFilterMode === mode ? 'active' : '') +
    '" data-branch-filter="' + mode + '" aria-pressed="' + (branchFilterMode === mode ? 'true' : 'false') + '">' +
    label + '<span>' + count + '</span></button>'
  ).join("");

  const pills = visibleBranches.map((branch) => {
    const active = selectedBranchName === branch.name;
    const kind = branch.is_remote ? "Remote" : "Local";
    const tracking = branch.upstream ? "Tracks " + branch.upstream : kind + " branch";
    return '<button type="button" class="branch-pill ' + (active ? 'active' : '') + (branch.is_head ? ' is-head' : '') +
      '" data-branch-name="' + escapeHtml(branch.name) + '" title="' + escapeHtml(tracking) + '" aria-pressed="' + (active ? 'true' : 'false') + '">' +
      '<span class="branch-pill-kind">' + kind + '</span><strong>' + escapeHtml(branch.name) + '</strong>' +
      (branch.is_head ? '<span class="branch-head-label">HEAD</span>' : '') +
      (branch.is_default ? '<span class="branch-default-label">DEFAULT</span>' : '') +
      (branch.upstream ? '<small>' + escapeHtml(branch.upstream) + '</small>' : '') + '</button>';
  }).join("");

  container.innerHTML =
    '<div class="branch-toolbar-top"><div><strong>Branches</strong><span>' + allBranches.length + ' refs · Select a branch to focus its history</span></div>' +
    '<label class="branch-search"><span class="sr-only">Search branches</span><input id="branch-search-input" type="search" value="' +
      escapeHtml(branchSearchQuery) + '" placeholder="Find branch…" /></label></div>' +
    '<div class="branch-filter-row">' + filters +
      '<button type="button" class="branch-filter-button branch-reset-button ' + (!selectedBranchName ? 'active' : '') +
      '" data-branch-reset aria-pressed="' + (!selectedBranchName ? 'true' : 'false') + '">All history</button></div>' +
    '<div class="branch-pill-list">' + (pills || '<span class="branch-toolbar-empty">No branches match this filter.</span>') + '</div>';

  const search = document.getElementById("branch-search-input");
  if (search) search.addEventListener("input", () => {
    branchSearchQuery = search.value;
    renderBranchNavigation(repo);
    const replacement = document.getElementById("branch-search-input");
    if (replacement) {
      replacement.focus();
      replacement.setSelectionRange(replacement.value.length, replacement.value.length);
    }
  });

  container.querySelectorAll("[data-branch-filter]").forEach((button) => {
    button.addEventListener("click", () => {
      branchFilterMode = button.getAttribute("data-branch-filter") || "all";
      if (selectedBranchName) {
        const selected = allBranches.find((branch) => branch.name === selectedBranchName);
        if (selected && ((branchFilterMode === "local" && selected.is_remote) || (branchFilterMode === "remote" && !selected.is_remote))) {
          selectedBranchName = null;
        }
      }
      renderSelectedRepositoryDetails();
    });
  });
  container.querySelectorAll("[data-branch-name]").forEach((button) => {
    button.addEventListener("click", () => {
      const name = button.getAttribute("data-branch-name");
      selectedBranchName = selectedBranchName === name ? null : name;
      renderSelectedRepositoryDetails();
    });
  });
  const reset = container.querySelector("[data-branch-reset]");
  if (reset) reset.addEventListener("click", () => {
    selectedBranchName = null;
    renderSelectedRepositoryDetails();
  });
}

function commitsForSelectedBranch(repo) {
  if (!selectedBranchName) return repo.commits || [];
  const branch = (repo.branches || []).find((item) => item.name === selectedBranchName);
  if (!branch || !branch.target_oid) return [];
  const byOid = new Map((repo.commits || []).map((commit) => [commit.oid, commit]));
  const visited = new Set();
  const pending = [branch.target_oid];
  while (pending.length) {
    const oid = pending.pop();
    if (!oid || visited.has(oid)) continue;
    visited.add(oid);
    const commit = byOid.get(oid);
    if (commit) (commit.parents || []).forEach((parent) => pending.push(parent));
  }
  return (repo.commits || []).filter((commit) => visited.has(commit.oid));
}

function renderRepositoryCiPanel(repo) {
  const panel = document.getElementById("repo-ci-detail-panel");
  if (!panel) return;
  panel.innerHTML =
    '<div class="repo-ci-panel-heading"><div><span class="dashboard-eyebrow">CONTINUOUS INTEGRATION</span><h3>Checks for ' + escapeHtml(repo.current_branch || "HEAD") + '</h3></div>' +
    '<button type="button" id="btn-refresh-repo-ci" class="btn btn-secondary">Refresh checks</button></div>' +
    '<div class="repo-ci-summary"><span class="ci-state-dot ci-neutral"></span><strong>Loading CI status…</strong></div>' +
    '<div class="repo-ci-check-list"><div class="ci-empty-state">Fetching individual check runs and jobs…</div></div>';
  const refresh = document.getElementById("btn-refresh-repo-ci");
  if (refresh) refresh.addEventListener("click", () => {
    refresh.disabled = true;
    refresh.textContent = "Refreshing…";
    void loadRepositoryCi(repo, true).then((details) => {
      if (selectedRepoId !== repo.id) return;
      updateRepositoryCiPanel(repo, details);
    }).finally(() => {
      if (refresh.isConnected) {
        refresh.disabled = false;
        refresh.textContent = "Refresh checks";
      }
    });
  });

  void fetchRepositoryCiDetails(repo, false).then((details) => {
    if (selectedRepoId === repo.id) updateRepositoryCiPanel(repo, details);
  });
}

function updateRepositoryCiPanel(repo, details) {
  const panel = document.getElementById("repo-ci-detail-panel");
  if (!panel || selectedRepoId !== repo.id) return;
  const summary = panel.querySelector(".repo-ci-summary");
  const list = panel.querySelector(".repo-ci-check-list");
  if (summary) {
    summary.innerHTML = '<span class="ci-state-dot ci-' + details.state + '"></span><strong class="ci-' +
      details.state + '">' + escapeHtml(details.label) + '</strong><span>' +
      details.checks.length + ' individual check(s)</span>' +
      (details.checkedAt ? '<time>Updated ' + escapeHtml(formatCiTimestamp(details.checkedAt)) + '</time>' : '');
  }
  if (list) {
    list.innerHTML = renderCiCheckRows(details) +
      (details.message && details.checks.length ? '<p class="ci-source-note">' + escapeHtml(details.message) + '</p>' : '');
  }
}

function renderGitTreeGraph(container, commits, selectedOid, onSelectCommit, branches) {
  if (!commits || commits.length === 0) {
    const message = selectedBranchName
      ? "No loaded commit history is available for " + selectedBranchName + ". Its tip may be outside the initial history window."
      : "No commits found in this repository yet (empty or unborn HEAD).";
    container.innerHTML = '<div class="empty-tree-state"><p>' + escapeHtml(message) + '</p></div>';
    return;
  }

  const rowHeight = 46;
  const laneSpacing = 22;
  const leftPad = 20;

  const branchTypeByName = new Map((branches || []).map((branch) => [branch.name, Boolean(branch.is_remote)]));
  const maxLane = commits.reduce((max, c) => Math.max(max, c.lane || 0), 0);
  const svgWidth = Math.max(90, leftPad * 2 + (maxLane + 1) * laneSpacing);
  const svgHeight = commits.length * rowHeight;

  const coordsByOid = new Map();
  commits.forEach((c, idx) => {
    const x = leftPad + (c.lane || 0) * laneSpacing;
    const y = idx * rowHeight + rowHeight / 2;
    coordsByOid.set(c.oid, { row: idx, x, y, lane: c.lane || 0 });
  });

  const pathsSvg = [];
  commits.forEach((commit) => {
    const from = coordsByOid.get(commit.oid);
    if (!from) return;

    (commit.parents || []).forEach((parentOid, pIdx) => {
      const to = coordsByOid.get(parentOid);
      if (!to) return;

      const edgeLane = pIdx === 0 ? from.lane : Math.max(from.lane, to.lane);
      const strokeColor = LANE_COLORS[edgeLane % LANE_COLORS.length];

      if (from.x === to.x) {
        pathsSvg.push(
          `<line x1="${from.x}" y1="${from.y}" x2="${to.x}" y2="${to.y}" stroke="${strokeColor}" stroke-width="2.4" stroke-linecap="round" />`
        );
      } else {
        const midY = from.y + rowHeight * 0.55;
        pathsSvg.push(
          `<path d="M ${from.x} ${from.y} C ${from.x} ${midY}, ${to.x} ${midY - 10}, ${to.x} ${to.y}" fill="none" stroke="${strokeColor}" stroke-width="2.2" stroke-linecap="round" />`
        );
      }
    });
  });

  const nodesSvg = commits.map((commit) => {
    const pt = coordsByOid.get(commit.oid);
    const color = LANE_COLORS[pt.lane % LANE_COLORS.length];
    const isMerge = (commit.parents || []).length > 1;
    const isSelected = commit.oid === selectedOid;

    const outerHalo =
      commit.is_head || isSelected
        ? `<circle cx="${pt.x}" cy="${pt.y}" r="9" fill="none" stroke="${color}" stroke-width="2" opacity="0.55" />`
        : "";

    const dot = isMerge
      ? `<circle cx="${pt.x}" cy="${pt.y}" r="5.5" fill="#0d1117" stroke="${color}" stroke-width="3" />`
      : `<circle cx="${pt.x}" cy="${pt.y}" r="5" fill="${color}" stroke="#0d1117" stroke-width="1.5" />`;

    return `<g class="git-svg-node" data-oid="${escapeHtml(commit.oid)}">${outerHalo}${dot}</g>`;
  });

  const rowsHtml = commits
    .map((commit) => {
      const isSelected = commit.oid === selectedOid;
      const laneColor = LANE_COLORS[(commit.lane || 0) % LANE_COLORS.length];

      const headBadge = commit.is_head
        ? `<span class="ref-badge ref-head" title="Current HEAD">HEAD</span>`
        : "";

      const branchBadges = (commit.branches || [])
        .map((b) => {
          const isRemote = branchTypeByName.has(b) ? branchTypeByName.get(b) : (b.includes("origin/") || b.includes("remotes/"));
          const cls = isRemote ? "ref-badge ref-remote" : "ref-badge ref-branch";
          return `<span class="${cls}" style="border-color:${laneColor}">${escapeHtml(b)}</span>`;
        })
        .join("");

      const tagBadges = (commit.tags || [])
        .map((t) => `<span class="ref-badge ref-tag">🏷 ${escapeHtml(t)}</span>`)
        .join("");

      return `
        <div class="git-tree-row ${isSelected ? "selected" : ""}" data-oid="${escapeHtml(commit.oid)}" style="height:${rowHeight}px">
          <div class="commit-main-col">
            <div class="commit-badges">${headBadge}${branchBadges}${tagBadges}</div>
            <span class="commit-summary" title="${escapeHtml(commit.message)}">${escapeHtml(commit.summary)}</span>
          </div>
          <div class="commit-meta-col">
            <span class="commit-author" title="${escapeHtml(commit.author_email)}">${escapeHtml(commit.author_name)}</span>
            <span class="commit-date">${escapeHtml(commit.date_iso)}</span>
            <code class="commit-sha">${escapeHtml(commit.short_oid)}</code>
          </div>
        </div>
      `;
    })
    .join("");

  container.innerHTML = `
    <div class="git-tree-viewport">
      <div class="git-tree-svg-column" style="width:${svgWidth}px">
        <svg width="${svgWidth}" height="${svgHeight}" viewBox="0 0 ${svgWidth} ${svgHeight}">
          ${pathsSvg.join("\n")}
          ${nodesSvg.join("\n")}
        </svg>
      </div>
      <div class="git-tree-rows-column">
        ${rowsHtml}
      </div>
    </div>
  `;

  container.querySelectorAll(".git-tree-row").forEach((rowEl) => {
    rowEl.addEventListener("click", () => {
      const oid = rowEl.getAttribute("data-oid");
      const found = commits.find((c) => c.oid === oid);
      if (found) onSelectCommit(found);
    });
  });
}

function positionCommitHoverCard(event) {
  const tooltip=document.getElementById("commit-hover-card"); if(!tooltip || tooltip.classList.contains("hidden")) return;
  const margin=12, offset=16, width=tooltip.offsetWidth||340, height=tooltip.offsetHeight||180;
  const x=event.clientX+offset+width>window.innerWidth-margin?event.clientX-width-offset:event.clientX+offset;
  const y=event.clientY+offset+height>window.innerHeight-margin?Math.max(margin,window.innerHeight-height-margin):event.clientY+offset;
  tooltip.style.left=Math.max(margin,x)+"px"; tooltip.style.top=Math.max(margin,y)+"px";
}
function commitHoverMarkup(details) {
  return '<div class="commit-hover-heading"><strong>'+escapeHtml(details.summary)+'</strong><code>'+escapeHtml(details.shortOid)+'</code></div>'+
    '<div class="commit-hover-author">'+escapeHtml(details.authorName)+' · '+escapeHtml(details.authorEmail)+'</div>'+
    '<div class="commit-hover-message">'+escapeHtml(details.message||details.summary)+'</div>'+
    '<div class="commit-hover-meta"><span>'+escapeHtml(details.authoredAt)+'</span><span>'+details.filesChanged+' files</span><span class="commit-hover-add">+'+details.insertions+'</span><span class="commit-hover-del">−'+details.deletions+'</span></div>'+
    '<code class="commit-hover-sha">'+escapeHtml(details.oid)+'</code>';
}
function wireCommitHoverDetails(container) {
  if(!container||container.dataset.commitHoverWired==="true")return;
  container.dataset.commitHoverWired="true";
  container.addEventListener("pointerover",async(event)=>{
    const target=event.target&&event.target.closest?event.target.closest("[data-oid]"):null;
    if(!target||!container.contains(target))return;
    const oid=target.getAttribute("data-oid"),repo=getRepositoryById(selectedRepoId);
    if(!oid||!repo||repo.source_kind!=="local_scan")return;
    const key=repo.path+"::"+oid;if(activeCommitHoverKey===key)return;activeCommitHoverKey=key;
    const tooltip=document.getElementById("commit-hover-card");if(!tooltip)return;
    tooltip.innerHTML='<div class="commit-hover-loading">Loading commit details…</div>';tooltip.classList.remove("hidden");positionCommitHoverCard(event);
    try{
      let details=commitDetailsCache.get(key);
      if(!details){details=await tauriInvoke("get_commit_details",{repoPath:repo.path,oid});commitDetailsCache.set(key,details);}
      if(activeCommitHoverKey!==key)return;tooltip.innerHTML=commitHoverMarkup(details);positionCommitHoverCard(event);
    }catch(error){if(activeCommitHoverKey===key)tooltip.innerHTML='<div class="commit-hover-loading">Could not load commit details: '+escapeHtml((error&&error.message)||error)+'</div>';}
  });
  container.addEventListener("pointermove",(event)=>{if(activeCommitHoverKey)positionCommitHoverCard(event);});
  container.addEventListener("pointerout",(event)=>{
    const current=event.target&&event.target.closest?event.target.closest("[data-oid]"):null;
    const next=event.relatedTarget&&event.relatedTarget.closest?event.relatedTarget.closest("[data-oid]"):null;
    if(current&&current!==next){activeCommitHoverKey=null;const tooltip=document.getElementById("commit-hover-card");if(tooltip)tooltip.classList.add("hidden");}
  });
}
function computeFilteredRepositories() {
  const query = searchQuery.trim().toLowerCase();
  const kindMatched = allRepositories.filter((repo) => {
    const isDependency = dependencyRepoIds.has(repo.id);
    if (repositoryKindFilter === "projects") return !isDependency;
    if (repositoryKindFilter === "dependencies") return isDependency;
    return true;
  });
  const searchMatched = kindMatched.filter((r) => {
    if (!query) return true;
    return (
      r.name.toLowerCase().includes(query) ||
      r.owner_name.toLowerCase().includes(query) ||
      r.owner_email.toLowerCase().includes(query) ||
      r.path.toLowerCase().includes(query) ||
      r.git_dir.toLowerCase().includes(query) ||
      r.parent_folder.toLowerCase().includes(query)
    );
  });

  const remoteRepos = searchMatched.filter((r) => r.has_remote);
  const localRepos = searchMatched.filter((r) => !r.has_remote);
  const remoteCount = remoteRepos.length;
  const localCount = localRepos.length;

  if (remoteCount === 0 || localCount === 0) {
    return { visibleRepos: searchMatched, remoteCount, localCount, effectiveMode: "grouped_fallback" };
  }
  if (requestedFilter === "remote") {
    return { visibleRepos: remoteRepos, remoteCount, localCount, effectiveMode: "remote" };
  }
  if (requestedFilter === "local") {
    return { visibleRepos: localRepos, remoteCount, localCount, effectiveMode: "local" };
  }
  return { visibleRepos: searchMatched, remoteCount, localCount, effectiveMode: "grouped" };
}

function renderRepositoryKindFilter() {
  const projects = allRepositories.filter((repo) => !dependencyRepoIds.has(repo.id)).length;
  const dependencies = dependencyRepoIds.size;
  const allCount = allRepositories.length;
  const values = { projects, dependencies, all: allCount };
  document.querySelectorAll("[data-repo-kind]").forEach((button) => {
    const kind = button.getAttribute("data-repo-kind");
    button.classList.toggle("active", kind === repositoryKindFilter);
    const count = button.querySelector("[data-repo-kind-count]");
    if (count) count.textContent = String(values[kind] || 0);
  });
}

function renderFilterBar(remoteCount, localCount, effectiveMode) {
  const totalCount = remoteCount + localCount;
  const countAllEl = document.getElementById("count-all");
  const countRemoteEl = document.getElementById("count-remote");
  const countLocalEl = document.getElementById("count-local");
  const fallbackBadge = document.getElementById("filter-fallback-badge");
  const statusNote = document.getElementById("filter-status-note");

  if (countAllEl) countAllEl.textContent = String(totalCount);
  if (countRemoteEl) countRemoteEl.textContent = String(remoteCount);
  if (countLocalEl) countLocalEl.textContent = String(localCount);

  document.querySelectorAll("[data-filter]").forEach((btn) => {
    const mode = btn.getAttribute("data-filter");
    btn.classList.toggle("active", mode === requestedFilter);
  });

  if (effectiveMode === "grouped_fallback") {
    if (fallbackBadge) fallbackBadge.classList.remove("hidden");
    if (statusNote) {
      if (localCount === 0 && remoteCount > 0) {
        statusNote.textContent = "No Local-only repos found — automatically grouped all into 1 unified list.";
      } else if (remoteCount === 0 && localCount > 0) {
        statusNote.textContent = "No Remote repos found — automatically grouped all into 1 unified list.";
      } else {
        statusNote.textContent = "Grouped Remote & Local into 1 unified list.";
      }
    }
  } else {
    if (fallbackBadge) fallbackBadge.classList.add("hidden");
    if (statusNote) {
      if (effectiveMode === "remote") {
        statusNote.textContent = `Showing ${remoteCount} Remote-linked repositories (GitHub / GitLab / origin).`;
      } else if (effectiveMode === "local") {
        statusNote.textContent = `Showing ${localCount} Local-only repositories (no remote configured).`;
      } else {
        statusNote.textContent = `Showing all ${totalCount} repositories (${remoteCount} Remote, ${localCount} Local).`;
      }
    }
  }
}

function renderConnectedAccountsBar() {
  const container = document.getElementById("accounts-connected-list");
  if (!container) return;
  if (connectedAccounts.length === 0) {
    container.innerHTML = '<div class="empty-list-state account-empty"><strong>No accounts connected</strong><span>Connect GitHub or GitLab to browse remote repositories here.</span></div>';
    return;
  }
  container.innerHTML = connectedAccounts.map((account) => {
    const avatar = account.avatar_url || fallbackRepoIconDataUri(account.provider, account.account_login);
    return '<article class="connected-account-card"><img src="' + escapeHtml(avatar) + '" alt="" class="account-chip-avatar" />' +
      '<div class="account-chip-text"><strong>' + escapeHtml(account.display_name || account.account_login) + '</strong><span>' +
      escapeHtml(String(account.provider || "").toUpperCase()) + ' · @' + escapeHtml(account.account_login) + '</span></div>' +
      '<span class="account-repo-count">' + (account.repositories || []).length + ' repos</span></article>';
  }).join("");
}

function setActivePage(page) {
  if (!["repositories", "accounts", "settings"].includes(page)) return;
  activePage = page;
  document.querySelectorAll(".app-page").forEach((section) => {
    const active = section.id === "page-" + page;
    section.classList.toggle("hidden", !active);
    section.classList.toggle("active", active);
  });
  document.querySelectorAll("[data-page]").forEach((tab) => {
    const active = tab.getAttribute("data-page") === page;
    tab.classList.toggle("active", active);
    tab.setAttribute("aria-selected", active ? "true" : "false");
  });
}

function getRepositoryById(id) {
  return allRepositories.find((repo) => repo.id === id) ||
    connectedAccounts.flatMap((account) => account.repositories || []).find((repo) => repo.id === id) ||
    null;
}

function renderAccountsPage() {
  const container = document.getElementById("accounts-repositories-list");
  const count = document.getElementById("accounts-repository-count");
  if (!container) return;
  const repositories = connectedAccounts.flatMap((account) => account.repositories || []);
  if (count) count.textContent = repositories.length + " repositories";
  if (!repositories.length) {
    container.innerHTML = '<div class="empty-list-state account-empty"><strong>No remote repositories to show</strong><span>Connect an account to fetch its repositories. Private repositories require a provider token.</span></div>';
    return;
  }
  container.innerHTML = repositories.map((repo) => {
    const latest = latestCommitFor(repo);
    return '<button type="button" class="dashboard-repo-card account-repo-card" data-account-repo-id="' + escapeHtml(repo.id) + '">' +
      '<div class="dashboard-card-top"><img class="dashboard-card-icon" src="' + escapeHtml(resolveRepoIconUrl(repo)) + '" alt="" />' +
      '<div class="dashboard-card-heading"><strong>' + escapeHtml(repositoryFullName(repo)) + '</strong><span>' + escapeHtml(repo.name || "") + '</span></div>' +
      '<span class="provider-badge provider-' + escapeHtml(repo.primary_provider || "local") + '">' + escapeHtml(String(repo.primary_provider || "REMOTE").toUpperCase()) + '</span></div>' +
      '<div class="dashboard-card-commit"><span class="dashboard-label">REMOTE REPOSITORY</span><strong>' + escapeHtml(repo.description || (latest ? latest.summary : "Repository discovered through account")) + '</strong>' +
      '<span>' + escapeHtml(repo.web_url || "") + '</span></div><div class="dashboard-card-footer"><span class="dashboard-branch">⎇ ' +
      escapeHtml(repo.current_branch || repo.default_branch || "Default branch unavailable") + '</span><span class="dashboard-ci ci-neutral">Remote</span></div></button>';
  }).join("");
  container.querySelectorAll("[data-account-repo-id]").forEach((card) => {
    card.addEventListener("click", () => {
      const id = card.getAttribute("data-account-repo-id");
      if (!id) return;
      selectedRepoId = id;
      selectedBranchName = null;
      branchStateRepoId = null;
      const repo = getRepositoryById(id);
      const selectedHead = repo ? latestCommitFor(repo) : null;
      selectedCommitOid = selectedHead ? selectedHead.oid : null;
      setActivePage("repositories");
      renderSelectedRepositoryDetails();
    });
  });
}

function renderSettingsPage() {
  if (activePage === "settings" && !document.getElementById("signing-certificates-loaded")) {
    const marker=document.createElement("span"); marker.id="signing-certificates-loaded"; marker.hidden=true; document.body.appendChild(marker);
    void refreshWindowsSigningCertificates();
  }
  const roots = appPreferences && Array.isArray(appPreferences.scanRoots) ? appPreferences.scanRoots : [];
  const scanButton = document.getElementById("btn-scan-repos");
  if (scanButton) {
    scanButton.disabled = repositoryScanInProgress;
    scanButton.textContent = repositoryScanInProgress ? "Scanning…" : "Scan now";
  }
  const rootSummary = document.getElementById("settings-scan-root-summary");
  if (rootSummary) rootSummary.textContent = roots.length ? roots.length + " scan folder(s) · " + roots.join("  •  ") : "No scan folders configured yet.";
  [
    ["settings-config-path", applicationDirectories && applicationDirectories.configDir],
    ["settings-data-path", applicationDirectories && applicationDirectories.dataDir],
    ["settings-software-root", applicationDirectories && applicationDirectories.softwareRoot]
  ].forEach(([id, value]) => {
    const element = document.getElementById(id);
    if (element) element.textContent = value || "Unavailable";
  });
}
function renderRepositoryList() {
  const listContainer = document.getElementById("repo-list-container");
  if (!listContainer) return;

  const { visibleRepos, remoteCount, localCount, effectiveMode } = computeFilteredRepositories();
  renderFilterBar(remoteCount, localCount, effectiveMode);
  renderRepositoryKindFilter();

  if (selectedRepoId && !visibleRepos.some((repo) => repo.id === selectedRepoId)) {
    selectedRepoId = null;
    selectedCommitOid = null;
  }

  if (visibleRepos.length === 0) {
    listContainer.innerHTML = `
      <div class="empty-list-state">
        <p>No Git repositories matched your filter.</p>
        <small>Click "Scan for .git Repos" or "Connect GitHub / GitLab" above.</small>
      </div>
    `;
    return;
  }

  if (selectedRepoId && !visibleRepos.some((r) => r.id === selectedRepoId)) {
    selectedRepoId = null;
    selectedCommitOid = null;
  }

  const groups = new Map();
  for (const repo of visibleRepos) {
    let groupKey = repo.parent_folder;
    if (indexGroupingMode === "git_dir") {
      groupKey = repo.git_dir;
    } else if (indexGroupingMode === "provider") {
      groupKey = repo.has_remote
        ? `Remote (${repo.primary_provider.toUpperCase()})`
        : "Local Only (No Remote)";
    }
    const list = groups.get(groupKey) || [];
    list.push(repo);
    groups.set(groupKey, list);
  }

  const htmlSections = [];
  for (const [groupTitle, repos] of groups.entries()) {
    const itemsHtml = repos
      .map((repo) => {
        const isSelected = repo.id === selectedRepoId;
        const iconSrc = resolveRepoIconUrl(repo);
        const fallbackSrc = fallbackRepoIconDataUri(repo.primary_provider, repo.name);
        const badgeLabel = repo.has_remote ? repo.primary_provider.toUpperCase() : "LOCAL";
        const badgeClass = repo.has_remote
          ? `provider-badge provider-${escapeHtml(repo.primary_provider)}`
          : "provider-badge provider-local";

        return `
          <div class="repo-card ${isSelected ? "selected" : ""}" data-repo-id="${escapeHtml(repo.id)}">
            <img
              src="${escapeHtml(iconSrc)}"
              alt="${escapeHtml(repo.name)} icon"
              class="repo-card-icon"
              onerror="this.onerror=null;this.src='${fallbackSrc}';"
            />
            <div class="repo-card-body">
              <div class="repo-card-top">
                <span class="repo-card-name">${escapeHtml(repo.name)}</span>
                <span class="${badgeClass}">${escapeHtml(badgeLabel)}</span>
              </div>
              <div class="repo-card-owner">
                <span>${escapeHtml(repo.owner_name)}</span>
                <span class="dot-sep">•</span>
                <span class="repo-card-branch">⎇ ${escapeHtml(repo.current_branch)}</span>
              </div>
              <div class="repo-card-path" title="${escapeHtml(repo.git_dir)}">
                ${escapeHtml(repo.git_dir)}
              </div>
            </div>
          </div>
        `;
      })
      .join("");

    htmlSections.push(`
      <div class="repo-group">
        <div class="repo-group-header">
          <span class="folder-icon">📁</span>
          <span class="repo-group-title" title="${escapeHtml(groupTitle)}">${escapeHtml(groupTitle)}</span>
          <span class="repo-group-count">${repos.length}</span>
        </div>
        <div class="repo-group-items">
          ${itemsHtml}
        </div>
      </div>
    `);
  }

  listContainer.innerHTML = htmlSections.join("");
  listContainer.querySelectorAll(".repo-card").forEach((cardEl) => {
    cardEl.addEventListener("click", () => {
      const id = cardEl.getAttribute("data-repo-id");
      if (id) {
        selectedRepoId = id;
        const repo = getRepositoryById(id);
        selectedCommitOid = (repo && repo.commits[0] && repo.commits[0].oid) || null;
        renderAll();
      }
    });
  });
}

function repositoryFullName(repo) {
  const remote = (repo.remotes || []).find((item) => item.name === "origin") || (repo.remotes || [])[0];
  if (remote && remote.owner && remote.repo_slug) return remote.owner + "/" + remote.repo_slug;
  return (repo.owner_name || "Local") + "/" + repo.name;
}

function latestCommitFor(repo) {
  const commits = repo.commits || [];
  return commits.find((commit) => commit.is_head && /^[a-f0-9]{7,40}$/i.test(commit.oid || "")) ||
    commits.find((commit) => /^[a-f0-9]{7,40}$/i.test(commit.oid || "")) || null;
}

function ciStateFor(status, conclusion) {
  const raw = String(conclusion || status || "").toLowerCase();
  if (raw === "success") return { label: "Passed", state: "success" };
  if (["failure", "failed", "error", "action_required", "timed_out", "startup_failure", "stale"].includes(raw)) {
    return { label: "Failed", state: "failure" };
  }
  if (["pending", "running", "in_progress", "queued", "requested", "waiting", "waiting_for_resource", "preparing", "canceling", "created", "scheduled"].includes(raw)) {
    return { label: raw === "queued" || raw === "requested" ? "Queued" : "Running", state: "pending" };
  }
  if (["canceled", "cancelled"].includes(raw)) return { label: "Canceled", state: "neutral" };
  if (raw === "skipped") return { label: "Skipped", state: "neutral" };
  if (raw === "manual") return { label: "Manual", state: "neutral" };
  if (raw === "neutral") return { label: "Neutral", state: "neutral" };
  return { label: raw ? "Unknown" : "No status", state: "neutral" };
}

function summarizeCiChecks(checks, message) {
  const counts = { success: 0, failure: 0, pending: 0, neutral: 0 };
  checks.forEach((check) => { counts[check.state] = (counts[check.state] || 0) + 1; });
  let state = "neutral";
  let label = "No CI checks";
  if (counts.failure) {
    state = "failure";
    label = counts.failure + " failing";
  } else if (counts.pending) {
    state = "pending";
    label = counts.pending + " running";
  } else if (counts.success) {
    state = "success";
    label = counts.success + "/" + checks.length + " passed";
  }
  if (!checks.length && message) label = "CI unavailable";
  else if (checks.length && counts.neutral === checks.length) label = checks.length + " skipped / neutral";
  return {
    state,
    label,
    checks,
    counts,
    message: message || "",
    checkedAt: new Date().toISOString()
  };
}

async function fetchJsonResponse(url, headers) {
  const response = await fetch(url, { headers: headers || { Accept: "application/json" } });
  let body = null;
  try { body = await response.json(); } catch (_) {}
  return { response, body };
}

function githubCiCheckFromRun(run) {
  const mapped = ciStateFor(run.status, run.conclusion);
  return {
    name: run.name || (run.app && run.app.name) || "GitHub check",
    provider: (run.app && run.app.name) || "GitHub Actions",
    state: mapped.state,
    label: mapped.label,
    description: run.output && run.output.title ? run.output.title : (run.output && run.output.summary ? run.output.summary : ""),
    url: run.html_url || "",
    timestamp: run.completed_at || run.started_at || run.created_at || ""
  };
}

function githubCiCheckFromStatus(status) {
  const mapped = ciStateFor(status.state, null);
  return {
    name: status.context || "Commit status",
    provider: "Commit status",
    state: mapped.state,
    label: mapped.label,
    description: status.description || "",
    url: status.target_url || "",
    timestamp: status.created_at || ""
  };
}

async function fetchGithubCiDetails(owner, name, commit) {
  const base = "https://api.github.com/repos/" + encodeURIComponent(owner) + "/" +
    encodeURIComponent(name) + "/commits/" + encodeURIComponent(commit.oid);
  const headers = {
    Accept: "application/vnd.github+json",
    "X-GitHub-Api-Version": "2022-11-28"
  };
  const results = await Promise.all([
    fetchJsonResponse(base + "/check-runs?per_page=100", headers),
    fetchJsonResponse(base + "/status", headers)
  ]);
  const runResult = results[0];
  const statusResult = results[1];
  const checks = [];
  if (runResult.response.ok && runResult.body && Array.isArray(runResult.body.check_runs)) {
    runResult.body.check_runs.forEach((run) => checks.push(githubCiCheckFromRun(run)));
  }
  if (statusResult.response.ok && statusResult.body && Array.isArray(statusResult.body.statuses)) {
    statusResult.body.statuses.forEach((status) => checks.push(githubCiCheckFromStatus(status)));
  }

  let message = "";
  if (!runResult.response.ok && !statusResult.response.ok) {
    const codes = [runResult.response.status, statusResult.response.status];
    if (codes.includes(403) || codes.includes(429)) message = "GitHub API rate limit or access restriction.";
    else if (codes.every((code) => code === 404)) message = "Checks are unavailable for this repository or commit.";
    else message = "GitHub could not return CI details (HTTP " + codes.join("/") + ").";
  } else if (!runResult.response.ok || !statusResult.response.ok) {
    message = "Some check sources could not be loaded.";
  }
  return summarizeCiChecks(checks, message);
}

async function fetchGitlabCiDetails(projectPath, commit) {
  const encodedProject = encodeURIComponent(projectPath);
  const pipelineUrl = "https://gitlab.com/api/v4/projects/" + encodedProject +
    "/pipelines?sha=" + encodeURIComponent(commit.oid) + "&per_page=1&order_by=id&sort=desc";
  const pipelineResult = await fetchJsonResponse(pipelineUrl);
  if (!pipelineResult.response.ok) {
    const message = pipelineResult.response.status === 403 || pipelineResult.response.status === 401
      ? "GitLab CI details are not public or require access."
      : "GitLab could not return pipeline details (HTTP " + pipelineResult.response.status + ").";
    return summarizeCiChecks([], message);
  }

  const pipeline = Array.isArray(pipelineResult.body) ? pipelineResult.body[0] : null;
  if (!pipeline) return summarizeCiChecks([], "No GitLab pipeline was found for this commit.");

  const jobsUrl = "https://gitlab.com/api/v4/projects/" + encodedProject +
    "/pipelines/" + encodeURIComponent(String(pipeline.id)) + "/jobs?per_page=100";
  const jobsResult = await fetchJsonResponse(jobsUrl);
  const checks = [];
  if (jobsResult.response.ok && Array.isArray(jobsResult.body)) {
    jobsResult.body.forEach((job) => {
      const mapped = ciStateFor(job.status, null);
      checks.push({
        name: job.name || "Pipeline job",
        provider: job.stage || "GitLab CI",
        state: mapped.state,
        label: mapped.label,
        description: job.failure_reason || "",
        url: job.web_url || pipeline.web_url || "",
        timestamp: job.finished_at || job.started_at || job.created_at || ""
      });
    });
  }

  if (!checks.length) {
    const mapped = ciStateFor(pipeline.status, null);
    checks.push({
      name: "Pipeline #" + pipeline.id,
      provider: "GitLab CI",
      state: mapped.state,
      label: mapped.label,
      description: jobsResult.response.ok ? "No individual jobs were returned." : "Job details are not available.",
      url: pipeline.web_url || "",
      timestamp: pipeline.updated_at || pipeline.created_at || ""
    });
  }
  const message = jobsResult.response.ok ? "" : "Pipeline summary loaded; individual job details were unavailable.";
  return summarizeCiChecks(checks, message);
}

function fetchRepositoryCiDetails(repo, forceRefresh) {
  const commit = latestCommitFor(repo);
  const remote = (repo.remotes || []).find((item) => item.name === "origin") || (repo.remotes || [])[0];
  if (!commit || !remote || !remote.web_url) {
    return Promise.resolve(summarizeCiChecks([], !commit ? "No local HEAD commit is available." : "No supported remote CI provider is configured."));
  }

  const cacheKey = repo.id + ":" + commit.oid;
  if (forceRefresh) ciStatusCache.delete(cacheKey);
  if (ciStatusCache.has(cacheKey)) return ciStatusCache.get(cacheKey);

  const provider = String(remote.provider || repo.primary_provider || "").toLowerCase();
  const request = (async () => {
    try {
      const parsed = new URL(remote.web_url);
      const parts = parsed.pathname.split("/").filter(Boolean);
      if (provider === "github" && parsed.hostname === "github.com" && parts.length >= 2) {
        return await fetchGithubCiDetails(parts[0], parts[1].replace(/\.git$/i, ""), commit);
      }
      if (provider === "gitlab" && parsed.hostname === "gitlab.com" && parts.length >= 2) {
        return await fetchGitlabCiDetails(parts.join("/"), commit);
      }
      return summarizeCiChecks([], "Individual CI details are not yet supported for this remote host.");
    } catch (error) {
      return summarizeCiChecks([], "Could not load CI details: " + String((error && error.message) || error));
    }
  })();
  ciStatusCache.set(cacheKey, request);
  return request;
}

function formatCiTimestamp(value) {
  if (!value) return "";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "";
  return new Intl.DateTimeFormat(undefined, { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" }).format(date);
}

function renderCiCheckRows(details) {
  if (!details.checks.length) {
    return '<div class="ci-empty-state">' + escapeHtml(details.message || "No individual checks were reported for this commit.") + '</div>';
  }
  const priority = { failure: 0, pending: 1, success: 2, neutral: 3 };
  const ordered = details.checks.slice().sort((a, b) => (priority[a.state] - priority[b.state]) || a.name.localeCompare(b.name));
  return ordered.map((check) => {
    const contents = '<span class="ci-state-dot ci-' + escapeHtml(check.state) + '" aria-hidden="true"></span>' +
      '<span class="ci-check-copy"><strong>' + escapeHtml(check.name) + '</strong><small>' +
      escapeHtml(check.provider || "CI") + (check.description ? " · " + escapeHtml(check.description) : "") +
      '</small></span><span class="ci-check-result ci-' + escapeHtml(check.state) + '">' + escapeHtml(check.label) + '</span>' +
      (check.timestamp ? '<time>' + escapeHtml(formatCiTimestamp(check.timestamp)) + '</time>' : '');
    if (check.url && /^https:\/\//i.test(check.url)) {
      return '<a class="ci-check-row" href="' + escapeHtml(check.url) + '" target="_blank" rel="noopener noreferrer">' + contents + '</a>';
    }
    return '<div class="ci-check-row">' + contents + '</div>';
  }).join("");
}

function updateCiCard(repo, details) {
  document.querySelectorAll("[data-ci-repo-id]").forEach((element) => {
    if (element.getAttribute("data-ci-repo-id") !== repo.id) return;
    element.textContent = details.label;
    element.className = "dashboard-ci ci-" + details.state;
    element.setAttribute("aria-label", "CI status: " + details.label);
  });
  document.querySelectorAll("[data-ci-dot-repo-id]").forEach((element) => {
    if (element.getAttribute("data-ci-dot-repo-id") === repo.id) {
      element.className = "ci-state-dot ci-" + details.state;
    }
  });
  document.querySelectorAll("[data-ci-count-repo-id]").forEach((element) => {
    if (element.getAttribute("data-ci-count-repo-id") === repo.id) {
      element.textContent = details.checks.length ? details.checks.length + " checks" : "Details";
    }
  });
  document.querySelectorAll("[data-ci-list-repo-id]").forEach((element) => {
    if (element.getAttribute("data-ci-list-repo-id") === repo.id) {
      element.innerHTML = renderCiCheckRows(details) + (details.message && details.checks.length
        ? '<p class="ci-source-note">' + escapeHtml(details.message) + '</p>'
        : "");
    }
  });
}

async function loadRepositoryCi(repo, forceRefresh) {
  document.querySelectorAll("[data-ci-list-repo-id]").forEach((element) => {
    if (element.getAttribute("data-ci-list-repo-id") === repo.id) element.innerHTML = '<div class="ci-empty-state">Loading individual checks…</div>';
  });
  const details = await fetchRepositoryCiDetails(repo, Boolean(forceRefresh));
  updateCiCard(repo, details);
  return details;
}

function refreshDashboardCiStatuses(repositories) {
  // Eagerly inspect a small first page; other cards load checks on expansion to bound public API usage.
  const targets = repositories.slice(0, 12);
  let index = 0;
  const worker = async () => {
    while (index < targets.length) {
      const repo = targets[index++];
      const details = await fetchRepositoryCiDetails(repo, false);
      updateCiCard(repo, details);
    }
  };
  const workerCount = Math.min(4, targets.length);
  const workers = [];
  for (let i = 0; i < workerCount; i++) workers.push(worker());
  Promise.all(workers).catch((error) => console.warn("Could not refresh repository CI status:", error));
}

function renderRepositoryDashboard(container) {
  const filter = computeFilteredRepositories();
  const repositories = filter.visibleRepos;
  renderFilterBar(filter.remoteCount, filter.localCount, filter.effectiveMode);
  renderRepositoryKindFilter();
  const projectCount = allRepositories.length - dependencyRepoIds.size;
  const dependencyCount = dependencyRepoIds.size;
  const remoteCount = allRepositories.filter((repo) => repo.has_remote).length;
  const roots = appPreferences && Array.isArray(appPreferences.scanRoots) ? appPreferences.scanRoots : [];

  if (allRepositories.length === 0) {
    container.innerHTML =
      '<section class="dashboard-view">' +
        '<div class="dashboard-heading"><div><div class="dashboard-eyebrow">LOCAL WORKSPACE</div><h2>Repositories</h2><p>Your local Git projects will appear here.</p></div>' +
        '<div class="dashboard-actions"><button id="btn-dashboard-setup" class="btn btn-secondary">Configure folders</button></div></div>' +
        '<div class="dashboard-empty ' + (repositoryScanInProgress ? 'dashboard-scan-empty' : '') + '">' +
          '<strong>' + (repositoryScanInProgress ? 'Scanning your folders…' : 'No repositories indexed yet') + '</strong>' +
          '<span>' + (repositoryScanInProgress ? 'GitManager is indexing Git folders in the background. You can keep using the app while this runs.' : 'Add a project folder or choose the recommended development folders to begin.') + '</span>' +
          (repositoryScanInProgress ? '<span class="dashboard-scan-progress" aria-hidden="true"><i></i><i></i><i></i></span>' : '') +
        '</div>' +
      '</section>';
    const setup = document.getElementById("btn-dashboard-setup");
    if (setup) setup.addEventListener("click", () => openScanSetup(false));
    return;
  }

  const cards = repositories.map((repo) => {
    const icon = resolveRepoIconUrl(repo);
    const fallback = fallbackRepoIconDataUri(repo.primary_provider, repo.name);
    const commit = latestCommitFor(repo);
    const commitTitle = commit ? commit.summary : "No local HEAD commit";
    const commitSha = commit ? commit.short_oid : "";
    const commitDate = commit ? commit.date_iso : "";
    const branchCount = (repo.branches || []).length;
    const isDependency = dependencyRepoIds.has(repo.id);
    const provider = repo.has_remote ? String(repo.primary_provider || "remote").toUpperCase() : "LOCAL";
    const dirty = Number(repo.dirty_files_count || 0);
    return '<article class="repo-card">' +
      '<button type="button" class="repo-card-open" data-dashboard-repo-id="' + escapeHtml(repo.id) + '">' +
        '<span class="repo-card-top">' +
          '<img class="repo-card-icon" src="' + escapeHtml(icon) + '" alt="" data-fallback="' + escapeHtml(fallback) + '" />' +
          '<span class="repo-card-heading"><strong>' + escapeHtml(repo.name || repositoryFullName(repo)) + '</strong><small>' +
            escapeHtml(repositoryFullName(repo)) + (isDependency ? ' <span class="repo-kind-label">Dependency</span>' : '') + '</small></span>' +
          '<span class="provider-badge provider-' + escapeHtml(repo.primary_provider || "local") + '">' + escapeHtml(provider) + '</span>' +
        '</span>' +
        '<span class="repo-card-commit"><strong>' + escapeHtml(repo.owner_name || "Local repository") + '</strong>' +
          '<small title="' + escapeHtml(commitTitle) + '">' + escapeHtml(commitTitle) + '</small>' +
          '<small class="repo-card-commit-meta">' + escapeHtml(commitSha + (commitDate ? " · " + commitDate : "")) + '</small></span>' +
        '<span class="repo-card-status-row"><span class="repo-card-branch">⑂ ' + escapeHtml(repo.current_branch || "Detached HEAD") + '</span>' +
          '<span class="repo-card-ci"><span class="ci-state-dot ci-neutral" data-ci-dot-repo-id="' + escapeHtml(repo.id) + '" aria-hidden="true"></span>' +
          '<span class="dashboard-ci ci-neutral" data-ci-repo-id="' + escapeHtml(repo.id) + '" aria-label="CI status">Checking CI…</span></span></span>' +
        '<span class="repo-card-meta"><span>' + branchCount + ' branches</span>' +
          '<span class="' + (dirty ? 'repo-dirty' : 'repo-clean') + '">' + (dirty ? dirty + ' changed' : 'Working tree clean') + '</span></span>' +
      '</button>' +
      '<details class="repo-ci-details" data-ci-expand data-repo-id="' + escapeHtml(repo.id) + '">' +
        '<summary><span class="ci-detail-count" data-ci-count-repo-id="' + escapeHtml(repo.id) + '">View individual checks</span></summary>' +
        '<div class="ci-check-list" data-ci-list-repo-id="' + escapeHtml(repo.id) + '"><div class="ci-empty-state">Expand to load individual CI checks.</div></div>' +
      '</details>' +
    '</article>';
  }).join("");

  if (repositories.length === 0) {
    container.innerHTML = '<section class="dashboard-view"><div class="dashboard-empty"><strong>No repositories match these filters</strong><span>Try another repository type, availability filter or search term.</span></div></section>';
    return;
  }

  const groups = new Map();
  repositories.forEach((repo, index) => {
    let groupKey = repo.parent_folder || "Repositories";
    if (indexGroupingMode === "git_dir") {
      groupKey = repo.git_dir || groupKey;
    } else if (indexGroupingMode === "provider") {
      groupKey = repo.has_remote ? "Remote (" + String(repo.primary_provider || "remote").toUpperCase() + ")" : "Local Only (No Remote)";
    }
    if (!groups.has(groupKey)) groups.set(groupKey, []);
    groups.get(groupKey).push(cards[index]);
  });
  const cardSections = Array.from(groups.entries()).map(([groupName, groupCards]) =>
    '<section class="repo-group-modern"><div class="repo-group-modern-heading"><strong>' + escapeHtml(groupName) +
    '</strong><span>' + groupCards.length + ' repos</span></div><div class="dashboard-card-grid repo-card-grid">' +
    groupCards.join("") + '</div></section>'
  ).join("");

  container.innerHTML =
    '<section class="dashboard-view">' +
      '<div class="dashboard-heading">' +
        '<div><div class="dashboard-eyebrow">LOCAL WORKSPACE</div><h2>Repositories</h2>' +
          '<p>' + escapeHtml(roots.length + " scan folder(s) · " + projectCount + " project(s) · " + dependencyCount + " dependency repo(s)") + '</p></div>' +
        '<div class="dashboard-actions"><button id="btn-dashboard-scan" class="btn btn-secondary" ' + (repositoryScanInProgress ? 'disabled' : '') + '>' +
          (repositoryScanInProgress ? 'Scanning…' : 'Scan now') + '</button>' +
          '<button id="btn-dashboard-setup" class="btn btn-secondary">Scan folders</button></div>' +
      '</div>' +
      '<div class="dashboard-stats">' +
        '<div><strong>' + projectCount + '</strong><span>Projects</span></div>' +
        '<div><strong>' + dependencyCount + '</strong><span>Dependencies</span></div>' +
        '<div><strong>' + remoteCount + '</strong><span>Remote-linked</span></div>' +
      '</div>' +
      cardSections +
    '</section>';

  container.querySelectorAll(".repo-card-icon").forEach((image) => {
    image.addEventListener("error", () => {
      if (image.dataset.fallback) image.src = image.dataset.fallback;
    }, { once: true });
  });
  container.querySelectorAll("[data-dashboard-repo-id]").forEach((button) => {
    button.addEventListener("click", () => {
      selectedRepoId = button.getAttribute("data-dashboard-repo-id");
      const selected = getRepositoryById(selectedRepoId);
      const selectedHead = selected ? latestCommitFor(selected) : null;
      selectedCommitOid = selectedHead ? selectedHead.oid : null;
      selectedBranchName = null;
      branchStateRepoId = null;
      renderAll();
    });
  });

  container.querySelectorAll("[data-ci-expand]").forEach((detailsElement) => {
    detailsElement.addEventListener("toggle", () => {
      if (!detailsElement.open || detailsElement.dataset.loaded === "true") return;
      const repo = getRepositoryById(detailsElement.getAttribute("data-repo-id"));
      if (!repo) return;
      detailsElement.dataset.loaded = "true";
      void loadRepositoryCi(repo, false).catch((error) => {
        detailsElement.dataset.loaded = "false";
        console.warn("Could not load individual CI checks:", error);
      });
    });
  });

  const setupButton = document.getElementById("btn-dashboard-setup");
  if (setupButton) setupButton.addEventListener("click", () => openScanSetup(false));
  const scanButton = document.getElementById("btn-dashboard-scan");
  if (scanButton) scanButton.addEventListener("click", () => void runMachineScan());
  refreshDashboardCiStatuses(repositories);
}
function defaultRepositoryLocalSettings() {
  return { autoUpdate:false, keepRemoteClean:false, commitClean:false,
    cleanerIncludeDependencies:false, cleanerIncludeVirtualenvs:false, cleanerExcludePatterns:[] };
}
function repositoryAutomationSettings(repo) {
  const cached = repositorySettingsCache.get(repo.path);
  if (cached) return cached;
  const legacy = appPreferences && appPreferences.repositorySettings && appPreferences.repositorySettings[repo.id];
  return Object.assign(defaultRepositoryLocalSettings(), legacy || {});
}
async function ensureRepositoryLocalSettings(repo) {
  if (!repo || repo.source_kind !== "local_scan" || repositorySettingsCache.has(repo.path)) return;
  const inFlight = repositorySettingsLoading.get(repo.path);
  if (inFlight) return inFlight;
  const task = (async () => {
    try {
      const settings = await tauriInvoke("load_repository_settings", { repoPath: repo.path, repoId: repo.id });
      repositorySettingsCache.set(repo.path, Object.assign(defaultRepositoryLocalSettings(), settings || {}));
    } catch (error) {
      console.warn("Could not load repository-local settings:", error);
      repositorySettingsCache.set(repo.path, defaultRepositoryLocalSettings());
    } finally {
      repositorySettingsLoading.delete(repo.path);
      if (selectedRepoId === repo.id) renderSelectedRepositoryDetails();
    }
  })();
  repositorySettingsLoading.set(repo.path, task);
  return task;
}
async function persistRepositoryLocalSettings(repo, status) {
  const settings = repositoryAutomationSettings(repo);
  if (status) status.textContent = "Saving local settings…";
  try {
    const saved = await tauriInvoke("save_repository_settings", { repoPath:repo.path, settings });
    repositorySettingsCache.set(repo.path, Object.assign(defaultRepositoryLocalSettings(), saved || settings));
    if (status) status.textContent = "Saved in .git/info/gitmanager-settings.json · local to this repository";
    return true;
  } catch (error) {
    if (status) status.textContent = "Could not save local settings: " + String((error && error.message) || error);
    return false;
  }
}
async function saveRepositoryAutoUpdate(repo, enabled, checkbox, status) {
  const settings = repositoryAutomationSettings(repo), previous = Boolean(settings.autoUpdate);
  settings.autoUpdate = enabled;
  if (checkbox) checkbox.disabled = true;
  if (status) status.textContent = "Saving…";
  try {
    const saved = await tauriInvoke("save_repository_settings", { repoPath:repo.path, settings });
    repositorySettingsCache.set(repo.path, Object.assign(defaultRepositoryLocalSettings(), saved || settings));
    if (status) status.textContent = enabled ? "Auto Update enabled · clean fast-forwards only" : "Auto Update disabled";
  } catch (error) {
    settings.autoUpdate = previous;
    if (checkbox) checkbox.checked = previous;
    if (status) status.textContent = "Could not save: " + String((error && error.message) || error);
  } finally { if (checkbox) checkbox.disabled = false; }
}
async function saveRepositoryCommitClean(repo, enabled, checkbox, status) {
  const settings = repositoryAutomationSettings(repo), previous = Boolean(settings.commitClean);
  if (enabled && !window.confirm("Install GitManager's pre-commit hook for this repository? It keeps generated/dependency files on disk and leaves them out of commits. Existing custom hooks will not be overwritten.")) {
    if (checkbox) checkbox.checked = previous;
    return;
  }
  if (checkbox) checkbox.disabled = true;
  if (status) status.textContent = enabled ? "Installing pre-commit hook…" : "Removing pre-commit hook…";
  try {
    const result = await tauriInvoke("set_repository_commit_clean", { repoPath:repo.path, enabled });
    settings.commitClean = enabled;
    const saved = await tauriInvoke("save_repository_settings", { repoPath:repo.path, settings });
    repositorySettingsCache.set(repo.path, Object.assign(defaultRepositoryLocalSettings(), saved || settings));
    if (status) status.textContent = result.message || (enabled ? "Commit Clean hook installed" : "Commit Clean hook removed");
  } catch (error) {
    try { await tauriInvoke("set_repository_commit_clean", { repoPath:repo.path, enabled:previous }); }
    catch (rollbackError) { if (status) status.textContent = "Could not apply or roll back hook: " + String((rollbackError && rollbackError.message) || rollbackError); }
    settings.commitClean = previous;
    if (checkbox) checkbox.checked = previous;
    if (status && !status.textContent.startsWith("Could not apply or roll back")) status.textContent = "Could not update Commit Clean: " + String((error && error.message) || error);
  } finally { if (checkbox) checkbox.disabled = false; }
}
async function saveRepositoryCleanerSettings(repo) {
  const settings = repositoryAutomationSettings(repo);
  const dependencies = document.getElementById("repo-cleaner-dependencies");
  const virtualenvs = document.getElementById("repo-cleaner-virtualenvs");
  const exclusions = document.getElementById("repo-cleaner-exclusions");
  const status = document.getElementById("repo-local-settings-status");
  settings.cleanerIncludeDependencies = Boolean(dependencies && dependencies.checked);
  settings.cleanerIncludeVirtualenvs = Boolean(virtualenvs && virtualenvs.checked);
  settings.cleanerExcludePatterns = String(exclusions && exclusions.value || "").split(/\r?\n/).map((line)=>line.trim()).filter(Boolean).slice(0,256);
  const button = document.getElementById("btn-save-repo-local-settings");
  if (button) button.disabled = true;
  try { await persistRepositoryLocalSettings(repo, status); }
  finally { if (button) button.disabled = false; }
}
function renderSelectedRepositoryDetails() {
  const headerEl = document.getElementById("repo-detail-header");
  const treeContainer = document.getElementById("git-tree-container");
  const treeStatsBadge = document.getElementById("tree-stats-badge");
  const inspectorEl = document.getElementById("repo-inspector-card");

  if (!headerEl || !treeContainer || !inspectorEl) return;

  const repo = getRepositoryById(selectedRepoId);
  const detailView = document.getElementById("repository-detail-view");
  const homeView = document.getElementById("repo-home-content");
  if (!repo) {
    if (detailView) detailView.classList.add("hidden");
    if (homeView) {
      homeView.classList.remove("hidden");
      renderRepositoryDashboard(homeView);
    }
    treeContainer.innerHTML = "";
    inspectorEl.innerHTML = "";
    return;
  }
  if (homeView) homeView.classList.add("hidden");
  if (detailView) detailView.classList.remove("hidden");
  if (repo.source_kind === "local_scan") void ensureRepositoryLocalSettings(repo);

  const iconSrc = resolveRepoIconUrl(repo);
  const fallbackSrc = fallbackRepoIconDataUri(repo.primary_provider, repo.name);
  renderRepositoryCiPanel(repo);
  renderBranchNavigation(repo);

  headerEl.innerHTML = `
    <div class="selected-repo-banner">
      <div class="selected-repo-identity">
        <img
          src="${escapeHtml(iconSrc)}"
          alt="${escapeHtml(repo.name)} icon"
          class="selected-repo-avatar"
          onerror="this.onerror=null;this.src='${fallbackSrc}';"
        />
        <div>
          <div class="selected-repo-title-row">
            <h2 class="selected-repo-name">${escapeHtml(repo.name)}</h2>
            <span class="status-pill ${repo.has_remote ? "pill-remote" : "pill-local"}">
              ${repo.has_remote ? `Remote (${escapeHtml(repo.primary_provider.toUpperCase())})` : "Local Repository (No Remote)"}
            </span>
            ${
              repo.dirty_files_count > 0
                ? `<span class="status-pill pill-dirty">${repo.dirty_files_count} uncommitted change(s)</span>`
                : `<span class="status-pill pill-clean">Working tree clean</span>`
            }
          </div>
          <div class="selected-repo-meta-row">
            <span><strong>Owner:</strong> ${escapeHtml(repo.owner_name)}</span>
            <span class="dot-sep">•</span>
            <span><strong>Email:</strong> <a href="mailto:${escapeHtml(repo.owner_email)}" class="inline-link">${escapeHtml(repo.owner_email)}</a></span>
            <span class="dot-sep">•</span>
            <span><strong>Active Branch:</strong> <code>${escapeHtml(repo.current_branch)}</code></span>
          </div>
          <div class="selected-repo-paths">
            <span><strong>Workdir:</strong> <code>${escapeHtml(repo.path)}</code></span>
            <span class="dot-sep">•</span>
            <span><strong>.git Index:</strong> <code>${escapeHtml(repo.git_dir)}</code></span>
          </div>
        </div>
      </div>
      <div class="selected-repo-actions">
        <button id="btn-back-to-dashboard" class="btn btn-secondary" type="button">← Workspace</button>
        ${
          repo.web_url
            ? `<button id="btn-open-remote-url" class="btn btn-secondary" data-url="${escapeHtml(repo.web_url)}">
                Open on ${escapeHtml(repo.primary_provider === "gitlab" ? "GitLab" : "GitHub / Remote")} ↗
              </button>`
            : ""
        }
      </div>
    </div>
  `;

  const autoUpdateCheckbox = document.getElementById("repo-auto-update");
  if (autoUpdateCheckbox) {
    autoUpdateCheckbox.addEventListener("change", () => {
      const status = document.getElementById("repo-auto-update-status");
      void saveRepositoryAutoUpdate(repo, autoUpdateCheckbox.checked, autoUpdateCheckbox, status);
    });
  }
  const commitCleanCheckbox = document.getElementById("repo-commit-clean");
  if (commitCleanCheckbox) {
    commitCleanCheckbox.addEventListener("change", () => {
      const status = document.getElementById("repo-commit-clean-status");
      void saveRepositoryCommitClean(repo, commitCleanCheckbox.checked, commitCleanCheckbox, status);
    });
  }

  const saveLocalSettingsButton = document.getElementById("btn-save-repo-local-settings");
  if (saveLocalSettingsButton) saveLocalSettingsButton.addEventListener("click", () => void saveRepositoryCleanerSettings(repo));

  const backToDashboardBtn = document.getElementById("btn-back-to-dashboard");
  if (backToDashboardBtn) {
    backToDashboardBtn.addEventListener("click", () => {
      selectedRepoId = null;
      selectedCommitOid = null;
      renderAll();
    });
  }

  const openRemoteBtn = document.getElementById("btn-open-remote-url");
  if (openRemoteBtn) {
    openRemoteBtn.addEventListener("click", async () => {
      const url = openRemoteBtn.getAttribute("data-url");
      if (!url) return;
      if (window.__TAURI__ && window.__TAURI__.shell && window.__TAURI__.shell.open) {
        await window.__TAURI__.shell.open(url);
      } else {
        window.open(url, "_blank", "noopener");
      }
    });
  }

  wireRepositoryArchivePanel(repo);
  const visibleCommits = commitsForSelectedBranch(repo);
  if (selectedCommitOid && !visibleCommits.some((commit) => commit.oid === selectedCommitOid)) {
    selectedCommitOid = visibleCommits[0] ? visibleCommits[0].oid : null;
  }
  if (treeStatsBadge) {
    const focusLabel = selectedBranchName ? " · " + selectedBranchName : " · all branches";
    treeStatsBadge.textContent = visibleCommits.length + " commits · " + (repo.branches || []).length + " branches" + focusLabel;
  }

  renderGitTreeGraph(treeContainer, visibleCommits, selectedCommitOid, (clickedCommit) => {
    selectedCommitOid = clickedCommit.oid;
    renderSelectedRepositoryDetails();
  }, repo.branches || []);
  wireCommitHoverDetails(treeContainer);

  const selectedCommit =
    repo.commits.find((c) => c.oid === selectedCommitOid) || repo.commits[0] || null;

  const remotesHtml =
    repo.remotes.length > 0
      ? repo.remotes
          .map(
            (rm) => `
          <div class="kv-item">
            <span class="kv-key">${escapeHtml(rm.name)} (${escapeHtml(rm.provider)})</span>
            <code class="kv-code">${escapeHtml(rm.url)}</code>
          </div>
        `
          )
          .join("")
      : `<div class="kv-empty">No remote configured (Local-only repository)</div>`;

  const branchesHtml =
    repo.branches.length > 0
      ? repo.branches
          .map(
            (b) => `
          <span class="branch-chip ${b.is_head ? "branch-head" : ""}">
            ${b.is_head ? "★ " : ""}${escapeHtml(b.name)}
          </span>
        `
          )
          .join("")
      : `<span class="kv-empty">No branches</span>`;

  inspectorEl.innerHTML = `
    <div class="inspector-card-block">
      <div class="inspector-avatar-row">
        <img
          src="${escapeHtml(iconSrc)}"
          alt="Repository Owner Icon"
          class="inspector-repo-icon"
          onerror="this.onerror=null;this.src='${fallbackSrc}';"
        />
        <div>
          <div class="inspector-repo-title">${escapeHtml(repo.name)}</div>
          <div class="inspector-repo-sub">Icon resolved via ${
            repo.has_remote ? escapeHtml(repo.primary_provider.toUpperCase()) : "Local Git Identity"
          }</div>
        </div>
      </div>

      <div class="kv-grid">
        <div class="kv-row">
          <span class="kv-label">Git Repository Name</span>
          <span class="kv-value">${escapeHtml(repo.name)}</span>
        </div>
        <div class="kv-row">
          <span class="kv-label">Repository Owner</span>
          <span class="kv-value">${escapeHtml(repo.owner_name)}</span>
        </div>
        <div class="kv-row">
          <span class="kv-label">Owner Email</span>
          <span class="kv-value"><code>${escapeHtml(repo.owner_email)}</code></span>
        </div>
        <div class="kv-row">
          <span class="kv-label">Indexed Parent Folder</span>
          <span class="kv-value"><code>${escapeHtml(repo.parent_folder)}</code></span>
        </div>
        <div class="kv-row">
          <span class="kv-label">.git Directory Path</span>
          <span class="kv-value"><code>${escapeHtml(repo.git_dir)}</code></span>
        </div>
      </div>
    </div>


    <div class="inspector-card-block gitmanager-archive-panel">
      <h4 class="inspector-section-title">Repository Archives</h4>
      <p class="archive-panel-note">Five restore points maximum per type · common secrets excluded by default.</p>
      <div class="archive-action-row">
        <button id="btn-capture-local-archive" class="btn btn-secondary" type="button">Save Local Archive</button>
        <button id="btn-capture-remote-archive" class="btn btn-secondary" type="button">Save Remote Archive</button>
        <button id="btn-refresh-repo-archives" class="btn btn-secondary" type="button">Refresh</button>
      </div>
      <div id="repo-archive-status" class="repository-setting-status" role="status">Loading saved restore points…</div>
      <div id="repo-archive-list" class="repo-archive-list"></div>
    </div>

    <div class="inspector-card-block automation-settings-card">
      <h4 class="inspector-section-title">Repository Local Settings</h4>
      <label class="repository-setting-option"><input id="repo-auto-update" type="checkbox" ${repositoryAutomationSettings(repo).autoUpdate ? "checked" : ""} ${repo.source_kind !== "local_scan" ? "disabled" : ""} /><span><strong>Auto Update</strong><small>Fast-forward only when this repository is clean and has no local-only commits.</small></span></label>
      <div id="repo-auto-update-status" class="repository-setting-status" role="status">${repo.source_kind === "local_scan" ? (repositoryAutomationSettings(repo).autoUpdate ? "Auto Update enabled · clean fast-forwards only" : "Auto Update is off") : "Available for locally indexed repositories only"}</div>
      <label class="repository-setting-option"><input id="repo-commit-clean" type="checkbox" ${repositoryAutomationSettings(repo).commitClean ? "checked" : ""} ${repo.source_kind !== "local_scan" ? "disabled" : ""} /><span><strong>Commit Clean</strong><small>Use a pre-commit hook to omit generated files from commits without deleting them.</small></span></label>
      <div id="repo-commit-clean-status" class="repository-setting-status" role="status">${repo.source_kind === "local_scan" ? (repositoryAutomationSettings(repo).commitClean ? "Commit Clean hook enabled" : "Commit Clean hook is not installed") : "Available for locally indexed repositories only"}</div>
      <label class="repository-setting-option"><input id="repo-cleaner-dependencies" type="checkbox" ${repositoryAutomationSettings(repo).cleanerIncludeDependencies ? "checked" : ""} ${repo.source_kind !== "local_scan" ? "disabled" : ""} /><span><strong>Include dependencies in Cleaner</strong><small>Allow preview to include dependency folders such as node_modules.</small></span></label>
      <label class="repository-setting-option"><input id="repo-cleaner-virtualenvs" type="checkbox" ${repositoryAutomationSettings(repo).cleanerIncludeVirtualenvs ? "checked" : ""} ${repo.source_kind !== "local_scan" ? "disabled" : ""} /><span><strong>Include Python virtual environments</strong><small>Allow validated virtual environments in this repository's cleanup preview.</small></span></label>
      <div class="repository-local-exclusions"><label for="repo-cleaner-exclusions">Cleaner exclusions</label><textarea id="repo-cleaner-exclusions" rows="4" placeholder=".next/&#10;generated/*.lock" ${repo.source_kind !== "local_scan" ? "disabled" : ""}>${escapeHtml((repositoryAutomationSettings(repo).cleanerExcludePatterns || []).join("\n"))}</textarea><small>One relative path or glob per line. Stored only in .git/info/gitmanager-settings.json and never committed.</small></div>
      <div class="settings-actions"><button id="btn-save-repo-local-settings" class="btn btn-secondary" type="button" ${repo.source_kind !== "local_scan" ? "disabled" : ""}>Save local settings</button></div>
      <div id="repo-local-settings-status" class="repository-setting-status" role="status">Saved separately for this repository.</div>
      <p class="repository-setting-note">Settings live under this repository's Git info directory, not in the worktree. Auto Update requires the Background Service. Existing custom hooks are never overwritten.</p>
    </div>

    <div class="inspector-card-block">
      <h4 class="inspector-section-title">Configured Remotes (${repo.remotes.length})</h4>
      ${remotesHtml}
    </div>

    <div class="inspector-card-block">
      <h4 class="inspector-section-title">Branches &amp; Refs (${repo.branches.length})</h4>
      <div class="branch-chip-list">${branchesHtml}</div>
    </div>

    ${
      selectedCommit
        ? `
      <div class="inspector-card-block commit-detail-box">
        <h4 class="inspector-section-title">Selected Commit Node</h4>
        <div class="kv-row">
          <span class="kv-label">Commit SHA</span>
          <span class="kv-value"><code>${escapeHtml(selectedCommit.oid)}</code></span>
        </div>
        <div class="kv-row">
          <span class="kv-label">Author</span>
          <span class="kv-value">${escapeHtml(selectedCommit.author_name)} &lt;${escapeHtml(selectedCommit.author_email)}&gt;</span>
        </div>
        <div class="kv-row">
          <span class="kv-label">Timestamp</span>
          <span class="kv-value">${escapeHtml(selectedCommit.date_iso)}</span>
        </div>
        <div class="kv-row">
          <span class="kv-label">Parent SHA(s)</span>
          <span class="kv-value"><code>${
            selectedCommit.parents.length > 0
              ? escapeHtml(selectedCommit.parents.map((p) => p.slice(0, 7)).join(", "))
              : "(root commit)"
          }</code></span>
        </div>
        <pre class="commit-full-message">${escapeHtml(selectedCommit.message)}</pre>
      </div>
    `
        : ""
    }
  `;
}

async function wireRepositoryArchivePanel(repo) {
  const localButton = document.getElementById("btn-capture-local-archive");
  const remoteButton = document.getElementById("btn-capture-remote-archive");
  const refreshButton = document.getElementById("btn-refresh-repo-archives");
  const status = document.getElementById("repo-archive-status");
  const list = document.getElementById("repo-archive-list");
  if (!list) return;

  const archiveRoot = (appPreferences && appPreferences.archiveRoot) || null;
  const setStatus = (value) => { if (status) status.textContent = value; };

  async function refresh() {
    if (refreshButton) refreshButton.disabled = true;
    try {
      const snapshots = await tauriInvoke("list_repository_archives", {
        repoPath: repo.path,
        kind: null,
        archiveRoot
      });
      if (!Array.isArray(snapshots) || snapshots.length === 0) {
        list.innerHTML = '<div class="kv-empty">No restore points yet. Save a Local or Remote Archive to start.</div>';
        return;
      }
      list.innerHTML = snapshots.map((snapshot) => {
        const date = snapshot.capturedAt ? new Date(snapshot.capturedAt).toLocaleString() : snapshot.id;
        const type = snapshot.kind === "remote" ? "Remote" : "Local";
        const excluded = Array.isArray(snapshot.excludedPaths) ? snapshot.excludedPaths.length : 0;
        return '<div class="repo-archive-item">' +
          '<div class="repo-archive-item-main">' +
            '<strong>' + escapeHtml(type + ' · ' + date) + '</strong>' +
            '<code>' + escapeHtml(snapshot.id) + '</code>' +
            '<small>' + Number(snapshot.fileCount || 0).toLocaleString() + ' files · ' +
              Number(snapshot.directoryCount || 0).toLocaleString() + ' folders · ' +
              Number(snapshot.sourceBytes || 0).toLocaleString() + ' source bytes · ' +
              excluded + ' excluded path(s)</small>' +
          '</div>' +
          '<button class="btn btn-secondary" type="button" data-archive-restore="' +
            escapeHtml(snapshot.id) + '" data-archive-kind="' + escapeHtml(snapshot.kind) + '">Restore</button>' +
        '</div>';
      }).join("");
    } catch (error) {
      list.innerHTML = '<div class="kv-empty">Could not load restore points: ' +
        escapeHtml((error && error.message) || error) + '</div>';
    } finally {
      if (refreshButton) refreshButton.disabled = false;
    }
  }

  async function capture(kind, button) {
    if (button) { button.disabled = true; button.textContent = "Saving…"; }
    setStatus("Capturing " + kind + " repository archive…");
    try {
      const snapshot = await tauriInvoke("capture_repository_archive", {
        repoPath: repo.path,
        kind,
        archiveRoot
      });
      setStatus("Saved " + kind + " restore point · " + snapshot.fileCount +
        " files · " + Number(snapshot.storedBytes || 0).toLocaleString() + " new bytes stored.");
      await refresh();
    } catch (error) {
      setStatus("Archive capture: " + String((error && error.message) || error));
      await refresh();
    } finally {
      if (button) {
        button.disabled = false;
        button.textContent = kind === "local" ? "Save Local Archive" : "Save Remote Archive";
      }
    }
  }

  if (localButton) localButton.onclick = () => void capture("local", localButton);
  if (remoteButton) remoteButton.onclick = () => void capture("remote", remoteButton);
  if (refreshButton) refreshButton.onclick = () => void refresh();
  list.onclick = async (event) => {
    const button = event.target.closest("[data-archive-restore]");
    if (!button) return;
    const snapshotId = button.getAttribute("data-archive-restore");
    const kind = button.getAttribute("data-archive-kind");
    if (!snapshotId || !kind) return;
    const accepted = window.confirm(
      "Restore this " + kind + " archive? GitManager will verify the contents and move the current repository folder aside before replacement."
    );
    if (!accepted) return;
    button.disabled = true;
    setStatus("Verifying and restoring " + snapshotId + "…");
    try {
      const result = await tauriInvoke("restore_repository_archive", {
        repoPath: repo.path,
        destinationPath: repo.path,
        snapshotId,
        kind,
        archiveRoot,
        confirmReplace: true
      });
      setStatus("Restored " + result.snapshot.id + ". Previous repository: " +
        (result.preservedPreviousPath || "not needed") + ".");
      await runMachineScan();
    } catch (error) {
      setStatus("Restore failed: " + String((error && error.message) || error));
    } finally {
      button.disabled = false;
      await refresh();
    }
  };
  await refresh();
}

function renderAll() {
  renderUpdateNotificationsTray();
  renderConnectedAccountsBar();
  renderAccountsPage();
  renderSettingsPage();
  renderSelectedRepositoryDetails();
  setActivePage(activePage);
}

async function runMachineScan(customRoot) {
  if (repositoryScanInProgress) return;
  repositoryScanInProgress = true;
  const scanBtn = document.getElementById("btn-scan-repos");
  if (scanBtn) {
    scanBtn.disabled = true;
    scanBtn.textContent = "Scanning repositories…";
  }
  renderAll();

  try {
    const rootPaths = customRoot && customRoot.trim().length > 0
      ? [customRoot.trim()]
      : (appPreferences && appPreferences.scanRoots) || [];
    const summary = await tauriInvoke("scan_machine_repositories", {
      rootPaths,
      maxDepth: (appPreferences && appPreferences.maxScanDepth) || 8,
      includeHiddenDirs: Boolean(appPreferences && appPreferences.includeHiddenDirs),
      indexDependencyRepositories: !appPreferences || appPreferences.indexDependencyRepositories !== false,
      filterMode: requestedFilter
    });
    allRepositories = Array.isArray(summary.repositories) ? summary.repositories.filter((repo) => repo.source_kind === "local_scan") : [];
    dependencyRepoIds = new Set(summary.dependencyRepoIds || []);
    selectedRepoId = null;
    selectedCommitOid = null;
  } catch (err) {
    console.error("GitManager repository scan failed:", err);
    if (allRepositories.length === 0) allRepositories = [];
  } finally {
    repositoryScanInProgress = false;
    if (scanBtn) {
      scanBtn.disabled = false;
      scanBtn.textContent = "Scan now";
    }
    renderAll();
  }
}

async function triggerManualUpdateCheck(customRoot) {
  setServiceCheckingVisual(true, "Svc: Checking Remotes...");
  try {
    const rootPaths = customRoot && customRoot.trim().length > 0
      ? [customRoot.trim()]
      : (appPreferences && appPreferences.scanRoots) || [];
    const updates = await tauriInvoke("check_repo_updates_now", { rootPaths });
    if (Array.isArray(updates) && updates.length > 0) {
      updates.forEach(pushRepoUpdateAlert);
    }
    setServiceCheckingVisual(false, `Svc: Checked (${updates.length} update(s))`);
  } catch (err) {
    setServiceCheckingVisual(false, "Svc: Update check failed");
    const note = document.getElementById("filter-status-note");
    if (note) note.textContent = "Could not check remote updates: " + String((err && err.message) || err);
  }
}

async function connectRemoteProviderAccount(provider, username, token) {
  const summary = await tauriInvoke("connect_git_account", {
    provider,
    usernameOrHost: username,
    accessToken: token.trim().length > 0 ? token.trim() : null
  });
  connectedAccounts = connectedAccounts.filter(
    (a) =>
      !(
        a.provider === summary.provider &&
        a.account_login.toLowerCase() === summary.account_login.toLowerCase()
      )
  );
  connectedAccounts.push(summary);


}

function wireTauriBackgroundServiceEvents() {
  if (window.__TAURI__ && window.__TAURI__.event && window.__TAURI__.event.listen) {
    window.__TAURI__.event.listen("gitmanager://update-check-state", (event) => {
      const payload = event.payload || {};
      if (payload.checking) {
        setServiceCheckingVisual(true, payload.status || "Svc: Checking Remotes...");
      } else {
        setServiceCheckingVisual(false, "Svc: Watching Remotes");
      }
    });

    window.__TAURI__.event.listen("gitmanager://repo-update-available", (event) => {
      if (event.payload) {
        pushRepoUpdateAlert(event.payload);
      }
    });
  }
}

async function refreshWindowsSigningCertificates() {
  const select=document.getElementById("signing-certificate-select"),status=document.getElementById("signing-status");if(!select)return;
  select.disabled=true;if(status)status.textContent="Reading CurrentUser Code Signing certificates…";
  try{
    const certificates=await tauriInvoke("list_windows_signing_certificates");
    select.innerHTML='<option value="">Choose a certificate…</option>'+(certificates||[]).map((certificate)=>
      '<option value="'+escapeHtml(certificate.thumbprint)+'">'+escapeHtml(certificate.subject)+' · expires '+escapeHtml(certificate.notAfter)+'</option>').join("");
    if(status)status.textContent=certificates&&certificates.length?certificates.length+" usable Code Signing certificate(s) found.":"No valid Code Signing certificate with a private key was found in CurrentUser\\My.";
  }catch(error){select.innerHTML='<option value="">Certificates unavailable</option>';if(status)status.textContent=String((error&&error.message)||error);}
  finally{select.disabled=false;}
}
async function signSelectedWindowsInstaller() {
  const select=document.getElementById("signing-certificate-select"),path=document.getElementById("signing-installer-path");
  const button=document.getElementById("btn-sign-windows-installer"),status=document.getElementById("signing-status");
  const thumbprint=select&&select.value,installerPath=path&&path.value.trim();
  if(!thumbprint||!installerPath){if(status)status.textContent="Choose a certificate and GitManager-Setup.exe first.";return;}
  if(button){button.disabled=true;button.textContent="Signing…";}if(status)status.textContent="Creating SHA-256 Authenticode signature and timestamp…";
  try{
    const result=await tauriInvoke("sign_windows_installer",{installerPath,thumbprint});
    if(status)status.textContent=result.chainTrusted?"Signature created and trust chain verified. SmartScreen reputation may still build over time.":"Signature created, but Windows could not verify the trust chain. This may be a self-signed or untrusted certificate.";
  }catch(error){if(status)status.textContent="Signing failed: "+String((error&&error.message)||error);}
  finally{if(button){button.disabled=false;button.textContent="Sign installer";}}
}

function wireEventListeners() {
  document.querySelectorAll("[data-page]").forEach((tab) => {
    tab.addEventListener("click", () => {
      const page = tab.getAttribute("data-page");
      if (page) setActivePage(page);
    });
  });
  const scanInput = document.getElementById("scan-path-input");
  const browseBtn = document.getElementById("btn-browse-folder");
  const scanBtn = document.getElementById("btn-scan-repos");
  const checkUpdatesBtn = document.getElementById("btn-check-updates-now");
  const registerOsSvcBtn = document.getElementById("btn-register-os-service");
  const searchInput = document.getElementById("repo-search-input");
  const indexGroupSelect = document.getElementById("index-group-select");
  const setupOpen = document.getElementById("btn-open-scan-settings");
  const setupClose = document.getElementById("btn-close-scan-setup");
  const setupModal = document.getElementById("setup-modal-backdrop");
  const setupBrowse = document.getElementById("btn-setup-browse-root");
  const archiveBrowse = document.getElementById("btn-setup-browse-archive-root");
  const setupAdd = document.getElementById("btn-setup-add-root");
  const setupSmart = document.getElementById("btn-setup-smart-roots");
  const setupManual = document.getElementById("setup-manual-root");
  const setupSave = document.getElementById("btn-save-scan-setup");
  const kindButtons = document.querySelectorAll("[data-repo-kind]");
  const refreshSigningButton=document.getElementById("btn-refresh-signing-certs");
  const browseSigningButton=document.getElementById("btn-browse-signing-installer");
  const signInstallerButton=document.getElementById("btn-sign-windows-installer");
  const signingPath=document.getElementById("signing-installer-path");
  if(refreshSigningButton)refreshSigningButton.addEventListener("click",()=>void refreshWindowsSigningCertificates());
  if(signInstallerButton)signInstallerButton.addEventListener("click",()=>void signSelectedWindowsInstaller());
  if(browseSigningButton)browseSigningButton.addEventListener("click",async()=>{
    const dialog=window.__TAURI__&&window.__TAURI__.dialog;if(!dialog||!dialog.open){if(signingPath)signingPath.focus();return;}
    const selected=await dialog.open({multiple:false,directory:false,title:"Choose the production GitManager setup installer",filters:[{name:"Windows installer",extensions:["exe","msi"]}]});
    if(typeof selected==="string"&&signingPath)signingPath.value=selected;
  });

  if (setupOpen) setupOpen.addEventListener("click", () => openScanSetup(false));
  if (setupClose) setupClose.addEventListener("click", () => {
    if (appPreferences && !appPreferences.firstRunComplete) {
      const smart = appPreferences.scanRoots || [];
      if (!smart.length) {
        appPreferences.scanRoots = [];
        void loadRecommendedScanRoots().then(() => saveScanSetup());
        return;
      }
      void saveScanSetup();
      return;
    }
    setupModal.classList.add("hidden");
  });
  if (setupBrowse) setupBrowse.addEventListener("click", () => void browseForScanRoot());
  if (archiveBrowse) archiveBrowse.addEventListener("click", () => void browseForArchiveRoot());
  if (setupAdd && setupManual) setupAdd.addEventListener("click", () => addScanRoot(setupManual.value));
  if (setupManual) setupManual.addEventListener("keydown", (event) => {
    if (event.key === "Enter") addScanRoot(setupManual.value);
  });
  if (setupSmart) setupSmart.addEventListener("click", () => void loadRecommendedScanRoots());
  if (setupSave) setupSave.addEventListener("click", () => void saveScanSetup());
  if (setupModal) setupModal.addEventListener("click", (event) => {
    if (event.target === setupModal && appPreferences && appPreferences.firstRunComplete) {
      setupModal.classList.add("hidden");
    }
  });
  const setupRootList = document.getElementById("setup-scan-root-list");
  if (setupRootList) setupRootList.addEventListener("click", (event) => {
    const button = event.target.closest("[data-remove-root]");
    if (!button) return;
    const index = Number(button.getAttribute("data-remove-root"));
    if (Number.isInteger(index)) {
      appPreferences.scanRoots.splice(index, 1);
      renderScanRootList();
    }
  });
  kindButtons.forEach((button) => button.addEventListener("click", () => {
    repositoryKindFilter = button.getAttribute("data-repo-kind") || "projects";
    if (repositoryKindFilter === "dependencies") requestedFilter = "grouped";
    renderAll();
  }));

  if (checkUpdatesBtn) {
    checkUpdatesBtn.addEventListener("click", () => {
      void triggerManualUpdateCheck(scanInput ? scanInput.value : "");
    });
  }

  if (registerOsSvcBtn) {
    registerOsSvcBtn.addEventListener("click", async () => {
      setServiceCheckingVisual(true, "Registering OS Service...");
      try {
        const info = await tauriInvoke("control_background_service", {
          enableInApp: true,
          installOsService: true
        });
        setServiceCheckingVisual(false, `Svc Active (${info.os_service_kind})`);
      } catch (e) {
        setServiceCheckingVisual(false, "Svc: Active (Background Worker)");
      }
    });
  }

  if (browseBtn) {
    browseBtn.addEventListener("click", async () => {
      if (window.__TAURI__ && window.__TAURI__.dialog && window.__TAURI__.dialog.open) {
        const selected = await window.__TAURI__.dialog.open({
          directory: true,
          multiple: false,
          title: "Select a root directory to scan for .git repositories"
        });
        if (typeof selected === "string" && scanInput) {
          scanInput.value = selected;
          await runMachineScan(selected);
        }
      } else if (scanInput) {
        scanInput.focus();
      }
    });
  }

  if (scanBtn) {
    scanBtn.addEventListener("click", () => {
      void runMachineScan(scanInput ? scanInput.value : "");
    });
  }

  if (searchInput) {
    searchInput.addEventListener("input", () => {
      searchQuery = searchInput.value;
      renderAll();
    });
  }

  if (indexGroupSelect) {
    indexGroupSelect.addEventListener("change", () => {
      indexGroupingMode = indexGroupSelect.value;
      renderAll();
    });
  }

  document.querySelectorAll("[data-filter]").forEach((btn) => {
    btn.addEventListener("click", () => {
      const mode = btn.getAttribute("data-filter");
      if (mode) {
        requestedFilter = mode;
        renderAll();
      }
    });
  });

  const modalBackdrop = document.getElementById("account-modal-backdrop");
  const openModalBtn = document.getElementById("btn-open-accounts");
  const closeModalBtn = document.getElementById("btn-close-modal");
  const cancelModalBtn = document.getElementById("btn-cancel-modal");
  const submitAccountBtn = document.getElementById("btn-submit-account");
  const providerSelect = document.getElementById("provider-select");
  const usernameInput = document.getElementById("account-username");
  const tokenInput = document.getElementById("account-token");
  const errorBox = document.getElementById("modal-error-box");

  const closeModal = () => {
    if (modalBackdrop) modalBackdrop.classList.add("hidden");
    if (errorBox) errorBox.classList.add("hidden");
  };

  if (openModalBtn) {
    openModalBtn.addEventListener("click", () => {
      if (modalBackdrop) modalBackdrop.classList.remove("hidden");
      if (usernameInput) usernameInput.focus();
    });
  }
  if (closeModalBtn) closeModalBtn.addEventListener("click", closeModal);
  if (cancelModalBtn) cancelModalBtn.addEventListener("click", closeModal);

  if (submitAccountBtn) {
    submitAccountBtn.addEventListener("click", async () => {
      const provider = (providerSelect && providerSelect.value) || "github";
      const username = (usernameInput && usernameInput.value.trim()) || "";
      const token = (tokenInput && tokenInput.value.trim()) || "";

      if (!username && !token && provider !== "github") {
        if (errorBox) {
          errorBox.textContent = "Please enter an account username or personal access token.";
          errorBox.classList.remove("hidden");
        }
        return;
      }

      submitAccountBtn.disabled = true;
      submitAccountBtn.textContent = "Connecting...";
      if (errorBox) errorBox.classList.add("hidden");

      try {
        await connectRemoteProviderAccount(provider, username, token);
        closeModal();
        renderAll();
      } catch (err) {
        if (errorBox) {
          errorBox.textContent = String((err && err.message) || err || "Failed to connect account");
          errorBox.classList.remove("hidden");
        }
      } finally {
        submitAccountBtn.disabled = false;
        submitAccountBtn.textContent = "Connect & Fetch Remote Repos";
      }
    });
  }
}

function updateScanRootSummary() {
  const input = document.getElementById("scan-path-input");
  if (!input) return;
  const roots = (appPreferences && appPreferences.scanRoots) || [];
  input.placeholder = roots.length
    ? roots.length + " scan folder(s) configured — enter a path for a one-time scan…"
    : "No scan folders configured — open Scan Setup…";
}

function renderScanRootList() {
  const list = document.getElementById("setup-scan-root-list");
  if (!list) return;
  const roots = (appPreferences && appPreferences.scanRoots) || [];
  if (!roots.length) {
    list.innerHTML = '<div class="setup-roots-empty">No folders selected yet.</div>';
    return;
  }
  list.innerHTML = roots.map((root, index) =>
    '<div class="setup-root-item"><span class="setup-root-index">' + (index + 1) + '</span>' +
    '<code title="' + escapeHtml(root) + '">' + escapeHtml(root) + '</code>' +
    '<button type="button" class="setup-root-remove" data-remove-root="' + index + '" aria-label="Remove scan folder">Remove</button></div>'
  ).join("");
}

function openScanSetup(isFirstRun) {
  if (!appPreferences) {
    appPreferences = {
      scanRoots: [], maxScanDepth: 8, includeHiddenDirs: false,
      indexDependencyRepositories: true, showDependencyRepositories: true,
      firstRunComplete: false
    };
  }
  const modal = document.getElementById("setup-modal-backdrop");
  if (!modal) return;
  const heading = document.getElementById("setup-modal-title");
  const subtitle = document.getElementById("setup-modal-subtitle");
  const save = document.getElementById("btn-save-scan-setup");
  if (heading) heading.textContent = isFirstRun ? "Set up your workspace" : "Scan settings";
  if (subtitle) subtitle.textContent = "Choose the folders GitManager should index. A drive root such as D:/ is allowed, but scanning a whole drive can take longer.";
  if (save) save.textContent = isFirstRun ? "Save & Start Indexing" : "Save Settings & Rescan";
  const depth = document.getElementById("setup-scan-depth");
  const hidden = document.getElementById("setup-include-hidden");
  const dependencies = document.getElementById("setup-index-dependencies");
  const showDependencies = document.getElementById("setup-show-dependencies");
  const archiveRoot = document.getElementById("setup-archive-root");
  const archiveLocal = document.getElementById("setup-archive-local");
  const archiveRemote = document.getElementById("setup-archive-remote");
  const archiveTrigger = document.getElementById("setup-archive-trigger");
  const archiveQuiet = document.getElementById("setup-archive-quiet");
  if (depth) depth.value = String(appPreferences.maxScanDepth || 8);
  if (hidden) hidden.checked = Boolean(appPreferences.includeHiddenDirs);
  if (dependencies) dependencies.checked = appPreferences.indexDependencyRepositories !== false;
  if (showDependencies) showDependencies.checked = appPreferences.showDependencyRepositories !== false;
  if (archiveRoot) archiveRoot.value = appPreferences.archiveRoot || "";
  if (archiveLocal) archiveLocal.checked = appPreferences.archiveLocalEnabled !== false;
  if (archiveRemote) archiveRemote.checked = appPreferences.archiveRemoteEnabled !== false;
  if (archiveTrigger) archiveTrigger.value = appPreferences.archiveTrigger || "manual";
  if (archiveQuiet) archiveQuiet.value = String(appPreferences.archiveQuietMs || 1500);
  renderScanRootList();
  modal.classList.remove("hidden");
}

function addScanRoot(value) {
  const root = String(value || "").trim();
  if (!root) return;
  if (!appPreferences.scanRoots.some((existing) => existing.toLowerCase() === root.toLowerCase())) {
    appPreferences.scanRoots.push(root);
  }
  const manual = document.getElementById("setup-manual-root");
  if (manual) manual.value = "";
  renderScanRootList();
}

async function browseForScanRoot() {
  if (window.__TAURI__ && window.__TAURI__.dialog && window.__TAURI__.dialog.open) {
    const selected = await window.__TAURI__.dialog.open({
      directory: true,
      multiple: false,
      title: "Choose a folder to index in GitManager"
    });
    if (typeof selected === "string") addScanRoot(selected);
  } else {
    const manual = document.getElementById("setup-manual-root");
    if (manual) manual.focus();
  }
}

async function browseForArchiveRoot() {
  const input = document.getElementById("setup-archive-root");
  const dialog = window.__TAURI__ && window.__TAURI__.dialog;
  if (!dialog || !dialog.open) {
    if (input) input.focus();
    return;
  }
  const selected = await dialog.open({
    multiple: false,
    directory: true,
    title: "Choose GitManager archive storage folder"
  });
  if (typeof selected === "string" && input) input.value = selected;
}

async function loadRecommendedScanRoots() {
  try {
    const roots = await tauriInvoke("get_default_scan_directories");
    if (Array.isArray(roots) && roots.length) {
      appPreferences.scanRoots = roots;
      renderScanRootList();
    }
  } catch (error) {
    console.warn("Could not load recommended scan folders:", error);
  }
}

async function saveScanSetup() {
  const save = document.getElementById("btn-save-scan-setup");
  const depth = document.getElementById("setup-scan-depth");
  const hidden = document.getElementById("setup-include-hidden");
  const dependencies = document.getElementById("setup-index-dependencies");
  const showDependencies = document.getElementById("setup-show-dependencies");
  const archiveRoot = document.getElementById("setup-archive-root");
  const archiveLocal = document.getElementById("setup-archive-local");
  const archiveRemote = document.getElementById("setup-archive-remote");
  const archiveTrigger = document.getElementById("setup-archive-trigger");
  const archiveQuiet = document.getElementById("setup-archive-quiet");
  const error = document.getElementById("setup-modal-error");
  if (error) {
    error.classList.add("hidden");
    error.textContent = "";
  }
  if (save) {
    save.disabled = true;
    save.textContent = "Saving…";
  }
  appPreferences.maxScanDepth = Number(depth && depth.value) || 8;
  appPreferences.includeHiddenDirs = Boolean(hidden && hidden.checked);
  appPreferences.indexDependencyRepositories = Boolean(dependencies && dependencies.checked);
  appPreferences.showDependencyRepositories = Boolean(showDependencies && showDependencies.checked);
  appPreferences.archiveRoot = String(archiveRoot && archiveRoot.value || appPreferences.archiveRoot || "").trim();
  appPreferences.archiveLocalEnabled = Boolean(archiveLocal && archiveLocal.checked);
  appPreferences.archiveRemoteEnabled = Boolean(archiveRemote && archiveRemote.checked);
  appPreferences.archiveTrigger = String(archiveTrigger && archiveTrigger.value || "manual");
  appPreferences.archiveQuietMs = Number(archiveQuiet && archiveQuiet.value) || 1500;
  appPreferences.firstRunComplete = true;

  try {
    appPreferences = await tauriInvoke("save_app_preferences", { preferences: appPreferences });
    repositoryKindFilter = appPreferences.showDependencyRepositories !== false ? "all" : "projects";
    updateScanRootSummary();
    document.getElementById("setup-modal-backdrop").classList.add("hidden");
    await runMachineScan();
  } catch (err) {
    if (error) {
      error.textContent = String((err && err.message) || err || "Could not save scan settings.");
      error.classList.remove("hidden");
    }
  } finally {
    if (save) {
      save.disabled = false;
      save.textContent = "Save & Start Indexing";
    }
  }
}

async function autoConnectGitHubIdentity() {
  try {
    const identity=await tauriInvoke("detect_github_identity"); if(!identity||!identity.login)return;
    githubIdentity=identity;
    const summary=await tauriInvoke("connect_git_account",{provider:"github",usernameOrHost:"",accessToken:null});
    connectedAccounts=connectedAccounts.filter((account)=>!(account.provider===summary.provider&&account.account_login.toLowerCase()===summary.account_login.toLowerCase()));
    connectedAccounts.push(summary);
    renderConnectedAccountsBar(); renderAccountsPage();
  } catch (_) {
    // A missing GitHub CLI session is normal; manual connection remains available.
  }
}

async function initializeGitManager() {
  try {
    applicationDirectories = await tauriInvoke("get_application_directories");
  } catch (error) {
    console.warn("Could not resolve GitManager storage directories:", error);
  }
  try {
    appPreferences = await tauriInvoke("load_app_preferences");
  } catch (error) {
    console.error("Could not load GitManager preferences:", error);
    appPreferences = {
      scanRoots: [],
      maxScanDepth: 8,
      includeHiddenDirs: false,
      indexDependencyRepositories: true,
      showDependencyRepositories: false,
      firstRunComplete: false
    };
  }
  repositoryKindFilter = appPreferences.showDependencyRepositories ? "all" : "projects";
  updateScanRootSummary();
  if (!appPreferences.firstRunComplete) {
    openScanSetup(true);
  } else {
    void runMachineScan();
  }
  void autoConnectGitHubIdentity();
}

window.addEventListener("DOMContentLoaded", () => {
  wireEventListeners();
  wireTauriBackgroundServiceEvents();
  void initializeGitManager();
});
