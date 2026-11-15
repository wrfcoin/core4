"""Source checks on wrfcoin-node.ps1's status-page URL (no PowerShell needed).

The Start-menu "WRFCoin Node" entry opens the node's /dashboard with the
operator token in the URL fragment, so the page's Stop button works without
copying the token. The token must only ever travel in the fragment.
"""

from __future__ import annotations

import re
import unittest
from pathlib import Path

LAUNCHER = (Path(__file__).resolve().parent / "wrfcoin-node.ps1").read_text(encoding="utf-8")


def function_body(name: str) -> str:
    start = LAUNCHER.index(f"function {name}")
    end = LAUNCHER.index("\n}\n", start)
    return LAUNCHER[start:end]


class StatusPageUrl(unittest.TestCase):
    def test_dashboard_url_carries_the_token_in_the_fragment(self) -> None:
        body = function_body("Get-StatusPageUrl")
        self.assertIn('return "$Api/dashboard#operator=$token"', body)
        self.assertIn("[System.IO.File]::ReadAllText($TokenFile)", body)
        # Only a well-formed token (the 32-byte hex Setup writes) is handed over.
        self.assertIn("$token -match '^[0-9a-f]{64}$'", body)

    def test_the_token_never_goes_in_a_query_string(self) -> None:
        self.assertIsNone(re.search(r"[?&](token|operator)=", LAUNCHER))

    def test_open_status_page_uses_it_and_respects_no_browser(self) -> None:
        body = function_body("Open-StatusPage")
        self.assertIn("if ($NoBrowser) { return }", body)
        self.assertIn("Start-Process (Get-StatusPageUrl $Api", body)

    def test_older_builds_without_the_dashboard_get_no_token(self) -> None:
        body = function_body("Get-StatusPageUrl")
        self.assertIn('if (-not $HasDashboard) { return "$Api/network/info" }', body)


if __name__ == "__main__":
    unittest.main()
