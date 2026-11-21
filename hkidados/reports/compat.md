| Área | Requisito | Estado | Motivo |
|---|---|---|---|
| Boot | Existe um caminho de boot utilizável no alvo | **BLOQUEADO** | o iPad13,4 só executa código assinado pela Apple; instalar outro loader exige quebrar o secure boot, o que este projeto não faz |
| Boot | A identidade de placa que o loader apresenta é a do alvo | **BLOQUEADO** | o loader do HakiOS se apresenta como iPhone X para o dyld e o launchd; o DeviceTree do alvo (DeviceTree.j517ap.im4p) é outro |
| Hardware | GPU: o Metal do HakiOS no hardware do alvo | **BLOQUEADO** | o driver Metal do HakiOS fala com a GPU da Qualcomm; a AGX da Apple tem comando, memória e firmware próprios |
| Hardware | Entrada: toque e botões | **BLOQUEADO** | o controlador de toque da Apple é outro, com firmware assinado |
| Hardware | SEP: serviços de segurança e chaves | **BLOQUEADO** | o HakiOS substitui o SEP por stubs (MiniAppleKeyStore, AMFI mínimo); a userland do iPadOS espera serviços reais do SEP |
| Kernel | O kernel do alvo e o kernel comunitário são a mesma geração | **BLOQUEADO** | a userland de 2026 usa traps e syscalls que o XNU de 2022 não tem; nem o código aberto mais recente (12377) alcança o kernel 13432 do 24B156 |
| Kernel | A arquitetura do binário bate com a do kernel | **BLOQUEADO** | a userland do M1 é arm64e (PAC); o port comunitário compila ARM64 sem arm64e |
| Kernel | Os drivers de plataforma existem para o SoC do alvo | **BLOQUEADO** | GICv3, timer, UART, interrupções e o platform expert do HakiOS são dos SoCs Qualcomm/MediaTek; o M1 não tem Device Tree no formato que o loader lê |
| Sistema de arquivos | Montar o volume do sistema do IPSW | **BLOQUEADO** | AEA com chave entregue pelo wkms da Apple, autenticada pelo dispositivo; decifrar exigiria contornar a proteção |
| Userland | Iniciar o launchd original do sistema | **BLOQUEADO** | o launchd extraído é o do RestoreRamDisk; o do sistema está no volume AEA. Além disso ele é arm64e e o kernel aberto não tem ABI arm64e: o exec recusa o binário antes de carregá-lo |
| Userland | Iniciar BackBoard/SpringBoard e a interface | **BLOQUEADO** | sem o volume do sistema não há o que iniciar; e sem os drivers de tela/touch do M1 não haveria saída |
| Userland | Bibliotecas e frameworks oficiais disponíveis | **PARCIAL** | são componentes reais do iPadOS 27.2.1, mas apenas os que estão fora do AEA |
| Kernel | Compilar o kernel do HakiOS neste ambiente | **NÃO TESTADO** | o build exige a toolchain do theos; o binário pré-compilado já prova o boot |
| Boot | Iniciar o kernel comunitário em QEMU | **OK** | o kernel comunitário inicia e executa userland em QEMU (virt + AAVMF); o que falta não é o kernel, é a userland do iPadOS e o hardware |
| IPSW | Baixar o firmware indicado | **OK** | baixado por HTTP da CDN da Apple e conferido contra o ipsw.me |
| IPSW | Verificar a integridade do arquivo | **OK** | tamanho e SHA-256 conferem com o ipsw.me; o ZIP tem 185 membros |
| IPSW | Ler o BuildManifest e identificar os componentes | **OK** | lido por HTTP Range antes de baixar o IPSW inteiro |
| Kernel | Reutilizar o XNU do HakiOS sem reescrevê-lo | **OK** | patch e arquivos preservados; o HkiPadOS só acrescenta ferramentas |
| Sistema de arquivos | Ler o cryptex de App (não cifrado) | **OK** | apfs_sealed.py lê a árvore selada (NOHEADER + HASHED) e a árvore de extents; o resultado é idêntico ao do 7-Zip, byte a byte |
