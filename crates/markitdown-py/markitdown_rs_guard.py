"""Warn when Microsoft's markitdown is installed next to markitdown-rs.

Both distributions install the `markitdown` package, so whichever was
installed last silently replaces the other's files (and uninstalling either
deletes shared ones). Plugins such as markitdown-ocr depend on `markitdown`,
so `pip install markitdown-ocr` brings it in. This module lives outside the
`markitdown` package, where neither overwrites it, and runs at start-up from
markitdown_rs.pth.
"""

import os
import sys

MESSAGE = """\
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


def check(site_dir=None):
    """The other markitdown's dist-info, when it sits next to markitdown-rs."""
    site_dir = site_dir or os.path.dirname(os.path.abspath(__file__))
    try:
        names = os.listdir(site_dir)
    except OSError:
        return None
    ours = any(n.startswith("markitdown_rs-") and n.endswith(".dist-info") for n in names)
    theirs = [
        n
        for n in names
        if n.lower().startswith("markitdown-")
        and n.endswith((".dist-info", ".egg-info"))
    ]
    return theirs[0] if ours and theirs else None


def warn():
    if os.environ.get("MARKITDOWN_RS_ALLOW_MARKITDOWN"):
        return
    site_dir = os.path.dirname(os.path.abspath(__file__))
    theirs = check(site_dir)
    if theirs:
        sys.stderr.write(MESSAGE.format(theirs=theirs, where=site_dir))


warn()
