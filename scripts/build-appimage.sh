#!/usr/bin/env bash
set -euo pipefail

# Diretório raiz do projeto
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${ROOT_DIR}"

BIN_PATH="${1:-}"
OUTPUT_PATH="${2:-${ROOT_DIR}/ekaza-cam-x86_64.AppImage}"

# 1. Localizar ou compilar o executável
if [ -z "${BIN_PATH}" ]; then
  if [ -f "${ROOT_DIR}/target/x86_64-unknown-linux-gnu/release/ekaza-cam" ]; then
    BIN_PATH="${ROOT_DIR}/target/x86_64-unknown-linux-gnu/release/ekaza-cam"
  elif [ -f "${ROOT_DIR}/target/release/ekaza-cam" ]; then
    BIN_PATH="${ROOT_DIR}/target/release/ekaza-cam"
  else
    echo "==> Binário de release não encontrado. Compilando..."
    cargo build --release
    BIN_PATH="${ROOT_DIR}/target/release/ekaza-cam"
  fi
fi

if [ ! -f "${BIN_PATH}" ]; then
  echo "Erro: executável não encontrado em ${BIN_PATH}" >&2
  exit 1
fi

echo "==> Usando binário: ${BIN_PATH}"

# 2. Preparar AppDir
APPDIR="${ROOT_DIR}/target/AppDir"
rm -rf "${APPDIR}"
mkdir -p "${APPDIR}/usr/bin"
mkdir -p "${APPDIR}/usr/share/applications"
mkdir -p "${APPDIR}/usr/share/metainfo"
mkdir -p "${APPDIR}/usr/share/icons/hicolor/256x256/apps"

# Copiar binário
cp "${BIN_PATH}" "${APPDIR}/usr/bin/ekaza-cam"
chmod +x "${APPDIR}/usr/bin/ekaza-cam"

# Copiar arquivos de desktop, metadados e ícone
cp "${ROOT_DIR}/assets/ekaza-cam.desktop" "${APPDIR}/ekaza-cam.desktop"
cp "${ROOT_DIR}/assets/ekaza-cam.desktop" "${APPDIR}/usr/share/applications/ekaza-cam.desktop"

cp "${ROOT_DIR}/assets/io.github.gabrielbraz827.ekaza-cam.metainfo.xml" "${APPDIR}/usr/share/metainfo/io.github.gabrielbraz827.ekaza-cam.metainfo.xml"

cp "${ROOT_DIR}/assets/ekaza-cam.png" "${APPDIR}/ekaza-cam.png"
cp "${ROOT_DIR}/assets/ekaza-cam.png" "${APPDIR}/usr/share/icons/hicolor/256x256/apps/ekaza-cam.png"

# Criar AppRun
cat << 'EOF' > "${APPDIR}/AppRun"
#!/bin/sh
set -e
HERE="$(dirname "$(readlink -f "$0")")"
export PATH="${HERE}/usr/bin:${PATH}"
export LD_LIBRARY_PATH="${HERE}/usr/lib:${LD_LIBRARY_PATH}"
export XDG_DATA_DIRS="${HERE}/usr/share:${XDG_DATA_DIRS:-/usr/local/share:/usr/share}"
exec "${HERE}/usr/bin/ekaza-cam" "$@"
EOF
chmod +x "${APPDIR}/AppRun"

# 3. Obter appimagetool se não estiver instalado
TOOL_BIN=""
if command -v appimagetool >/dev/null 2>&1; then
  TOOL_BIN="$(command -v appimagetool)"
else
  TOOL_CACHE="${ROOT_DIR}/target/appimagetool-x86_64.AppImage"
  if [ ! -f "${TOOL_CACHE}" ]; then
    echo "==> Baixando appimagetool..."
    wget -q -O "${TOOL_CACHE}" "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage"
    chmod +x "${TOOL_CACHE}"
  fi
  TOOL_BIN="${TOOL_CACHE}"
fi

# 4. Gerar AppImage
echo "==> Gerando AppImage em: ${OUTPUT_PATH}"
export ARCH="x86_64"
export APPIMAGE_EXTRACT_AND_RUN=1

"${TOOL_BIN}" "${APPDIR}" "${OUTPUT_PATH}"


chmod +x "${OUTPUT_PATH}"
echo "==> AppImage criado com sucesso: ${OUTPUT_PATH}"
