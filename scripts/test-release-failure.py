"""Exercise the real notarized recipe with a failing packager and isolated tools."""
from pathlib import Path
import subprocess
import tempfile


def main():
    root = Path(__file__).resolve().parents[1]
    body = (root / "Justfile").read_text().split("release-notarized version:\n", 1)[1]
    body = body.split("\n# Publish a notarized release", 1)[0]
    recipe = "\n".join(line[4:] if line.startswith("    ") else line for line in body.splitlines())
    recipe = recipe.replace("{{version}}", "3.0.1")
    with tempfile.TemporaryDirectory(prefix="goose-release-failure-") as directory:
        work = Path(directory)
        (work / "ui/desktop").mkdir(parents=True)
        bins = work / "bin"
        bins.mkdir()
        mocks = {
            "security": "echo 'Developer ID Application: Test'",
            "git": "echo fixture",
            "just": "exit 0",
            "node": "exit 0",
            "pnpm": "echo 'intentional packaging failure' >&2\nexit 23",
            "xcrun": 'touch "$HOME/notarization-attempted"',
        }
        for name, command in mocks.items():
            executable = bins / name
            executable.write_text("#!/bin/sh\n" + command + "\n")
            executable.chmod(0o755)
        result = subprocess.run(
            ["/bin/bash", "-c", recipe], cwd=work, text=True, capture_output=True,
            env={"HOME": str(work), "PATH": f"{bins}:/usr/bin:/bin",
                 "APPLE_TEAM_ID": "fixture", "APPLE_ID": "fixture@example.test",
                 "APPLE_ID_PASSWORD": "fixture"},
        )
        assert result.returncode == 23, (result.returncode, result.stdout, result.stderr)
        assert "intentional packaging failure" in result.stderr
        assert "Building the DMG" not in result.stdout
        assert ">>> DONE" not in result.stdout
        assert not (work / "notarization-attempted").exists()
    print("PASS: packaging failure exits immediately before notarization or success output")


if __name__ == "__main__":
    main()
