#!/usr/bin/env python3
"""Windows NSIS installer helpers for the macOS cross build (release-macos.sh).

`loader-config --target <triple>`
    Print a `cargo tauri build --config` JSON that ships WebView2Loader.dll next to cview-app.exe.
    On GNU-ABI Windows targets `webview2-com-sys` links WebView2Loader.dll dynamically (its static
    library is MSVC-only), so the exe does not start without the DLL beside it. tauri-bundler adds the
    DLL by itself only when the target name ends with `-gnu`; `aarch64-pc-windows-gnullvm` is left out.
    The DLL comes from the exact `webview2-com-sys` package Cargo.lock resolves for that target.

`check --installer <setup.exe> --machine <0x8664|0xaa64>`
    Open the built installer, take cview-app.exe and every DLL installed next to it, and require that
    each DLL they import (normal and delay-load) is either a Windows system DLL or installed in the same
    folder, with the expected PE machine. This is what Windows needs to start the program.
"""
import argparse
import json
import os
from pathlib import Path
import struct
import subprocess
import sys
import tempfile

REPO = Path(__file__).resolve().parent.parent
MAIN_EXE = 'cview-app.exe'
LOADER = 'WebView2Loader.dll'
# CARGO_CFG_TARGET_ARCH → webview2-com-sys 안의 폴더 이름, PE Machine.
ARCHES = {'x86_64': ('x64', 0x8664), 'aarch64': ('arm64', 0xAA64)}

# Windows 10/11 에 늘 들어 있는 DLL. 여기 없는 DLL 을 가져오면 설치 파일이 exe 옆에 넣어야 한다.
# 새 Windows API 를 쓰게 돼 검사가 멈추면, 정말 Windows 에 들어 있는 DLL 인지 확인하고 더한다.
# vcruntime140·msvcp140(VC++ 재배포), lib*.dll(MinGW 런타임), WebView2Loader.dll 은 시스템 DLL 이 아니다.
SYSTEM_DLLS = frozenset(name + '.dll' for name in '''
    advapi32 avrt bcrypt bcryptprimitives cfgmgr32 combase comctl32 comdlg32 crypt32 d2d1 d3d11 d3d12
    d3dcompiler_47 dbghelp dnsapi dwmapi dwrite dxgi gdi32 gdiplus hid imm32 iphlpapi kernel32 kernelbase
    mf mfplat mfreadwrite mpr msimg32 msvcrt mswsock ncrypt netapi32 normaliz ntdll ole32 oleacc oleaut32
    opengl32 pdh powrprof propsys psapi rpcrt4 secur32 setupapi shcore shell32 shlwapi sspicli ucrtbase
    uiautomationcore urlmon user32 userenv usp10 uxtheme version windowscodecs winhttp wininet winmm
    winspool wintrust wldap32 ws2_32 wtsapi32
'''.split()) | {'winspool.drv'}


def fail(message):
    raise SystemExit(f'✗ {message}')


def is_system_dll(name):
    n = name.lower()
    return n.startswith(('api-ms-win-', 'ext-ms-')) or n in SYSTEM_DLLS


def pe_machine(data, name='file'):
    """PE 머리의 Machine 값. PE 가 아니면 ValueError."""
    if data[:2] != b'MZ' or len(data) < 0x40:
        raise ValueError(f'{name}: PE 파일이 아닙니다')
    offset = struct.unpack_from('<I', data, 0x3C)[0]
    if data[offset:offset + 4] != b'PE\0\0' or len(data) < offset + 6:
        raise ValueError(f'{name}: PE 서명이 없습니다')
    return struct.unpack_from('<H', data, offset + 4)[0]


def parse_imports(readobj_text):
    """`llvm-readobj --coff-imports` 출력 → 가져오는 DLL 이름(일반·지연 로드, 나온 순서, 중복 없이)."""
    names, lines = [], readobj_text.splitlines()
    for i, line in enumerate(lines[:-1]):
        nxt = lines[i + 1]
        if line in ('Import {', 'DelayImport {') and nxt.startswith('  Name: '):
            name = nxt[len('  Name: '):].strip()
            if name.lower() not in (n.lower() for n in names):
                names.append(name)
    return names


def parse_listing(slt_text):
    """`7zz l -slt` 출력 → 설치 파일 안 경로 목록(보관 파일 자체의 머리 부분은 뺀다)."""
    paths, body = [], False
    for line in slt_text.splitlines():
        if line.startswith('----------'):
            body = True
        elif body and line.startswith('Path = '):
            paths.append(line[len('Path = '):])
    return paths


def install_root_files(paths):
    """$INSTDIR 바로 아래(exe 와 같은 폴더)에 설치되는 파일. NSIS 플러그인($PLUGINSDIR)·하위 폴더는 뺀다."""
    return [p for p in paths if '/' not in p and '\\' not in p and not p.startswith('$')]


def audit(root_files, machine, pe_info):
    """설치 폴더에서 exe 를 시작할 수 있는지.

    root_files: exe 와 같은 폴더에 설치되는 파일 이름.
    pe_info(name) → (Machine, 가져오는 DLL 목록). exe 와 그 옆 DLL 에만 부른다.
    돌려주는 값: (보고 줄, 문제 줄). 문제가 없어야 통과다.
    """
    report, problems = [], []
    lower = {}
    for name in root_files:
        if name.lower() in lower:
            problems.append(f'설치 파일에 {name} 이(가) 두 번 들어 있습니다')
        lower[name.lower()] = name
    if MAIN_EXE not in lower:
        return report, problems + [f'설치 파일에 {MAIN_EXE} 이(가) 없습니다']
    queue, seen = [lower[MAIN_EXE]], set()
    while queue:
        name = queue.pop(0)
        if name.lower() in seen:
            continue
        seen.add(name.lower())
        got, imports = pe_info(name)
        if got != machine:
            problems.append(f'{name} 의 PE Machine 이 {got:#06x} 입니다(기대 {machine:#06x})')
        bundled = [d for d in imports if d.lower() in lower]
        missing = [d for d in imports if d.lower() not in lower and not is_system_dll(d)]
        system = len(imports) - len(bundled) - len(missing)
        report.append(f'{name} ({got:#06x}): 시스템 DLL {system}개'
                      + (f' · 같은 폴더에 함께 설치: {", ".join(bundled)}' if bundled else ''))
        for dll in missing:
            problems.append(f'{name} 이(가) 부르는 {dll} 이(가) Windows 시스템 DLL 도 아니고 설치 폴더에도 없습니다')
        queue += [lower[d.lower()] for d in bundled]
    return report, problems


def run(cmd):
    return subprocess.run(cmd, check=True, capture_output=True, text=True).stdout


def check(args):
    installer = Path(args.installer)
    if not installer.is_file():
        fail(f'설치 파일이 없습니다: {installer}')
    machine = int(args.machine, 0)
    root = install_root_files(parse_listing(run([args.sevenzip, 'l', '-slt', str(installer)])))
    pe_names = [n for n in root if n.lower() == MAIN_EXE or n.lower().endswith('.dll')]
    with tempfile.TemporaryDirectory() as tmp:
        if pe_names:
            # -r-: 이름이 같아도 하위 폴더 파일은 꺼내지 않는다(e 는 폴더 없이 한곳에 푼다).
            run([args.sevenzip, 'e', '-y', '-r-', f'-o{tmp}', str(installer), *pe_names])

        def pe_info(name):
            path = Path(tmp) / name
            if not path.is_file():
                fail(f'{installer.name} 에서 {name} 을(를) 꺼내지 못했습니다')
            got = pe_machine(path.read_bytes(), name)
            imports = parse_imports(run([args.readobj, '--coff-imports', str(path)]))
            if not imports:
                fail(f'{name} 의 가져오기 목록을 읽지 못했습니다')
            return got, imports

        report, problems = audit(root, machine, pe_info)
    print(f'   {installer.name} — exe 옆에 설치되는 파일: {", ".join(root)}')
    for line in report:
        print(f'   {line}')
    if problems:
        fail(f'{installer.name} 로 설치한 CView 가 Windows 에서 시작되지 않습니다:\n   ' + '\n   '.join(problems))


def loader_path(metadata, arch):
    """cargo metadata 에서 이 타깃이 쓰는 webview2-com-sys 의 WebView2Loader.dll 경로."""
    folder, machine = ARCHES[arch]
    found = sorted({Path(p['manifest_path']).parent for p in metadata['packages'] if p['name'] == 'webview2-com-sys'})
    if len(found) != 1:
        fail(f'webview2-com-sys 패키지를 하나로 정하지 못했습니다: {[str(p) for p in found] or "없음"}')
    path = found[0] / folder / LOADER
    if not path.is_file():
        fail(f'{path} 이(가) 없습니다')
    got = pe_machine(path.read_bytes(), str(path))
    if got != machine:
        fail(f'{path} 의 PE Machine 이 {got:#06x} 입니다(기대 {machine:#06x})')
    return path


def loader_config(args):
    arch = args.target.split('-', 1)[0]
    if '-windows-' not in args.target or arch not in ARCHES:
        fail(f'지원하지 않는 Windows 타깃입니다: {args.target}')
    metadata = json.loads(run(['cargo', 'metadata', '--format-version', '1', '--locked',
                               '--filter-platform', args.target,
                               '--manifest-path', str(REPO / 'Cargo.toml')]))
    # 키는 원본 경로, 값은 $INSTDIR 기준 설치 경로 — exe 와 같은 폴더.
    print(json.dumps({'bundle': {'resources': {str(loader_path(metadata, arch)): LOADER}}}))


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest='command', required=True)
    p = sub.add_parser('loader-config')
    p.add_argument('--target', required=True)
    p.set_defaults(func=loader_config)
    p = sub.add_parser('check')
    p.add_argument('--installer', required=True)
    p.add_argument('--machine', required=True)
    p.add_argument('--readobj', default=os.environ.get('LLVM_READOBJ', 'llvm-readobj'))
    p.add_argument('--sevenzip', default='7zz')
    p.set_defaults(func=check)
    args = parser.parse_args()
    try:
        args.func(args)
    except ValueError as e:
        fail(str(e))


if __name__ == '__main__':
    sys.exit(main())
