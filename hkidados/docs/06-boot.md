# 6. Boot: o do alvo e o que foi realmente executado

## 6.1 A cadeia do iPad13,4

```
Bootrom (imutável, no SoC)
  └─ LLB        Firmware/all_flash/LLB.j517.RELEASE.im4p        1.126.220
      └─ iBoot  Firmware/all_flash/iBoot.j517.RELEASE.im4p      1.126.220
          ├─ SEP  sep-firmware.j517.RELEASE.im4p                5.949.720
          └─ kernelcache.release.ipad13                        23.257.193
              └─ launchd (arm64e, minos 27.2.0)
                  └─ … → BackBoard → SpringBoard
```

Cada elo é assinado pela Apple e personalizado pelo `TSS` para o aparelho. O
`iBSS` (DFU) é o mesmo binário do `iBoot`; o `iBEC` difere por 9 bytes. O
`DeviceTree.j517ap.im4p` (47.255 bytes) é o que diz ao kernel qual hardware
existe. Não há estágio não assinado em lugar nenhum.

## 6.2 A cadeia do HakiOS

No telefone:

```
ABL / fastboot boot  →  UEFI (hakios-uefi)  →  loader.efi  →  payload (kernel + ramdisk + cmdline)
```

No QEMU, que é o que este projeto executa:

```
AAVMF (edk2)  →  \EFI\BOOT\BOOTAA64.EFI (loader.efi)  →  -device loader,file=payload.bin
                                                          →  XNU  →  /sbin/launchd
```

O `loader.efi` monta um Device Tree próprio e uma `boot_args` com a identidade de
um iPhone X. Ele não lê o DeviceTree do iPad, e o iPad não o executaria.

## 6.3 O que foi executado

Máquina: 15 GB de RAM, QEMU 10.0.13, AAVMF `2025.02-8+deb13u1`, sem KVM
(`-accel tcg`), `-M virt,gic-version=3 -cpu cortex-a76`, 3 GB para o guest.

```sh
# 1. payload com os binários pré-compilados do HakiOS
cd ../vendor/hakios-xnu-kernel
ARGS=$(python3 tools/hakios-config.py args --defconfig defconfig)   # "cpus=1"
python3 xnuloader/mkpayload.py -o qemu/payload.bin \
    kernel=prebuilt/hakios-kernel ramdisk=prebuilt/userland/hello --cmdline "$ARGS"

# 2. boot
cd qemu && QEMU_MEM=3G QEMU_SMP=2 ./run.sh 220 > /tmp/q2.log 2>&1
```

O log completo e filtrado está em `reports/qemu-open.log`; o script que repete
tudo é `scripts/hakios-boot-open.sh`.

### Trechos reais

```
UEFI firmware (version 2025.02-8+deb13u1 built at 18:06:17 on Nov  8 2025)
BdsDxe: starting Boot0001 "UEFI Misc Device" from PciRoot(0x0)/Pci(0x2,0x0)
== XNULOADER v3 ==
   hello hakios-kernel!
   kernel.ver=2.3.4-hakios-darwin22.5-g63fc2b46
CurrentEL=1 MIDR=414fd0b1 MMFR0=101122 CNTFRQ=62500000 image=a1000000
platform qemu-virt
```

```
Darwin Kernel Version 22.5.0: Sun Oct 25 22:52:46 PDT 2026;
shimoku:xnu-8796.121.2[xnu-8796.121.2-dirty]/BUILD/obj/DEVELOPMENT_ARM64_BCM2837
...
iBoot version: iBoot-8422.100.650
Security policy loaded: Apple Mobile File Integrity (stand-in) (AMFI)
Security policy loaded: Seatbelt sandbox policy (stand-in) (Sandbox)
...
[    7.487460]: BSD root: md0, major 1, minor 0
[    7.513621]: bsd_init: done
[    7.631007]: pid 1 exited -- no exit reason available -- (signal 0, exit 0)
```

```
load_init_program: attempting to load /usr/appleinternal/sbin/launchd.development
load_init_program: attempting to load /sbin/launchd
Hello from user space on Darwin!
I am process 1, uid 0, console fd 0.
```

### O que isso prova

- o kernel comunitário **inicia** num ARM64 virtual e chega a `bsd_init: done`;
- ele monta um root em memória (`md0`) e **executa o processo 1**;
- a userland de teste (`hello`, do próprio HakiOS) roda e escreve no console;
- o `launchd` que ele procura é `/sbin/launchd` — o mesmo caminho que o Darwin usa.

### O que isso não prova

- não é o kernel do iPadOS: é `xnu-8796.121.2`, arm64, compilado para BCM2837;
- não é a userland do iPadOS: é o `hello` do HakiOS;
- não é hardware Apple: é QEMU `virt`, com GICv3, que o M1 não tem;
- o `iBoot-8422.100.650` do log é uma string que o kernel do HakiOS coloca em
  `boot_args`, não um iBoot real.

## 6.4 Por que não se tenta o iPad

Instalar outro loader no iPad13,4 exigiria quebrar a assinatura do iBoot — o que
este projeto não faz. Mesmo quebrasse, o kernel comunitário não tem drivers para
AIC, DART, ANS, DCP, AGX ou o controlador de toque (ver `docs/04`), e a userland
está em um volume AEA. Não existe caminho técnico legítimo daqui até a tela do
iPadOS.

## 6.5 O init do iPadOS no kernel do HakiOS: o que acontece

O kernel do HakiOS aceita um Mach-O qualquer em `md0` como processo 1 — é o que
`prebuilt/userland/hello` faz. Então dá para trocar o `hello` por um binário real
do iPadOS e medir onde o `exec` para. `scripts/try-ipados-init.sh` faz os quatro
casos, um boot cada, e guarda o log em `reports/init-attempts/`.

| # | init em `md0` | o que o kernel disse |
|---|---|---|
| 1 | `hello` do HakiOS (arm64, estático) | `Hello from user space on Darwin!` |
| 2 | o mesmo `hello`, cpusubtype trocado para arm64e | `Process 1 exec of /sbin/launchd failed, errno 86` |
| 3 | `sbin/launchd` do RestoreRamDisk (arm64e, 10 dylibs) | idem, `errno 86` |
| 4 | fatia arm64 de `webpushd` do cryptex, cpusubtype zerado | `unexpected SIGKILL of init with reason -- namespace 9 code 0x1` |

O caso 2 é o controle do 3: é o mesmo binário que roda no caso 1, com 4 bytes
trocados. O `errno 86` é, portanto, do **subtipo de CPU**, não do conteúdo do
`launchd`. Em `bsd/sys/errno.h` do XNU, 86 é `EBADARCH`; o kernel do HakiOS é
arm64 (`DEVELOPMENT_ARM64_BCM2837`) e não tem ABI arm64e para userland, que
exigiria `ptrauth` no kernel. Toda a userland do iPadOS 27 é arm64e: 387 de 387
Mach-O no ramdisk, 15 de 15 no cryptex.

O caso 4 tira o `EBADARCH` do caminho: o kernel aceita o cabeçalho e o `exec`
morre logo depois. `namespace 9` é `OS_REASON_EXEC` e o código 1 é
`EXEC_EXIT_REASON_BAD_MACHO` — os dois de `bsd/sys/reason.h` do XNU. O kernel
recusa o Mach-O; o motivo exato dentro do carregador não foi determinado neste
experimento, e fica registrado como não determinado.

Nenhum dos dois é o obstáculo principal — o principal é o volume `OS` ser AEA —
mas os dois são medidos, não supostos, e é neles que uma tentativa de boot da
userland do iPadOS tropeça primeiro. O caminho que o HakiOS percorreu no
telefone, e que serve de modelo: uma imagem APFS de sistema (`rd=md0`) com
`/sbin/launchd` de verdade, montada a partir de um IPSW da **mesma geração** do
kernel. Para o iPadOS 27 isso exigiria um kernel xnu-13432 e a chave do AEA.
