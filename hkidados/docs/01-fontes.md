# 1. Fontes

## 1.1 O projeto comunitário: HakiOS

- Repositório principal: <https://github.com/oosexgang/hakios-xnu-kernel>
- Commit usado em todo este trabalho: `63fc2b4633bac3af8e57dadab9f5a8d4cb940438`
- Clonado em `../vendor/hakios-xnu-kernel` (fora deste diretório, para não misturar).

O que existe lá, e o que este projeto usa:

| diretório | conteúdo | usado pelo HkiPadOS |
|---|---|---|
| `xnu/` | `apply.sh`, `hakios.patch`, `files/` — o XNU 8796.121.2 com o patch da comunidade | sim: é o kernel (não modificado) |
| `xnuloader/` | o loader EFI que monta o payload (kernel + ramdisk + cmdline) | sim: `prebuilt/loader.efi` no boot de QEMU |
| `drivers/` | audio, bluetooth, camera, core, gpu, haptic, input, modem, net, power, storage | não: são de SoCs Qualcomm/MediaTek |
| `tools/apfs/` | `apfs.py`, `apfs-tool.c`, `dmg.py` — leitor APFS/UDIF | **sim, copiado verbatim** (`hkipados/vendor/hakios/`) |
| `sysimage/` | `unpack.sh`, `select-system.py`, `ipsw-member.py`, `mkvarskel.py` | não: reimplementado em Python, com as mesmas ideias |
| `userland/` | `build.sh`, `src/`, o `hello` de syscalls cruas | sim: `prebuilt/userland/hello` no boot de QEMU |
| `prebuilt/` | `hakios-kernel` (16.8 MB), `loader.efi` (66 KB), `userland/` | sim: o boot de QEMU usa estes binários |
| `qemu/` | `run.sh`, `run-darwin.sh`, `screendump.sh` | sim: `qemu/run.sh` no teste de boot |
| `devices/` | `redwood` (POCO X5 Pro), `qemu`, `manifest.conf` | não |

Repositórios irmãos, citados pelo HakiOS e não baixados aqui:

- `hakios-sm7325` — POCO X5 Pro 5G (SM7325 / SC7280), o port que roda no telefone;
- `hakios-mt6768` — Infinix Hot 20 (MT6768), port experimental;
- `hakios-uefi` — o UEFI dos dispositivos;
- `hakios-metal` — o modelo de comando do Metal sobre a Adreno 642L;
- `hackios-web` — o site.

Fatos medidos pelo próprio HakiOS, e que este relatório cita como *dele*:

- kernel `xnu-8796.121.2`, Darwin 22.5, `DEVELOPMENT_ARM64_BCM2837` — confirmado no
  log de boot deste projeto (`reports/qemu-open.log`);
- no POCO X5 Pro 5G, o window server desenha por Metal na Adreno 642L, 100–104 fps
  em *page swipes* (não 120) — `README.md` e `docs/reference/metal-measurements.md`;
- a userland que ele inicia é a do iOS **16.5 (20F66)**, de um iPhone X
  (`iPhone10,3/10,6`, `d22ap`), obtida do IPSW do próprio usuário;
- o loader se apresenta com essa identidade de iPhone; não existe identidade de iPad.

Licença: MIT (o repositório). O XNU é APSL 2.0. Ver `NOTICE.md`.

## 1.2 O firmware: IPSW do iPad13,4

Página indicada: <https://ipsw.me/install/iPad13,4/24B156/>

| | |
|---|---|
| arquivo | `iPad_Pro_Spring_2021_27.2.1_24B156_Restore.ipsw` |
| URL (CDN da Apple) | `https://updates.cdn-apple.com/2026FallFCS/3f4c8ab7-3a0d-46ab-8bfb-f57bdf668637/iPad_Pro_Spring_2021_27.2.1_24B156_Restore.ipsw` |
| tamanho | 11.655.546.609 bytes |
| SHA-256 | `b00bb0cbc17a810ebd36bb4befa6cc7599c1abdbf92093abf2a4db2d8aebe333` |
| versão | iPadOS 27.2.1, build 24B156 |
| publicado | 2026-11-09, ainda assinado em 2026-11-21 |
| produto | iPad13,4 — iPad Pro 11" (3ª geração), board `j517ap`, plataforma `t8103` (M1) |
| ZIP | ZIP64, 185 membros, 11.828.110.330 bytes descomprimidos |

O arquivo foi baixado inteiro, o tamanho e o SHA-256 conferem com o ipsw.me, e o
CRC32 de cada membro foi verificado pelo `ZipArchive` deste projeto.

O firmware não é distribuído com o código: fica em `cache/`, fora do controle de
versão. O comando `fetch` só baixa o que você mandar, da URL que o ipsw.me indica.

## 1.3 O que é da Apple e o que não é

Nada de código da Apple é distribuído aqui. O que este projeto faz com o IPSW é
ler, medir e extrair o que não está cifrado. Os binários extraídos ficam em
`build/`, também fora do controle de versão.
