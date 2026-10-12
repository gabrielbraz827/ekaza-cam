# ekaza-cam

Aplicativo **desktop** (Linux e Windows) para descobrir câmeras **ONVIF/Tuya** na
rede local, resolver automaticamente o stream **RTSP** e assistir com **player
embutido** — tudo num único binário em Rust.

Feito originalmente para câmeras WiFi white-label da **Ekaza** (que por baixo são
Tuya), mas funciona com qualquer câmera que fale **ONVIF**.

[![CI](https://github.com/gabrielbraz827/ekaza-cam/actions/workflows/ci.yml/badge.svg)](https://github.com/gabrielbraz827/ekaza-cam/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

---

## ✨ Recursos

- 🔎 **Descoberta automática** na rede via ONVIF WS-Discovery (multicast + varredura unicast do /24).
- 🔗 Resolve as **URLs RTSP** de cada perfil via ONVIF (`GetProfiles`/`GetStreamUri`, com WS-Security quando a câmera exige).
- 🖼️ **Player embutido** na própria janela: visão única ou **grade** com todas as câmeras.
- 🎚️ Alterna **stream principal** (ex.: H.265, alta resolução) ↔ **sub-stream** (H.264, leve).
- 🔊 **Áudio** da câmera selecionada.
- ⏺️ **Gravação** em MP4 sem re-encode (`-c copy`, vídeo + áudio).
- 📷 **Snapshot** do frame atual em PNG.
- ↗️ **Abrir no player externo** (ffplay/mpv/vlc) com áudio.
- 💾 **Persistência**: guarda usuário/senha/preferências e faz **cache das câmeras** — não re-escaneia a rede a cada abertura; só quando você mandar procurar de novo.
- ➕ Adicionar câmera por **IP manual**.

## 📦 Instalação

### Binários prontos
Baixe o executável da sua plataforma na página de [**Releases**](https://github.com/gabrielbraz827/ekaza-cam/releases):

- **Linux (AppImage)**: Baixe `ekaza-cam-linux-x86_64.AppImage`, dê permissão de execução e rode:
  ```bash
  chmod +x ekaza-cam-linux-x86_64.AppImage
  ./ekaza-cam-linux-x86_64.AppImage
  ```
- **Windows**: Baixe `ekaza-cam-windows-x86_64.zip`, descompacte e execute `ekaza-cam.exe`.

### Compilar do código-fonte
Requer [Rust](https://rustup.rs/) (stable, ≥ 1.80):

```bash
git clone https://github.com/gabrielbraz827/ekaza-cam.git
cd ekaza-cam
cargo build --release
./target/release/ekaza-cam
```

Para gerar o **AppImage** localmente:

```bash
./scripts/build-appimage.sh
```

No Linux, para compilar são necessárias algumas libs de sistema (X11/Wayland/GL):

```bash
# Debian/Ubuntu
sudo apt-get install -y libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev \
  libxkbcommon-dev libgl1-mesa-dev libwayland-dev
```

### Dependência de runtime: FFmpeg
O player embutido usa o **FFmpeg** (binários `ffmpeg` e `ffplay`) em runtime.

- **Linux**: `sudo pacman -S ffmpeg` / `sudo apt install ffmpeg`
- **Windows**: baixe em [ffmpeg.org](https://ffmpeg.org/download.html) e adicione ao `PATH`.

## 🚀 Uso

1. Preencha **Usuário** (geralmente `admin`) e **Senha** — a senha do **ONVIF** que você
   habilitou no app da câmera.
2. Clique em **🔎 Procurar**. As câmeras aparecem na lista à esquerda (e ficam em cache).
3. Clique numa câmera para assistir, ou marque **Grade (todas)**.
4. Use **🔊 Áudio**, **⏺ Gravar**, **📷 Snapshot** ou **↗ Abrir externo** na câmera selecionada.
5. Não apareceu no scan? Informe o **IP manual** e **➕ Adicionar**.

> Dica: ative o **ONVIF** nas configurações da câmera no app do fabricante.
> Em muitas câmeras Tuya isso liga o servidor RTSP na porta 554.

## 💾 Onde ficam os dados

Configuração e cache de câmeras:

| SO | Caminho |
|----|---------|
| Linux | `~/.config/ekaza-cam/config.json` |
| Windows | `%APPDATA%\ekaza-cam\config.json` |

Gravações e snapshots vão para a pasta de **Vídeos**/**Imagens** do usuário.

> **Segurança**: a senha é salva **em texto puro** no arquivo de config (padrão de muitos
> apps desktop). Desmarque **“Lembrar senha”** para não gravá-la. O arquivo fica no seu
> diretório de usuário.

## 🧠 Como funciona

```
WS-Discovery (UDP 3702) ─► device_service ONVIF ─► GetProfiles / GetStreamUri ─► rtsp://…
                                                                                   │
                                      ffmpeg (decode → frames RGBA no pipe) ◄───────┘
                                                   │
                                        textura egui desenhada na janela
```

O player embutido mostra **vídeo**; o **áudio** roda num processo `ffplay` dedicado
(baixa latência e sem bindings nativos do FFmpeg — só o binário precisa estar instalado).

```
src/
  main.rs    → janela (eframe)
  app.rs     → interface (egui): scan, lista, grade, player, controles
  config.rs  → persistência (config + cache de câmeras)
  net.rs     → descoberta WS-Discovery
  onvif.rs   → GetProfiles + GetStreamUri (WS-Security)
  rtsp.rs    → normalização de URL e checagem de auth
  video.rs   → player/áudio/gravação via FFmpeg
  crypto.rs  → SHA1/MD5/Base64
```

## 🤝 Contribuindo

PRs são bem-vindos! O CI roda `fmt`, `clippy` e build para Linux e Windows.
Antes de abrir PR, rode `cargo fmt --all` e `cargo clippy --all-targets`.

## ⚖️ Aviso legal

Use **apenas em câmeras que você possui ou tem autorização** para acessar. Este projeto
é para acesso local legítimo aos seus próprios dispositivos. Você é responsável pelo uso.

## 📄 Licença

[MIT](LICENSE) © gabrielbraz827
