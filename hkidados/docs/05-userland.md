# 5. A userland que foi possível extrair

Duas imagens do IPSW não são AEA e por isso puderam ser lidas: o cryptex de App
(16 MiB) e o RestoreRamDisk (245 MB). O que segue é o inventário real delas,
produzido por `./bin/hkidados userland` e guardado em `reports/userland.json`.

## 5.1 Cryptex1,AppOS — 16 MiB, APFS selado

| | |
|---|---|
| entradas extraídas | 2.893 (1.155 arquivos + 869 `decmpfs` + 869 resource forks) |
| bytes | 9,1 MB |
| bundles | 5, todos de Safari |
| `decmpfs` | **869 fluxos, todos tipo 14 (LZBITMAP)** |
| contêiner `ZBM` no resource fork | 867 |
| Mach-O universal no prefixo literal | 17 |

Os cinco bundles:

```
System/Library/Accounts/Notification/CloudBookmarksAccountsNotifier.bundle
System/Library/PreferenceBundles/MobileSafariSettings.bundle
System/Library/PreferencesSyncBundles/SafariWatchPreferenceSyncServer.bundle
System/Library/UsageBundles/SafariUsageBundle.bundle
System/Library/UserNotifications/Bundles/com.apple.Safari.PasswordBreachNotifications.bundle
```

São componentes reais do iPadOS 27.2.1, mas **não** são a interface: não há
`SpringBoard`, `BackBoard`, `UIKitCore` nem `WindowServer` aqui. O cryptex de App
é o conteúdo de apps que fica fora do volume do sistema; a interface está no AEA.

### Por que os arquivos saem com 0 byte

Todo arquivo do cryptex é comprimido com `decmpfs`. O atributo tem 16 bytes:

```
66 70 6d 63   "cmpf"
0e 00 00 00   tipo 14
a0 92 02 00 00 00 00 00   tamanho descomprimido (168.864)
```

e o conteúdo vai para `com.apple.ResourceFork`, que começa com `ZBM\x09` — o
contêiner do **LZBITMAP**. O 7-Zip 25.01 extrai o atributo como
`nome:com.apple.decmpfs` e deixa `nome` com 0 byte; ele não remonta o arquivo.

Para provar que o conteúdo é real, o prefixo literal do fluxo contém um Mach-O
universal:

```
ca fe ba be 00 00 00 02 01 00 00 0c 80
```

duas fatias, `cputype 0x0100000c` (arm64), `cpusubtype 0x80000002` (arm64e).

O `hkipados/decmpfs.py` lê o cabeçalho, classifica o tipo e procura esse
cabeçalho; **não** descomprime. Reconstruir exigiria o decodificador LZBITMAP
(bitmap + LZVN), que não existe no 7-Zip nem no leitor do HakiOS. Fingir que
descomprimiu seria pior do que dizer que não.

## 5.2 RestoreRamDisk — 245 MB, APFS

| | |
|---|---|
| entradas | 1.564 (1.269 arquivos + 295 symlinks) |
| Mach-O | **383, todos arm64e, todos com chained fixups** |
| plataforma | `iOS` 351, `driverKit` 32 |
| `minos` | 27.2.0 em 382, 18.0.0 em 1 |
| `decmpfs` | 0 (o ramdisk não usa) |

### O `launchd`

```
/sbin/launchd
arm64e (0x0100000c / 0xc0000002), ptrauth ABI, chained fixups
platform iOS, minos 27.2.0, sdk 27.2.0
32 entitlements
```

Os entitlements não são decorativos; eles dizem de que kernel o binário espera:

```
com.apple.private.kernel.darkboot
com.apple.private.pmap.load-trust-cache
com.apple.private.security.storage.launchd
com.apple.private.security.storage.driverkitd
com.apple.private.vfs.pivot-root
com.apple.private.vfs.graftdmg
com.apple.private.security.system-mount-authority
com.apple.private.amfi.can-allow-non-platform
com.apple.apfs.get-dev-by-role
com.apple.rootless.storage.early_boot_mount
com.apple.developer.hardened-process
com.apple.developer.lockdown-mode.state
...
```

`pivot-root`, `graftdmg` e `load-trust-cache` são contratos do kernel. Um XNU de
2022 não os tem.

### O resto do ramdisk

```
bin/    cat expr ln mkdir mv rm
sbin/   apfs_hfs_convert fsck fsck_apfs fsck_hfs halt launchd mount
        mount_apfs mount_hfs mount_tmpfs newfs_apfs newfs_hfs reboot
usr/libexec/  DumpPanic PurpleReverseProxy SavageUtil YonkersUtil
              diskimagesiod driverkitd restorecameraispd seputil xpcproxy
usr/lib/      108 bibliotecas (dyld, libCoreEntitlements, libAppleArchive, ...)
System/Library/Frameworks/          15 (CFNetwork, Foundation, IOKit, IOSurface,
                                       Security, CryptoKit, Network, ...)
System/Library/PrivateFrameworks/   69 (APFS, AppServerSupport, ...)
System/Library/LaunchDaemons/       10 (diskimagesiod, driverkitd, syslogd, ...)
```

É a userland de restauração: monta APFS, checa o sistema de arquivos, instala. Não
tem interface, e não deveria ter.

## 5.3 O que não foi extraído

| o que | onde está | por quê |
|---|---|---|
| `SpringBoard`, `BackBoard`, `UIKitCore` | volume `OS` (AEA, 8,3 GB) | cifrado |
| `WindowServer`/`SkyLight` | volume `OS` | cifrado |
| frameworks de sistema | volume `OS` | cifrado |
| `launchd` do sistema | volume `OS` | cifrado; o do ramdisk é outro |
| `dyld` compartilhado | volume `OS` | cifrado |

O `Ap,SystemVolumeCanonicalMetadata` (37,5 MB de `.mtree`) lista os arquivos do
volume do sistema, mas sem o conteúdo. É o máximo que se consegue sem a chave.

## 5.4 O que isso prova

1. Os binários são reais, arm64e, de 27.2.0 — não são uma imitação.
2. O `launchd` pede serviços que o kernel comunitário não oferece.
3. A interface não está em nenhuma imagem legível; não há como "iniciar o
   SpringBoard" a partir do que se pode extrair legalmente.
