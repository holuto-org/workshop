#!/usr/bin/env python3
"""Check the Linux smoke launch boundary without starting a GUI or printer.

Usage: python3 packaging/test-smoke.py /absolute/path/to/workshop
The real executable handles every CLI command; only the GUI/display is replaced.
"""

import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

if len(sys.argv) < 2:
    raise SystemExit(__doc__)

BINARY = Path(sys.argv.pop(1)).resolve()
SMOKE = Path(__file__).with_name("smoke-linux.sh").resolve()


class SmokeSafety(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.env = dict(os.environ)
        for variable in ("HOME", "XDG_STATE_HOME", "XDG_CONFIG_HOME"):
            directory = self.root / variable
            directory.mkdir()
            self.env[variable] = str(directory)
        self.bin = self.root / "bin"
        self.bin.mkdir()
        self.env["PATH"] = str(self.bin) + os.pathsep + self.env["PATH"]
        self.env["SMOKE_BINARY"] = str(BINARY)
        self.env["SMOKE_RESULT"] = str(self.root / "result.json")
        self.env["SMOKE_READY"] = str(self.root / "ready")
        os.mkfifo(self.env["SMOKE_READY"])
        self.executable("workshop", '''
import json, os, subprocess, sys
if len(sys.argv) > 1:
    os.execv(os.environ["SMOKE_BINARY"], [os.environ["SMOKE_BINARY"], *sys.argv[1:]])
config = subprocess.check_output([os.environ["SMOKE_BINARY"], "config", "show", "--json"])
with open(os.environ["SMOKE_RESULT"], "w") as result:
    json.dump({"home": os.environ["HOME"], "settings": json.loads(config)["config"]}, result)
with open(os.environ["SMOKE_READY"], "w") as ready:
    ready.write("ready")
''')
        # Synchronize with the launch probe, then deliberately report no window.
        self.executable("xdotool", '''
import os, sys
with open(os.environ["SMOKE_READY"]) as ready:
    ready.read()
sys.exit(1)
''')

    def executable(self, name, body):
        path = self.bin / name
        path.write_text(f"#!{sys.executable}\n{body}")
        path.chmod(0o755)

    def launch(self):
        logs = self.root / "logs"
        logs.mkdir()
        result = subprocess.run(
            ["bash", str(SMOKE), str(self.bin / "workshop"), str(logs)],
            env=self.env, capture_output=True, text=True, timeout=30,
        )
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn("Main application window did not appear", result.stderr)
        return json.loads(Path(self.env["SMOKE_RESULT"]).read_text())

    def test_standalone_launch_does_not_inherit_saved_configuration(self):
        subprocess.run(
            [str(BINARY), "config", "set", "--api-url", "https://example.invalid"],
            env=self.env, check=True, capture_output=True,
        )
        result = self.launch()
        self.assertNotEqual(result["home"], self.env["HOME"])
        # Fresh state uses the compiled release default, never the saved override.
        self.assertEqual(
            result["settings"]["apiUrl"], os.environ.get("HOLUTO_API_URL", "")
        )
        self.assertFalse(result["settings"]["startOnLaunch"])
        saved = json.loads(subprocess.check_output(
            [str(BINARY), "config", "show", "--json"], env=self.env,
        ))["config"]
        self.assertEqual(saved["apiUrl"], "https://example.invalid")
        self.assertTrue(saved["telemetryEnabled"])

    def test_diagnostics_are_disabled_before_gui_launch(self):
        self.assertFalse(self.launch()["settings"]["telemetryEnabled"])


if __name__ == "__main__":
    unittest.main()
