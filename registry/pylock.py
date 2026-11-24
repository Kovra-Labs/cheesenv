#!/usr/bin/env python3
"""Hash-locked, cross-platform pip requirements for Python-based modules.

    python3.12 registry/pylock.py pgadmin4==9.19 --python 3.12 --module pgadmin

Resolves the requirement for every Stackyard platform with `pip install
--dry-run --report` (wheels only, nothing is installed or executed), merges
the results into one requirements file with environment markers and SHA-256
hashes, and writes it into `registry/modules/<module>.json` under
`setup.files` (`requirements.txt` + `build.txt`).

Packages that ship only an sdist are resolved through a metadata-only stub
wheel (built from the sdist's PKG-INFO, never by running its setup.py) and
locked to the sdist hash. Their build backend (setuptools) is locked in
`build.txt` so the module can install it first and build with
`--no-build-isolation`: pip never fetches an unhashed build dependency.
"""
import argparse
import hashlib
import io
import json
import os
import re
import subprocess
import sys
import tarfile
import tempfile
import urllib.request
import zipfile

from pip._vendor.packaging.requirements import Requirement
from pip._vendor.packaging.utils import canonicalize_name
from pip._vendor.packaging.version import Version

HERE = os.path.dirname(os.path.abspath(__file__))


def manylinux(arch, top=28):
    tags = [f"manylinux_2_{m}_{arch}" for m in range(top, 16, -1)]
    return tags + [f"manylinux2014_{arch}", f"linux_{arch}"]


def macos(archs, majors):
    tags = []
    for major, minors in majors:
        for minor in minors:
            tags += [f"macosx_{major}_{minor}_{a}" for a in archs]
    return tags


PLATFORMS = {
    "linux-x64": (manylinux("x86_64"), 'sys_platform == "linux" and platform_machine == "x86_64"'),
    "linux-arm64": (manylinux("aarch64"), 'sys_platform == "linux" and platform_machine == "aarch64"'),
    "macos-x64": (
        macos(["x86_64", "universal2", "intel"], [(13, [0]), (12, [0]), (11, [0]), (10, range(15, 8, -1))]),
        'sys_platform == "darwin" and platform_machine == "x86_64"',
    ),
    "macos-arm64": (
        macos(["arm64", "universal2"], [(14, [0]), (13, [0]), (12, [0]), (11, [0]), (10, [15, 9])]),
        'sys_platform == "darwin" and platform_machine == "arm64"',
    ),
    "windows-x64": (["win_amd64"], 'sys_platform == "win32"'),
}

MISSING = re.compile(r"No matching distribution found for ([^\s]+)")
DEPENDS = re.compile(r" depends on ([^\s]+)")


def has_wheel(spec):
    req = Requirement(spec)
    releases = pypi(req.name)["releases"]
    return any(
        f["packagetype"] == "bdist_wheel"
        for v, files in releases.items()
        if req.specifier.contains(v)
        for f in files
    )


def pypi(name, version=None):
    path = f"{name}/{version}/json" if version else f"{name}/json"
    with urllib.request.urlopen(f"https://pypi.org/pypi/{path}", timeout=60) as r:
        return json.load(r)


def sdist_of(spec):
    """Newest release matching `spec` that has an sdist: (name, version, url, sha256)."""
    req = Requirement(spec)
    data = pypi(req.name)
    for v in sorted(data["releases"], key=Version, reverse=True):
        if Version(v).is_prerelease or not req.specifier.contains(v):
            continue
        for f in data["releases"][v]:
            if f["packagetype"] == "sdist" and not f.get("yanked"):
                return data["info"]["name"], v, f["url"], f["digests"]["sha256"]
    raise SystemExit(f"no sdist or wheel for {spec}")


def read_sdist(url, sha256):
    with urllib.request.urlopen(url, timeout=120) as r:
        blob = r.read()
    if hashlib.sha256(blob).hexdigest() != sha256:
        raise SystemExit(f"hash mismatch for {url}")
    pkg_info = requires = pyproject = ""
    with tarfile.open(fileobj=io.BytesIO(blob)) as tar:
        for m in tar.getmembers():
            parts = m.name.split("/")
            if not m.isfile():
                continue
            text = lambda: tar.extractfile(m).read().decode("utf-8", "replace")
            if len(parts) == 2 and parts[1] == "PKG-INFO":
                pkg_info = text()
            elif len(parts) == 2 and parts[1] == "pyproject.toml":
                pyproject = text()
            elif parts[-1] == "requires.txt" and parts[-2].endswith(".egg-info"):
                requires = text()
    deps = [l for l in pkg_info.splitlines() if l.startswith("Requires-Dist:")]
    if not deps and requires:
        if "[" in requires:
            raise SystemExit(f"{url}: extras/markers in requires.txt need manual handling")
        deps = [f"Requires-Dist: {l.strip()}" for l in requires.splitlines() if l.strip()]
    build = ["setuptools"]
    m = re.search(r"\[build-system\][^\[]*?requires\s*=\s*\[([^\]]*)\]", pyproject, re.S)
    if m:
        build = re.findall(r"['\"]([^'\"]+)['\"]", m.group(1))
    backend = re.search(r"build-backend\s*=\s*['\"]([^'\"]+)", pyproject)
    if backend and not backend.group(1).startswith("setuptools"):
        raise SystemExit(f"{url}: unsupported build backend {backend.group(1)}")
    return deps, build


def write_stub(stubs, name, version, deps):
    """A metadata-only wheel so `--platform` resolution can see the sdist."""
    dist = f"{canonicalize_name(name).replace('-', '_')}-{version}"
    meta = f"Metadata-Version: 2.1\nName: {name}\nVersion: {version}\n" + "".join(d + "\n" for d in deps)
    wheel = "Wheel-Version: 1.0\nGenerator: stackyard-pylock\nRoot-Is-Purelib: true\nTag: py3-none-any\n"
    files = {f"{dist}.dist-info/METADATA": meta, f"{dist}.dist-info/WHEEL": wheel}
    record = "".join(f"{p},,\n" for p in files) + f"{dist}.dist-info/RECORD,,\n"
    files[f"{dist}.dist-info/RECORD"] = record
    with zipfile.ZipFile(os.path.join(stubs, f"{dist}-py3-none-any.whl"), "w") as z:
        for p, c in files.items():
            z.writestr(p, c)


def resolve(reqs, platform_tags, python, stubs, work):
    report = os.path.join(work, "report.json")
    cmd = [
        sys.executable, "-m", "pip", "install", "--dry-run", "--ignore-installed",
        "--disable-pip-version-check", "--no-cache-dir", "--report", report,
        "--only-binary", ":all:", "--python-version", python, "--implementation", "cp",
        "--abi", "cp" + python.replace(".", ""), "--target", os.path.join(work, "target"),
        "--find-links", stubs,
    ]
    for t in platform_tags:
        cmd += ["--platform", t]
    p = subprocess.run(cmd + reqs, capture_output=True, text=True)
    if p.returncode != 0:
        out = p.stdout + p.stderr
        m = MISSING.search(out)
        if m:
            return None, m.group(1)
        # A conflict report can hide a dependency that simply has no wheels.
        for spec in DEPENDS.findall(out):
            if not has_wheel(spec):
                return None, spec
        raise SystemExit(out[-4000:])
    with open(report) as f:
        return json.load(f)["install"], None


def lock_platforms(reqs, python, stubs, work, sdists, platforms):
    """{platform: [(name, version, url, sha256)]}, adding stubs for sdist-only packages."""
    out = {}
    for key in platforms:
        tags = PLATFORMS[key][0]
        while True:
            installs, missing = resolve(reqs, tags, python, stubs, work)
            if installs is not None:
                break
            name, version, url, sha = sdist_of(missing)
            if canonicalize_name(name) in sdists:
                raise SystemExit(f"{key}: {missing} still unresolved")
            print(f"  sdist-only: {name}=={version}", file=sys.stderr)
            deps, build = read_sdist(url, sha)
            write_stub(stubs, name, version, deps)
            sdists[canonicalize_name(name)] = (name, version, url, sha, build)
        rows = []
        for i in installs:
            name, version = i["metadata"]["name"], i["metadata"]["version"]
            cname = canonicalize_name(name)
            if cname in sdists:
                _, _, url, sha, _ = sdists[cname]
            else:
                url = i["download_info"]["url"]
                hashes = i["download_info"].get("archive_info", {}).get("hashes", {})
                sha = hashes.get("sha256")
                if not sha:
                    raise SystemExit(f"{key}: no sha256 for {url}")
            rows.append((cname, version, url, sha))
        out[key] = rows
        print(f"{key}: {len(rows)} packages", file=sys.stderr)
    return out


def render(per_platform, header):
    platforms = set(per_platform)
    entries = {}
    for key, rows in per_platform.items():
        for name, version, _, sha in rows:
            e = entries.setdefault((name, version), {"platforms": set(), "hashes": set()})
            e["platforms"].add(key)
            e["hashes"].add(sha)
    lines = [header]
    for (name, version), e in sorted(entries.items()):
        marker = ""
        if e["platforms"] != platforms:
            marker = " ; " + " or ".join(f"({PLATFORMS[k][1]})" for k in sorted(e["platforms"]))
        hashes = " ".join(f"--hash=sha256:{h}" for h in sorted(e["hashes"]))
        lines.append(f"{name}=={version}{marker} {hashes}")
    return "\n".join(lines) + "\n"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("requirement", nargs="+")
    ap.add_argument("--python", required=True, help="interpreter version of the module, e.g. 3.12")
    ap.add_argument("--module", required=True, help="registry/modules/<module>.json to update")
    a = ap.parse_args()
    # pip evaluates environment markers (python_version < "3.14") with the
    # running interpreter, not --python-version: they must be the same.
    running = "%d.%d" % sys.version_info[:2]
    if running != a.python:
        raise SystemExit(f"run with Python {a.python} (this is {running}), e.g. python{a.python} registry/pylock.py ...")

    path = os.path.join(HERE, "modules", f"{a.module}.json")
    with open(path) as f:
        manifest = json.load(f)
    # Lock exactly the platforms the module ships an interpreter for.
    platforms = sorted({k for v in manifest["versions"] for k in v["assets"]})
    unknown = set(platforms) - set(PLATFORMS)
    if unknown:
        raise SystemExit(f"no wheel tags known for {sorted(unknown)}")

    with tempfile.TemporaryDirectory() as work:
        stubs = os.path.join(work, "stubs")
        os.makedirs(stubs)
        sdists = {}
        runtime = lock_platforms(a.requirement, a.python, stubs, work, sdists, platforms)
        build_reqs = sorted({r for *_, build in sdists.values() for r in build}) or ["setuptools"]
        build = lock_platforms(build_reqs, a.python, stubs, work, {}, platforms)

    note = "# Generated by registry/pylock.py; do not edit."
    req_txt = render(runtime, f"{note}\n# {' '.join(a.requirement)} (Python {a.python})")
    build_txt = render(build, f"{note}\n# build backend for: {', '.join(sorted(v[0] for v in sdists.values())) or '-'}")

    files = manifest.setdefault("setup", {}).setdefault("files", {})
    files["requirements.txt"] = req_txt
    files["build.txt"] = build_txt
    with open(path, "w") as f:
        f.write(json.dumps(manifest, indent=2, ensure_ascii=False) + "\n")
    print(f"updated {path}: {req_txt.count(chr(10)) - 2} runtime + {build_txt.count(chr(10)) - 2} build packages")


if __name__ == "__main__":
    main()
