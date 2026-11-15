#!/usr/bin/env python3
"""Resolve and build GitManager against the newest CI-green Next.js Engine source.

The Engine is fetched from GitHub source archives, never from the published npm
package. A SHA is resolved at build time and recorded as provenance; it is not
pinned in GitManager source.
"""

import argparse
import http.client
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request
from datetime import datetime, timezone
from pathlib import Path, PurePosixPath
from typing import Dict, List, Tuple

ENGINE_REPOSITORY = "Kastrick8/nextjs-engine"
ENGINE_BRANCH = "main-3"
ENGINE_API = "https://api.github.com/repos/" + ENGINE_REPOSITORY
REQUIRED_CHECKS = (
    "Engine runtime CI",
    "Engine cross-browser compatibility",
    "CodeQL",
    "Code Quality: Push on main-3",
)
USER_AGENT = "GitManager-Nextjs-Engine-Builder/1.0"
MAX_API_BYTES = 12 * 1024 * 1024
MAX_ARCHIVE_BYTES = 250 * 1024 * 1024
MAX_MEMBER_BYTES = 80 * 1024 * 1024
MAX_ARCHIVE_MEMBERS = 50000
REQUEST_ATTEMPTS = 3
RETRYABLE_HTTP_STATUS = frozenset((408, 429, 500, 502, 503, 504))


def log_ok(message: str) -> None:
    print("[ OK ] " + message, flush=True)


def log_info(message: str) -> None:
    print("[INFO] " + message, flush=True)


def log_error(message: str) -> None:
    print("[ERR ] " + message, file=sys.stderr, flush=True)


def request_bytes(url: str, limit: int, accept: str) -> bytes:
    parsed_url = urllib.parse.urlsplit(url)
    if parsed_url.scheme != "https" or parsed_url.hostname not in {"api.github.com", "codeload.github.com"}:
        raise RuntimeError("Refusing to request Engine source data from an untrusted host.")

    headers = {
        "User-Agent": USER_AGENT,
        "Accept": accept,
        "X-GitHub-Api-Version": "2022-11-28",
    }
    # GitHub Actions supplies a short-lived, read-scoped token to avoid the
    # strict unauthenticated API rate limit on multi-platform installer builds.
    # Local builds remain usable without a token and never prompt for credentials.
    token = os.environ.get("GITHUB_TOKEN") or os.environ.get("GH_TOKEN")
    if parsed_url.hostname == "api.github.com" and token and not any(char in token for char in "\\r\\n"):
        headers["Authorization"] = "Bearer " + token

    request = urllib.request.Request(url, headers=headers)
    last_error = None
    for attempt in range(REQUEST_ATTEMPTS):
        try:
            with urllib.request.urlopen(request, timeout=60) as response:
                parsed = urllib.parse.urlsplit(response.geturl())
                if parsed.scheme != "https" or parsed.hostname not in {"api.github.com", "codeload.github.com"}:
                    raise RuntimeError("Refusing an Engine download redirect to an untrusted host.")
                declared = response.headers.get("Content-Length")
                if declared:
                    try:
                        if int(declared) > limit:
                            raise RuntimeError("The Engine response exceeds the configured download limit.")
                    except ValueError:
                        raise RuntimeError("The Engine response returned an invalid content length.")
                chunks = []
                total = 0
                while True:
                    chunk = response.read(1024 * 1024)
                    if not chunk:
                        break
                    total += len(chunk)
                    if total > limit:
                        raise RuntimeError("The Engine response exceeds the configured download limit.")
                    chunks.append(chunk)
                return b"".join(chunks)
        except urllib.error.HTTPError as error:
            if error.code not in RETRYABLE_HTTP_STATUS:
                raise RuntimeError("Could not access the Next.js Engine source: " + str(error)) from error
            last_error = error
            error.close()
        except (http.client.HTTPException, OSError) as error:
            # IncompleteRead is raised by http.client when a chunked response
            # ends before the advertised chunk length. Discard this partial
            # body and retry the complete bounded request from the beginning.
            last_error = error

        if attempt + 1 < REQUEST_ATTEMPTS:
            delay = min(2 ** attempt, 4)
            log_info(
                "GitHub source request was interrupted; retrying in "
                + str(delay)
                + "s (attempt "
                + str(attempt + 2)
                + "/"
                + str(REQUEST_ATTEMPTS)
                + ")."
            )
            time.sleep(delay)

    raise RuntimeError(
        "Could not access the Next.js Engine source after "
        + str(REQUEST_ATTEMPTS)
        + " attempts: "
        + str(last_error)
    ) from last_error


def github_json(url: str) -> Dict[str, object]:
    payload = request_bytes(url, MAX_API_BYTES, "application/vnd.github+json")
    try:
        decoded = json.loads(payload.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise RuntimeError("GitHub returned invalid JSON while resolving Engine source.") from error
    if not isinstance(decoded, dict):
        raise RuntimeError("GitHub returned an unexpected response while resolving Engine source.")
    return decoded


def resolve_latest_green_commit() -> Tuple[str, str, Dict[str, Dict[str, object]]]:
    """Pick the newest main-3 commit with all required CI runs green and reachable."""
    branch_url = ENGINE_API + "/branches/" + urllib.parse.quote(ENGINE_BRANCH, safe="")
    branch = github_json(branch_url)
    branch_head = str((branch.get("commit") or {}).get("sha") or "")
    if not re.fullmatch(r"[0-9a-f]{40}", branch_head):
        raise RuntimeError("Could not resolve the current Engine main-3 branch head.")

    latest_runs: Dict[Tuple[str, str], Dict[str, object]] = {}
    for page in range(1, 6):
        runs_url = (
            ENGINE_API
            + "/actions/runs?branch="
            + urllib.parse.quote(ENGINE_BRANCH, safe="")
            + "&status=completed&per_page=100&page="
            + str(page)
        )
        payload = github_json(runs_url)
        runs = payload.get("workflow_runs")
        if not isinstance(runs, list) or not runs:
            break
        for run in runs:
            if not isinstance(run, dict) or run.get("head_branch") != ENGINE_BRANCH:
                continue
            name = str(run.get("name") or "")
            if name not in REQUIRED_CHECKS:
                continue
            sha = str(run.get("head_sha") or "")
            if not re.fullmatch(r"[0-9a-f]{40}", sha):
                continue
            key = (sha, name)
            current = latest_runs.get(key)
            if current is None or str(run.get("updated_at") or "") > str(current.get("updated_at") or ""):
                latest_runs[key] = run

        # Inspect the full recent run window before selecting a candidate. A
        # partially paginated workflow set must not hide a newer green commit.

    by_commit: Dict[str, Dict[str, Dict[str, object]]] = {}
    for (sha, name), run in latest_runs.items():
        by_commit.setdefault(sha, {})[name] = run

    candidates = []
    for sha, checks in by_commit.items():
        if not all(name in checks for name in REQUIRED_CHECKS):
            continue
        if any(
            checks[name].get("status") != "completed"
            or checks[name].get("conclusion") != "success"
            for name in REQUIRED_CHECKS
        ):
            continue
        latest_check_time = max(str(checks[name].get("updated_at") or "") for name in REQUIRED_CHECKS)
        candidates.append((latest_check_time, sha, checks))

    candidates.sort(key=lambda item: item[0], reverse=True)
    for _, candidate_sha, checks in candidates:
        if candidate_sha == branch_head:
            return candidate_sha, branch_head, checks
        compare_url = ENGINE_API + "/compare/" + candidate_sha + "..." + branch_head
        try:
            comparison = github_json(compare_url)
        except RuntimeError:
            continue
        if (
            comparison.get("status") in ("ahead", "identical")
            and int(comparison.get("behind_by") or 0) == 0
        ):
            return candidate_sha, branch_head, checks

    raise RuntimeError(
        "No reachable main-3 commit was found with successful Engine runtime CI, "
        "cross-browser compatibility, CodeQL, and code-quality checks. Wait for Engine "
        "CI to finish green, then retry the GitManager build."
    )


def safe_extract_source(archive_path: Path, destination: Path) -> None:
    """Extract only regular files/directories from the GitHub source archive."""
    destination.mkdir(parents=True, exist_ok=True)
    total_unpacked = 0
    try:
        archive = tarfile.open(archive_path, mode="r:gz")
    except (tarfile.TarError, OSError) as error:
        raise RuntimeError("The downloaded Engine source archive is invalid.") from error

    with archive:
        members = archive.getmembers()
        if len(members) > MAX_ARCHIVE_MEMBERS:
            raise RuntimeError("The Engine archive contains too many entries.")
        top_level = None
        for member in members:
            archive_path_name = PurePosixPath(member.name)
            if archive_path_name.is_absolute() or any(part in ("", ".", "..") for part in archive_path_name.parts):
                raise RuntimeError("The Engine archive contains an unsafe path.")
            if top_level is None:
                top_level = archive_path_name.parts[0]
            if archive_path_name.parts[0] != top_level:
                raise RuntimeError("The Engine archive contains multiple root directories.")
            relative = archive_path_name.parts[1:]
            if not relative:
                continue
            target = destination.joinpath(*relative)
            try:
                target.resolve().relative_to(destination.resolve())
            except (OSError, ValueError):
                raise RuntimeError("The Engine archive contains a path outside its extraction directory.")
            if member.isdir():
                target.mkdir(parents=True, exist_ok=True)
                continue
            if not member.isfile():
                raise RuntimeError("The Engine archive contains a link or unsupported special file.")
            if member.size < 0 or member.size > MAX_MEMBER_BYTES:
                raise RuntimeError("The Engine archive contains an oversized file.")
            total_unpacked += member.size
            if total_unpacked > MAX_ARCHIVE_BYTES * 4:
                raise RuntimeError("The extracted Engine source exceeds the configured size limit.")
            target.parent.mkdir(parents=True, exist_ok=True)
            source = archive.extractfile(member)
            if source is None:
                raise RuntimeError("Could not read a file from the Engine source archive.")
            with source, target.open("wb") as output:
                shutil.copyfileobj(source, output, length=1024 * 1024)


def verify_engine_source(source_root: Path) -> None:
    required = (
        source_root / "package.json",
        source_root / "src" / "engine" / "index.ts",
        source_root / "src" / "engine" / "plugins" / "enginePlugin.js",
    )
    missing = [str(path.relative_to(source_root)) for path in required if not path.is_file()]
    if missing:
        raise RuntimeError("The resolved Engine source is incomplete: " + ", ".join(missing))
    try:
        manifest = json.loads((source_root / "package.json").read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise RuntimeError("The resolved Engine package manifest could not be read.") from error
    if manifest.get("name") != "nextjs-engine":
        raise RuntimeError("The Engine source repository package identity did not match.")


def ensure_engine_source(cache_root: Path) -> Tuple[Path, str, str, Dict[str, Dict[str, object]]]:
    commit, branch_head, checks = resolve_latest_green_commit()
    source_root = cache_root / "source"
    marker = source_root / ".gitmanager-managed-source"
    metadata_path = source_root / ".gitmanager-source.json"

    if marker.is_file() and metadata_path.is_file():
        try:
            current = json.loads(metadata_path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError):
            current = {}
        if current.get("commit") == commit:
            verify_engine_source(source_root)
            log_ok("Using cached CI-green Next.js Engine source " + commit)
            return source_root, commit, branch_head, checks

    if source_root.exists():
        if not marker.is_file():
            raise RuntimeError(
                "The Engine cache directory exists without GitManager's ownership marker; "
                "move it aside before building rather than allowing the build to replace it."
            )
        shutil.rmtree(source_root)

    cache_root.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="engine-source-", dir=str(cache_root)) as temporary:
        staging = Path(temporary) / "source"
        staging.mkdir()
        archive_path = Path(temporary) / "engine-source.tar.gz"
        archive_url = "https://codeload.github.com/Kastrick8/nextjs-engine/tar.gz/" + commit
        log_info("Downloading CI-green Engine source " + commit)
        archive_path.write_bytes(request_bytes(archive_url, MAX_ARCHIVE_BYTES, "application/vnd.github+json"))
        safe_extract_source(archive_path, staging)
        verify_engine_source(staging)
        (staging / ".gitmanager-managed-source").write_text(
            "Generated by GitManager; safe to refresh through tools/nextjs_engine_source.py.\n",
            encoding="utf-8",
        )
        source_metadata = {
            "repository": "https://github.com/Kastrick8/nextjs-engine",
            "branch": ENGINE_BRANCH,
            "branch_head": branch_head,
            "commit": commit,
            "required_checks": {
                name: {
                    "run_id": checks[name].get("id"),
                    "conclusion": checks[name].get("conclusion"),
                    "updated_at": checks[name].get("updated_at"),
                    "url": checks[name].get("html_url"),
                }
                for name in REQUIRED_CHECKS
            },
        }
        (staging / ".gitmanager-source.json").write_text(
            json.dumps(source_metadata, indent=2) + "\n",
            encoding="utf-8",
        )
        os.replace(str(staging), str(source_root))

    log_ok("Resolved latest reachable CI-green Engine commit " + commit)
    return source_root, commit, branch_head, checks


def node_version() -> Tuple[int, int, int]:
    node = shutil.which("node")
    npm = shutil.which("npm")
    if not node or not npm:
        raise RuntimeError(
            "The Engine frontend build requires Node.js 20.9+ and npm. Install Node.js 22 or newer, then retry."
        )
    try:
        result = subprocess.run([node, "--version"], check=False, capture_output=True, text=True, timeout=15)
    except (OSError, subprocess.TimeoutExpired) as error:
        raise RuntimeError("Could not execute Node.js: " + str(error)) from error
    match = re.search(r"v?(\d+)\.(\d+)\.(\d+)", result.stdout)
    if result.returncode != 0 or not match:
        raise RuntimeError("Node.js returned an unrecognized version string.")
    version = tuple(int(match.group(index)) for index in range(1, 4))
    if version < (20, 9, 0):
        raise RuntimeError("Next.js 16 requires Node.js 20.9 or newer; detected " + result.stdout.strip())
    return version


def run_command(arguments: List[str], cwd: Path, environment: Dict[str, str]) -> int:
    try:
        if os.name == "nt":
            return subprocess.call(subprocess.list2cmdline(arguments), cwd=str(cwd), env=environment, shell=True)
        return subprocess.call(arguments, cwd=str(cwd), env=environment)
    except OSError as error:
        log_error("Could not start " + arguments[0] + ": " + str(error))
        return 1


def prepare_workspace(project_root: Path, source_root: Path) -> Path:
    """Create an isolated, complete Engine checkout and overlay GitManager's app routes.

    The downloaded source tree remains unchanged: its root app/, examples, tests,
    scripts, docs, package manifest and development tooling stay available in
    cache_root/source. Only the generated workspace receives GitManager's app/.
    """
    cache_root = source_root.parent
    workspace = cache_root / "workspace"
    marker = workspace / ".gitmanager-generated-workspace"

    frontend_source = project_root / "frontend"
    app_source = frontend_source / "app"
    required_frontend_files = (
        app_source / "page.tsx",
        frontend_source / "components" / "gitmanager-app.tsx",
        frontend_source / "lib" / "use-gitmanager.ts",
        frontend_source / "next.config.js",
    )
    missing = [str(path.relative_to(project_root)) for path in required_frontend_files if not path.is_file()]
    if missing:
        raise RuntimeError("GitManager's React frontend is incomplete: " + ", ".join(missing))

    source_metadata_path = source_root / ".gitmanager-source.json"
    try:
        source_metadata = json.loads(source_metadata_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise RuntimeError("The downloaded Engine source has no valid provenance marker.") from error
    engine_commit = str(source_metadata.get("commit") or "")
    if not re.fullmatch(r"[0-9a-f]{40}", engine_commit):
        raise RuntimeError("The downloaded Engine source commit is missing or invalid.")

    # Hash tracked frontend inputs so unchanged builds reuse the full workspace
    # and its node_modules rather than copying/installing the Engine each time.
    frontend_digest = hashlib.sha256()
    for path in sorted(item for item in frontend_source.rglob("*") if item.is_file()):
        relative = path.relative_to(frontend_source).as_posix()
        frontend_digest.update(relative.encode("utf-8"))
        frontend_digest.update(b"\0")
        frontend_digest.update(hashlib.sha256(path.read_bytes()).digest())
    frontend_sha256 = frontend_digest.hexdigest()

    required_engine_paths = (
        workspace / "package.json",
        workspace / "src" / "engine" / "index.ts",
        workspace / "src" / "engine" / "plugins" / "enginePlugin.js",
        workspace / "scripts",
        workspace / "tests",
        workspace / "examples",
        workspace / "docs",
        workspace / "app" / "page.tsx",
    )
    if workspace.exists():
        if not marker.is_file():
            raise RuntimeError(
                "The generated Engine workspace exists without GitManager's ownership marker; "
                "refusing to overwrite it."
            )
        try:
            previous = json.loads(marker.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError):
            previous = {}
        if (
            previous.get("engine_commit") == engine_commit
            and previous.get("frontend_sha256") == frontend_sha256
            and all(path.exists() for path in required_engine_paths)
        ):
            log_ok("Reusing the full Engine workspace and its build dependencies.")
            return workspace
        shutil.rmtree(workspace)

    ignored = shutil.ignore_patterns(
        ".git",
        "node_modules",
        ".next",
        "dist",
        "out",
        "target",
        "coverage",
        "playwright-report",
        "test-results",
        ".turbo",
    )
    shutil.copytree(source_root, workspace, ignore=ignored)

    # Replace only the disposable workspace app router. The downloaded Engine
    # source/app remains untouched and retains all demo and integration routes.
    workspace_app = workspace / "app"
    if workspace_app.exists():
        shutil.rmtree(workspace_app)
    shutil.copytree(app_source, workspace_app)
    (workspace_app / "__turbopack_stub__.ts").write_text(
        "export default {};\n",
        encoding="utf-8",
    )

    for folder in ("components", "lib"):
        shutil.copytree(
            frontend_source / folder,
            workspace / folder,
            dirs_exist_ok=True,
        )

    public_source = frontend_source / "public"
    if public_source.is_dir():
        shutil.copytree(public_source, workspace / "public", dirs_exist_ok=True)

    # Keep the Engine root package.json, tsconfig, scripts, examples, docs, tests,
    # compiler and source tree; only use GitManager's Tauri-compatible app config.
    shutil.copy2(frontend_source / "next.config.js", workspace / "next.config.js")
    marker.write_text(
        json.dumps({
            "generated_by": "GitManager",
            "engine_repository": "Kastrick8/nextjs-engine",
            "engine_commit": engine_commit,
            "frontend_sha256": frontend_sha256,
            "workspace_mode": "full-engine-repository-with-gitmanager-app-overlay",
            "engine_source_app_preserved": True,
        }, indent=2) + "\n",
        encoding="utf-8",
    )
    return workspace


def install_engine_build_dependencies(workspace: Path) -> int:
    manifest = workspace / "package.json"
    node_modules = workspace / "node_modules"
    next_binary = node_modules / ".bin" / ("next.cmd" if os.name == "nt" else "next")
    stamp = workspace / ".gitmanager-npm-install.json"
    package_digest = hashlib.sha256(manifest.read_bytes()).hexdigest()
    if next_binary.is_file() and stamp.is_file():
        try:
            previous = json.loads(stamp.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError):
            previous = {}
        if previous.get("package_json_sha256") == package_digest:
            log_ok("Engine Node.js build dependencies are already prepared.")
            return 0

    npm = shutil.which("npm")
    if not npm:
        log_error("npm was not found in PATH.")
        return 1
    log_info("Installing build dependencies in the isolated full-repository Engine workspace.")
    environment = dict(os.environ)
    environment["NEXT_TELEMETRY_DISABLED"] = "1"
    result = run_command([npm, "install", "--ignore-scripts", "--no-audit", "--no-fund"], workspace, environment)
    if result != 0:
        log_error("Could not install the build-time dependencies required by the fetched Engine source.")
        return result
    if not next_binary.is_file():
        log_error("The Engine dependency install completed without a Next.js executable.")
        return 1
    stamp.write_text(json.dumps({"package_json_sha256": package_digest}, indent=2) + "\n", encoding="utf-8")
    return 0


def build_frontend(project_root: Path, mode: str) -> int:
    try:
        version = node_version()
        cache_root = project_root / ".cache" / "nextjs-engine"
        source_root, commit, branch_head, checks = ensure_engine_source(cache_root)
        workspace = prepare_workspace(project_root, source_root)
        log_ok("Node.js " + ".".join(str(part) for part in version) + " is available.")
        install_result = install_engine_build_dependencies(workspace)
        if install_result != 0:
            return install_result

        environment = dict(os.environ)
        environment["NEXT_TELEMETRY_DISABLED"] = "1"
        environment["NEXTJS_ENGINE_SOURCE_COMMIT"] = commit
        npm = shutil.which("npm")
        if not npm:
            raise RuntimeError("npm was not found after dependency installation.")

        node = shutil.which("node")
        next_cli = workspace / "node_modules" / "next" / "dist" / "bin" / "next"
        if not node or not next_cli.is_file():
            raise RuntimeError("The fetched Engine source did not install a runnable Next.js CLI.")

        if mode == "dev":
            command = [
                node, str(next_cli), "dev",
                "--hostname", "127.0.0.1", "--port", "3000",
            ]
            log_info("Starting the Engine-backed Next.js development server at http://localhost:3000")
            return run_command(command, workspace, environment)

        output_dir = workspace / "out"
        command = [node, str(next_cli), "build"]
        log_info("Building the GitManager frontend as a Next.js static export.")
        result = run_command(command, workspace, environment)
        if result != 0:
            return result
        if not (output_dir / "index.html").is_file() or not (output_dir / "_next" / "static").is_dir():
            raise RuntimeError(
                "Next.js completed but did not produce out/index.html and out/_next/static."
            )

        build_metadata = {
            "repository": "https://github.com/Kastrick8/nextjs-engine",
            "branch": ENGINE_BRANCH,
            "branch_head": branch_head,
            "commit": commit,
            "workspace_mode": "full-engine-repository-with-gitmanager-app-overlay",
            "source_app_preserved": True,
            "resolved_at": datetime.now(timezone.utc).isoformat(),
            "required_checks": {
                name: {
                    "run_id": checks[name].get("id"),
                    "conclusion": checks[name].get("conclusion"),
                    "updated_at": checks[name].get("updated_at"),
                    "url": checks[name].get("html_url"),
                }
                for name in REQUIRED_CHECKS
            },
        }
        (output_dir / "engine-source.json").write_text(
            json.dumps(build_metadata, indent=2) + "\n",
            encoding="utf-8",
        )
        log_ok("Static frontend built with CI-green Engine source " + commit)
        return 0
    except (OSError, RuntimeError, ValueError) as error:
        log_error(str(error))
        return 1


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Fetch the newest CI-green Next.js Engine source and build GitManager's frontend."
    )
    parser.add_argument("mode", choices=("build", "dev"))
    parser.add_argument(
        "--project-root",
        type=Path,
        default=Path(__file__).resolve().parents[1],
        help="GitManager repository root (defaults to the parent of tools/).",
    )
    args = parser.parse_args()
    project_root = args.project_root.resolve()
    if not (project_root / "src-tauri").is_dir() or not (project_root / "ui").is_dir():
        log_error("The supplied project root is not a GitManager repository.")
        return 2
    return build_frontend(project_root, args.mode)


if __name__ == "__main__":
    sys.exit(main())
