# Planted-leak fixtures (#2129)

Each file plants a secret reaching one claimed sink. `scripts/test_check_secret_leak.py`
requires **every** one to be rejected, and the clean file to be accepted.

Without these the scanner proved nothing: review of PR #2239 found that a one-line planted
leak was caught while the **semantically identical multiline form exited 0**, and a planted
`bail!("...{signing_key}")` exited 0 because error macros were not sinks.

Fixtures are `.rs.txt`, not `.rs`, so cargo never compiles them.
