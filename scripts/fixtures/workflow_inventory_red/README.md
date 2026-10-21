# RED fixture for `scripts/check_workflow_inventory.py`

Not a workflow directory. These files live under `scripts/fixtures/` precisely so GitHub
never registers them — a fixture that GitHub schedules is a real workflow wearing a test's
name.

`undeclared-gate.yml` is the #2890 defect in miniature: a `workflow_dispatch`-only file
whose name promises a standing gate. CI runs the checker against this directory and
requires exit **1**.

Why a fixture as well as `--self-test`: the self-test drives the classifier as a Python
function. This drives the **shipped CLI over real files on disk** — argument parsing,
globbing, file reading, exit code. A checker can classify perfectly and still exit 0 from
`main()`, and that gap is exactly how a gate ships unable to fail.
