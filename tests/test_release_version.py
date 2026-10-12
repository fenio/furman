import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


SCRIPT = Path(__file__).resolve().parents[1] / "scripts/check-release-version.py"
spec = importlib.util.spec_from_file_location("release_version", SCRIPT)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class ReleaseVersionTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root / "src-tauri").mkdir()
        self.write_versions("0.3.17")

    def write_versions(self, version):
        for filename, data in {
            "package.json": {"version": version},
            "package-lock.json": {"version": version, "packages": {"": {"version": version}}},
            "src-tauri/tauri.conf.json": {"version": version},
        }.items():
            (self.root / filename).write_text(json.dumps(data), encoding="utf-8")
        (self.root / "src-tauri/Cargo.toml").write_text(f'[package]\nname = "furman"\nversion = "{version}"\n', encoding="utf-8")
        (self.root / "src-tauri/Cargo.lock").write_text(f'[[package]]\nname = "unrelated"\nversion = "9.9.9"\n\n[[package]]\nname = "furman"\nversion = "{version}"\n', encoding="utf-8")

    def test_matching_versions(self):
        self.assertEqual(module.check_versions(self.root, "v0.3.17"), [])

    def test_wrong_tag_reports_every_version_field(self):
        problems = module.check_versions(self.root, "v0.3.18")
        self.assertEqual(len(problems), 6)

    def test_each_json_version_field_is_checked(self):
        for filename in ("package.json", "package-lock.json", "src-tauri/tauri.conf.json"):
            with self.subTest(filename=filename):
                self.write_versions("0.3.17")
                path = self.root / filename
                data = json.loads(path.read_text())
                data["version"] = "0.3.16"
                path.write_text(json.dumps(data))
                self.assertEqual(len(module.check_versions(self.root, "v0.3.17")), 1)

    def test_npm_lock_root_version_and_missing_fields(self):
        path = self.root / "package-lock.json"
        for data in ({"version": "0.3.17", "packages": {"": {"version": "0.3.16"}}}, {"version": "0.3.17"}):
            with self.subTest(data=data):
                path.write_text(json.dumps(data))
                self.assertEqual(len(module.check_versions(self.root, "v0.3.17")), 1)

    def test_each_cargo_version_is_checked(self):
        for filename in ("src-tauri/Cargo.toml", "src-tauri/Cargo.lock"):
            with self.subTest(filename=filename):
                self.write_versions("0.3.17")
                path = self.root / filename
                path.write_text(path.read_text().replace('"0.3.17"', '"0.3.16"'))
                self.assertEqual(len(module.check_versions(self.root, "v0.3.17")), 1)

    def test_missing_or_duplicate_furman_lock_entry(self):
        path = self.root / "src-tauri/Cargo.lock"
        entry = '[[package]]\nname = "furman"\nversion = "0.3.17"\n'
        for content in ("", entry + entry):
            with self.subTest(content=content):
                path.write_text(content)
                self.assertEqual(len(module.check_versions(self.root, "v0.3.17")), 1)

    def test_missing_or_malformed_files_fail(self):
        for filename in ("package.json", "package-lock.json", "src-tauri/tauri.conf.json", "src-tauri/Cargo.toml", "src-tauri/Cargo.lock"):
            with self.subTest(filename=filename):
                self.write_versions("0.3.17")
                path = self.root / filename
                path.unlink()
                self.assertTrue(module.check_versions(self.root, "v0.3.17"))
                path.write_text("not valid JSON or TOML!")
                self.assertTrue(module.check_versions(self.root, "v0.3.17"))

    def test_invalid_tags_fail(self):
        for tag in ("0.3.17", "v", "vnext", "v01.3.17", "v0.3", "v0.3.17-01", "v0.3.17\n"):
            with self.subTest(tag=tag):
                self.assertTrue(module.check_versions(self.root, tag))

    def test_prerelease_and_build_metadata_match_exactly(self):
        self.write_versions("0.4.0-rc.1+build.2")
        self.assertEqual(module.check_versions(self.root, "v0.4.0-rc.1+build.2"), [])
        self.assertTrue(module.check_versions(self.root, "v0.4.0-rc.1"))

    def test_cli_exit_status(self):
        for tag, expected_code in (("v0.3.17", 0), ("v0.3.18", 1)):
            with self.subTest(tag=tag):
                result = subprocess.run([sys.executable, str(SCRIPT), tag, "--root", str(self.root)], capture_output=True, text=True)
                self.assertEqual(result.returncode, expected_code)


if __name__ == "__main__":
    unittest.main()
