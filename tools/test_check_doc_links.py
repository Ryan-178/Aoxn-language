"""Temporary-fixture coverage; no repository fixtures or third-party dependencies."""
import contextlib
import importlib.util
import io
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.dont_write_bytecode = True
SPEC = importlib.util.spec_from_file_location("check_doc_links", Path(__file__).with_name("check_doc_links.py"))
checker = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = checker
SPEC.loader.exec_module(checker)


class LinkAuditTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()

    def put(self, name, text=""):
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")
        return path

    def findings(self, text, history=()):
        self.put("docs/current.md", text)
        return checker.audit(self.root, set(history))[0]

    def test_existing_relative_encoded_directory_and_fragment(self):
        self.put("with space.txt")
        found = self.findings('[ok](../with%20space.txt?q=1#anchor)\n[dir](../docs/)\n[here](#anything)')
        self.assertEqual(found, [])

    def test_missing_and_deleted_are_separate(self):
        found = self.findings('[missing](../absent.rs)\n[old](../src/llvm.rs)', {"src/llvm.rs"})
        self.assertEqual([(f.category, f.line) for f in found], [("missing-path", 1), ("historical-path", 2)])

    def test_external_absolute_and_outside_links_never_fetched(self):
        self.assertEqual(self.findings('[a](https://example.invalid/a) [b](mailto:a@b) [c](//host/a) [d](/route) [e](../../outside) [f](D:/absent)'), [])

    def test_code_comments_and_extensionless_import_not_links(self):
        text = ('<!-- [hidden](bad)\n[hidden](bad2) -->\n'
                '```ax\nimport * from "stdlib/time"\n[fake](bad)\n```\n'
                '`[inline](bad)`\n    [indented](bad)\nimport * from "stdlib"\n')
        self.assertEqual(self.findings(text), [])

    def test_explicit_extensionless_link_is_still_checked(self):
        self.assertEqual(self.findings('[real link](../missing)')[0].category, "missing-path")

    def test_inline_image_title_angle_and_parentheses(self):
        self.put("test/a(1).zip")
        self.put("a b.txt")
        self.assertEqual(self.findings('[zip](../test/a(1).zip "title") [space](<../a b.txt>)'), [])
        self.assertEqual(self.findings('![image](gone.png)')[0].target, "gone.png")

    def test_reference_links(self):
        found = self.findings('[full][label] [label][] [label]\n\n[label]: ../missing.md "title"')
        self.assertEqual(len(found), 2)  # one per source line + definition
        self.assertTrue(all(f.category == "missing-path" for f in found))

    def test_command_hints_root_relative_and_never_executed(self):
        self.put("examples/ok.ax")
        self.put("docs/local.ax")
        found = self.findings('```sh\ncargo run -- run examples/ok.ax\naoxn check local.ax\nbash missing.sh\n```\n`aoxn check src/llvm.rs`', {"src/llvm.rs"})
        self.assertEqual([(f.category, f.target) for f in found], [("command-drift", "missing.sh"), ("historical-path", "src/llvm.rs")])

    def test_pruning_and_exact_allowlist(self):
        for directory in ("target", "node_modules", ".git", "wiki"):
            self.put(f"{directory}/nested/bad.md", '[bad](gone)')
        for name in checker.HISTORICAL_REPORTS:
            self.put(name, '[old](gone)')
        self.put("docs/not-a-historical-report.md", '[bad](gone)')
        found, scanned, excluded = checker.audit(self.root, set())
        self.assertEqual((len(found), scanned, excluded), (1, 1, len(checker.HISTORICAL_REPORTS)))

    def test_exit_strategy(self):
        self.put("README.md", '[bad](missing)')
        with patch.object(checker, "historical_paths", return_value=set()), contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(checker.main(["--root", str(self.root)]), 0)
            self.assertEqual(checker.main(["--root", str(self.root), "--strict"]), 1)
        with patch.object(checker, "historical_paths", side_effect=RuntimeError("unavailable")), contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(checker.main(["--root", str(self.root)]), 2)

    def test_actual_local_git_deletion_history(self):
        def git(*args):
            return subprocess.run(["git", "-C", str(self.root), *args], check=True, capture_output=True)
        git("init")
        self.put("gone file.md", "old")
        git("add", "--", "gone file.md")
        git("-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "commit", "-m", "fixture")
        git("rm", "--", "gone file.md")
        git("-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "commit", "-m", "delete")
        self.assertIn("gone file.md", checker.historical_paths(self.root))


if __name__ == "__main__":
    unittest.main()
