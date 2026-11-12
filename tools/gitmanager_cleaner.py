#!/usr/bin/env python3
"""GitManager's cross-platform, preview-first developer cache cleaner.

This helper ports the safe project/cache cleanup model from DevToolchain-Clean.
It never follows symlinks, never removes .git, confines repository cleanup to
the selected worktree, and requires --confirm YES before deleting anything.
Remote cleanup uses a temporary clone and pushes a new branch, never the
remote default branch.
"""
from __future__ import annotations

import argparse
import fnmatch
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Iterable, Optional
from urllib.parse import urlparse

VERSION = "1.0.0"
MANAGED_IGNORE_BEGIN = "# >>> GitManager managed clean rules >>>"
MANAGED_IGNORE_END = "# <<< GitManager managed clean rules <<<"
def managed_ignore_rules(plan: dict, removed_paths: list[str]) -> list[str]:
    """Ignore only paths this cleanup actually removed; never blanket-ignore user folders."""
    removed = set(removed_paths)
    rules = []
    for item in plan.get("candidates", []):
        if item.get("path") not in removed:
            continue
        relative = str(item.get("relative_path", "")).replace("\\", "/").strip("/")
        if not relative:
            continue
        rule = "/" + relative + ("/" if item.get("kind") == "directory" else "")
        rules.append(rule)
    return sorted(set(rules), key=str.casefold)


@dataclass
class Candidate:
    path: str
    relative_path: str
    label: str
    category: str
    size_bytes: int
    kind: str
    tracked: bool = False
    ignored_by_git: bool = False


def _is_relative_to(path: Path, root: Path) -> bool:
    try:
        path.relative_to(root)
        return True
    except ValueError:
        return False


def _is_linklike(path: Path) -> bool:
    """Recognize symlinks and (on Windows/Python 3.12+) junctions."""
    try:
        if path.is_symlink():
            return True
        is_junction = getattr(os.path, "isjunction", None)
        return bool(is_junction and is_junction(path))
    except OSError:
        return True


def _has_symlink_component(path: Path, boundary: Optional[Path] = None) -> bool:
    """Return true if path or any existing component below boundary is a link."""
    try:
        resolved_boundary = (boundary or (Path(path.anchor) if path.anchor else path.parent)).resolve(strict=False)
        current = resolved_boundary
        relative = path.absolute().relative_to(resolved_boundary.absolute())
        for part in relative.parts:
            current = current / part
            if _is_linklike(current):
                return True
        return False
    except (OSError, ValueError):
        return True


def _size_without_links(path: Path) -> int:
    try:
        if _is_linklike(path):
            return 0
        if path.is_file():
            return path.stat(follow_symlinks=False).st_size
        total = 0
        for base, dirs, files in os.walk(path, topdown=True, followlinks=False):
            base_path = Path(base)
            dirs[:] = [name for name in dirs if not _is_linklike(base_path / name)]
            for name in files:
                candidate = base_path / name
                try:
                    if not _is_linklike(candidate):
                        total += candidate.stat(follow_symlinks=False).st_size
                except OSError:
                    continue
        return total
    except OSError:
        return 0


def _git(repo: Path, *args: str, timeout: int = 20) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["git", "-C", str(repo), *args],
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
        timeout=timeout,
        env={**os.environ, "GIT_TERMINAL_PROMPT": "0"},
    )


def _git_repo_root(path: Path) -> Path:
    result = subprocess.run(
        ["git", "-C", str(path), "rev-parse", "--show-toplevel"],
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
        timeout=10,
    )
    if result.returncode != 0:
        raise ValueError(f"Not a Git working tree: {path}")
    return Path(result.stdout.strip()).resolve(strict=True)


def _tracked(repo: Path, relative: str) -> bool:
    return bool(_git(repo, "ls-files", "--", relative).stdout.strip())


def _ignored(repo: Path, relative: str) -> bool:
    return _git(repo, "check-ignore", "-q", "--", relative).returncode == 0


def _project_kinds(directory: Path) -> set[str]:
    try:
        entries = {item.name.lower() for item in directory.iterdir() if not _is_linklike(item)}
    except OSError:
        return set()
    kinds: set[str] = set()
    if entries.intersection({
        "package.json", "package-lock.json", "npm-shrinkwrap.json",
        "pnpm-lock.yaml", "yarn.lock", "bun.lock", "bun.lockb",
    }):
        kinds.add("node")
    if "cargo.toml" in entries:
        kinds.add("rust")
    if entries.intersection({
        "pyproject.toml", "requirements.txt", "requirements-dev.txt",
        "setup.py", "setup.cfg", "pipfile",
    }):
        kinds.add("python")
    if any(name.endswith((".csproj", ".fsproj", ".vbproj", ".sln")) for name in entries):
        kinds.add("dotnet")
    if entries.intersection({
        "build.gradle", "build.gradle.kts", "settings.gradle",
        "settings.gradle.kts", "gradlew",
    }):
        kinds.add("gradle")
    if "pom.xml" in entries:
        kinds.add("maven")
    if "cmakelists.txt" in entries or "cmakepresets.json" in entries:
        kinds.add("cmake")
    if "pubspec.yaml" in entries:
        kinds.add("dart")
    if "build.zig" in entries:
        kinds.add("zig")
    if "go.mod" in entries or "go.work" in entries:
        kinds.add("go")
    return kinds


def _custom_exclusions(repo: Path) -> list[str]:
    file = repo / ".gitmanager-cleanignore"
    if not file.is_file() or _is_linklike(file):
        return []
    try:
        return [
            line.strip()
            for line in file.read_text(encoding="utf-8").splitlines()
            if line.strip() and not line.lstrip().startswith("#")
        ]
    except OSError:
        return []


def _matches_exclusion(relative: str, patterns: Iterable[str]) -> bool:
    normalized = relative.replace(os.sep, "/")
    for pattern in patterns:
        candidate = pattern.strip().lstrip("/")
        if not candidate:
            continue
        if fnmatch.fnmatch(normalized, candidate) or fnmatch.fnmatch(
            Path(normalized).name, candidate.rstrip("/")
        ):
            return True
        if candidate.endswith("/") and (
            normalized == candidate[:-1] or normalized.startswith(candidate)
        ):
            return True
    return False


def _safe_candidate(path: Path, root: Path) -> bool:
    try:
        root = root.resolve(strict=True)
        absolute = path.absolute()
        if absolute == root or not _is_relative_to(absolute, root):
            return False
        if any(part.lower() == ".git" for part in absolute.relative_to(root).parts):
            return False
        if _has_symlink_component(absolute, root):
            return False
        if len(absolute.parts) < 3:
            return False
        return path.exists() and (path.is_dir() or path.is_file())
    except (OSError, ValueError):
        return False


def build_repo_plan(
    repo_path: str | Path,
    *,
    include_dependencies: bool = False,
    include_virtualenvs: bool = False,
    include_build_caches: bool = True,
    include_tracked: bool = False,
) -> dict:
    repo = _git_repo_root(Path(repo_path).expanduser())
    exclusions = _custom_exclusions(repo)
    candidates: dict[str, Candidate] = {}
    skipped_tracked: list[str] = []

    def add(target: Path, label: str, category: str) -> None:
        if not _safe_candidate(target, repo):
            return
        relative = target.relative_to(repo).as_posix()
        if _matches_exclusion(relative, exclusions):
            return
        is_tracked = _tracked(repo, relative)
        if is_tracked and not include_tracked:
            skipped_tracked.append(relative)
            return
        candidates[relative.casefold()] = Candidate(
            path=str(target),
            relative_path=relative,
            label=label,
            category=category,
            size_bytes=_size_without_links(target),
            kind="directory" if target.is_dir() else "file",
            tracked=is_tracked,
            ignored_by_git=_ignored(repo, relative),
        )

    # Skip large/generated trees while still checking the project root itself.
    prune_names = {
        ".git", ".hg", ".svn", "node_modules", "target", "vendor",
        ".venv", "venv", "env", ".tox", ".nox", ".uv", "__pycache__",
        "dist", "build", "bin", "obj", ".next", ".turbo", ".nuxt",
        ".output", ".svelte-kit", ".parcel-cache", ".vite", ".cache",
        ".pytest_cache", ".mypy_cache", ".ruff_cache", ".gradle",
        ".dart_tool", ".zig-cache", "zig-out",
    }
    for current, dirs, files in os.walk(repo, topdown=True, followlinks=False):
        current_path = Path(current)
        dirs[:] = sorted(
            name for name in dirs
            if name.lower() not in prune_names and not _is_linklike(current_path / name)
        )
        kinds = _project_kinds(current_path)
        if not kinds or not include_build_caches:
            continue

        if "node" in kinds:
            for name, label in [
                (".next", "Next.js output"), (".turbo", "Turborepo cache"),
                (".nuxt", "Nuxt output"), (".output", "Nitro output"),
                (".svelte-kit", "SvelteKit output"), (".parcel-cache", "Parcel cache"),
                (".cache", "Node project cache"), (".vite", "Vite cache"),
                ("dist", "Node distribution output"),
            ]:
                add(current_path / name, label, "Build cache")
            for name in (".cache", ".vite", ".tmp"):
                add(current_path / "node_modules" / name, f"node_modules {name} cache", "Build cache")
            for name in files:
                if name.endswith(".tsbuildinfo"):
                    add(current_path / name, "TypeScript build info", "Build cache")
            if include_dependencies:
                add(current_path / "node_modules", "Node.js dependencies", "Dependencies")
                add(current_path / ".pnpm-store", "Project pnpm store", "Dependencies")
                add(current_path / ".yarn" / "cache", "Project Yarn cache", "Dependencies")
                add(current_path / ".yarn" / "unplugged", "Project Yarn unplugged packages", "Dependencies")

        if "rust" in kinds:
            add(current_path / "target", "Cargo build output", "Build cache")

        if "python" in kinds:
            for name in (".pytest_cache", ".mypy_cache", ".ruff_cache", "__pycache__"):
                add(current_path / name, f"Python {name}", "Build cache")
            if include_virtualenvs:
                for name in (".venv", "venv", "env", ".tox", ".nox", ".uv"):
                    candidate = current_path / name
                    if candidate.is_dir() and (
                        (candidate / "pyvenv.cfg").is_file() or name in (".tox", ".nox")
                    ):
                        add(candidate, "Python virtual environment", "Virtual environment")

        if "dotnet" in kinds:
            for name in ("bin", "obj"):
                add(current_path / name, f".NET {name} output", "Build cache")

        if "gradle" in kinds:
            add(current_path / ".gradle", "Project Gradle cache", "Build cache")
            add(current_path / "build", "Gradle build output", "Build cache")

        if "maven" in kinds:
            add(current_path / "target", "Maven build output", "Build cache")

        if "cmake" in kinds:
            for name in ("build", "CMakeFiles", "CMakeCache.txt"):
                add(current_path / name, f"CMake {name}", "Build cache")
            for name in dirs:
                if name.startswith("cmake-build-"):
                    add(current_path / name, "CMake IDE build directory", "Build cache")

        if "dart" in kinds:
            add(current_path / ".dart_tool", "Dart/Flutter tool cache", "Build cache")
            add(current_path / "build", "Flutter build output", "Build cache")

        if "zig" in kinds:
            add(current_path / ".zig-cache", "Zig local cache", "Build cache")
            add(current_path / "zig-out", "Zig generated output", "Build cache")

    rows = sorted(candidates.values(), key=lambda item: (-item.size_bytes, item.relative_path.casefold()))
    return {
        "version": VERSION,
        "scope": "repository",
        "root": str(repo),
        "candidate_count": len(rows),
        "total_bytes": sum(item.size_bytes for item in rows),
        "candidates": [asdict(item) for item in rows],
        "skipped_tracked": sorted(set(skipped_tracked)),
        "errors": [],
        "options": {
            "include_dependencies": include_dependencies,
            "include_virtualenvs": include_virtualenvs,
            "include_build_caches": include_build_caches,
            "include_tracked": include_tracked,
        },
    }


def _home_cache_paths() -> list[tuple[Path, str]]:
    home = Path.home()
    local = Path(os.environ.get("LOCALAPPDATA", home / "AppData/Local"))
    roaming = Path(os.environ.get("APPDATA", home / "AppData/Roaming"))
    paths = [
        (local / "npm-cache", "npm cache"), (local / "pnpm-cache", "pnpm cache"),
        (local / "Yarn/Cache", "Yarn cache"), (local / "node-gyp/Cache", "node-gyp cache"),
        (home / ".bun/install/cache", "Bun package cache"), (home / ".bun/tmp", "Bun temporary cache"),
        (home / ".cargo/registry/cache", "Cargo registry downloads"),
        (home / ".cargo/git/checkouts", "Cargo Git checkouts"),
        (home / ".rustup/downloads", "Rustup downloads"), (home / ".rustup/tmp", "Rustup temporary files"),
        (local / "pip/Cache", "pip cache"), (local / "pypa/Cache", "PyPA cache"),
        (local / "uv/cache", "uv cache"), (home / ".cache/uv", "uv cache"),
        (home / ".cache/pip", "pip cache"), (local / "pypoetry/Cache", "Poetry cache"),
        (home / ".mypy_cache", "mypy cache"), (home / ".ruff_cache", "Ruff cache"),
        (local / "go-build", "Go build cache"), (home / ".cache/go-build", "Go build cache"),
        (home / ".gradle/caches", "Gradle caches"), (home / ".gradle/daemon", "Gradle daemon state"),
        (home / ".gradle/native", "Gradle native cache"), (home / ".cache/zig", "Zig cache"),
        (local / "NuGet/v3-cache", "NuGet HTTP cache"), (local / "NuGet/plugins-cache", "NuGet plugin cache"),
    ]
    if sys.platform == "win32":
        paths.extend([
            (roaming / "npm-cache", "npm roaming cache"),
            (local / "Microsoft/MSBuild/DiagnosticLogs", "MSBuild diagnostic logs"),
            (local / "Microsoft/VisualStudio/Roslyn/Cache", "Roslyn cache"),
            (local / "Microsoft/VisualStudio/Packages", "Visual Studio installer package cache"),
            (local / "Mozilla/sccache/cache", "sccache compiler cache"),
            (local / "deno", "Deno cache"), (local / "golangci-lint", "golangci-lint cache"),
        ])
    return paths


def build_system_plan(*, include_windows_junk: bool = False) -> dict:
    candidates: dict[str, Candidate] = {}
    errors: list[str] = []
    home = Path.home().resolve()
    for path, label in _home_cache_paths():
        try:
            if not path.exists() or _is_linklike(path):
                continue
            absolute = path.resolve(strict=True)
            if not _is_relative_to(absolute, home):
                continue
            if absolute == home or len(absolute.parts) < 3 or _has_symlink_component(path, home):
                continue
            candidates[str(absolute).casefold()] = Candidate(
                path=str(absolute), relative_path=str(absolute), label=label,
                category="Global cache", size_bytes=_size_without_links(absolute),
                kind="directory" if absolute.is_dir() else "file",
            )
        except OSError as exc:
            errors.append(f"{path}: {exc}")

    if include_windows_junk and sys.platform == "win32":
        windir = Path(os.environ.get("WINDIR", r"C:\Windows"))
        program_data = Path(os.environ.get("PROGRAMDATA", r"C:\ProgramData"))
        curated = [
            (windir / "Temp", "Windows temporary files"),
            (windir / "SoftwareDistribution/Download", "Windows Update downloads"),
            (program_data / "Microsoft/Windows/DeliveryOptimization/Cache", "Delivery Optimization cache"),
            (program_data / "Microsoft/Windows/WER/ReportQueue", "Windows Error Reporting queue"),
            (program_data / "Microsoft/Windows/WER/ReportArchive", "Windows Error Reporting archive"),
        ]
        for path, label in curated:
            try:
                if path.exists() and not _is_linklike(path):
                    candidates[str(path.resolve()).casefold()] = Candidate(
                        path=str(path.resolve()), relative_path=str(path.resolve()),
                        label=label, category="Windows junk",
                        size_bytes=_size_without_links(path), kind="directory",
                    )
            except OSError as exc:
                errors.append(f"{path}: {exc}")

    rows = sorted(candidates.values(), key=lambda item: (-item.size_bytes, item.path.casefold()))
    return {
        "version": VERSION, "scope": "system", "root": str(home),
        "candidate_count": len(rows), "total_bytes": sum(item.size_bytes for item in rows),
        "candidates": [asdict(item) for item in rows], "errors": errors,
        "options": {"include_windows_junk": include_windows_junk},
    }


def _delete_candidate(item: dict, root: Optional[Path] = None) -> None:
    path = Path(item["path"])
    if root is not None and not _safe_candidate(path, root):
        raise ValueError(f"Safety check rejected candidate: {path}")
    if _is_linklike(path):
        raise ValueError(f"Refusing to delete symlink/junction: {path}")
    if path.is_dir():
        shutil.rmtree(path)
    elif path.is_file():
        path.unlink()


def apply_plan(plan: dict, *, confirmation: str, include_tracked: bool = False) -> dict:
    if confirmation != "YES":
        raise ValueError("Deletion requires --apply and the exact confirmation word YES.")
    root = Path(plan["root"]).resolve() if plan["scope"] == "repository" else None
    removed: list[str] = []
    failures: list[str] = []
    freed = 0
    for item in plan["candidates"]:
        if item.get("tracked") and not include_tracked:
            continue
        try:
            _delete_candidate(item, root)
            removed.append(item["path"])
            freed += int(item.get("size_bytes", 0))
        except FileNotFoundError:
            continue
        except Exception as exc:
            failures.append(f"{item['path']}: {exc}")
    return {
        "scope": plan["scope"], "removed_count": len(removed),
        "freed_bytes_estimate": freed, "removed": removed, "failures": failures,
    }


def _validate_remote_url(url: str) -> None:
    if any(character.isspace() for character in url) or url.startswith("-"):
        raise ValueError("Remote URL contains invalid whitespace or option-like input.")
    parsed = urlparse(url)
    if parsed.scheme in ("https", "ssh") and parsed.hostname:
        if parsed.scheme == "https" and (parsed.username or parsed.password):
            raise ValueError("Do not embed credentials in a remote URL; use your Git credential manager.")
        if parsed.password:
            raise ValueError("Do not embed passwords in SSH URLs.")
        return
    if re.fullmatch(r"[A-Za-z0-9_.-]+@[A-Za-z0-9_.-]+:[A-Za-z0-9_./-]+(?:\.git)?", url):
        return
    raise ValueError("Use an HTTPS or SSH Git remote URL (local paths and custom protocols are not allowed).")


def _clone_remote(url: str, destination: Path, branch: Optional[str]) -> Path:
    _validate_remote_url(url)
    command = ["git", "clone", "--depth", "1", "--single-branch"]
    if branch:
        command.extend(["--branch", branch])
    command.extend([url, str(destination)])
    result = subprocess.run(
        command, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        check=False, timeout=180,
        env={**os.environ, "GIT_TERMINAL_PROMPT": "0", "GCM_INTERACTIVE": "Never"},
    )
    if result.returncode != 0:
        raise RuntimeError(f"git clone failed: {(result.stderr or result.stdout).strip()}")
    return _git_repo_root(destination)


def remote_plan(
    url: str, branch: Optional[str] = None, *,
    include_dependencies: bool = True, include_virtualenvs: bool = False,
) -> dict:
    with tempfile.TemporaryDirectory(prefix="gitmanager-clean-preview-") as temp:
        clone = _clone_remote(url, Path(temp) / "repo", branch)
        plan = build_repo_plan(
            clone, include_dependencies=include_dependencies,
            include_virtualenvs=include_virtualenvs, include_build_caches=True,
            include_tracked=True,
        )
        plan.update({"scope": "remote", "remote_url": url,
                     "remote_branch": branch or "default branch", "preview_only": True})
        return plan


MANAGED_EXCLUDE_BEGIN = "# >>> GitManager Commit Clean excludes >>>"
MANAGED_EXCLUDE_END = "# <<< GitManager Commit Clean excludes <<<"


def _git_common_dir(repo: Path) -> Path:
    result = _git(repo, "rev-parse", "--git-common-dir")
    if result.returncode != 0:
        raise ValueError(f"Could not resolve the common Git directory: {result.stderr.strip()}")
    path = Path(result.stdout.strip())
    if not path.is_absolute():
        path = repo / path
    return path.resolve(strict=True)


def _default_hooks_dir(repo: Path) -> Path:
    configured = _git(repo, "config", "--local", "--get", "core.hooksPath")
    if configured.returncode == 0 and configured.stdout.strip():
        raise ValueError(
            "This repository already uses a custom core.hooksPath. GitManager will not modify custom hook directories."
        )
    common_dir = _git_common_dir(repo)
    hooks_dir = common_dir / "hooks"
    if _is_linklike(hooks_dir):
        raise ValueError("GitManager refused a symlinked .git/hooks directory.")
    if not _is_relative_to(hooks_dir.resolve(strict=False), common_dir):
        raise ValueError("GitManager refused an unexpected hooks directory.")
    return hooks_dir


def _managed_exclude_rules(plan: dict, removed_paths: list[str]) -> list[str]:
    removed = set(removed_paths)
    rules = []
    for item in plan.get("candidates", []):
        if item.get("path") not in removed:
            continue
        relative = str(item.get("relative_path", "")).replace("\\", "/").strip("/")
        if relative:
            rules.append("/" + relative + ("/" if item.get("kind") == "directory" else ""))
    return sorted(set(rules), key=str.casefold)


def _local_exclude_file(repo: Path) -> Path:
    common_dir = _git_common_dir(repo)
    info_dir = common_dir / "info"
    exclude_file = info_dir / "exclude"
    if _is_linklike(info_dir) or _is_linklike(exclude_file):
        raise ValueError("GitManager refused a symlinked .git/info/exclude path.")
    return exclude_file


def _update_local_excludes(repo: Path, plan: dict, removed_paths: list[str]) -> None:
    rules = _managed_exclude_rules(plan, removed_paths)
    if not rules:
        return
    exclude_file = _local_exclude_file(repo)
    exclude_file.parent.mkdir(parents=True, exist_ok=True)
    existing = exclude_file.read_text(encoding="utf-8") if exclude_file.exists() else ""
    lines = existing.splitlines()
    begin = lines.index(MANAGED_EXCLUDE_BEGIN) if MANAGED_EXCLUDE_BEGIN in lines else -1
    previous_rules = []
    if begin >= 0:
        try:
            end = lines.index(MANAGED_EXCLUDE_END, begin + 1)
        except ValueError:
            end = begin
        previous_rules = [
            line for line in lines[begin + 1:end]
            if line.strip() and not line.lstrip().startswith("#")
        ]
        lines = lines[:begin] + lines[end + 1:]
    existing_rules = set(lines)
    combined = sorted((set(previous_rules) | set(rules)) - existing_rules, key=str.casefold)
    if combined:
        body = "\n".join(lines).rstrip()
        block = "\n".join([MANAGED_EXCLUDE_BEGIN, *combined, MANAGED_EXCLUDE_END])
        exclude_file.write_text(body + ("\n\n" if body else "") + block + "\n", encoding="utf-8")


def commit_clean_index(
    repo_path: str | Path,
    *,
    include_dependencies: bool = True,
) -> dict:
    """Remove generated candidates from the Git index only; preserve every working-tree file."""
    repo = _git_repo_root(Path(repo_path).expanduser())
    plan = build_repo_plan(
        repo,
        include_dependencies=include_dependencies,
        include_virtualenvs=False,
        include_build_caches=True,
        include_tracked=True,
    )
    omitted: list[str] = []
    failures: list[str] = []
    tracked_candidates = [item for item in plan["candidates"] if item.get("tracked")]
    # Validate the local exclude destination before altering the index so a
    # symlinked .git/info/exclude cannot leave a half-applied cleanup.
    if tracked_candidates:
        _local_exclude_file(repo)
    for item in tracked_candidates:
        relative = item["relative_path"]
        if not _safe_candidate(Path(item["path"]), repo):
            failures.append(f"{relative}: safety check rejected the path")
            continue
        result = _git(repo, "rm", "--cached", "-r", "-f", "--", relative, timeout=120)
        if result.returncode == 0:
            omitted.append(relative)
        else:
            failures.append(f"{relative}: {(result.stderr or result.stdout).strip()}")

    if omitted:
        _update_local_excludes(repo, plan, [item["path"] for item in plan["candidates"] if item["relative_path"] in omitted])

    return {
        "scope": "commit",
        "repository": str(repo),
        "removed_count": len(omitted),
        "omitted_paths": omitted,
        "working_tree_preserved": True,
        "failures": failures,
    }


def _commit_hook_contents() -> str:
    return """#!/bin/sh
# GitManager managed Commit Clean hook v1
HOOK_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
REPO_ROOT=$(git rev-parse --show-toplevel 2>/dev/null) || exit 1
if command -v py >/dev/null 2>&1 && py -3 -c 'import sys; raise SystemExit(sys.version_info < (3, 10))' >/dev/null 2>&1; then
  exec py -3 "$HOOK_DIR/gitmanager_cleaner.py" commit-clean --path "$REPO_ROOT"
elif command -v python3 >/dev/null 2>&1 && python3 -c 'import sys; raise SystemExit(sys.version_info < (3, 10))' >/dev/null 2>&1; then
  exec python3 "$HOOK_DIR/gitmanager_cleaner.py" commit-clean --path "$REPO_ROOT"
elif command -v python >/dev/null 2>&1 && python -c 'import sys; raise SystemExit(sys.version_info < (3, 10))' >/dev/null 2>&1; then
  exec python "$HOOK_DIR/gitmanager_cleaner.py" commit-clean --path "$REPO_ROOT"
fi
echo "[GitManager] Commit Clean requires Python 3.10 or newer (python3, py -3, or python)." >&2
exit 1
"""


def set_commit_clean_hook(
    repo_path: str | Path,
    script_path: str | Path,
    *,
    enabled: bool,
) -> dict:
    repo = _git_repo_root(Path(repo_path).expanduser())
    hooks_dir = _default_hooks_dir(repo)
    hook_path = hooks_dir / "pre-commit"
    helper_path = hooks_dir / "gitmanager_cleaner.py"
    if _is_linklike(hook_path) or _is_linklike(helper_path):
        raise ValueError("GitManager refused a symlinked pre-commit hook or helper path.")
    hook_contents = _commit_hook_contents()

    if not enabled:
        if not hook_path.exists():
            return {"enabled": False, "changed": False, "message": "No GitManager Commit Clean hook was installed."}
        try:
            current = hook_path.read_text(encoding="utf-8")
        except OSError as exc:
            raise ValueError(f"Could not inspect the pre-commit hook: {exc}") from exc
        if current != hook_contents:
            raise ValueError(
                "The current pre-commit hook is not the exact GitManager-managed hook. It was left untouched."
            )
        hook_path.unlink()
        # Delete only the exact helper copy installed beside our hook.
        if helper_path.is_file():
            try:
                helper_text = helper_path.read_text(encoding="utf-8")
            except OSError:
                helper_text = ""
            if (
                "GitManager's cross-platform, preview-first developer cache cleaner." in helper_text
                and "def set_commit_clean_hook(" in helper_text
            ):
                helper_path.unlink()
        return {"enabled": False, "changed": True, "message": "GitManager Commit Clean hook removed."}

    source = Path(script_path).resolve(strict=True)
    if not source.is_file():
        raise ValueError(f"Cleaner helper does not exist: {source}")
    hooks_dir.mkdir(parents=True, exist_ok=True)
    if hook_path.exists():
        current = hook_path.read_text(encoding="utf-8")
        if current != hook_contents:
            raise ValueError(
                "A pre-commit hook already exists. GitManager will not overwrite or chain an existing custom hook."
            )
    if helper_path.exists() and helper_path.read_bytes() != source.read_bytes():
        try:
            installed_helper = helper_path.read_text(encoding="utf-8")
        except OSError:
            installed_helper = ""
        if (
            "GitManager's cross-platform, preview-first developer cache cleaner." not in installed_helper
            or "def set_commit_clean_hook(" not in installed_helper
        ):
            raise ValueError("A different gitmanager_cleaner.py already exists in .git/hooks; it was left untouched.")
        # Refresh only a copy that identifies itself as GitManager-managed.
        shutil.copy2(source, helper_path)

    if not helper_path.exists():
        shutil.copy2(source, helper_path)
    if not hook_path.exists():
        hook_path.write_text(hook_contents, encoding="utf-8", newline="\n")
        try:
            hook_path.chmod(hook_path.stat().st_mode | 0o111)
        except OSError:
            pass
    return {
        "enabled": True,
        "changed": True,
        "hook_path": str(hook_path),
        "message": "GitManager Commit Clean hook installed. Generated outputs will be omitted from commits while their working-tree files are preserved.",
    }


def remote_clean(
    url: str, *, confirmation: str, branch: Optional[str] = None,
    include_dependencies: bool = True, include_virtualenvs: bool = False,
    manage_gitignore: bool = True, push: bool = False,
) -> dict:
    if confirmation != "YES":
        raise ValueError("Remote cleanup requires --apply and the exact confirmation word YES.")
    if not push:
        raise ValueError("Remote cleanup must explicitly set --push. It only pushes a new branch, never the default branch.")
    with tempfile.TemporaryDirectory(prefix="gitmanager-clean-remote-") as temp:
        clone = _clone_remote(url, Path(temp) / "repo", branch)
        plan = build_repo_plan(
            clone, include_dependencies=include_dependencies,
            include_virtualenvs=include_virtualenvs, include_build_caches=True,
            include_tracked=True,
        )
        removed = apply_plan(plan, confirmation="YES", include_tracked=True)
        if manage_gitignore and removed["removed_count"]:
            ignore_file = clone / ".gitignore"
            existing = ignore_file.read_text(encoding="utf-8") if ignore_file.exists() else ""
            managed_rules = managed_ignore_rules(plan, removed["removed"])
            if managed_rules:
                lines = existing.splitlines()
                begin = lines.index(MANAGED_IGNORE_BEGIN) if MANAGED_IGNORE_BEGIN in lines else -1
                previous_rules = []
                if begin >= 0:
                    try:
                        end = lines.index(MANAGED_IGNORE_END, begin + 1)
                    except ValueError:
                        # A malformed marker must never cause user-authored lines to be dropped.
                        end = begin
                    previous_rules = [
                        line for line in lines[begin + 1:end]
                        if line.strip() and not line.lstrip().startswith("#")
                    ]
                    lines = lines[:begin] + lines[end + 1:]
                existing_lines = set(lines)
                combined_rules = sorted(
                    (set(previous_rules) | set(managed_rules)) - existing_lines,
                    key=str.casefold,
                )
                if combined_rules:
                    block = [MANAGED_IGNORE_BEGIN, *combined_rules, MANAGED_IGNORE_END]
                    merged = "\n".join(lines).rstrip()
                    ignore_file.write_text(
                        merged + ("\n\n" if merged else "") + "\n".join(block) + "\n",
                        encoding="utf-8",
                    )
        status = _git(clone, "status", "--porcelain")
        if not status.stdout.strip():
            return {"scope": "remote", "changed": False,
                    "message": "No cleanable tracked outputs were found.", **removed}
        _git(clone, "config", "user.name", "GitManager").check_returncode()
        _git(clone, "config", "user.email", "gitmanager@users.noreply.github.com").check_returncode()
        _git(clone, "add", "--all").check_returncode()
        commit = _git(clone, "commit", "-m", "gitManager | Cleaned repo of junk")
        if commit.returncode != 0:
            raise RuntimeError(f"git commit failed: {(commit.stderr or commit.stdout).strip()}")
        stamp = time.strftime("%Y%m%d-%H%M%S", time.gmtime())
        push_branch = f"gitmanager/clean/{stamp}"
        pushed = _git(
            clone, "push", "origin", f"HEAD:refs/heads/{push_branch}", timeout=180
        )
        if pushed.returncode != 0:
            raise RuntimeError(
                f"Push failed; the temporary cleanup branch was not published: {(pushed.stderr or pushed.stdout).strip()}"
            )
        head = _git(clone, "rev-parse", "HEAD").stdout.strip()
        return {
            **removed, "scope": "remote", "changed": True, "branch": push_branch,
            "commit": head, "commit_message": "gitManager | Cleaned repo of junk",
            "pushed": True,
        }


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Preview and safely remove reconstructable developer caches."
    )
    sub = parser.add_subparsers(dest="command", required=True)
    for command in ("plan", "apply"):
        current = sub.add_parser(command)
        current.add_argument("--scope", choices=("repository", "system"), default="repository")
        current.add_argument("--path", default=".", help="Git repository path for repository scope")
        current.add_argument("--include-dependencies", action="store_true")
        current.add_argument("--include-virtualenvs", action="store_true")
        current.add_argument("--no-build-caches", action="store_true")
        current.add_argument("--include-windows-junk", action="store_true")
        current.add_argument("--include-tracked", action="store_true")
        if command == "apply":
            current.add_argument("--confirm", required=True, help="Must be exactly YES")

    commit_clean = sub.add_parser("commit-clean", help="Omit generated output from the Git index before commit without deleting working files")
    commit_clean.add_argument("--path", default=".")
    commit_clean.add_argument("--no-dependencies", action="store_true")

    hook = sub.add_parser("commit-hook", help="Install or remove the repository-local GitManager pre-commit hook")
    hook.add_argument("--path", required=True)
    hook.add_argument("--script", help="Path to the bundled GitManager cleaner Python helper")
    hook.add_argument("--disable", action="store_true")

    remote = sub.add_parser("remote-plan", help="Temporarily clone and preview tracked generated outputs")
    remote.add_argument("--url", required=True)
    remote.add_argument("--branch")
    remote.add_argument("--no-dependencies", action="store_true")
    remote.add_argument("--include-virtualenvs", action="store_true")

    remote = sub.add_parser("remote-clean", help="Clean a temporary clone and push a new cleanup branch")
    remote.add_argument("--url", required=True)
    remote.add_argument("--branch")
    remote.add_argument("--confirm", required=True)
    remote.add_argument("--push", action="store_true", help="Required: push the cleanup branch to origin")
    remote.add_argument("--no-dependencies", action="store_true")
    remote.add_argument("--include-virtualenvs", action="store_true")
    remote.add_argument("--no-managed-gitignore", action="store_true")
    return parser


def main(argv: Optional[list[str]] = None) -> int:
    args = _parser().parse_args(argv)
    try:
        if args.command in ("plan", "apply"):
            if args.scope == "repository":
                plan = build_repo_plan(
                    args.path, include_dependencies=args.include_dependencies,
                    include_virtualenvs=args.include_virtualenvs,
                    include_build_caches=not args.no_build_caches,
                    include_tracked=args.include_tracked,
                )
            else:
                plan = build_system_plan(include_windows_junk=args.include_windows_junk)
            if args.command == "plan":
                print(json.dumps(plan, indent=2))
            else:
                result = apply_plan(
                    plan, confirmation=args.confirm, include_tracked=args.include_tracked
                )
                print(json.dumps(result, indent=2))
        elif args.command == "commit-clean":
            result = commit_clean_index(
                args.path,
                include_dependencies=not args.no_dependencies,
            )
            print(json.dumps(result, indent=2))
        elif args.command == "commit-hook":
            if args.disable:
                result = set_commit_clean_hook(args.path, "", enabled=False)
            else:
                if not args.script:
                    raise ValueError("Installing the hook requires --script.")
                result = set_commit_clean_hook(args.path, args.script, enabled=True)
            print(json.dumps(result, indent=2))
        elif args.command == "remote-plan":
            result = remote_plan(
                args.url, args.branch, include_dependencies=not args.no_dependencies,
                include_virtualenvs=args.include_virtualenvs,
            )
            print(json.dumps(result, indent=2))
        else:
            result = remote_clean(
                args.url, confirmation=args.confirm, branch=args.branch,
                include_dependencies=not args.no_dependencies,
                include_virtualenvs=args.include_virtualenvs,
                manage_gitignore=not args.no_managed_gitignore, push=args.push,
            )
            print(json.dumps(result, indent=2))
        return 0
    except Exception as exc:
        print(json.dumps({"error": str(exc), "command": args.command}), file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
