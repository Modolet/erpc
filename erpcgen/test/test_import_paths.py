"""Regression tests for IDL imports from a separate build directory.

Author: modolet <y@xxyx.io>
Date: 2026-09-09
SPDX-License-Identifier: BSD-3-Clause

Run directly with ERPCGEN set to the generator executable. Optionally set
ERPCGEN_RUNNER to a runner executable such as wine for cross-platform testing.
"""

import os
import subprocess
import tempfile
import unittest
from pathlib import Path


class ImportPathTests(unittest.TestCase):
    def setUp(self):
        self.workspace = tempfile.TemporaryDirectory(prefix="erpcgen import paths ")
        self.addCleanup(self.workspace.cleanup)
        self.root = Path(self.workspace.name)
        self.build = self.root / "build"
        self.build.mkdir()
        self.idl = self.root / "idl files"
        (self.idl / "nested").mkdir(parents=True)
        (self.idl / "main.erpc").write_text(
            'program import_paths\nimport "nested/service.erpc"\n'
            'import "sibling.erpc"\n',
            encoding="utf-8",
        )
        (self.idl / "nested" / "service.erpc").write_text(
            'import "types.erpc"\ninterface Service { ping(Value value) -> void }\n',
            encoding="utf-8",
        )
        (self.idl / "nested" / "types.erpc").write_text(
            "struct Value { int32 number }\n", encoding="utf-8"
        )
        (self.idl / "sibling.erpc").write_text(
            "interface Sibling { pong() -> void }\n", encoding="utf-8"
        )
        executable = os.environ.get("ERPCGEN", "erpcgen")
        runner = os.environ.get("ERPCGEN_RUNNER")
        self.command = [runner, executable] if runner else [executable]

    def generate(self, args):
        for language, generated_file in (
            ("c", "c_import_paths_client.h"),
            ("rust", "import_paths.rs"),
        ):
            with self.subTest(language=language):
                output = self.build / language
                result = subprocess.run(
                    self.command + ["-g", language, "-o", language] + args,
                    cwd=self.build,
                    capture_output=True,
                    text=True,
                    timeout=60,
                    check=False,
                )
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                generated = (output / generated_file).read_text(encoding="utf-8")
                self.assertIn("ping(", generated)
                self.assertIn("pong(", generated)

    def test_forward_slash_input(self):
        self.generate(["../idl files/main.erpc"])

    def test_absolute_forward_slash_input(self):
        self.generate([(self.idl / "main.erpc").as_posix()])

    def test_input_resolved_from_include_directory(self):
        self.generate(["-I", "../idl files", "main.erpc"])


if __name__ == "__main__":
    unittest.main()
