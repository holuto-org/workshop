"""Check the shipped ELF's direct dependencies against Debian package metadata."""
import re
import subprocess
import sys

binary, package = sys.argv[1:]
dynamic = subprocess.check_output(["readelf", "--dynamic", binary], text=True)
sections = subprocess.check_output(["readelf", "--sections", binary], text=True)
depends = subprocess.check_output(["dpkg-deb", "-f", package, "Depends"], text=True)
assert ".debug_info" not in sections, "Shipped ELF contains debug information"
assert ".symtab" not in sections, "Shipped ELF contains a symbol table"
# Fail closed when a new direct dependency appears: add its Debian package to build.sh.
owners = {
    "libc.so.6": "libc6",
    "libm.so.6": "libc6",
    "libdl.so.2": "libc6",
    "libpthread.so.0": "libc6",
    "librt.so.1": "libc6",
    "ld-linux-x86-64.so.2": "libc6",
    "ld-linux-aarch64.so.1": "libc6",
    "libgcc_s.so.1": "libgcc-s1",
    "libfontconfig.so.1": "libfontconfig1",
}
needed = re.findall(r"\(NEEDED\).*\[(.*?)\]", dynamic)
assert needed, "ELF dependency inspection found no NEEDED entries"
declared = {part.strip().split()[0] for part in depends.split(",")}
for library in needed:
    assert library in owners, f"No Debian package mapping for NEEDED {library}"
    assert owners[library] in declared, f"Missing Depends: {owners[library]} ({library})"
print("Verified NEEDED libraries:", ", ".join(needed))
