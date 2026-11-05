"""Warn when the `markitdown` package next to markitdown-rs is not
markitdown-rs's own.

Microsoft's markitdown installs the same `markitdown` package, so whichever
was installed last silently replaces the other's files, and uninstalling
either deletes shared ones. Plugins such as markitdown-ocr depend on
`markitdown`, so `pip install markitdown-ocr` brings it in. This module
lives outside the `markitdown` package, where neither overwrites it.
markitdown_rs.pth calls install() at start-up, which only adds an import
hook: the check runs once, the first time `markitdown` is imported.
"""

import os
import sys

CONFLICT = """\
markitdown-rs: Microsoft's markitdown package ({theirs}) is installed in the
same environment ({where}). Both provide the `markitdown` module, so one has
overwritten the other's files, and `import markitdown` may not be
markitdown-rs. This usually comes from installing a plugin that depends on
markitdown, such as markitdown-ocr. To fix it:

    pip uninstall -y markitdown markitdown-rs
    pip install markitdown-rs
    pip install --no-deps markitdown-ocr      # plugins without their markitdown dependency

`pip install 'markitdown-rs[ocr]'` installs markitdown-ocr's other
dependencies. With uv, add `markitdown; sys_platform == 'never'` to
override-dependencies instead. Set MARKITDOWN_RS_ALLOW_MARKITDOWN=1 to
silence this warning.
"""

DAMAGED = """\
markitdown-rs: the `markitdown` package in {where} is {state}, although
markitdown-rs is installed. Another package removed or replaced its files
(for example `pip uninstall markitdown` after installing both). Reinstall
markitdown-rs:

    pip install --force-reinstall --no-deps markitdown-rs

Set MARKITDOWN_RS_ALLOW_MARKITDOWN=1 to silence this warning.
"""

_checked = False


def check(site_dir=None):
    """What is wrong next to markitdown-rs: ("conflict", their dist-info),
    ("damaged", "missing" or "not markitdown-rs's"), or None."""
    site_dir = site_dir or os.path.dirname(os.path.abspath(__file__))
    try:
        names = os.listdir(site_dir)
    except OSError:
        return None
    if not any(n.startswith("markitdown_rs-") and n.endswith(".dist-info") for n in names):
        return None
    theirs = [
        n
        for n in names
        if n.lower().startswith("markitdown-")
        and n.endswith((".dist-info", ".egg-info"))
    ]
    if theirs:
        return ("conflict", theirs[0])
    init = os.path.join(site_dir, "markitdown", "__init__.py")
    try:
        with open(init, encoding="utf-8") as f:
            ours = "markitdown-rs" in f.read(4096)
    except OSError:
        return ("damaged", "missing")
    return None if ours else ("damaged", "not markitdown-rs's")


def warn():
    """Check once per process and report on stderr."""
    global _checked
    if _checked:
        return
    _checked = True
    if os.environ.get("MARKITDOWN_RS_ALLOW_MARKITDOWN"):
        return
    site_dir = os.path.dirname(os.path.abspath(__file__))
    problem = check(site_dir)
    if problem and problem[0] == "conflict":
        sys.stderr.write(CONFLICT.format(theirs=problem[1], where=site_dir))
    elif problem:
        sys.stderr.write(DAMAGED.format(state=problem[1], where=site_dir))


class _ImportHook:
    """Runs the check when `markitdown` is first imported, then steps aside."""

    @classmethod
    def find_spec(cls, name, path=None, target=None):
        if name == "markitdown":
            try:
                sys.meta_path.remove(cls)
            except ValueError:
                pass
            warn()
        return None


def install():
    if _ImportHook not in sys.meta_path:
        sys.meta_path.insert(0, _ImportHook)
