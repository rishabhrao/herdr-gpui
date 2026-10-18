#!/usr/bin/env python3
"""Install the pinned Windows Herdr daemon for the native CI tests.

Usage: python install-herdr-windows.py DESTINATION_DIR

Downloads Herdr's x86_64 Windows release, checks its SHA-256, extracts it into
DESTINATION_DIR, and prints the path of herdr.exe for HERDR_TEST_BINARY. Herdr
publishes no ARM64 Windows build; Windows on ARM runs this one under x64
emulation, and the archive carries an ARM64 OpenConsole for its terminals.
Emulated there, its panes echo typed text but never act on Enter, so CI runs
the live-daemon GUI test on x64 only.

The digest is the one GitHub records for this exact asset; bump the version
and digest together. Nothing here discovers or touches a user's own Herdr.
Pure standard library, for the Windows runner.
"""
import hashlib
from pathlib import Path
import shutil
import sys
import tempfile
import urllib.request
import zipfile

VERSION = "0.9.5"
SHA256 = "d2c6c9c971c43a0bfdde8c692a9a1a7a45c3e4807cd01ae45662b4734fac1cbf"
URL = f"https://github.com/herdrdev/herdr/releases/download/v{VERSION}/herdr-windows-x86_64.zip"
LIMIT = 64 * 1024 * 1024


def install(destination):
    destination = Path(destination)
    if destination.exists() and any(destination.iterdir()):
        raise ValueError(f"{destination} is not empty")
    with tempfile.TemporaryDirectory(prefix="herdr-download-") as temp:
        archive = Path(temp) / "herdr.zip"
        digest = hashlib.sha256()
        with urllib.request.urlopen(URL, timeout=120) as response, archive.open("xb") as output:
            total = 0
            while chunk := response.read(1024 * 1024):
                total += len(chunk)
                if total > LIMIT:
                    raise ValueError("Herdr archive exceeds the size limit")
                digest.update(chunk)
                output.write(chunk)
        if digest.hexdigest() != SHA256:
            raise ValueError(f"Herdr archive digest mismatch: {digest.hexdigest()}")
        with zipfile.ZipFile(archive) as zipped:
            for member in zipped.infolist():
                # Refuse anything that would land outside the destination.
                parts = Path(member.filename).parts
                if member.filename.startswith(("/", "\\")) or ".." in parts or ":" in member.filename:
                    raise ValueError(f"unsafe archive member: {member.filename}")
            destination.mkdir(parents=True, exist_ok=True)
            zipped.extractall(destination)
    binary = destination / "herdr.exe"
    if not binary.is_file():
        shutil.rmtree(destination, ignore_errors=True)
        raise ValueError("Herdr archive has no herdr.exe")
    return binary.resolve()


def main():
    if len(sys.argv) != 2:
        sys.exit(__doc__.strip().splitlines()[2])
    try:
        print(install(sys.argv[1]))
    except (OSError, ValueError) as error:
        sys.exit(f"Herdr install failed: {error}")


if __name__ == "__main__":
    main()
