from __future__ import annotations

import http.client
import json
import importlib.util
import urllib.error
import unittest
import tempfile
from pathlib import Path
from unittest import mock


SCRIPT_PATH = Path(__file__).with_name("nextjs_engine_source.py")
SPEC = importlib.util.spec_from_file_location("gitmanager_engine_source", SCRIPT_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError("Could not load nextjs_engine_source.py for tests.")
engine_source = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(engine_source)


class FakeResponse:
    def __init__(self, *, chunks=(), error=None, url=None):
        self.headers = {}
        self._chunks = list(chunks)
        self._error = error
        self._url = url or "https://api.github.com/repos/Kastrick8/nextjs-engine/branches/main-3"

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc, traceback):
        return False

    def geturl(self):
        return self._url

    def read(self, _size):
        if self._error is not None:
            error, self._error = self._error, None
            raise error
        return self._chunks.pop(0) if self._chunks else b""


class RequestRetryTests(unittest.TestCase):
    URL = "https://api.github.com/repos/Kastrick8/nextjs-engine/branches/main-3"

    def test_retries_a_truncated_chunked_response_from_the_beginning(self):
        interrupted = FakeResponse(error=http.client.IncompleteRead(b"partial", 92833))
        complete = FakeResponse(chunks=[b'{"commit":{"sha":"abc"}}'])
        with (
            mock.patch.object(engine_source.urllib.request, "urlopen", side_effect=[interrupted, complete]) as open_url,
            mock.patch.object(engine_source.time, "sleep"),
        ):
            payload = engine_source.request_bytes(self.URL, 1024, "application/vnd.github+json")

        self.assertEqual(payload, b'{"commit":{"sha":"abc"}}')
        self.assertEqual(open_url.call_count, 2)

    def test_retries_temporary_github_server_errors(self):
        unavailable = urllib.error.HTTPError(self.URL, 503, "Service unavailable", {}, None)
        complete = FakeResponse(chunks=[b"ok"])
        with (
            mock.patch.object(engine_source.urllib.request, "urlopen", side_effect=[unavailable, complete]) as open_url,
            mock.patch.object(engine_source.time, "sleep"),
        ):
            payload = engine_source.request_bytes(self.URL, 10, "application/vnd.github+json")

        self.assertEqual(payload, b"ok")
        self.assertEqual(open_url.call_count, 2)

    def test_stops_after_bounded_retries_for_repeated_truncation(self):
        responses = [
            FakeResponse(error=http.client.IncompleteRead(b"partial", 100))
            for _ in range(engine_source.REQUEST_ATTEMPTS)
        ]
        with (
            mock.patch.object(engine_source.urllib.request, "urlopen", side_effect=responses) as open_url,
            mock.patch.object(engine_source.time, "sleep"),
        ):
            with self.assertRaisesRegex(RuntimeError, "after 3 attempts"):
                engine_source.request_bytes(self.URL, 1024, "application/vnd.github+json")

        self.assertEqual(open_url.call_count, engine_source.REQUEST_ATTEMPTS)

    def test_does_not_retry_a_response_that_exceeds_the_size_limit(self):
        response = FakeResponse(chunks=[b"too large"])
        with mock.patch.object(engine_source.urllib.request, "urlopen", return_value=response) as open_url:
            with self.assertRaisesRegex(RuntimeError, "exceeds the configured download limit"):
                engine_source.request_bytes(self.URL, 2, "application/vnd.github+json")

        self.assertEqual(open_url.call_count, 1)


class EngineWorkspaceOverlayTests(unittest.TestCase):
    def test_uses_gitmanager_error_hooks_without_modifying_engine_source(self):
        with tempfile.TemporaryDirectory(prefix="gitmanager-engine-overlay-") as temporary:
            root = Path(temporary)
            source = root / "cache" / "source"
            frontend = root / "project" / "frontend"
            (source / "src" / "engine" / "plugins").mkdir(parents=True)
            for directory in ("scripts", "tests", "examples", "docs", "app"):
                (source / directory).mkdir(parents=True, exist_ok=True)
            (source / "package.json").write_text('{"name":"nextjs-engine"}', encoding="utf-8")
            (source / "src" / "engine" / "index.ts").write_text("export {};", encoding="utf-8")
            (source / "src" / "engine" / "plugins" / "enginePlugin.js").write_text("module.exports = {};", encoding="utf-8")
            (source / "app" / "page.tsx").write_text("export default function Page(){ return null; }", encoding="utf-8")
            engine_instrumentation = "export function register(){ throw new Error('Engine source sentinel'); }\\n"
            engine_global_error = "export default function EngineSourceGlobalError(){ return null; }\\n"
            (source / "instrumentation.ts").write_text(engine_instrumentation, encoding="utf-8")
            (source / "app" / "global-error.tsx").write_text(engine_global_error, encoding="utf-8")
            (source / ".gitmanager-source.json").write_text(
                json.dumps({"commit": "a" * 40}), encoding="utf-8"
            )

            (frontend / "app").mkdir(parents=True)
            (frontend / "components").mkdir()
            (frontend / "lib").mkdir()
            (frontend / "app" / "page.tsx").write_text("export default function GitManagerPage(){ return null; }", encoding="utf-8")
            (frontend / "app" / "error.tsx").write_text("export default function GitManagerRouteError(){ return null; }", encoding="utf-8")
            (frontend / "app" / "global-error.tsx").write_text("export default function GitManagerGlobalError(){ return null; }", encoding="utf-8")
            (frontend / "components" / "gitmanager-app.tsx").write_text("export {};", encoding="utf-8")
            (frontend / "lib" / "use-gitmanager.ts").write_text("export {};", encoding="utf-8")
            gitmanager_instrumentation = "export function register(): void {}\\n"
            (frontend / "instrumentation.ts").write_text(gitmanager_instrumentation, encoding="utf-8")
            (frontend / "next.config.js").write_text("module.exports = {};", encoding="utf-8")

            workspace = engine_source.prepare_workspace(root / "project", source)

            self.assertEqual((workspace / "instrumentation.ts").read_text(encoding="utf-8"), gitmanager_instrumentation)
            self.assertIn("GitManagerRouteError", (workspace / "app" / "error.tsx").read_text(encoding="utf-8"))
            self.assertIn("GitManagerGlobalError", (workspace / "app" / "global-error.tsx").read_text(encoding="utf-8"))
            self.assertEqual((source / "instrumentation.ts").read_text(encoding="utf-8"), engine_instrumentation)
            self.assertEqual((source / "app" / "global-error.tsx").read_text(encoding="utf-8"), engine_global_error)
            self.assertTrue(json.loads((workspace / ".gitmanager-generated-workspace").read_text(encoding="utf-8"))["gitmanager_runtime_error_handling_overlay"])



if __name__ == "__main__":
    unittest.main()
