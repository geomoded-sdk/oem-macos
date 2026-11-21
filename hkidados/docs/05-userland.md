# 5. A userland que foi possível extrair

Duas imagens do IPSW não são AEA e por isso puderam ser lidas: o cryptex de App
(16 MiB) e o RestoreRamDisk (245 MB). O que segue é o inventário real delas,
produzido por `./bin/hkidados userland` e guardado em `reports/userland.json`.

## 5.1 Cryptex1,AppOS — 16 MiB, APFS selado

| | |
|---|---|
| arquivos extraídos | **1.155** (mais 153 diretórios) |
| bytes | **18.422.104** (17,6 MB) — o mesmo total que o 7-Zip, conferido por SHA-256 |
| `decmpfs` | **869 fluxos, todos tipo 14 (LZBITMAP), 869 descomprimidos** |
| Mach-O universal | **15** (arm64 + arm64e), iOS 27.2.0, chained fixups |
| nós da árvore selada | 5.023, cada um conferido contra o SHA-256 que o pai nomeia |
| árvore de extents (`fext`) | bloco 7, 1.106 nós |
| tempo | 0,6 s para ler, descomprimir e escrever tudo |

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

### O volume selado, lido sem o 7-Zip

O cryptex é um volume APFS **selado**: a árvore do sistema de arquivos é gravada
com `APFS_BTREE_NOHEADER` (sem `obj_phys`, portanto sem Fletcher-64) e
`APFS_BTREE_HASHED` (cada entrada de filho traz 8 bytes de oid — um deslocamento
a partir do oid da raiz — e 32 bytes do SHA-256 do nó). Os extents não estão nos
registros `FILE_EXTENT` da árvore do sistema: ficam numa árvore separada, física,
apontada por `apfs_fext_tree_oid` (offset 0x408 do superbloco), com chave de 16
bytes `(id do fluxo, deslocamento lógico)`.

`hkipados/apfs_sealed.py` implementa exatamente esses dois pontos, seguindo
`apfs_lib.c` do APFS que a Apple publicou (APSL 2.0, em
`xnu/files/bsd/miscfs/apfs/` no clone do HakiOS): `btree_open`, `node_at_h`,
`node_child_addr`, `node_parse`, `node_entry`. O resto — inodes, nomes,
atributos, extents, decmpfs — continua sendo o leitor do HakiOS
(`vendor/hakios/apfs.py`), que não foi alterado.

A prova de que a leitura está certa não é "abriu": são os **5.023 hashes** que o
volume guarda e que o leitor confere nó a nó, mais a comparação byte a byte com
o 7-Zip 25.01 — 1.155 nomes, 18.422.104 bytes, **0 diferenças de SHA-256**.

### O `decmpfs` tipo 14 (LZBITMAP), decodificado

Todo arquivo do cryptex é comprimido com `decmpfs`. O atributo tem 16 bytes:

```
66 70 6d 63   "fpmc" (lido como u32 little-endian: 0x636d7066, "cmpf")
0e 00 00 00   tipo 14 (LZBITMAP)
a0 92 02 00 00 00 00 00   tamanho descomprimido (168.864)
```

O conteúdo vai para `com.apple.ResourceFork` — um fluxo de atributo, não um
arquivo — e começa com uma tabela de deslocamentos, um por bloco de 64 KB
(`[16, 3925, 8704, 10625]` para um arquivo de 168.864 bytes, que são 3 blocos),
seguida dos blocos, o primeiro começando com `ZBM\x09`. Um bloco que não
encolheu vem como `0xff` seguido dos bytes.

O decodificador não é nosso: o HakiOS traz `libzbitmap.c` (Ernesto A. Fernández,
Corellium, MIT/GPL-2.0+) e `unzbm.c`. Os dois foram copiados **verbatim** para
`hkipados/vendor/hakios/lzbitmap/`, com o SHA-256 em `PROVENANCE.md`. O
HkiPadOS os usa de duas formas:

- `unzbm`, compilado (`make unzbm`), para uma árvore que o 7-Zip já extraiu;
- `libzbitmap.so` + `ctypes` (`decmpfs.decompress`), para descomprimir na
  memória enquanto lê o APFS — é o caminho do leitor próprio.

Resultado medido: **869 de 869 fluxos**, 0 falhas, 18.422.104 bytes
recuperados. Cada bloco tem o tamanho conferido contra o cabeçalho; um bloco que
não bata de tamanho é recusado, não escrito.

## 5.2 RestoreRamDisk — 245 MB, APFS

| | |
|---|---|
| entradas | **2.025** (1.260 arquivos + 459 diretórios + 306 symlinks) |
| bytes | **1.236.840.790** (1,2 GB) |
| Mach-O | **387, todos arm64e, todos com chained fixups** |
| plataforma | `iOS` 355, `driverKit` 32 |
| `minos` | 27.2.0 em 386, 18.0.0 em 1 |
| `decmpfs` | 0 (o ramdisk não usa) |
| leitura | APFS não selado, 263 nós, 6,3 s sem 7-Zip |

O ramdisk chega do IPSW embrulhado num **IM4P** (`type rdsk`): 27 bytes de
cabeçalho DER e o container APFS de 245.366.784 bytes logo depois. O
`hkipados/apfsx.py` tira o embrulho; o `im4p.locate` só lê o cabeçalho, sem
copiar os 234 MB para descobrir onde o payload começa.

O 7-Zip 25.01 e o leitor próprio concordam no total de bytes de arquivos
regulares (1.236.840.790) e nos nomes, mas **não** nos symlinks: em 10 casos o
7-Zip escreveu um arquivo de 0 byte onde o APFS tem um symlink
(`usr/lib/libextension.dylib`, `sbin/mount_apfs`, `sbin/fsck_apfs`, …), e em 3
casos escreveu um symlink com o caminho absoluto da extração em vez do alvo
gravado no volume. O leitor próprio acerta os 306. É por isso que o número de
Mach-O subiu de 383 para 387: quatro binários que o 7-Zip deixou vazios são, no
volume, symlinks para binários reais.

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
