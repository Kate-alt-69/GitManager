#!/usr/bin/env python3
"""Resolve and build GitManager against the newest CI-green Next.js Engine source.

The Engine is fetched from GitHub source archives, never from the published npm
package. A SHA is resolved at build time and recorded as provenance; it is not
pinned in GitManager source.
"""

import argparse
import contextlib
import http.client
import hashlib
import json
import os
import platform
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
from typing import Dict, Iterator, List, Tuple

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
API_REQUEST_ATTEMPTS = 2
API_REQUEST_TIMEOUT_SECONDS = 15
RETRYABLE_HTTP_STATUS = frozenset((408, 429, 500, 502, 503, 504))
DEFAULT_ENGINE_CACHE_TTL_SECONDS = 15 * 60
FRONTEND_BUILD_CACHE_FILE = ".gitmanager-next-build-cache.json"


def log_ok(message: str) -> None:
    print("[ OK ] " + message, flush=True)


def log_info(message: str) -> None:
    print("[INFO] " + message, flush=True)


def log_error(message: str) -> None:
    print("[ERR ] " + message, file=sys.stderr, flush=True)


@contextlib.contextmanager
def timed_stage(label: str) -> Iterator[None]:
    started = time.monotonic()
    try:
        yield
    finally:
        log_info(label + " took " + format(time.monotonic() - started, ".1f") + "s.")


def request_bytes(
    url: str,
    limit: int,
    accept: str,
    *,
    attempts: int = REQUEST_ATTEMPTS,
    timeout_seconds: int = 60,
) -> bytes:
    parsed_url = urllib.parse.urlsplit(url)
    if parsed_url.scheme != "https" or parsed_url.hostname not in {"api.github.com", "codeload.github.com"}:
        raise RuntimeError("Refusing to request Engine source data from an untrusted host.")

    base_headers = {
        "User-Agent": USER_AGENT,
        "Accept": accept,
        "X-GitHub-Api-Version": "2022-11-28",
    }
    # GitHub Actions supplies a short-lived, read-scoped token to avoid the
    # strict unauthenticated API rate limit on multi-platform installer builds.
    # Local builds remain usable without a token and never prompt for credentials.
    token = os.environ.get("GITHUB_TOKEN") or os.environ.get("GH_TOKEN")
    if parsed_url.hostname == "api.github.com" and token and not any(char in token for char in "\r\n"):
        base_headers["Authorization"] = "Bearer " + token

    resumable = parsed_url.hostname == "codeload.github.com"
    chunks: List[bytes] = []
    total = 0
    resume_validator = None
    last_error = None

    for attempt in range(attempts):
        # Small API responses are deliberately restarted from byte zero. Large
        # codeload archives retain partial bytes and try an HTTP Range request.
        if not resumable:
            chunks = []
            total = 0

        request_headers = dict(base_headers)
        if resumable and total:
            request_headers["Range"] = "bytes=" + str(total) + "-"
            if resume_validator:
                request_headers["If-Range"] = resume_validator
            log_info("Resuming the Engine source archive at byte " + str(total) + ".")

        request = urllib.request.Request(url, headers=request_headers)
        response_offset = total
        range_expected_bytes = None
        try:
            with urllib.request.urlopen(request, timeout=timeout_seconds) as response:
                parsed = urllib.parse.urlsplit(response.geturl())
                if parsed.scheme != "https" or parsed.hostname not in {"api.github.com", "codeload.github.com"}:
                    raise RuntimeError("Refusing an Engine download redirect to an untrusted host.")

                status = getattr(response, "status", None) or getattr(response, "code", None) or 200
                if resumable and response_offset:
                    if status == 206:
                        content_range = response.headers.get("Content-Range", "")
                        match = re.fullmatch(r"bytes (\d+)-(\d+)/(?:\d+|\*)", content_range.strip())
                        if not match or int(match.group(1)) != response_offset:
                            raise RuntimeError(
                                "The Engine archive server returned an invalid Content-Range while resuming; "
                                "refusing to combine unrelated archive bytes."
                            )
                        range_start, range_end = int(match.group(1)), int(match.group(2))
                        range_total = match.group(3)
                        if (
                            range_end < range_start
                            or (
                                range_total != "*"
                                and range_end != int(range_total) - 1
                            )
                        ):
                            raise RuntimeError(
                                "The Engine archive server returned inconsistent or incomplete Content-Range bounds."
                            )
                        range_expected_bytes = range_end - range_start + 1
                    elif status == 200:
                        # A server may ignore Range or the If-Range validator may
                        # no longer match. Its 200 body is a complete replacement.
                        log_info("The Engine archive server restarted the transfer; downloading from byte zero.")
                        chunks = []
                        total = 0
                        response_offset = 0
                        resume_validator = None
                    else:
                        raise RuntimeError(
                            "The Engine archive server returned HTTP "
                            + str(status)
                            + " for a byte-range request."
                        )
                elif resumable and status == 206:
                    content_range = response.headers.get("Content-Range", "")
                    match = re.fullmatch(r"bytes (\d+)-(\d+)/(?:\d+|\*)", content_range.strip())
                    if not match or int(match.group(1)) != 0:
                        raise RuntimeError("The Engine archive server returned an unexpected partial response.")
                    range_start, range_end = int(match.group(1)), int(match.group(2))
                    range_total = match.group(3)
                    if (
                        range_end < range_start
                        or (
                            range_total != "*"
                            and range_end != int(range_total) - 1
                        )
                    ):
                        raise RuntimeError(
                            "The Engine archive server returned inconsistent or incomplete Content-Range bounds."
                        )
                    range_expected_bytes = range_end - range_start + 1

                declared = response.headers.get("Content-Length")
                expected_response_bytes = None
                if declared:
                    try:
                        expected_response_bytes = int(declared)
                        if expected_response_bytes < 0 or expected_response_bytes + response_offset > limit:
                            raise RuntimeError("The Engine response exceeds the configured download limit.")
                        if range_expected_bytes is not None and expected_response_bytes != range_expected_bytes:
                            raise RuntimeError("The Engine archive Content-Length disagrees with Content-Range.")
                    except ValueError:
                        raise RuntimeError("The Engine response returned an invalid content length.")

                if resumable:
                    etag = response.headers.get("ETag")
                    last_modified = response.headers.get("Last-Modified")
                    if etag and not etag.strip().startswith("W/"):
                        resume_validator = etag
                    elif last_modified:
                        resume_validator = last_modified

                response_bytes = 0
                while True:
                    chunk = response.read(1024 * 1024)
                    if not chunk:
                        break
                    response_bytes += len(chunk)
                    total += len(chunk)
                    if total > limit:
                        raise RuntimeError("The Engine response exceeds the configured download limit.")
                    chunks.append(chunk)

                expected_bytes = expected_response_bytes if expected_response_bytes is not None else range_expected_bytes
                if expected_bytes is not None and response_bytes != expected_bytes:
                    raise http.client.IncompleteRead(
                        b"",
                        max(0, expected_bytes - response_bytes),
                    )
                return b"".join(chunks)
        except urllib.error.HTTPError as error:
            if error.code == 416 and resumable and total:
                log_info("The Engine archive server rejected the resume offset; restarting the archive download.")
                chunks = []
                total = 0
                resume_validator = None
                last_error = error
            elif error.code not in RETRYABLE_HTTP_STATUS:
                raise RuntimeError("Could not access the Next.js Engine source: " + str(error)) from error
            last_error = error
            if error.fp is not None:
                error.close()
        except http.client.IncompleteRead as error:
            if resumable and isinstance(error.partial, (bytes, bytearray)) and error.partial:
                partial = bytes(error.partial)
                if total + len(partial) > limit:
                    raise RuntimeError("The Engine response exceeds the configured download limit.") from error
                chunks.append(partial)
                total += len(partial)
            last_error = error
        except (http.client.HTTPException, OSError) as error:
            last_error = error

        if attempt + 1 < attempts:
            delay = min(2 ** attempt, 4)
            log_info(
                "GitHub source request was interrupted; retrying in "
                + str(delay)
                + "s (attempt "
                + str(attempt + 2)
                + "/"
                + str(attempts)
                + ")."
            )
            time.sleep(delay)

    raise RuntimeError(
        "Could not access the Next.js Engine source after "
        + str(attempts)
        + " attempts: "
        + str(last_error)
    ) from last_error


def github_json(url: str) -> Dict[str, object]:
    payload = request_bytes(
        url,
        MAX_API_BYTES,
        "application/vnd.github+json",
        attempts=API_REQUEST_ATTEMPTS,
        timeout_seconds=API_REQUEST_TIMEOUT_SECONDS,
    )
    try:
        decoded = json.loads(payload.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise RuntimeError("GitHub returned invalid JSON while resolving Engine source.") from error
    if not isinstance(decoded, dict):
        raise RuntimeError("GitHub returned an unexpected response while resolving Engine source.")
    return decoded


def resolve_engine_branch_head() -> str:
    """Read only the current Engine branch SHA; used for the unchanged-cache fast path."""
    branch_url = ENGINE_API + "/branches/" + urllib.parse.quote(ENGINE_BRANCH, safe="")
    branch = github_json(branch_url)
    branch_head = str((branch.get("commit") or {}).get("sha") or "")
    if not re.fullmatch(r"[0-9a-f]{40}", branch_head):
        raise RuntimeError("Could not resolve the current Engine main-3 branch head.")
    return branch_head


def resolve_latest_green_commit(
    branch_head: str | None = None,
) -> Tuple[str, str, Dict[str, Dict[str, object]]]:
    """Pick the newest main-3 commit with all required CI runs green and reachable."""
    if branch_head is None:
        branch_head = resolve_engine_branch_head()
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
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise RuntimeError("The resolved Engine package manifest could not be read.") from error
    if not isinstance(manifest, dict) or manifest.get("name") != "nextjs-engine":
        raise RuntimeError("The Engine source repository package identity did not match.")


def _atomic_write_json(path: Path, payload: Dict[str, object]) -> None:
    """Write JSON atomically so an interrupted build never leaves partial metadata."""
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary_path = None
    try:
        with tempfile.NamedTemporaryFile(
            mode="w",
            encoding="utf-8",
            dir=str(path.parent),
            prefix=path.name + ".",
            suffix=".tmp",
            delete=False,
        ) as temporary:
            temporary_path = Path(temporary.name)
            temporary.write(json.dumps(payload, indent=2) + "\n")
        os.replace(str(temporary_path), str(path))
    finally:
        if temporary_path is not None:
            temporary_path.unlink(missing_ok=True)


def _source_checks_from_metadata(metadata: Dict[str, object]) -> Dict[str, Dict[str, object]]:
    raw_checks = metadata.get("required_checks")
    if not isinstance(raw_checks, dict):
        raise RuntimeError("The cached Engine source has no required CI-check provenance.")

    checks: Dict[str, Dict[str, object]] = {}
    for name in REQUIRED_CHECKS:
        item = raw_checks.get(name)
        if not isinstance(item, dict):
            raise RuntimeError("The cached Engine source is missing CI evidence for " + name + ".")
        if item.get("conclusion") != "success" or not item.get("run_id") or not item.get("updated_at"):
            raise RuntimeError("The cached Engine source has incomplete or unsuccessful CI evidence for " + name + ".")
        checks[name] = {
            "id": item.get("run_id"),
            "conclusion": item.get("conclusion"),
            "updated_at": item.get("updated_at"),
            "html_url": item.get("url"),
        }
    return checks


def _load_verified_source_cache(source_root: Path) -> Tuple[Dict[str, object], Dict[str, Dict[str, object]]]:
    marker = source_root / ".gitmanager-managed-source"
    metadata_path = source_root / ".gitmanager-source.json"
    if not marker.is_file() or not metadata_path.is_file():
        raise RuntimeError("The Engine source cache is missing its GitManager ownership or provenance marker.")

    try:
        metadata = json.loads(metadata_path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise RuntimeError("The cached Engine source provenance file is invalid.") from error
    if not isinstance(metadata, dict):
        raise RuntimeError("The cached Engine source provenance must be a JSON object.")
    if metadata.get("repository") != "https://github.com/" + ENGINE_REPOSITORY:
        raise RuntimeError("The cached Engine source belongs to a different repository.")
    if metadata.get("branch") != ENGINE_BRANCH:
        raise RuntimeError("The cached Engine source belongs to a different Engine branch.")

    commit = str(metadata.get("commit") or "")
    branch_head = str(metadata.get("branch_head") or "")
    if not re.fullmatch(r"[0-9a-f]{40}", commit) or not re.fullmatch(r"[0-9a-f]{40}", branch_head):
        raise RuntimeError("The cached Engine source contains an invalid commit or branch-head SHA.")

    checks = _source_checks_from_metadata(metadata)
    verify_engine_source(source_root)
    return metadata, checks


def _engine_cache_ttl_seconds() -> int:
    value = os.environ.get("GITMANAGER_ENGINE_CACHE_TTL_SECONDS")
    if value is None:
        return DEFAULT_ENGINE_CACHE_TTL_SECONDS
    try:
        return max(0, int(value))
    except ValueError:
        log_info("Ignoring invalid GITMANAGER_ENGINE_CACHE_TTL_SECONDS; using the six-hour default.")
        return DEFAULT_ENGINE_CACHE_TTL_SECONDS


def _source_cache_age_seconds(metadata: Dict[str, object]) -> float:
    checked_at = metadata.get("resolved_at")
    if not checked_at:
        # Older GitManager cache manifests recorded the successful CI runs but
        # not when they were checked. Treat that already-verified snapshot as
        # the starting point for this cache policy instead of forcing the
        # first upgraded build through the slow GitHub Actions pagination path.
        return 0.0
    try:
        timestamp = datetime.fromisoformat(str(checked_at).replace("Z", "+00:00")).timestamp()
    except (TypeError, ValueError, OverflowError):
        return float("inf")
    return max(0.0, time.time() - timestamp)


def _source_metadata_payload(
    branch_head: str,
    commit: str,
    checks: Dict[str, Dict[str, object]],
) -> Dict[str, object]:
    return {
        "repository": "https://github.com/" + ENGINE_REPOSITORY,
        "branch": ENGINE_BRANCH,
        "branch_head": branch_head,
        "commit": commit,
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


def ensure_engine_source(cache_root: Path) -> Tuple[Path, str, str, Dict[str, Dict[str, object]]]:
    source_root = cache_root / "source"
    marker = source_root / ".gitmanager-managed-source"
    if source_root.exists() and not marker.is_file():
        raise RuntimeError(
            "The Engine cache directory exists without GitManager's ownership marker; "
            "move it aside before building rather than allowing the build to replace it."
        )

    cached_metadata = None
    cached_checks = None
    if source_root.is_dir():
        try:
            cached_metadata, cached_checks = _load_verified_source_cache(source_root)
        except RuntimeError as error:
            log_info("The existing Engine source cache cannot be trusted for reuse: " + str(error))

    refresh_requested = os.environ.get("GITMANAGER_ENGINE_REFRESH", "").strip().lower() in {
        "1", "true", "yes", "on",
    }
    if cached_metadata is not None and cached_checks is not None and not refresh_requested:
        if "resolved_at" not in cached_metadata:
            cached_metadata["resolved_at"] = datetime.now(timezone.utc).isoformat()
            _atomic_write_json(source_root / ".gitmanager-source.json", cached_metadata)
        if _source_cache_age_seconds(cached_metadata) < _engine_cache_ttl_seconds():
            commit = str(cached_metadata["commit"])
            branch_head = str(cached_metadata["branch_head"])
            log_ok(
                "Using verified cached CI-green Next.js Engine source "
                + commit
                + " (GitHub refresh is checked every "
                + str(_engine_cache_ttl_seconds())
                + "s; set GITMANAGER_ENGINE_REFRESH=1 to force it)."
            )
            return source_root, commit, branch_head, cached_checks

    branch_head_hint = None
    if cached_metadata is not None and cached_checks is not None and not refresh_requested:
        try:
            branch_head_hint = resolve_engine_branch_head()
        except (RuntimeError, OSError, urllib.error.URLError) as error:
            commit = str(cached_metadata["commit"])
            branch_head = str(cached_metadata["branch_head"])
            log_info(
                "Could not check the Engine branch head; using the previously verified "
                + commit
                + ". Cause: "
                + str(error)
            )
            return source_root, commit, branch_head, cached_checks

        if (
            branch_head_hint == cached_metadata.get("branch_head")
            and cached_metadata.get("commit") == branch_head_hint
        ):
            commit = str(cached_metadata["commit"])
            refreshed = _source_metadata_payload(branch_head_hint, commit, cached_checks)
            _atomic_write_json(source_root / ".gitmanager-source.json", refreshed)
            log_ok("Engine branch is unchanged and already CI-green; reusing it without scanning Actions history.")
            return source_root, commit, branch_head_hint, cached_checks

    try:
        commit, branch_head, checks = resolve_latest_green_commit(branch_head_hint)
    except (RuntimeError, OSError, urllib.error.URLError) as error:
        if cached_metadata is None or cached_checks is None or refresh_requested:
            raise
        commit = str(cached_metadata["commit"])
        branch_head = str(cached_metadata["branch_head"])
        log_info(
            "GitHub Engine refresh failed; continuing with the previously verified "
            + commit
            + " because its recorded required CI checks are still successful. Cause: "
            + str(error)
        )
        return source_root, commit, branch_head, cached_checks

    if cached_metadata is not None and cached_checks is not None and cached_metadata.get("commit") == commit:
        refreshed = _source_metadata_payload(branch_head, commit, checks)
        _atomic_write_json(source_root / ".gitmanager-source.json", refreshed)
        log_ok("The verified Engine source is already cached; refreshed its CI provenance without downloading it.")
        return source_root, commit, branch_head, checks

    try:
        cache_root.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(prefix="engine-source-", dir=str(cache_root)) as temporary:
            staging = Path(temporary) / "source"
            staging.mkdir()
            archive_path = Path(temporary) / "engine-source.tar.gz"
            archive_url = "https://codeload.github.com/" + ENGINE_REPOSITORY + "/tar.gz/" + commit
            log_info("Downloading CI-green Engine source " + commit)
            archive_path.write_bytes(request_bytes(archive_url, MAX_ARCHIVE_BYTES, "application/vnd.github+json"))
            safe_extract_source(archive_path, staging)
            verify_engine_source(staging)
            (staging / ".gitmanager-managed-source").write_text(
                "Generated by GitManager; safe to refresh through tools/nextjs_engine_source.py.\n",
                encoding="utf-8",
            )
            _atomic_write_json(staging / ".gitmanager-source.json", _source_metadata_payload(branch_head, commit, checks))
    
            # Keep the previous verified source available until the replacement is
            # fully downloaded, extracted, identity-checked and provenance-stamped.
            backup = None
            if source_root.exists():
                backup = cache_root / (".source-backup-" + str(os.getpid()) + "-" + str(time.time_ns()))
                os.replace(str(source_root), str(backup))
            try:
                os.replace(str(staging), str(source_root))
            except OSError:
                if backup is not None and backup.exists() and not source_root.exists():
                    os.replace(str(backup), str(source_root))
                raise
            else:
                if backup is not None:
                    shutil.rmtree(backup, ignore_errors=True)
    
    except (OSError, RuntimeError, tarfile.TarError, ValueError) as error:
        if cached_metadata is not None and cached_checks is not None and source_root.is_dir() and not refresh_requested:
            try:
                fallback_metadata, fallback_checks = _load_verified_source_cache(source_root)
            except RuntimeError:
                raise
            fallback_commit = str(fallback_metadata["commit"])
            fallback_head = str(fallback_metadata["branch_head"])
            log_info(
                "Could not refresh the Engine archive; continuing with the previously verified "
                + fallback_commit
                + ". Cause: "
                + str(error)
            )
            return source_root, fallback_commit, fallback_head, fallback_checks
        raise
    log_ok("Resolved latest reachable CI-green Engine commit " + commit)
    return source_root, commit, branch_head, checks

def node_version() -> Tuple[int, int, int]:
    node = shutil.which("node")
    if not node:
        raise RuntimeError(
            "The Engine frontend build requires Node.js 20.9+. Install Node.js 22 or newer, then retry."
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
        app_source / "error.tsx",
        app_source / "global-error.tsx",
        frontend_source / "components" / "gitmanager-app.tsx",
        frontend_source / "lib" / "use-gitmanager.ts",
        frontend_source / "instrumentation.ts",
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
        workspace / "app" / "error.tsx",
        workspace / "app" / "global-error.tsx",
        workspace / "instrumentation.ts",
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
    # The Engine plugin generates Node-only error instrumentation by default.
    # GitManager is a static Tauri export, so overlay its browser-safe boundaries
    # and empty server hook into the disposable workspace (never into Engine source).
    shutil.copy2(frontend_source / "instrumentation.ts", workspace / "instrumentation.ts")
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

    # GitManager's artwork is maintained in frontend/static/image. Publish the
    # original PNG assets unchanged through Next's static-export public directory.
    image_source = frontend_source / "static" / "image"
    if image_source.is_dir():
        shutil.copytree(image_source, workspace / "public" / "image", dirs_exist_ok=True)

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
            "gitmanager_runtime_error_handling_overlay": True,
        }, indent=2) + "\n",
        encoding="utf-8",
    )
    return workspace


def validate_next_compiled_metadata(workspace: Path) -> str | None:
    """Catch malformed bundled package manifests before Node loads the Next.js CLI."""
    package_path = workspace / "node_modules" / "next" / "dist" / "compiled" / "p-queue" / "package.json"
    if not package_path.is_file():
        return "Next.js is missing its bundled p-queue package manifest: " + str(package_path)
    try:
        package = json.loads(package_path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        return "Next.js bundled p-queue package metadata is invalid (" + str(error) + "): " + str(package_path)
    if not isinstance(package, dict):
        return "Next.js bundled p-queue package metadata must contain a JSON object: " + str(package_path)
    return None


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
            metadata_error = validate_next_compiled_metadata(workspace)
            if metadata_error is None:
                log_ok("Engine Node.js build dependencies are already prepared.")
                return 0
            log_info(metadata_error)
            log_info("Discarding the invalid generated dependency tree before reinstalling it.")
            shutil.rmtree(node_modules, ignore_errors=True)
            stamp.unlink(missing_ok=True)

    npm = shutil.which("npm")
    if not npm:
        log_error("npm was not found in PATH.")
        return 1
    log_info("Installing build dependencies in the isolated full-repository Engine workspace.")
    environment = dict(os.environ)
    environment["NEXT_TELEMETRY_DISABLED"] = "1"
    install_args = [npm, "install", "--ignore-scripts", "--no-audit", "--no-fund"]
    result = run_command(install_args, workspace, environment)
    if result != 0:
        log_error("Could not install the build-time dependencies required by the fetched Engine source.")
        return result

    metadata_error = validate_next_compiled_metadata(workspace)
    if metadata_error is not None:
        # npm can report success while a package's bundled metadata is unusable
        # by Node at runtime. Rebuild only this GitManager-owned workspace tree,
        # preferring the registry over stale cached package content, and retry once.
        log_info(metadata_error)
        log_info("Rebuilding the isolated Node.js dependency tree once with registry-preferred package retrieval.")
        shutil.rmtree(node_modules, ignore_errors=True)
        result = run_command(install_args + ["--prefer-online"], workspace, environment)
        if result != 0:
            log_error("The clean retry could not install the Engine build-time dependencies.")
            return result
        metadata_error = validate_next_compiled_metadata(workspace)
        if metadata_error is not None:
            log_error(metadata_error)
            log_error("The Next.js package metadata remains invalid after a clean reinstall; refusing to start the frontend build.")
            return 1

    if not next_binary.is_file():
        log_error("The Engine dependency install completed without a Next.js executable.")
        return 1
    stamp.write_text(json.dumps({"package_json_sha256": package_digest}, indent=2) + "\n", encoding="utf-8")
    return 0


def _frontend_build_fingerprint(
    workspace: Path,
    engine_commit: str,
    node_version_tuple: Tuple[int, int, int],
    environment: Dict[str, str],
) -> str:
    """Fingerprint every input that can reasonably change the static export."""
    marker_path = workspace / ".gitmanager-generated-workspace"
    try:
        marker = json.loads(marker_path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise RuntimeError("The generated Engine workspace marker is invalid; cannot reuse a frontend build.") from error
    if not isinstance(marker, dict):
        raise RuntimeError("The generated Engine workspace marker must contain a JSON object.")
    frontend_sha256 = str(marker.get("frontend_sha256") or "")
    if not re.fullmatch(r"[0-9a-f]{64}", frontend_sha256) or marker.get("engine_commit") != engine_commit:
        raise RuntimeError("The generated Engine workspace does not match the resolved Engine/frontend inputs.")

    digest = hashlib.sha256()
    fingerprint_inputs = {
        "engine_commit": engine_commit,
        "frontend_sha256": frontend_sha256,
        "node_version": list(node_version_tuple),
        "platform": platform.system(),
        "machine": platform.machine(),
    }
    digest.update(json.dumps(fingerprint_inputs, sort_keys=True, separators=(",", ":")).encode("utf-8"))

    for relative in (
        "package.json",
        "package-lock.json",
        "npm-shrinkwrap.json",
        "pnpm-lock.yaml",
        "yarn.lock",
        "next.config.js",
        "tsconfig.json",
        "postcss.config.js",
        "postcss.config.mjs",
        "tailwind.config.js",
        "tailwind.config.ts",
    ):
        path = workspace / relative
        if path.is_file():
            digest.update(relative.encode("utf-8"))
            digest.update(b"\0")
            digest.update(hashlib.sha256(path.read_bytes()).digest())

    # Next.js can inline NEXT_PUBLIC_* values and can also use other NEXT_*
    # configuration variables during compilation. Hash them without persisting
    # their values in the cache manifest.
    relevant_environment = {
        key for key in environment
        if key in {"NODE_ENV", "NODE_OPTIONS", "TZ", "LANG", "LC_ALL"}
        or key.startswith("NEXT_PUBLIC_")
        or (key.startswith("NEXT_") and key != "NEXT_TELEMETRY_DISABLED")
    }
    for key in sorted(relevant_environment):
        digest.update(key.encode("utf-8"))
        digest.update(b"\0")
        digest.update(environment[key].encode("utf-8"))
        digest.update(b"\0")
    return digest.hexdigest()


def _frontend_output_digest(output_dir: Path) -> Tuple[str, int]:
    """Hash the static export so a changed or truncated output is never reused."""
    if not (output_dir / "index.html").is_file() or not (output_dir / "_next" / "static").is_dir():
        raise RuntimeError("The cached Next.js export is missing index.html or _next/static.")
    digest = hashlib.sha256()
    count = 0
    for path in sorted(item for item in output_dir.rglob("*") if item.is_file() and not item.is_symlink()):
        relative = path.relative_to(output_dir).as_posix()
        # This provenance file is refreshed after every successful Engine
        # resolution and is not an input to the generated frontend assets.
        if relative == "engine-source.json":
            continue
        file_hash = hashlib.sha256(path.read_bytes()).hexdigest()
        digest.update(relative.encode("utf-8"))
        digest.update(b"\0")
        digest.update(str(path.stat().st_size).encode("ascii"))
        digest.update(b"\0")
        digest.update(file_hash.encode("ascii"))
        digest.update(b"\0")
        count += 1
    if count == 0:
        raise RuntimeError("The Next.js static export contains no reusable files.")
    return digest.hexdigest(), count


def _frontend_build_cache_valid(workspace: Path, fingerprint: str) -> bool:
    cache_path = workspace / FRONTEND_BUILD_CACHE_FILE
    try:
        cache = json.loads(cache_path.read_text(encoding="utf-8"))
        if (
            not isinstance(cache, dict)
            or cache.get("schema_version") != 1
            or cache.get("fingerprint") != fingerprint
            or not isinstance(cache.get("output_files"), int)
            or not isinstance(cache.get("output_sha256"), str)
        ):
            return False
        digest, count = _frontend_output_digest(workspace / "out")
        return count == cache["output_files"] and digest == cache["output_sha256"]
    except (OSError, UnicodeDecodeError, json.JSONDecodeError, RuntimeError):
        return False


def _save_frontend_build_cache(workspace: Path, fingerprint: str) -> None:
    output_digest, output_files = _frontend_output_digest(workspace / "out")
    _atomic_write_json(
        workspace / FRONTEND_BUILD_CACHE_FILE,
        {
            "schema_version": 1,
            "fingerprint": fingerprint,
            "output_sha256": output_digest,
            "output_files": output_files,
            "created_at": datetime.now(timezone.utc).isoformat(),
        },
    )


def _build_metadata(
    branch_head: str,
    commit: str,
    checks: Dict[str, Dict[str, object]],
) -> Dict[str, object]:
    return {
        "repository": "https://github.com/" + ENGINE_REPOSITORY,
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

def build_frontend(project_root: Path, mode: str) -> int:
    try:
        with timed_stage("Node.js version check"):
            version = node_version()

        cache_root = project_root / ".cache" / "nextjs-engine"
        with timed_stage("Engine source resolution"):
            source_root, commit, branch_head, checks = ensure_engine_source(cache_root)
        with timed_stage("Engine workspace preparation"):
            workspace = prepare_workspace(project_root, source_root)
        log_ok("Node.js " + ".".join(str(part) for part in version) + " is available.")

        environment = dict(os.environ)
        environment["NEXT_TELEMETRY_DISABLED"] = "1"
        environment["NEXTJS_ENGINE_SOURCE_COMMIT"] = commit

        output_dir = workspace / "out"
        fingerprint = None
        if mode == "build":
            with timed_stage("Frontend export cache verification"):
                fingerprint = _frontend_build_fingerprint(workspace, commit, version, environment)
                cache_hit = _frontend_build_cache_valid(workspace, fingerprint)
            if cache_hit:
                _atomic_write_json(output_dir / "engine-source.json", _build_metadata(branch_head, commit, checks))
                log_ok(
                    "Reusing the verified Next.js static export for Engine source "
                    + commit
                    + "; npm installation and the Next.js compiler were skipped."
                )
                return 0

        with timed_stage("Engine npm dependency preparation"):
            install_result = install_engine_build_dependencies(workspace)
        if install_result != 0:
            return install_result

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

        command = [node, str(next_cli), "build"]
        log_info("Building the GitManager frontend as a Next.js static export.")
        with timed_stage("Next.js static export compilation"):
            result = run_command(command, workspace, environment)
        if result != 0:
            return result
        if not (output_dir / "index.html").is_file() or not (output_dir / "_next" / "static").is_dir():
            raise RuntimeError(
                "Next.js completed but did not produce out/index.html and out/_next/static."
            )

        with timed_stage("Static export verification and cache manifest"):
            _atomic_write_json(output_dir / "engine-source.json", _build_metadata(branch_head, commit, checks))
            if fingerprint is None:
                raise RuntimeError("The frontend build fingerprint was not initialized.")
            _save_frontend_build_cache(workspace, fingerprint)
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
    parser.add_argument(
        "--refresh-engine-source",
        action="store_true",
        help="Bypass the Engine source cache TTL and resolve the newest CI-green main-3 commit.",
    )
    args = parser.parse_args()
    if args.refresh_engine_source:
        os.environ["GITMANAGER_ENGINE_REFRESH"] = "1"
    project_root = args.project_root.resolve()
    if not (project_root / "src-tauri").is_dir() or not (project_root / "frontend").is_dir():
        log_error("The supplied project root is not a GitManager repository.")
        return 2
    return build_frontend(project_root, args.mode)


if __name__ == "__main__":
    sys.exit(main())
