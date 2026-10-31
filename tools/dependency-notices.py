"""Record resolved Rust dependency provenance, including pinned path vendors."""
import json
from pathlib import Path
import subprocess

# Only these workspace-relative paths are redistributed third-party crates.
# Keep the original published VCS revision, not the local [patch] checkout.
VENDORED = {
    "blitz-dom": (
        "third-party/blitz-dom", "92b11f1bd8ab7975411328b0406a95a09445b4e7",
        "https://github.com/DioxusLabs/blitz", "MIT OR Apache-2.0",
    ),
    "blitz-paint": (
        "third-party/blitz-paint", "92b11f1bd8ab7975411328b0406a95a09445b4e7",
        "https://github.com/DioxusLabs/blitz", "MIT OR Apache-2.0",
    ),
    "anyrender_vello_cpu": (
        "third-party/anyrender_vello_cpu", "cf0f14776102ec775af61bcdcaac2ab345f4fafb",
        "https://github.com/DioxusLabs/anyrender", "MIT OR Apache-2.0",
    ),
    "vello_cpu": (
        "third-party/vello_cpu", "f3000c8d9e7a9c7e6abb09b9587238bf8b4860ff",
        "https://github.com/linebender/vello", "Apache-2.0 OR MIT",
    ),
    "parley_engine": (
        "third-party/parley-engine", "6e1143dda817360dd3a2a64f9133c6e5dfabd881",
        "https://github.com/linebender/parley", "Apache-2.0 OR MIT",
    ),
}

metadata = json.loads(subprocess.check_output(
    ["cargo", "metadata", "--locked", "--format-version", "1"],
    text=True, encoding="utf-8"))
root = Path(__file__).resolve().parent.parent
packages = []
for package in metadata["packages"]:
    manifest = Path(package["manifest_path"]).resolve()
    source = package["source"]
    if source is None:
        vendor = VENDORED.get(package["name"])
        if vendor is None or manifest != root / vendor[0] / "Cargo.toml":
            # Workspace packages are local code, not external dependencies.
            continue
        _, commit, repository, license = vendor
    else:
        vcs_path = manifest.parent / ".cargo_vcs_info.json"
        vcs = json.loads(vcs_path.read_text()) if vcs_path.exists() else None
        commit = vcs.get("git", {}).get("sha1") if vcs else None
        repository = package["repository"]
        license = package["license"]
    packages.append({
        "name": package["name"], "version": package["version"],
        "source": source, "repository": repository,
        "license": license, "commit": commit,
    })
Path("third-party/rust-dependencies.json").write_text(
    json.dumps(sorted(packages, key=lambda p: (p["name"], p["version"])), indent=2)
    + "\n")
print(f"Recorded {len(packages)} external packages")
