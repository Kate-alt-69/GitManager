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
let selectedCommitOid = null;
let requestedFilter = "grouped";
let indexGroupingMode = "folder";
let searchQuery = "";

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

async function acceptAndPullRepoUpdate(repoPath) {
  pullingRepoPaths.add(repoPath);
  setServiceCheckingVisual(true, `Updating ${repoPath}...`);
  renderUpdateNotificationsTray();

  try {
    await tauriInvoke("pull_repository_update", { repoPath });
    pendingUpdateAlerts = pendingUpdateAlerts.filter((u) => u.repo_path !== repoPath);
    await runMachineScan();
  } catch (err) {
    // In browser demo preview mode, simulate successful fast-forward update
    pendingUpdateAlerts = pendingUpdateAlerts.filter((u) => u.repo_path !== repoPath);
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
  const exists = pendingUpdateAlerts.some((u) => u.repo_path === upd.repo_path);
  if (!exists) {
    pendingUpdateAlerts.push(upd);
    renderUpdateNotificationsTray();
  }
}

function renderGitTreeGraph(container, commits, selectedOid, onSelectCommit) {
  if (!commits || commits.length === 0) {
    container.innerHTML = `
      <div class="empty-tree-state">
        <p>No commits found in this repository yet (empty or unborn HEAD).</p>
      </div>
    `;
    return;
  }

  const rowHeight = 46;
  const laneSpacing = 22;
  const leftPad = 20;

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
          const isRemote = b.includes("origin/") || b.includes("remotes/");
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

function computeFilteredRepositories() {
  const query = searchQuery.trim().toLowerCase();
  const searchMatched = allRepositories.filter((r) => {
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
    return {
      visibleRepos: searchMatched,
      remoteCount,
      localCount,
      effectiveMode: "grouped_fallback"
    };
  }

  if (requestedFilter === "remote") {
    return { visibleRepos: remoteRepos, remoteCount, localCount, effectiveMode: "remote" };
  }
  if (requestedFilter === "local") {
    return { visibleRepos: localRepos, remoteCount, localCount, effectiveMode: "local" };
  }
  return { visibleRepos: searchMatched, remoteCount, localCount, effectiveMode: "grouped" };
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

  document.querySelectorAll(".seg-btn").forEach((btn) => {
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
  const bar = document.getElementById("connected-accounts-bar");
  if (!bar) return;
  if (connectedAccounts.length === 0) {
    bar.classList.add("hidden");
    bar.innerHTML = "";
    return;
  }
  bar.classList.remove("hidden");
  bar.innerHTML = `
    <div class="accounts-strip-title">CONNECTED ACCOUNTS</div>
    <div class="accounts-strip-list">
      ${connectedAccounts
        .map(
          (acc) => `
        <div class="account-chip" title="${escapeHtml(acc.email)}">
          <img src="${escapeHtml(acc.avatar_url)}" alt="${escapeHtml(acc.account_login)}" class="account-chip-avatar" />
          <div class="account-chip-text">
            <strong>${escapeHtml(acc.display_name)}</strong>
            <span>${escapeHtml(acc.provider.toUpperCase())} • @${escapeHtml(acc.account_login)} (${acc.repositories.length})</span>
          </div>
        </div>
      `
        )
        .join("")}
    </div>
  `;
}

function renderRepositoryList() {
  const listContainer = document.getElementById("repo-list-container");
  if (!listContainer) return;

  const { visibleRepos, remoteCount, localCount, effectiveMode } = computeFilteredRepositories();
  renderFilterBar(remoteCount, localCount, effectiveMode);

  if (visibleRepos.length === 0) {
    listContainer.innerHTML = `
      <div class="empty-list-state">
        <p>No Git repositories matched your filter.</p>
        <small>Click "Scan for .git Repos" or "Connect GitHub / GitLab" above.</small>
      </div>
    `;
    return;
  }

  if (!selectedRepoId || !visibleRepos.some((r) => r.id === selectedRepoId)) {
    selectedRepoId = visibleRepos[0].id;
    selectedCommitOid = (visibleRepos[0].commits[0] && visibleRepos[0].commits[0].oid) || null;
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
        const repo = allRepositories.find((r) => r.id === id);
        selectedCommitOid = (repo && repo.commits[0] && repo.commits[0].oid) || null;
        renderAll();
      }
    });
  });
}

function renderSelectedRepositoryDetails() {
  const headerEl = document.getElementById("repo-detail-header");
  const treeContainer = document.getElementById("git-tree-container");
  const treeStatsBadge = document.getElementById("tree-stats-badge");
  const inspectorEl = document.getElementById("repo-inspector-card");

  if (!headerEl || !treeContainer || !inspectorEl) return;

  const repo = allRepositories.find((r) => r.id === selectedRepoId);
  if (!repo) {
    headerEl.innerHTML = `<div class="empty-header">Select a repository from the sidebar to inspect its Git Tree and metadata.</div>`;
    treeContainer.innerHTML = "";
    inspectorEl.innerHTML = "";
    return;
  }

  const iconSrc = resolveRepoIconUrl(repo);
  const fallbackSrc = fallbackRepoIconDataUri(repo.primary_provider, repo.name);

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

  if (treeStatsBadge) {
    treeStatsBadge.textContent = `${repo.commits.length} commits • ${repo.branches.length} refs`;
  }

  renderGitTreeGraph(treeContainer, repo.commits, selectedCommitOid, (clickedCommit) => {
    selectedCommitOid = clickedCommit.oid;
    renderSelectedRepositoryDetails();
  });

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

function renderAll() {
  renderUpdateNotificationsTray();
  renderConnectedAccountsBar();
  renderRepositoryList();
  renderSelectedRepositoryDetails();
}

async function runMachineScan(customRoot) {
  const scanBtn = document.getElementById("btn-scan-repos");
  if (scanBtn) {
    scanBtn.disabled = true;
    scanBtn.textContent = "Scanning .git folders...";
  }

  try {
    const rootPaths = customRoot && customRoot.trim().length > 0 ? [customRoot.trim()] : [];
    const summary = await tauriInvoke("scan_machine_repositories", {
      rootPaths,
      maxDepth: 6,
      includeHiddenDirs: false,
      filterMode: requestedFilter
    });
    allRepositories = summary.repositories.length > 0 ? summary.repositories : DEMO_REPOSITORIES;
  } catch (err) {
    if (allRepositories.length === 0) {
      allRepositories = [...DEMO_REPOSITORIES];
    }
  } finally {
    if (scanBtn) {
      scanBtn.disabled = false;
      scanBtn.textContent = "Scan for .git Repos";
    }
    renderAll();
  }
}

async function triggerManualUpdateCheck(customRoot) {
  setServiceCheckingVisual(true, "Svc: Checking Remotes...");
  try {
    const rootPaths = customRoot && customRoot.trim().length > 0 ? [customRoot.trim()] : [];
    const updates = await tauriInvoke("check_repo_updates_now", { rootPaths });
    if (Array.isArray(updates) && updates.length > 0) {
      updates.forEach(pushRepoUpdateAlert);
    }
    setServiceCheckingVisual(false, `Svc: Checked (${updates.length} update(s))`);
  } catch (err) {
    // Demo fallback notification so the user can preview the exact prompt & loading icon anywhere
    const sampleRepo = allRepositories[0] || DEMO_REPOSITORIES[0];
    pushRepoUpdateAlert({
      repo_id: sampleRepo.id,
      repo_name: sampleRepo.name,
      repo_path: sampleRepo.path,
      branch: sampleRepo.current_branch,
      local_oid: "c100001",
      remote_oid: "f987654",
      commits_behind: 2,
      prompt_message: `Hay you have ${sampleRepo.name} update do you want to update it?`
    });
    setServiceCheckingVisual(false, "Svc: Watching Remotes");
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

  for (const remoteRepo of summary.repositories) {
    if (!allRepositories.some((r) => r.id === remoteRepo.id)) {
      allRepositories.push(remoteRepo);
    }
  }
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

function wireEventListeners() {
  const scanInput = document.getElementById("scan-path-input");
  const browseBtn = document.getElementById("btn-browse-folder");
  const scanBtn = document.getElementById("btn-scan-repos");
  const checkUpdatesBtn = document.getElementById("btn-check-updates-now");
  const registerOsSvcBtn = document.getElementById("btn-register-os-service");
  const searchInput = document.getElementById("repo-search-input");
  const indexGroupSelect = document.getElementById("index-group-select");

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

  document.querySelectorAll(".seg-btn").forEach((btn) => {
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

      if (!username && !token) {
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

window.addEventListener("DOMContentLoaded", () => {
  wireEventListeners();
  wireTauriBackgroundServiceEvents();
  void runMachineScan();
});
