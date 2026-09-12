"""Reference-only schema migration: reproducible from v3, fail-closed under -O."""
import hashlib
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


class MigrationTests(unittest.TestCase):
    def test_reference_export_and_rejection_guards(self):
        approved = ROOT / "tests/visual/approved"
        before = {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in approved.glob("*.frame.json")}
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "result"
            command = [sys.executable, "-O", str(ROOT / "tools/migrate_fixture_v3.py"), "--continuation-styles", "--out", str(output)]
            subprocess.run(command, check=True, capture_output=True)
            ledger = json.loads((output / "migration-ledger.json").read_text())
            self.assertEqual(len(ledger["entries"]), 24)
            for entry in ledger["entries"]:
                self.assertEqual(entry["after_sha256"], before[entry["file"]])
            self.assertNotEqual(subprocess.run(command, capture_output=True).returncode, 0)
            wrong = Path(directory) / "wrong"
            self.assertNotEqual(subprocess.run(command[:-1] + [str(wrong), "--source-revision", "HEAD"], capture_output=True).returncode, 0)
            self.assertFalse(wrong.exists())
        self.assertEqual(before, {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in approved.glob("*.frame.json")})


if __name__ == "__main__":
    unittest.main()
