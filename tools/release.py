"""Verify native CI binaries and package all supported release architectures."""
import argparse
import hashlib
import io
import json
from pathlib import Path
import re
import shutil
import struct
import subprocess
import tarfile
import tomllib
import zipfile

ROOT = Path(__file__).resolve().parents[1]
PLATFORMS = (
    ("windows", "x86_64", "Windows-X64", "x86_64-pc-windows-msvc", "windows-2022"),
    ("linux", "x86_64", "Linux-X64", "x86_64-unknown-linux-gnu", "ubuntu-24.04"),
    ("windows", "arm64", "Windows-ARM64", "aarch64-pc-windows-msvc", "windows-11-arm"),
    ("linux", "arm64", "Linux-ARM64", "aarch64-unknown-linux-gnu", "ubuntu-24.04-arm"),
)


def version():
    with (ROOT / "Cargo.toml").open("rb") as source:
        return tomllib.load(source)["package"]["version"]


def digest(path):
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def verify_binary(path, operating_system, architecture):
    with path.open("rb") as source:
        data = source.read(64)
        if operating_system == "windows":
            if data[:2] != b"MZ" or len(data) < 64:
                raise ValueError(f"Not a PE executable: {path}")
            source.seek(struct.unpack_from("<I", data, 0x3c)[0])
            header = source.read(6)
            machine = 0xAA64 if architecture == "arm64" else 0x8664
            if len(header) != 6 or header[:4] != b"PE\0\0" or struct.unpack_from("<H", header, 4)[0] != machine:
                raise ValueError(f"Wrong PE architecture ({architecture} expected): {path}")
        else:
            machine = 183 if architecture == "arm64" else 62
            if len(data) < 20 or data[:6] != b"\x7fELF\x02\x01" or struct.unpack_from("<H", data, 18)[0] != machine:
                raise ValueError(f"Wrong ELF architecture ({architecture} expected): {path}")


def run_binary(path, *arguments):
    return subprocess.run([str(path), *arguments], check=True, capture_output=True, timeout=60).stdout


def verify_native(args):
    os_name, arch, _, target, _ = next(p for p in PLATFORMS if p[3] == args.target)
    info = subprocess.check_output(["rustc", "-vV"], text=True)
    if f"host: {target}\n" not in info.replace("\r\n", "\n"):
        raise ValueError(f"Expected native Rust host {target}: {info}")
    directory = args.directory.resolve()
    extension = ".exe" if os_name == "windows" else ""
    for stem in ("yyte", "markdownViewer"):
        path = directory / (stem + extension)
        verify_binary(path, os_name, arch)
        actual = run_binary(path, "--version").decode().strip()
        if actual != f"{stem} {version()}":
            raise ValueError(f"Wrong version: {actual}")
        run_binary(path, "--help")
    viewer = directory / ("markdownViewer" + extension)
    demo = str(ROOT / "demo/markdown-preview.md")
    plain = run_binary(viewer, "--plain", demo)
    for marker in (b"40x28", b"Connections:", "╞".encode(), "║ Terminal Markdown preview".encode()):
        if marker not in plain:
            raise ValueError(f"Missing preview marker {marker!r}")
    if b"\x1b" in plain:
        raise ValueError("Plain output contains terminal escapes")
    colored = run_binary(viewer, "--print", "--color", demo)
    for rgb in (b"198;120;221", b"152;195;121", b"127;135;145"):
        if b"\x1b[38;2;" + rgb + b"m" not in colored:
            raise ValueError(f"Missing syntax color {rgb!r}")
    print(f"Verified native {target}: version, help, spacing, table, images, Mermaid, syntax colors")


def package(args):
    if not re.fullmatch(r"[0-9a-f]{40}", args.commit):
        raise ValueError("--commit must be a full SHA")
    if not re.fullmatch(r"v[0-9]+(?:\.[0-9]+){1,2}", args.version):
        raise ValueError("--version must be a release tag such as v0.5")
    sources = subprocess.check_output(["git", "archive", "--format=tar", args.commit,
                                      "Cargo.toml", "README.md", "COPYING", "docs", "demo"], cwd=ROOT)
    with tarfile.open(fileobj=io.BytesIO(sources)) as source:
        members = {m.name: source.extractfile(m).read() for m in source.getmembers() if m.isfile()}
    package_version = tomllib.loads(members.pop("Cargo.toml").decode())["package"]["version"]
    if args.version.lstrip("v") not in (package_version, package_version.removesuffix(".0")):
        raise ValueError("Tag and committed package version disagree")
    args.output.mkdir(parents=True, exist_ok=True)
    archives = []
    for os_name, arch, artifact, target, runner in PLATFORMS:
        stage = args.output / f"yyterminal-editor-{args.version}-{os_name}-{arch}"
        stage.mkdir()  # Fail on stale packaging directories; never reuse another build.
        binaries = {}
        extension = ".exe" if os_name == "windows" else ""
        for stem in ("yyte", "markdownViewer"):
            name = stem + extension
            path = args.artifacts / ("yyte-" + artifact) / "target/release" / name
            verify_binary(path, os_name, arch)
            shutil.copy2(path, stage / name)
            binaries[name] = digest(path)
        for name, content in members.items():
            path = stage / name
            if not path.resolve().is_relative_to(stage.resolve()):
                raise ValueError(f"Unsafe archive path: {name}")
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(content)
        metadata = dict(release=args.version, package_version=package_version, source_commit=args.commit,
                        platform=f"{os_name}-{arch}", target=target, ci_runner=runner,
                        ci_run=args.ci_run, binaries=binaries)
        (stage / "BUILDINFO.json").write_text(json.dumps(metadata, indent=2) + "\n", encoding="utf-8")
        if os_name == "windows":
            archive = stage.with_name(stage.name + ".zip")
            with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as output:
                for path in sorted(stage.rglob("*")):
                    if path.is_file():
                        output.write(path, path.relative_to(args.output))
            with zipfile.ZipFile(archive) as result:
                if result.testzip():
                    raise ValueError(f"ZIP checksum failure: {archive}")
        else:
            archive = stage.with_name(stage.name + ".tar.gz")
            with tarfile.open(archive, "w:gz", compresslevel=9) as output:
                for path in [stage, *sorted(stage.rglob("*"))]:
                    info = output.gettarinfo(str(path), path.relative_to(args.output).as_posix())
                    info.uid = info.gid = 0
                    info.uname = info.gname = ""
                    info.mode = 0o755 if path.is_dir() or path.name in binaries else 0o644
                    if path.is_file():
                        with path.open("rb") as content:
                            output.addfile(info, content)
                    else:
                        output.addfile(info)
        archives.append(archive)
        print(f"Packaged {archive.name} ({target})")
    (args.output / "SHA256SUMS.txt").write_text(
        "".join(f"{digest(path)}  {path.name}\n" for path in archives), encoding="ascii")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    verify = commands.add_parser("verify", help="Verify and execute native CI binaries")
    verify.add_argument("--target", choices=[p[3] for p in PLATFORMS], required=True)
    verify.add_argument("--directory", type=Path, default=ROOT / "target/release")
    verify.set_defaults(action=verify_native)
    release = commands.add_parser("package", help="Package all four CI artifacts")
    for name in ("artifacts", "output"):
        release.add_argument("--" + name, type=Path, required=True)
    for name in ("version", "commit", "ci-run"):
        release.add_argument("--" + name, required=True)
    release.set_defaults(action=package)
    args = parser.parse_args()
    args.action(args)


if __name__ == "__main__":
    main()
