"""Copy the selected GStreamer plugins and their PE import closure, plus notices.

No system DLLs or unrelated SDK plugins are bundled. This uses Python's standard
library so a fresh Windows source checkout needs no extra packaging packages.
"""
import argparse
import ast
import hashlib
import json
import os
import re
from pathlib import Path
import shutil
import struct

# These are Windows10/11 components, not any DLL that happens to be installed on
# the build machine. In particular, the Visual C++ runtime must come from Redist.
OS_DLLS = set("advapi32 bcrypt cfgmgr32 combase comctl32 comdlg32 crypt32 cryptbase cryptnet cryptui d3d11 d3d12 d3dcompiler_47 dbghelp dcomp dhcpcsvc dinput8 dnsapi dsound dwmapi dxgi dxguid dwrite gdi32 hid imm32 iphlpapi kernel32 kernelbase mmdevapi mpr msimg32 mswsock ncrypt netapi32 ntdll ole32 oleacc oleaut32 opengl32 powrprof propsys psapi rpcrt4 secur32 setupapi shell32 shcore shlwapi sspicli ucrtbase user32 userenv usp10 uxtheme version winhttp wininet winmm winspool wintrust wldap32 ws2_32 wtsapi32 xinput1_4".split())
OS_DLLS.update(["bcryptprimitives", "d3d9", "uiautomationcore"])


def imports(path):
    data = path.read_bytes()
    if data[:2] != b"MZ":
        raise ValueError(f"Not a PE image: {path.name}")
    pe = struct.unpack_from("<I", data, 0x3C)[0]
    if data[pe:pe + 4] != b"PE\0\0":
        raise ValueError(f"Bad PE signature: {path.name}")
    count, optional_size = struct.unpack_from("<H12xH", data, pe + 6)
    optional = pe + 24
    magic = struct.unpack_from("<H", data, optional)[0]
    directory = optional + {0x20B: 112, 0x10B: 96}[magic]
    sections = []
    for i in range(count):
        section = optional + optional_size + i * 40
        size, rva, raw_size, raw = struct.unpack_from("<IIII", data, section + 8)
        sections.append((rva, max(size, raw_size), raw))

    def offset(rva):
        for base, size, raw in sections:
            if base <= rva < base + size:
                return raw + rva - base
        raise ValueError(f"Unmapped PE RVA in {path.name}")

    def name(rva):
        start = offset(rva)
        return data[start:data.index(b"\0", start)].decode("ascii")

    result = set()
    # Normal imports use RVA names. All selected native binaries are PE32+;
    # the optional delay-import table also uses RVAs when its attributes bit0 is1.
    for index, stride, name_index in [(1, 20, 3), (13, 32, 1)]:
        rva, size = struct.unpack_from("<II", data, directory + index * 8)
        if not rva or not size:
            continue
        start = offset(rva)
        for pos in range(start, start + size, stride):
            descriptor = struct.unpack_from("<" + "I" * (stride // 4), data, pos)
            if not any(descriptor):
                break
            if index == 13 and not descriptor[0] & 1:
                raise ValueError(f"Unsupported VA delay import in {path.name}")
            result.add(name(descriptor[name_index]))
    return result


def bundle(source, sdk, crt, output, executable):
    plugins = []
    for platform in ["common", "windows"]:
        text = (source / f"components/servo/gstreamer_plugin_lists/{platform}.rs.in").read_text()
        plugins.extend(ast.literal_eval("\n".join(line for line in text.splitlines() if not line.strip().startswith("//"))))
    available = {p.name.lower(): p for p in (sdk / "bin").glob("*.dll")}
    available.update({p.name.lower(): p for p in (sdk / "lib/gstreamer-1.0").glob("*.dll")})
    available.update({p.name.lower(): p for p in crt.glob("*.dll")})
    if not (crt / "vcruntime140.dll").is_file():
        raise ValueError("Pass the licensed x64 Microsoft.VC143.CRT Redist folder, not System32")
    queue = [executable] + list(output.glob("*.dll")) + [available[name.lower() + ".dll"] for name in plugins]
    selected = {}
    system = Path(os.environ["SystemRoot"]) / "System32"
    while queue:
        path = queue.pop()
        key = path.name.lower()
        if key in selected:
            continue
        selected[key] = path
        for dependency in imports(path):
            dll = dependency.lower()
            if (output / dependency).is_file():
                queue.append(output / dependency)
            elif dll in available:
                queue.append(available[dll])
            elif dll.startswith(("api-ms-", "ext-ms-")) or (dll.removesuffix(".dll") in OS_DLLS and (system / dependency).is_file()):
                continue
            else:
                raise ValueError(f"Missing dependency {dependency} required by {path.name}")
    records = {}
    for key, path in sorted(selected.items()):
        if path == executable or path.parent == output:
            continue
        target = output / path.name
        shutil.copy2(path, target)
        records[path.name] = hashlib.sha256(target.read_bytes()).hexdigest()
    notices = output / "licenses/gstreamer"
    shutil.copytree(sdk / "share/licenses", notices, dirs_exist_ok=True)
    version = re.search(r"^Version:\s*(\S+)", (sdk / "lib/pkgconfig/gstreamer-1.0.pc").read_text(), re.M)
    if not version:
        raise ValueError("GStreamer SDK version metadata is missing")
    (output / "media-runtime.json").write_text(json.dumps({"gstreamer": version.group(1), "plugins": plugins, "sha256": records}, indent=2), encoding="utf-8")
    print(f"Bundled {len(records)} media DLLs with their dependency notices.")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    for name in ["source", "sdk", "crt", "output", "executable"]:
        parser.add_argument("--" + name, type=Path, required=True)
    args = parser.parse_args()
    bundle(args.source.resolve(), args.sdk.resolve(), args.crt.resolve(), args.output.resolve(), args.executable.resolve())
