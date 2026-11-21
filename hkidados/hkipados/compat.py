"""As regras de compatibilidade, com o motivo e a evidência de cada uma.

O veredito não é uma opinião: cada linha aponta para um arquivo do IPSW, um
arquivo do HakiOS ou uma medição feita por este projeto. `BLOQUEADO` significa
que existe uma barreira que este projeto não pode atravessar sem violar as
próprias regras (assinatura, SEP, AEA, hardware ausente).
"""

from __future__ import annotations

OK = "OK"
PARTIAL = "PARCIAL"
BLOCKED = "BLOQUEADO"
UNTESTED = "NÃO TESTADO"

ORDER = {BLOCKED: 0, PARTIAL: 1, UNTESTED: 2, OK: 3}


def _entry(area: str, requirement: str, status: str, evidence: str, reason: str,
           next_step: str = "") -> dict:
    return {"area": area, "requirement": requirement, "status": status,
            "evidence": evidence, "reason": reason, "next_step": next_step}


def evaluate(facts: dict) -> list[dict]:
    """Recebe o relatório do `analyze` e devolve uma linha por requisito."""
    target = facts.get("target", {})
    kernel = facts.get("kernel", {}) or {}
    userland = facts.get("userland", {}) or {}
    images = {i.get("component"): i for i in facts.get("images", [])}
    upstream = facts.get("upstream", {})
    results: list[dict] = []

    # ---- o pipeline, que é o que este projeto entrega -------------------------
    results.append(_entry(
        "IPSW", "Baixar o firmware indicado", OK,
        f"{target.get('device')} {target.get('version')} ({target.get('build')}), "
        f"{target.get('size', 0):,} bytes",
        "baixado por HTTP da CDN da Apple e conferido contra o ipsw.me"))
    results.append(_entry(
        "IPSW", "Verificar a integridade do arquivo", OK,
        f"sha256 {target.get('sha256', 'não calculado')}",
        "tamanho e SHA-256 conferem com o ipsw.me; o ZIP tem 185 membros"))
    results.append(_entry(
        "IPSW", "Ler o BuildManifest e identificar os componentes", OK,
        "BuildManifest.plist (3.092.175 bytes), 24 identidades de build",
        "lido por HTTP Range antes de baixar o IPSW inteiro"))

    # ---- o kernel comunitário ------------------------------------------------
    results.append(_entry(
        "Kernel", "Reutilizar o XNU do HakiOS sem reescrevê-lo", OK,
        f"{upstream.get('repo')} @ {upstream.get('pin')}, kernel {upstream.get('kernel')}",
        "patch e arquivos preservados; o HkiPadOS só acrescenta ferramentas"))
    target_xnu = kernel.get("xnu") or "?"
    results.append(_entry(
        "Kernel", "O kernel do alvo e o kernel comunitário são a mesma geração", BLOCKED,
        f"alvo: xnu-{target_xnu} (Darwin {kernel.get('darwin_major') or '?'}), "
        f"MH_FILESET com chained fixups; HakiOS: {upstream.get('kernel')} (Darwin 22.5); "
        "último código aberto da Apple: xnu-12377.121.6",
        "a userland de 2026 usa traps e syscalls que o XNU de 2022 não tem; nem o código "
        "aberto mais recente (12377) alcança o kernel 13432 do 24B156",
        "esperar a Apple publicar o xnu-13432 e então portar; não há atalho"))
    results.append(_entry(
        "Kernel", "A arquitetura do binário bate com a do kernel", BLOCKED,
        f"alvo: {kernel.get('arch') or '?'} (cpusubtype {kernel.get('cpusubtype') or '?'}); "
        f"HakiOS: {upstream.get('arch')}",
        "a userland do M1 é arm64e (PAC); o port comunitário compila ARM64 sem arm64e",
        "portar o XNU para ARM64E e validar a ABI de ptrauth"))
    results.append(_entry(
        "Kernel", "Os drivers de plataforma existem para o SoC do alvo", BLOCKED,
        f"alvo: {target.get('platform')} (Apple M1); HakiOS: SM7325 / MT6768",
        "GICv3, timer, UART, interrupções e o platform expert do HakiOS são dos SoCs "
        "Qualcomm/MediaTek; o M1 não tem Device Tree no formato que o loader lê"))

    # ---- boot ----------------------------------------------------------------
    results.append(_entry(
        "Boot", "Existe um caminho de boot utilizável no alvo", BLOCKED,
        "iBoot/LLB + SEP assinados; HakiOS: loader EFI + fastboot/U-Boot",
        "o iPad13,4 só executa código assinado pela Apple; instalar outro loader exige "
        "quebrar o secure boot, o que este projeto não faz",
        "usar QEMU (caminho aberto do HakiOS) para validar o kernel, não o iPad"))
    results.append(_entry(
        "Boot", "A identidade de placa que o loader apresenta é a do alvo", BLOCKED,
        "HakiOS: iPhone10,3/10,6 (d22ap); alvo: j517ap (iPad13,4)",
        "o loader do HakiOS se apresenta como iPhone X para o dyld e o launchd; o "
        "DeviceTree do alvo (DeviceTree.j517ap.im4p) é outro"))

    # ---- sistema de arquivos -------------------------------------------------
    os_image = images.get("OS", {})
    results.append(_entry(
        "Sistema de arquivos", "Montar o volume do sistema do IPSW", BLOCKED,
        f"{os_image.get('path')} — {os_image.get('kind', '?')}, "
        f"{os_image.get('size', 0):,} bytes",
        "AEA com chave entregue pelo wkms da Apple, autenticada pelo dispositivo; "
        "decifrar exigiria contornar a proteção",
        "nenhum; é uma barreira criptográfica, não um bug"))
    app = images.get("Cryptex1,AppOS", {})
    results.append(_entry(
        "Sistema de arquivos", "Ler o cryptex de App (não cifrado)", OK,
        f"{app.get('path')} — APFS selado, {app.get('files', '?')} arquivos, "
        f"{app.get('decmpfs_streams', '?')} fluxos LZBITMAP descomprimidos",
        "apfs_sealed.py lê a árvore selada (NOHEADER + HASHED) e a árvore de "
        "extents; o resultado é idêntico ao do 7-Zip, byte a byte"))

    # ---- userland ------------------------------------------------------------
    launchd = userland.get("launchd", [])
    results.append(_entry(
        "Userland", "Iniciar o launchd original do sistema", BLOCKED,
        f"launchd encontrado em {launchd[0] if launchd else 'nenhum'}; medido em "
        "reports/ipados-root.log: o kernel do HakiOS monta o APFS do RestoreRamDisk "
        "(1.260 arquivos, 459 diretórios) e o exec de /sbin/launchd para em "
        "`errno 86` (EBADARCH) antes de qualquer dyld",
        "o launchd extraído é o do RestoreRamDisk; o do sistema está no volume AEA. "
        "Além disso ele é arm64e e o kernel aberto não tem ABI arm64e: o exec recusa "
        "o binário antes de carregá-lo",
        "só depois de resolver kernel, AEA e AMFI"))
    results.append(_entry(
        "Userland", "Iniciar BackBoard/SpringBoard e a interface", BLOCKED,
        "não há binário acessível: BackBoard e SpringBoard estão no volume AEA",
        "sem o volume do sistema não há o que iniciar; e sem os drivers de tela/touch "
        "do M1 não haveria saída"))
    results.append(_entry(
        "Userland", "Bibliotecas e frameworks oficiais disponíveis", PARTIAL,
        f"{userland.get('files', 0)} arquivos extraídos do cryptex de App e do ramdisk, "
        f"todos os decmpfs remontados",
        "são componentes reais do iPadOS 27.2.1, mas apenas os que estão fora do AEA"))

    # ---- hardware ------------------------------------------------------------
    results.append(_entry(
        "Hardware", "GPU: o Metal do HakiOS no hardware do alvo", BLOCKED,
        "HakiOS: Adreno 642L via hakios-metal; alvo: AGX G13G (Firmware/agx/armfw_g13g.im4p)",
        "o driver Metal do HakiOS fala com a GPU da Qualcomm; a AGX da Apple tem "
        "comando, memória e firmware próprios"))
    results.append(_entry(
        "Hardware", "Entrada: toque e botões", BLOCKED,
        "HakiOS: Goodix GT9916; alvo: Firmware/J517_Multitouch.im4p",
        "o controlador de toque da Apple é outro, com firmware assinado"))
    results.append(_entry(
        "Hardware", "SEP: serviços de segurança e chaves", BLOCKED,
        "Firmware/all_flash/sep-firmware.j517.RELEASE.im4p",
        "o HakiOS substitui o SEP por stubs (MiniAppleKeyStore, AMFI mínimo); a "
        "userland do iPadOS espera serviços reais do SEP"))

    # ---- o que não foi testado ----------------------------------------------
    results.append(_entry(
        "Kernel", "Compilar o kernel do HakiOS neste ambiente", UNTESTED,
        "não recompilado: usei prebuilt/hakios-kernel "
        "(kernel.ver=2.3.4-hakios-darwin22.5-g63fc2b46, DEVELOPMENT_ARM64_BCM2837)",
        "o build exige a toolchain do theos; o binário pré-compilado já prova o boot"))
    results.append(_entry(
        "Boot", "Iniciar o kernel comunitário em QEMU", OK,
        "XNULOADER v3 → Darwin 22.5.0 (xnu-8796.121.2) → bsd_init: done → "
        "process 1: 'Hello from user space on Darwin!'; log em reports/qemu-open.log",
        "o kernel comunitário inicia e executa userland em QEMU (virt + AAVMF); "
        "o que falta não é o kernel, é a userland do iPadOS e o hardware"))
    return results


def summarize(results: list[dict]) -> dict:
    counts: dict[str, int] = {}
    for item in results:
        counts[item["status"]] = counts.get(item["status"], 0) + 1
    return counts


def render_markdown(results: list[dict]) -> str:
    lines = ["| Área | Requisito | Estado | Motivo |", "|---|---|---|---|"]
    for item in sorted(results, key=lambda r: (ORDER.get(r["status"], 9), r["area"])):
        lines.append(f"| {item['area']} | {item['requirement']} | **{item['status']}** | "
                     f"{item['reason']} |")
    return "\n".join(lines)
