from __future__ import annotations

import importlib.util
import os
import stat
import tempfile
import unittest
from pathlib import Path


SETUP_PATH = Path(__file__).with_name("setup.py")
SPEC = importlib.util.spec_from_file_location("cloud_node_setup", SETUP_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError("Could not load Cloud Node setup.py for tests.")
setup = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(setup)


class SecretFilePermissionTests(unittest.TestCase):
    def test_secret_file_permissions_are_restricted(self) -> None:
        with tempfile.TemporaryDirectory(prefix="gitmanager-cloud-node-test-") as directory:
            secret_file = Path(directory) / "node-private-key"
            setup.secure_write(secret_file, "test-only-private-key\n", secret=True)

            if os.name == "nt":
                sid = setup.windows_current_user_sid()
                system_root = Path(os.environ["SystemRoot"])
                icacls = system_root / "System32" / "icacls.exe"
                result = setup.run_checked([icacls, secret_file], timeout=15)
                self.assertEqual(result.returncode, 0, result.stderr or result.stdout)
                self.assertIn(sid, result.stdout)
                for broad_sid in ("S-1-1-0", "S-1-5-11", "S-1-5-32-545"):
                    self.assertNotIn(broad_sid, result.stdout)

                # Re-running setup must also repair existing-key ACLs.
                setup.restrict_secret_file_permissions(secret_file)
            else:
                self.assertEqual(stat.S_IMODE(secret_file.stat().st_mode), 0o600)
                setup.restrict_secret_file_permissions(secret_file)
                self.assertEqual(stat.S_IMODE(secret_file.stat().st_mode), 0o600)


if __name__ == "__main__":
    unittest.main()
