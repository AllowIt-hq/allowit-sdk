"""Test the core Lean checker's audit logic with a stand-in Lean executable."""

from pathlib import Path
import os
import re
import subprocess
import sys
import tempfile
import unittest

LEAN = Path(__file__).resolve().parents[1] / "lean"
CHECK = LEAN / "check.py"
THEOREMS = re.findall(r"^theorem\s+(\w+)", (LEAN / "AllowIt.lean").read_text(), re.MULTILINE)

# The stand-in prints what Lean 4 prints for `#print axioms` on each theorem of
# the file it receives. MODE changes one detail to exercise each rejection.
STAND_IN = """import os, re, sys
mode = os.environ.get("MODE", "ok")
if sys.argv[1] == "--version":
    version = "4.12.0" if mode == "version" else "4.11.0"
    print(f"Lean (version {version}, arm64-apple-darwin, commit 0, Release)")
    raise SystemExit(0)
if mode == "fail":
    print("error: unknown identifier", file=sys.stderr)
    raise SystemExit(1)
source = open(sys.argv[1]).read()
names = re.findall(r"^theorem\\s+(\\w+)", source, re.MULTILINE)
if mode == "missing":
    names = names[1:]
for index, name in enumerate(names):
    axioms = "propext, Quot.sound"
    if mode == "axiom" and index == 0:
        axioms = "propext, AllowIt.trustMe"
    print(f"'AllowIt.{name}' depends on axioms: [{axioms}]")
"""


class CoreLeanCheckTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.directory = tempfile.TemporaryDirectory()
        cls.lean = Path(cls.directory.name) / "lean"
        cls.lean.write_text(f"#!{sys.executable}\n{STAND_IN}")
        cls.lean.chmod(0o755)

    @classmethod
    def tearDownClass(cls):
        cls.directory.cleanup()

    def run_check(self, mode):
        environment = dict(os.environ, LEAN_BIN=str(self.lean), MODE=mode)
        return subprocess.run([sys.executable, str(CHECK)], capture_output=True, text=True, env=environment)

    def test_model_has_the_documented_theorems(self):
        self.assertEqual(len(THEOREMS), 13)
        self.assertEqual(len(set(THEOREMS)), 13)

    def test_pinned_toolchain(self):
        self.assertEqual((LEAN / "lean-toolchain").read_text().strip(), "leanprover/lean4:v4.11.0")

    def test_accepts_a_complete_audit_with_standard_axioms(self):
        result = self.run_check("ok")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("Verified 13 theorems in AllowIt.lean with Lean 4.11.0", result.stdout)
        self.assertNotIn("NativeDaily", result.stdout + result.stderr)

    def test_rejects_another_lean_version(self):
        result = self.run_check("version")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Expected leanprover/lean4:v4.11.0", result.stderr)

    def test_rejects_a_failed_lean_run(self):
        self.assertNotEqual(self.run_check("fail").returncode, 0)

    def test_rejects_an_incomplete_theorem_audit(self):
        result = self.run_check("missing")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Incomplete theorem audit", result.stderr)

    def test_rejects_an_application_axiom(self):
        result = self.run_check("axiom")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Unapproved axiom dependency", result.stderr)

    def test_rejects_a_missing_checker(self):
        environment = dict(os.environ, LEAN_BIN=str(Path(self.directory.name) / "absent"))
        result = subprocess.run([sys.executable, str(CHECK)], capture_output=True, text=True, env=environment)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Lean checker unavailable", result.stderr)

    def test_model_has_no_proof_holes_or_overrides(self):
        source = (LEAN / "AllowIt.lean").read_text()
        self.assertIsNone(re.search(
            r"\b(?:sorry|admit|axiom|native_decide|implemented_by|extern|unsafe)\b|debug\.skipKernelTC", source))


if __name__ == "__main__":
    unittest.main()
