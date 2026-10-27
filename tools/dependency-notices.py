"""Record exact dependency provenance from Cargo metadata without local paths."""
import json
from pathlib import Path
import subprocess

metadata = json.loads(subprocess.check_output(
    ["cargo", "metadata", "--locked", "--format-version", "1"], text=True, encoding="utf-8"))
packages = []
for package in metadata["packages"]:
    if package["source"] is None:
        continue
    manifest = Path(package["manifest_path"])
    vcs_path = manifest.parent / ".cargo_vcs_info.json"
    vcs = json.loads(vcs_path.read_text()) if vcs_path.exists() else None
    packages.append({
        "name": package["name"], "version": package["version"],
        "source": package["source"], "repository": package["repository"],
        "license": package["license"], "commit": vcs.get("git", {}).get("sha1") if vcs else None,
    })
Path("third-party/rust-dependencies.json").write_text(
    json.dumps(sorted(packages, key=lambda p: (p["name"], p["version"])), indent=2) + "\n")
print(f"Recorded {len(packages)} external packages")
