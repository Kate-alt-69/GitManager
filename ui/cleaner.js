/* GitManager Developer Cleaner UI. Uses the Tauri commands registered by the Rust backend. */
(() => {
  let lastPreview = null;

  const byId = (id) => document.getElementById(id);
  const selectedRepo = () => allRepositories.find((repo) => repo.id === selectedRepoId) || null;

  function formatBytes(value) {
    let size = Math.max(0, Number(value) || 0);
    const units = ["B", "KiB", "MiB", "GiB", "TiB"];
    let index = 0;
    while (size >= 1024 && index < units.length - 1) {
      size /= 1024;
      index += 1;
    }
    return size.toFixed(2) + " " + units[index];
  }

  function setStatus(message) {
    const status = byId("cleaner-status");
    if (status) status.textContent = message;
  }

  function invalidatePreview() {
    lastPreview = null;
    const apply = byId("btn-cleaner-apply");
    if (apply) apply.disabled = true;
    const output = byId("cleaner-plan-output");
    if (output) {
      output.textContent = "Settings changed. Preview again to review the updated cleanup plan.";
      output.className = "cleaner-plan-output cleaner-empty";
    }
  }

  function updateScopeFields() {
    const scope = byId("cleaner-scope").value;
    byId("cleaner-repository-fields").classList.toggle("hidden", scope !== "repository");
    byId("cleaner-remote-fields").classList.toggle("hidden", scope !== "remote");
    byId("cleaner-windows-junk-option").classList.toggle(
      "hidden",
      scope !== "system" || !/Win/i.test(navigator.platform || "")
    );
    const note = byId("cleaner-scope-note");
    if (scope === "repository") {
      note.textContent = "Local cleanup is confined to this Git worktree, never follows symlinks, and skips Git-tracked paths by default. Dependency folders are opt-in.";
    } else if (scope === "system") {
      note.textContent = "System cleanup only targets known developer cache locations in your current user profile. It does not scan whole drives or remove installed toolchains.";
    } else {
      note.textContent = "GitManager clones the selected remote temporarily, removes recognized generated outputs and opted-in dependency folders, then pushes a new cleanup branch. It never pushes to the default branch.";
    }
    invalidatePreview();
  }

  function openDialog() {
    const repo = selectedRepo();
    const endpoint = repo && repo.remotes
      ? (repo.remotes.find((remote) => remote.name === "origin") || repo.remotes[0])
      : null;
    byId("cleaner-scope").value = "repository";
    byId("cleaner-repo-path").value = (repo && repo.path) || byId("scan-path-input").value || "";
    byId("cleaner-remote-url").value = (repo && repo.primary_remote_url) || (endpoint && endpoint.url) || "";
    byId("cleaner-remote-branch").value = "";
    byId("cleaner-confirm").value = "";
    ["cleaner-include-dependencies", "cleaner-include-venvs", "cleaner-include-windows-junk"].forEach((id) => {
      byId(id).checked = false;
    });
    updateScopeFields();
    setStatus("Ready to preview");
    byId("cleaner-modal-backdrop").classList.remove("hidden");
  }

  function addCandidate(output, item) {
    const row = document.createElement("div");
    row.className = "cleaner-candidate";
    const copy = document.createElement("div");
    copy.className = "cleaner-candidate-copy";
    const title = document.createElement("strong");
    title.textContent = item.label || item.category || "Cleanup item";
    const path = document.createElement("span");
    const flags = [];
    if (item.tracked) flags.push("Git-tracked");
    if (item.ignored_by_git) flags.push("ignored by .gitignore");
    path.textContent = (item.relative_path || item.path || "") + (flags.length ? " · " + flags.join(" · ") : "");
    copy.append(title, path);
    const size = document.createElement("span");
    size.className = "cleaner-candidate-size";
    size.textContent = formatBytes(item.size_bytes);
    row.append(copy, size);
    output.appendChild(row);
  }

  function renderResult(result, preview) {
    const output = byId("cleaner-plan-output");
    output.replaceChildren();
    output.className = "cleaner-plan-output";
    if (result.error) {
      const error = document.createElement("div");
      error.className = "cleaner-empty";
      error.textContent = result.error;
      output.appendChild(error);
      return;
    }
    const candidates = Array.isArray(result.candidates) ? result.candidates : [];
    const count = Number(result.candidate_count ?? result.removed_count ?? candidates.length);
    const bytes = Number(result.total_bytes ?? result.freed_bytes_estimate ?? 0);
    const summary = document.createElement("div");
    summary.className = "cleaner-plan-summary";
    const total = document.createElement("span");
    total.textContent = preview
      ? count + " candidate(s) · " + formatBytes(bytes) + " estimated"
      : Number(result.removed_count || 0) + " item(s) removed · " + formatBytes(bytes) + " estimated";
    const scope = document.createElement("span");
    scope.textContent = result.scope || "";
    summary.append(total, scope);
    output.appendChild(summary);

    candidates.slice(0, 60).forEach((item) => addCandidate(output, item));
    if (candidates.length > 60) {
      const extra = document.createElement("div");
      extra.className = "cleaner-empty";
      extra.textContent = "And " + (candidates.length - 60) + " more item(s)…";
      output.appendChild(extra);
    }
    if (Array.isArray(result.skipped_tracked) && result.skipped_tracked.length) {
      const skipped = document.createElement("div");
      skipped.className = "cleaner-empty";
      skipped.textContent = result.skipped_tracked.length + " Git-tracked path(s) skipped. Remote cleanup is performed in a temporary clone.";
      output.appendChild(skipped);
    }
    if (Array.isArray(result.failures) && result.failures.length) {
      const failures = document.createElement("div");
      failures.className = "cleaner-empty";
      failures.textContent = result.failures.join("\n");
      output.appendChild(failures);
    }
    if (result.branch) {
      const pushed = document.createElement("div");
      pushed.className = "cleaner-empty";
      pushed.textContent = "Pushed branch: " + result.branch + " · Commit: " + (result.commit || "");
      output.appendChild(pushed);
    }
    if (preview && candidates.length === 0) {
      const empty = document.createElement("div");
      empty.className = "cleaner-empty";
      empty.textContent = "No cleanable paths were found for this selection.";
      output.appendChild(empty);
    }
  }

  function cleanerArgs() {
    return {
      scope: byId("cleaner-scope").value,
      repoPath: byId("cleaner-repo-path").value.trim() || null,
      remoteUrl: byId("cleaner-remote-url").value.trim() || null,
      branch: byId("cleaner-remote-branch").value.trim() || null,
      includeDependencies: byId("cleaner-include-dependencies").checked,
      includeVirtualenvs: byId("cleaner-include-venvs").checked,
      includeWindowsJunk: byId("cleaner-include-windows-junk").checked
    };
  }

  async function preview() {
    const button = byId("btn-cleaner-preview");
    const apply = byId("btn-cleaner-apply");
    button.disabled = true;
    button.textContent = "Scanning…";
    apply.disabled = true;
    setStatus("Building a safe cleanup plan…");
    try {
      const plan = await tauriInvoke("cleaner_preview", cleanerArgs());
      lastPreview = plan;
      renderResult(plan, true);
      setStatus("Preview ready — nothing has been deleted");
      apply.disabled = !(Number(plan.candidate_count) > 0 && byId("cleaner-confirm").value === "YES");
    } catch (error) {
      lastPreview = null;
      renderResult({ error: String((error && error.message) || error) }, true);
      setStatus("Preview failed");
    } finally {
      button.disabled = false;
      button.textContent = "Preview cleanup";
    }
  }

  async function apply() {
    if (!lastPreview || Number(lastPreview.candidate_count) < 1) return;
    if (byId("cleaner-confirm").value !== "YES") {
      setStatus("Type YES exactly to confirm");
      return;
    }
    if (!window.confirm("Delete the previewed paths? Remote cleanup pushes a new branch only.")) return;
    const applyButton = byId("btn-cleaner-apply");
    const previewButton = byId("btn-cleaner-preview");
    applyButton.disabled = true;
    applyButton.textContent = "Cleaning…";
    previewButton.disabled = true;
    setStatus("Applying cleanup…");
    const args = Object.assign({}, cleanerArgs(), { confirmText: "YES" });
    try {
      const result = await tauriInvoke("cleaner_apply", args);
      lastPreview = null;
      renderResult(result, false);
      setStatus(result.pushed ? "Cleanup branch pushed: " + result.branch : "Cleanup finished");
      if (args.scope === "repository" && args.repoPath && typeof runMachineScan === "function") {
        await runMachineScan(args.repoPath);
      }
    } catch (error) {
      renderResult({ error: String((error && error.message) || error) }, false);
      setStatus("Cleanup failed");
    } finally {
      applyButton.textContent = "Apply cleanup";
      previewButton.disabled = false;
    }
  }

  window.addEventListener("DOMContentLoaded", () => {
    byId("btn-open-cleaner")?.addEventListener("click", openDialog);
    byId("btn-close-cleaner")?.addEventListener("click", () => byId("cleaner-modal-backdrop").classList.add("hidden"));
    byId("cleaner-modal-backdrop")?.addEventListener("click", (event) => {
      if (event.target === byId("cleaner-modal-backdrop")) byId("cleaner-modal-backdrop").classList.add("hidden");
    });
    byId("cleaner-scope")?.addEventListener("change", () => {
      if (byId("cleaner-scope").value === "remote") byId("cleaner-include-dependencies").checked = true;
      updateScopeFields();
    });
    ["cleaner-repo-path", "cleaner-remote-url", "cleaner-remote-branch",
      "cleaner-include-dependencies", "cleaner-include-venvs", "cleaner-include-windows-junk"]
      .forEach((id) => byId(id)?.addEventListener("change", invalidatePreview));
    byId("cleaner-confirm")?.addEventListener("input", () => {
      const enabled = Boolean(lastPreview && Number(lastPreview.candidate_count) > 0 && byId("cleaner-confirm").value === "YES");
      byId("btn-cleaner-apply").disabled = !enabled;
    });
    byId("btn-cleaner-preview")?.addEventListener("click", () => void preview());
    byId("btn-cleaner-apply")?.addEventListener("click", () => void apply());
  });
})();