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
