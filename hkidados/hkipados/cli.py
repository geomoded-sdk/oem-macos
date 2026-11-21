"""A linha de comando do HkiPadOS."""

from __future__ import annotations

import argparse
import json
import os
import sys
import urllib.request

from . import __version__, aea, analyze, apfsx, compat, im4p, macho
from .manifest import BuildManifest
from .source import open_source
from .zipx import ZipArchive, human_size

IPSWME = "https://api.ipsw.me/v4/device/{device}?type=ipsw"


def _json(data) -> str:
    return json.dumps(data, indent=2, ensure_ascii=False)


def _write(path: str | None, text: str) -> None:
    if path:
        os.makedirs(os.path.dirname(os.path.abspath(path)), exist_ok=True)
        with open(path, "w", encoding="utf-8") as handle:
            handle.write(text)
    print(text)


# --------------------------------------------------------------------------- fetch

def cmd_fetch(args) -> int:
    if args.url:
        url, expected = args.url, None
    else:
        with urllib.request.urlopen(IPSWME.format(device=args.device), timeout=60) as response:
            catalogue = json.load(response)
        match = None
        for firmware in catalogue.get("firmwares", []):
            if firmware["buildid"].lower() == args.build.lower():
                match = firmware
                break
        if match is None:
            print(f"fetch: {args.build} não existe para {args.device}", file=sys.stderr)
            return 2
        url = match["url"]
        expected = {"size": match["filesize"], "sha256": match["sha256sum"],
                    "version": match["version"], "build": match["buildid"],
                    "signed": match.get("signed")}
        print(f"{args.device} {expected['version']} ({expected['build']}): {url}")
        print(f"{expected['size']:,} bytes, assinado={expected['signed']}")
    os.makedirs(args.out, exist_ok=True)
    target = os.path.join(args.out, url.rsplit("/", 1)[-1])
    if os.path.exists(target) and expected and os.path.getsize(target) == expected["size"]:
        print(f"já está completo: {target}")
    else:
        _download(url, target, expected["size"] if expected else None)
    if expected:
        size = os.path.getsize(target)
        if size != expected["size"]:
            print(f"fetch: tamanho {size:,} != {expected['size']:,}", file=sys.stderr)
            return 1
        print("tamanho confere. calculando SHA-256…")
        digest = analyze.sha256_file(target)
        if digest != expected["sha256"]:
            print(f"fetch: SHA-256 {digest} != {expected['sha256']}", file=sys.stderr)
            return 1
        print(f"SHA-256 confere: {digest}")
    print(target)
    return 0


def _download(url: str, target: str, size: int | None) -> None:
    part = target + ".part"
    have = os.path.getsize(part) if os.path.exists(part) else 0
    request = urllib.request.Request(url, headers={"User-Agent": "HkiPadOS/0.1"})
    if have:
        request.add_header("Range", f"bytes={have}-")
    with urllib.request.urlopen(request, timeout=120) as response, open(part, "ab") as out:
        total = size or int(response.headers.get("Content-Length", 0)) + have
        while True:
            block = response.read(1 << 20)
            if not block:
                break
            out.write(block)
            have += len(block)
            if total:
                print(f"\r  {have / total:6.1%}  {human_size(have)} / {human_size(total)}",
                      end="", flush=True)
    print()
    os.replace(part, target)


# --------------------------------------------------------------------------- verify

def cmd_verify(args) -> int:
    path = args.path
    size = os.path.getsize(path)
    print(f"{path}: {size:,} bytes ({human_size(size)})")
    digest = analyze.sha256_file(path)
    print(f"sha256 {digest}")
    ok = True
    if args.expect_sha256 and digest != args.expect_sha256:
        print("sha256 NÃO confere", file=sys.stderr)
        ok = False
    source = open_source(path)
    archive = ZipArchive(source)
    print(f"ZIP64={archive.zip64} membros={archive.n_total} "
          f"diretório central={archive.cd_size:,} bytes")
    manifest = BuildManifest.load(archive.read("BuildManifest.plist"))
    print(f"iPadOS {manifest.product_version} ({manifest.product_build}); "
          f"{len(manifest.identities)} identidades")
    if args.crc:
        for index, name in enumerate(archive.order, 1):
            archive.crc_check(name)
            print(f"\r  CRC {index}/{archive.n_total}", end="", flush=True)
        print("\n  todos os CRCs conferem")
    source.close()
    return 0 if ok else 1


# --------------------------------------------------------------------------- ls

def cmd_ls(args) -> int:
    if args.long:
        print(analyze.describe(args.path))
        return 0
    source = open_source(args.path)
    archive = ZipArchive(source)
    for name in archive.order:
        member = archive.members[name]
        print(f"{human_size(member.usize):>10}  {name}")
    source.close()
    return 0


# --------------------------------------------------------------------------- extract

def cmd_extract(args) -> int:
    written = analyze.extract_members(args.path, args.out, args.names)
    for name, size in written.items():
        print(f"{name}: {size:,} bytes")
    return 0


# --------------------------------------------------------------------------- analyze

def cmd_analyze(args) -> int:
    report = analyze.analyze_ipsw(args.path, product_type=args.device,
                                  variant=args.variant, do_hash=args.hash,
                                  decompress=not args.no_decompress,
                                  extract_dir=args.extract_images)
    text = _json(report)
    if args.json:
        os.makedirs(os.path.dirname(os.path.abspath(args.json)), exist_ok=True)
        with open(args.json, "w", encoding="utf-8") as handle:
            handle.write(text + "\n")
    if args.quiet:
        kernel = report.get("kernel", {})
        print(f"iPadOS {report['target']['version']} ({report['target']['build']}) "
              f"para {report['target']['device']} / {report['target']['board']}")
        print(f"kernel: {kernel.get('description')} — {kernel.get('arch')} "
              f"{kernel.get('cpusubtype')} — {kernel.get('darwin_version')}")
        for image in report.get("images", []):
            print(f"imagem {image['component']:22} {image['kind']:6} "
                  f"{image['size']:>13,}  {image['path']}")
    else:
        print(text)
    return 0


# --------------------------------------------------------------------------- userland

def cmd_userland(args) -> int:
    source = open_source(args.path)
    archive = ZipArchive(source)
    manifest = BuildManifest.load(archive.read("BuildManifest.plist"))
    identity = manifest.identity(args.device, "Erase")
    wanted = {"Cryptex1,AppOS": "app-cryptex", "RestoreRamDisk": "restore-ramdisk"}
    os.makedirs(args.out, exist_ok=True)
    report = {"source": args.path, "trees": {}}
    for component, folder in wanted.items():
        member_path = identity.path_of(component)
        if not member_path or member_path not in archive.members:
            continue
        image = os.path.join(args.out, os.path.basename(member_path))
        print(f"extraindo {member_path} ({human_size(archive.members[member_path].usize)})…")
        archive.extract(member_path, image)
        tree = os.path.join(args.out, folder)
        print(f"abrindo com 7-Zip em {tree}…")
        result = apfsx.extract_with_7z(image, tree)
        inventory = analyze.inventory_tree(tree)
        inventory.update(result)
        inventory["seven_zip"] = apfsx.seven_zip_info(image)
        inventory["image"] = image
        report["trees"][component] = inventory
        print(f"  {inventory['files']} arquivos, {human_size(inventory['bytes'])}, "
              f"arquiteturas {inventory['architectures']}")
        decmpfs = inventory.get("decmpfs", {})
        if decmpfs.get("streams"):
            print(f"  decmpfs: {decmpfs['streams']} fluxos, tipos {decmpfs['types']}, "
                  f"contêineres {decmpfs['containers']}, "
                  f"{decmpfs['fat_headers_in_literal_prefix']} com Mach-O universal no prefixo")
        for launchd in inventory["launchd"][:5]:
            print(f"  launchd: {launchd}")
        if not args.keep:
            os.unlink(image)
    # entitlements de um binário conhecido, como prova de que os Mach-O são reais
    launchd_paths = report["trees"].get("RestoreRamDisk", {}).get("launchd", [])
    if launchd_paths:
        with open(launchd_paths[0], "rb") as handle:
            data = handle.read(min(os.path.getsize(launchd_paths[0]), 4 << 20))
        parsed = macho.parse(data)
        report["launchd_macho"] = parsed.as_dict()
        report["launchd_macho"]["path"] = launchd_paths[0]
        print(f"  launchd: {parsed.arch}, {parsed.platform} {parsed.minos}, "
              f"{len(parsed.entitlements_xml or '')} bytes de entitlements")
    if args.json:
        os.makedirs(os.path.dirname(os.path.abspath(args.json)), exist_ok=True)
        with open(args.json, "w", encoding="utf-8") as handle:
            json.dump(report, handle, indent=2, ensure_ascii=False)
            handle.write("\n")
    source.close()
    return 0


# --------------------------------------------------------------------------- compat

def cmd_compat(args) -> int:
    report = analyze.analyze_ipsw(args.path, product_type=args.device,
                                  decompress=not args.no_decompress)
    if args.userland:
        report["userland"] = {}
        for entry in os.scandir(args.userland):
            if entry.is_dir():
                report["userland"] = analyze.inventory_tree(entry.path)
                break
    results = compat.evaluate(report)
    report["compat"] = results
    report["compat_summary"] = compat.summarize(results)
    if args.md:
        os.makedirs(os.path.dirname(os.path.abspath(args.md)), exist_ok=True)
        with open(args.md, "w", encoding="utf-8") as handle:
            handle.write(compat.render_markdown(results) + "\n")
    if args.json:
        os.makedirs(os.path.dirname(os.path.abspath(args.json)), exist_ok=True)
        with open(args.json, "w", encoding="utf-8") as handle:
            handle.write(_json(report) + "\n")
    print(compat.render_markdown(results))
    print()
    print("resumo:", ", ".join(f"{k}={v}" for k, v in sorted(report["compat_summary"].items())))
    return 1 if report["compat_summary"].get(compat.BLOCKED) else 0


# --------------------------------------------------------------------------- doctor

def cmd_doctor(_args) -> int:
    import shutil
    print(f"HkiPadOS {__version__}")
    print(f"python {sys.version.split()[0]}")
    for tool in ("7z", "7zz", "7za", "git", "curl"):
        print(f"  {tool:6} {shutil.which(tool) or 'AUSENTE'}")
    try:
        import lzfse
        print(f"  lzfse  {getattr(lzfse, '__file__', 'ok')}")
    except ImportError:
        print("  lzfse  AUSENTE (pip install lzfse; só o kernelcache precisa)")
    return 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="hkidados",
        description="HkiPadOS: analisa um IPSW oficial e reutiliza o kernel do HakiOS.")
    parser.add_argument("--version", action="version", version=f"HkiPadOS {__version__}")
    sub = parser.add_subparsers(dest="command", required=True)

    fetch = sub.add_parser("fetch", help="baixar o IPSW do ipsw.me e conferir o SHA-256")
    fetch.add_argument("--device", default="iPad13,4")
    fetch.add_argument("--build", default="24B156")
    fetch.add_argument("--out", default="cache")
    fetch.add_argument("--url", help="baixar de uma URL em vez de consultar o ipsw.me")
    fetch.set_defaults(func=cmd_fetch)

    verify = sub.add_parser("verify", help="conferir tamanho, SHA-256 e o ZIP")
    verify.add_argument("path")
    verify.add_argument("--expect-sha256")
    verify.add_argument("--crc", action="store_true", help="descomprimir tudo e conferir o CRC")
    verify.set_defaults(func=cmd_verify)

    ls = sub.add_parser("ls", help="listar os membros do IPSW")
    ls.add_argument("path")
    ls.add_argument("--long", action="store_true")
    ls.set_defaults(func=cmd_ls)

    extract = sub.add_parser("extract", help="extrair membros do IPSW")
    extract.add_argument("path")
    extract.add_argument("names", nargs="+")
    extract.add_argument("--out", required=True)
    extract.set_defaults(func=cmd_extract)

    analyze_cmd = sub.add_parser("analyze", help="o relatório do IPSW")
    analyze_cmd.add_argument("path")
    analyze_cmd.add_argument("--device", default="iPad13,4")
    analyze_cmd.add_argument("--variant", default="Erase")
    analyze_cmd.add_argument("--hash", action="store_true", help="calcular o SHA-256 do IPSW")
    analyze_cmd.add_argument("--no-decompress", action="store_true")
    analyze_cmd.add_argument("--extract-images", metavar="DIR")
    analyze_cmd.add_argument("--json")
    analyze_cmd.add_argument("--quiet", action="store_true")
    analyze_cmd.set_defaults(func=cmd_analyze)

    userland = sub.add_parser("userland", help="extrair e inventariar o que não está cifrado")
    userland.add_argument("path")
    userland.add_argument("--device", default="iPad13,4")
    userland.add_argument("--out", default="build/userland")
    userland.add_argument("--json")
    userland.add_argument("--keep", action="store_true", help="manter as imagens extraídas")
    userland.set_defaults(func=cmd_userland)

    compat_cmd = sub.add_parser("compat", help="o veredito, requisito por requisito")
    compat_cmd.add_argument("path")
    compat_cmd.add_argument("--device", default="iPad13,4")
    compat_cmd.add_argument("--userland", metavar="DIR")
    compat_cmd.add_argument("--no-decompress", action="store_true")
    compat_cmd.add_argument("--json")
    compat_cmd.add_argument("--md")
    compat_cmd.set_defaults(func=cmd_compat)

    doctor = sub.add_parser("doctor", help="o que falta nesta máquina")
    doctor.set_defaults(func=cmd_doctor)
    return parser


def main(argv=None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    try:
        return args.func(args)
    except KeyboardInterrupt:
        return 130
    except Exception as error:  # noqa: BLE001 - a CLI explica a falha, não despeja um traceback
        if os.environ.get("HKIPADOS_DEBUG"):
            raise
        print(f"hkidados: {type(error).__name__}: {error}", file=sys.stderr)
        return 2
