#!/usr/bin/env python3

import tempfile
import unittest
from pathlib import Path

import yaml

from check_launch_api_spec import check
from launch_api_routes import parse_routes

ROOT = Path(__file__).resolve().parents[1]
SPEC = ROOT / "docs/api/launch-api.openapi.yaml"
ROUTES = ROOT / "native-chain/src/bin/launch_api/routes/mod.rs"


class LaunchApiSpecCheckTest(unittest.TestCase):
    def _write_spec(self, document: dict) -> Path:
        temp = tempfile.NamedTemporaryFile(mode="w", suffix=".yaml", delete=False)
        yaml.safe_dump(document, temp, sort_keys=False)
        temp.close()
        self.addCleanup(Path(temp.name).unlink)
        return Path(temp.name)

    def test_current_tree_is_green(self):
        self.assertEqual(check(SPEC, ROUTES), [])

    def test_invented_path_is_red(self):
        document = yaml.safe_load(SPEC.read_text())
        document["paths"]["/invented"] = {
            "get": {"responses": {"200": {"description": "invented"}}}
        }
        errors = check(self._write_spec(document), ROUTES)
        self.assertIn("phantom spec operation: GET /invented", errors)

    def test_removed_core_route_is_red(self):
        document = yaml.safe_load(SPEC.read_text())
        del document["paths"]["/health"]
        errors = check(self._write_spec(document), ROUTES)
        self.assertIn("missing core operation: GET /health", errors)

    def test_unregistered_head_options_trace_are_red(self):
        for method in ("head", "options", "trace"):
            with self.subTest(method=method):
                document = yaml.safe_load(SPEC.read_text())
                document["paths"]["/health"][method] = {
                    "responses": {"200": {"description": "phantom"}}
                }
                errors = check(self._write_spec(document), ROUTES)
                self.assertIn(f"phantom spec operation: {method.upper()} /health", errors)

    def test_privacy_routes_are_labelled_feature_gated(self):
        privacy = [r for r in parse_routes(ROUTES) if r["path"].startswith("/privacy/")]
        self.assertGreaterEqual(len(privacy), 13)
        self.assertEqual({r["feature"] for r in privacy}, {"privacy"})
        ungated = [r for r in parse_routes(ROUTES) if r["path"] == "/health"]
        self.assertEqual([r["feature"] for r in ungated], [None])

    def test_cfg_label_comes_from_the_guard_not_the_path(self):
        source = (
            'let scope = scope.route("/open/{id}", web::get().to(a));\n'
            '#[cfg(feature = "demo")]\n'
            'let scope = { scope.route("/plain", web::post().to(b)) };\n'
            'let scope = scope.route("/after", web::get().to(c));\n'
        )
        with tempfile.NamedTemporaryFile("w", suffix=".rs", delete=False) as handle:
            handle.write(source)
        self.addCleanup(Path(handle.name).unlink)
        labels = {r["path"]: r["feature"] for r in parse_routes(handle.name)}
        self.assertEqual(labels, {"/open/{id}": None, "/plain": "demo", "/after": None})

    def test_gated_route_in_spec_without_marker_is_red(self):
        document = yaml.safe_load(SPEC.read_text())
        document["paths"]["/privacy/keys/register"] = {
            "post": {"responses": {"200": {"description": "gated"}}}
        }
        path = self._write_spec(document)
        errors = check(path, ROUTES)
        self.assertIn(
            "feature-gated operation not marked x-feature: privacy: POST /privacy/keys/register",
            errors,
        )
        document["paths"]["/privacy/keys/register"]["post"]["x-feature"] = "privacy"
        self.assertEqual(check(self._write_spec(document), ROUTES), [])


if __name__ == "__main__":
    unittest.main()
