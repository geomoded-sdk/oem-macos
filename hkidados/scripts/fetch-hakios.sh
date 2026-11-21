#!/bin/sh
# Clona o HakiOS no commit fixado e confere os dois arquivos que o HkiPadOS reutiliza.
# Nada é modificado: se o SHA-256 divergir, o script falha em vez de continuar.
set -eu

PIN=63fc2b4633bac3af8e57dadab9f5a8d4cb940438
REPO=https://github.com/oosexgang/hakios-xnu-kernel
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
DEST=${1:-$ROOT/../vendor/hakios-xnu-kernel}

if [ ! -d "$DEST/.git" ]; then
    echo "clonando $REPO em $DEST"
    git clone "$REPO" "$DEST"
fi

git -C "$DEST" fetch --depth 1 origin "$PIN"
git -C "$DEST" checkout --detach "$PIN"
echo "HakiOS em $(git -C "$DEST" rev-parse HEAD)"

cd "$ROOT"
sha256sum -c <<'EOF'
a051520d0c1f4f9d783c484be3beaeb536a1b4364fd8ed2e6a9b5c2d98592588  hkipados/vendor/hakios/apfs.py
d7aab6e514a3b6a63cc666bd6129529e315e04fb2eff897b6333ce8d2dba8423  hkipados/vendor/hakios/dmg.py
EOF
