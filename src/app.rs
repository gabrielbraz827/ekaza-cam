//! Interface gráfica (egui/eframe): descoberta, lista de câmeras, player embutido.
//! Persiste credenciais/preferências e faz cache das câmeras descobertas.

use crate::{config, net, onvif, rtsp, video};
use eframe::egui;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Uma câmera já resolvida (pronta para tocar).
#[derive(Clone)]
pub struct CamEntry {
    pub ip: String,
    pub main_uri: Option<String>,
    pub sub_uri: Option<String>,
    pub status: String,
}

impl CamEntry {
    fn uri(&self, sub: bool) -> Option<&String> {
        if sub {
            self.sub_uri.as_ref().or(self.main_uri.as_ref())
        } else {
            self.main_uri.as_ref().or(self.sub_uri.as_ref())
        }
    }

    fn from_saved(s: config::SavedCamera) -> Self {
        CamEntry {
            ip: s.ip,
            main_uri: s.main_uri,
            sub_uri: s.sub_uri,
            status: s.status,
        }
    }

    fn to_saved(&self) -> config::SavedCamera {
        config::SavedCamera {
            ip: self.ip.clone(),
            main_uri: self.main_uri.clone(),
            sub_uri: self.sub_uri.clone(),
            status: self.status.clone(),
        }
    }
}

/// Resultado de uma varredura/consulta entregue pela thread.
struct ScanOutcome {
    cameras: Vec<CamEntry>,
    /// true substitui a lista (scan completo); false acrescenta (IP manual).
    replace: bool,
}

/// Estado compartilhado com a thread de varredura.
#[derive(Default)]
struct ScanState {
    running: bool,
    log: String,
    result: Option<ScanOutcome>,
}

/// Um stream ativo (processo + textura egui).
struct PlayStream {
    vs: video::VideoStream,
    tex: Option<egui::TextureHandle>,
    last_gen: u64,
}

pub struct App {
    subnet: String,
    user: String,
    pass: String,
    remember_pass: bool,
    timeout: u64,
    use_sub: bool,
    grid: bool,
    manual_ip: String,

    cameras: Vec<CamEntry>,
    selected: Option<usize>,
    streams: HashMap<String, PlayStream>,
    scan: Arc<Mutex<ScanState>>,
    error: Option<String>,
    ffmpeg_ok: bool,
    ffplay_ok: bool,
    last_scan: Option<String>,
    dirty: bool,

    // áudio / gravação / snapshot
    audio_on: bool,
    audio: Option<video::AudioStream>,
    recs: HashMap<String, video::RecordStream>, // key = IP da câmera
    toast: Option<String>,
}

impl App {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let cfg = config::Config::load();
        let subnet = if cfg.subnet.trim().is_empty() {
            net::local_subnet().unwrap_or_default()
        } else {
            cfg.subnet
        };
        App {
            subnet,
            user: cfg.user,
            pass: cfg.pass,
            remember_pass: cfg.remember_pass,
            timeout: cfg.timeout.max(1),
            use_sub: cfg.use_sub,
            grid: cfg.grid,
            manual_ip: String::new(),
            cameras: cfg.cameras.into_iter().map(CamEntry::from_saved).collect(),
            selected: None,
            streams: HashMap::new(),
            scan: Arc::new(Mutex::new(ScanState::default())),
            error: None,
            ffmpeg_ok: video::ffmpeg_available(),
            ffplay_ok: video::ffplay_available(),
            last_scan: cfg.last_scan,
            dirty: false,
            audio_on: false,
            audio: None,
            recs: HashMap::new(),
            toast: None,
        }
    }

    /// Monta a config atual a partir do estado do app.
    fn to_config(&self) -> config::Config {
        config::Config {
            user: self.user.clone(),
            pass: self.pass.clone(),
            remember_pass: self.remember_pass,
            subnet: self.subnet.clone(),
            timeout: self.timeout,
            use_sub: self.use_sub,
            grid: self.grid,
            cameras: self.cameras.iter().map(|c| c.to_saved()).collect(),
            last_scan: self.last_scan.clone(),
        }
    }

    /// Salva a config no disco (e limpa o flag dirty).
    fn persist(&mut self) {
        if let Err(e) = self.to_config().save() {
            self.error = Some(format!("config: {e}"));
        }
        self.dirty = false;
    }

    fn selected_cam(&self) -> Option<&CamEntry> {
        self.selected.and_then(|i| self.cameras.get(i))
    }

    /// Chave do stream em exibição para a câmera selecionada (para snapshot).
    fn selected_play_key(&self) -> Option<String> {
        let cam = self.selected_cam()?;
        if self.grid {
            cam.uri(true).map(|u| format!("{u}#640x360"))
        } else {
            let (w, h) = if self.use_sub {
                (640, 360)
            } else {
                (1280, 720)
            };
            cam.uri(self.use_sub).map(|u| format!("{u}#{w}x{h}"))
        }
    }

    /// Liga/desliga o áudio conforme `audio_on` e a câmera selecionada.
    fn reconcile_audio(&mut self) {
        let want = if self.audio_on {
            self.selected_cam()
                .and_then(|c| c.uri(false).or(c.sub_uri.as_ref()))
                .cloned()
        } else {
            None
        };

        let current = self.audio.as_ref().map(|a| a.url.clone());
        if current == want {
            return;
        }
        self.audio = None; // para o áudio atual
        if let Some(url) = want {
            match video::AudioStream::start(&url) {
                Ok(a) => self.audio = Some(a),
                Err(e) => {
                    self.error = Some(format!("áudio: {e}"));
                    self.audio_on = false;
                }
            }
        }
    }

    /// Salva o frame atual da câmera selecionada como PNG.
    fn do_snapshot(&mut self) {
        let Some(key) = self.selected_play_key() else {
            self.toast = Some("Selecione uma câmera tocando.".into());
            return;
        };
        let Some(ps) = self.streams.get(&key) else {
            return;
        };
        let (w, h) = (ps.vs.frame.w, ps.vs.frame.h);
        let Some(data) = ps.vs.frame.current() else {
            self.toast = Some("Ainda sem imagem para capturar.".into());
            return;
        };
        let ip = self
            .selected_cam()
            .map(|c| c.ip.clone())
            .unwrap_or_default();
        let ts = chrono::Local::now().format("%Y%m%d_%H%M%S");
        let path = output_path(&format!("snapshot_{ip}_{ts}.png"));
        match image::RgbaImage::from_raw(w as u32, h as u32, data)
            .ok_or_else(|| "frame invalido".to_string())
            .and_then(|img| img.save(&path).map_err(|e| e.to_string()))
        {
            Ok(_) => self.toast = Some(format!("📷 Snapshot salvo: {path}")),
            Err(e) => self.toast = Some(format!("Falha no snapshot: {e}")),
        }
    }

    /// Inicia/para a gravação da câmera selecionada (stream principal, sem re-encode).
    fn toggle_record(&mut self) {
        let Some(cam) = self.selected_cam().cloned() else {
            return;
        };
        if self.recs.remove(&cam.ip).is_some() {
            self.toast = Some(format!("⏹ Gravação de {} finalizada.", cam.ip));
            return;
        }
        let Some(url) = cam.uri(false).cloned() else {
            return;
        };
        let ts = chrono::Local::now().format("%Y%m%d_%H%M%S");
        let path = output_path(&format!("rec_{}_{ts}.mp4", cam.ip));
        match video::RecordStream::start(&url, &path) {
            Ok(r) => {
                self.toast = Some(format!("⏺ Gravando {} → {}", cam.ip, r.path));
                self.recs.insert(cam.ip, r);
            }
            Err(e) => self.toast = Some(format!("Falha ao gravar: {e}")),
        }
    }

    /// Abre a câmera selecionada em player externo (com áudio).
    fn open_external(&mut self) {
        let Some(cam) = self.selected_cam() else {
            return;
        };
        let Some(url) = cam.uri(self.use_sub) else {
            return;
        };
        let full = rtsp::with_credentials(url, &self.user, &self.pass);
        let players: &[(&str, &[&str])] = &[
            ("ffplay", &["-rtsp_transport", "tcp", "-fflags", "nobuffer"]),
            ("mpv", &["--rtsp-transport=tcp"]),
            ("vlc", &["--rtsp-tcp"]),
        ];
        for (bin, args) in players {
            let mut cmd = std::process::Command::new(bin);
            cmd.args(*args).arg(&full);
            if cmd.spawn().is_ok() {
                self.toast = Some(format!("↗ Abrindo em {bin}"));
                return;
            }
        }
        self.toast = Some("Nenhum player externo encontrado (ffplay/mpv/vlc).".into());
    }

    fn start_scan(&self) {
        {
            let mut s = self.scan.lock().unwrap();
            if s.running {
                return;
            }
            s.running = true;
            s.log = "procurando...".into();
        }
        let scan = self.scan.clone();
        let subnet = if self.subnet.trim().is_empty() {
            None
        } else {
            Some(self.subnet.trim().to_string())
        };
        let user = self.user.clone();
        let pass = self.pass.clone();
        let timeout = Duration::from_secs(self.timeout.max(1));

        std::thread::spawn(move || {
            let devices = net::discover(subnet, timeout);
            let mut cams = Vec::new();
            for d in &devices {
                cams.push(resolve_camera(d, &user, &pass, timeout));
            }
            let mut s = scan.lock().unwrap();
            s.log = format!("{} câmera(s) encontrada(s)", cams.len());
            s.result = Some(ScanOutcome {
                cameras: cams,
                replace: true,
            });
            s.running = false;
        });
    }

    fn add_manual(&self) {
        let ip = self.manual_ip.trim().to_string();
        if ip.parse::<std::net::Ipv4Addr>().is_err() {
            return;
        }
        let scan = self.scan.clone();
        let user = self.user.clone();
        let pass = self.pass.clone();
        let timeout = Duration::from_secs(self.timeout.max(1));
        {
            let mut s = scan.lock().unwrap();
            s.running = true;
            s.log = format!("consultando {ip}...");
        }
        std::thread::spawn(move || {
            let addr: std::net::Ipv4Addr = ip.parse().unwrap();
            let entry = match net::resolve_single(addr, 0, timeout) {
                Some(d) => resolve_camera(&d, &user, &pass, timeout),
                None => CamEntry {
                    ip: addr.to_string(),
                    main_uri: None,
                    sub_uri: None,
                    status: "ONVIF não respondeu".into(),
                },
            };
            let mut s = scan.lock().unwrap();
            s.log = format!("{} adicionada", entry.ip);
            s.result = Some(ScanOutcome {
                cameras: vec![entry],
                replace: false,
            });
            s.running = false;
        });
    }

    /// Lista das câmeras que devem estar tocando agora: (key, url, w, h).
    fn desired_streams(&self) -> Vec<(String, String, usize, usize)> {
        let mut out = Vec::new();
        if self.grid {
            for cam in &self.cameras {
                if let Some(u) = cam.uri(true) {
                    out.push((format!("{u}#640x360"), u.clone(), 640, 360));
                }
            }
        } else if let Some(i) = self.selected {
            if let Some(cam) = self.cameras.get(i) {
                if let Some(u) = cam.uri(self.use_sub) {
                    let (w, h) = if self.use_sub {
                        (640, 360)
                    } else {
                        (1280, 720)
                    };
                    out.push((format!("{u}#{w}x{h}"), u.clone(), w, h));
                }
            }
        }
        out
    }
}

/// Resolve uma câmera em CamEntry via ONVIF.
fn resolve_camera(d: &net::Device, user: &str, pass: &str, timeout: Duration) -> CamEntry {
    match onvif::query(d.ip, &d.device_service, user, pass, timeout) {
        Ok(info) => {
            let main = info.profiles.first().map(|p| rtsp::normalize(&p.uri));
            let sub = info.profiles.get(1).map(|p| rtsp::normalize(&p.uri));
            let status = if info.auth_required {
                "ONVIF: autenticado".to_string()
            } else {
                "ONVIF: aberto".to_string()
            };
            CamEntry {
                ip: d.ip.to_string(),
                main_uri: main,
                sub_uri: sub,
                status,
            }
        }
        Err(e) => CamEntry {
            ip: d.ip.to_string(),
            main_uri: None,
            sub_uri: None,
            status: format!("erro: {e}"),
        },
    }
}

/// Reconcilia o conjunto de streams ativos com o desejado.
fn reconcile(
    streams: &mut HashMap<String, PlayStream>,
    desired: &[(String, String, usize, usize)],
    ctx: &egui::Context,
    error: &mut Option<String>,
) {
    use std::collections::HashSet;
    let want: HashSet<&String> = desired.iter().map(|(k, ..)| k).collect();
    streams.retain(|k, _| want.contains(k));

    for (key, url, w, h) in desired {
        if streams.contains_key(key) {
            continue;
        }
        let ctx_cb = ctx.clone();
        match video::VideoStream::start(url, *w, *h, move || ctx_cb.request_repaint()) {
            Ok(vs) => {
                streams.insert(
                    key.clone(),
                    PlayStream {
                        vs,
                        tex: None,
                        last_gen: 0,
                    },
                );
            }
            Err(e) => *error = Some(format!("ffmpeg: {e}")),
        }
    }
}

/// Desenha um stream (atualiza textura e exibe).
fn draw_stream(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    ps: &mut PlayStream,
    key: &str,
    size: egui::Vec2,
) {
    let (w, h) = (ps.vs.frame.w, ps.vs.frame.h);
    if let Some(data) = ps.vs.frame.latest(&mut ps.last_gen) {
        let img = egui::ColorImage::from_rgba_unmultiplied([w, h], &data);
        match ps.tex.as_mut() {
            Some(t) => t.set(img, egui::TextureOptions::LINEAR),
            None => ps.tex = Some(ctx.load_texture(key, img, egui::TextureOptions::LINEAR)),
        }
    }
    match &ps.tex {
        Some(t) => {
            let sized = egui::load::SizedTexture::new(t.id(), egui::vec2(w as f32, h as f32));
            ui.add(egui::Image::from_texture(sized).fit_to_exact_size(size));
        }
        None => {
            ui.allocate_ui(size, |ui| {
                ui.centered_and_justified(|ui| {
                    ui.spinner();
                    ui.label("conectando ao stream...");
                });
            });
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 1) Colhe resultado da varredura (uma vez).
        {
            let mut s = self.scan.lock().unwrap();
            if let Some(out) = s.result.take() {
                if out.replace {
                    self.cameras = out.cameras;
                } else {
                    for cam in out.cameras {
                        if let Some(existing) = self.cameras.iter_mut().find(|c| c.ip == cam.ip) {
                            *existing = cam;
                        } else {
                            self.cameras.push(cam);
                        }
                    }
                }
                self.last_scan = Some(chrono::Local::now().format("%d/%m/%Y %H:%M").to_string());
                self.dirty = true;
                if self.selected.is_none() && !self.cameras.is_empty() {
                    self.selected = Some(0);
                }
            }
        }

        // 2) Barra superior.
        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.heading("📷 ekaza-cam");
                ui.separator();
                ui.label("Sub-rede:");
                if ui
                    .add(egui::TextEdit::singleline(&mut self.subnet).desired_width(110.0))
                    .lost_focus()
                {
                    self.dirty = true;
                }
                ui.label("Usuário:");
                if ui
                    .add(egui::TextEdit::singleline(&mut self.user).desired_width(80.0))
                    .lost_focus()
                {
                    self.dirty = true;
                }
                ui.label("Senha:");
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut self.pass)
                            .password(true)
                            .desired_width(100.0),
                    )
                    .lost_focus()
                {
                    self.dirty = true;
                }
                let running = self.scan.lock().unwrap().running;
                ui.add_enabled_ui(!running, |ui| {
                    if ui.button("🔎 Procurar").clicked() {
                        self.start_scan();
                    }
                });
                if running {
                    ui.spinner();
                }
            });
            ui.horizontal(|ui| {
                if ui.checkbox(&mut self.grid, "Grade (todas)").changed() {
                    self.dirty = true;
                }
                if ui
                    .checkbox(&mut self.use_sub, "Sub-stream (leve)")
                    .changed()
                {
                    self.dirty = true;
                }
                if ui
                    .checkbox(&mut self.remember_pass, "Lembrar senha")
                    .changed()
                {
                    self.dirty = true;
                }
                ui.separator();
                ui.label("IP manual:");
                ui.add(egui::TextEdit::singleline(&mut self.manual_ip).desired_width(110.0));
                if ui.button("➕ Adicionar").clicked() {
                    self.add_manual();
                }
                if ui.button("⏹ Parar tudo").clicked() {
                    self.streams.clear();
                }
            });
            // Controles da câmera selecionada
            ui.horizontal(|ui| {
                let has_sel = self.selected_cam().is_some();
                let recording = self
                    .selected_cam()
                    .map(|c| self.recs.contains_key(&c.ip))
                    .unwrap_or(false);
                ui.add_enabled_ui(has_sel, |ui| {
                    let mut audio = self.audio_on;
                    if ui.toggle_value(&mut audio, "🔊 Áudio").changed() {
                        self.audio_on = audio;
                    }
                    let rec_label = if recording {
                        "⏹ Parar gravação"
                    } else {
                        "⏺ Gravar"
                    };
                    if ui.button(rec_label).clicked() {
                        self.toggle_record();
                    }
                    if ui.button("📷 Snapshot").clicked() {
                        self.do_snapshot();
                    }
                    if ui.button("↗ Abrir externo").clicked() {
                        self.open_external();
                    }
                });
                if recording {
                    ui.colored_label(egui::Color32::from_rgb(255, 80, 80), "● REC");
                }
            });
            ui.horizontal(|ui| {
                let log = self.scan.lock().unwrap().log.clone();
                if !log.is_empty() {
                    ui.label(log);
                    ui.separator();
                }
                if let Some(ls) = &self.last_scan {
                    ui.weak(format!("última varredura: {ls} (cache)"));
                }
            });
            ui.add_space(4.0);
            if !self.ffmpeg_ok {
                ui.colored_label(
                    egui::Color32::RED,
                    "⚠ ffmpeg não encontrado no PATH — o player embutido não vai funcionar.",
                );
            }
            if !self.ffplay_ok {
                ui.colored_label(
                    egui::Color32::YELLOW,
                    "⚠ ffplay não encontrado — áudio e “abrir externo” podem não funcionar.",
                );
            }
            if let Some(e) = &self.error {
                ui.colored_label(egui::Color32::LIGHT_RED, e);
            }
            if let Some(t) = &self.toast {
                ui.colored_label(egui::Color32::LIGHT_GREEN, t);
            }
        });

        // 3) Lista lateral de câmeras.
        egui::SidePanel::left("cams")
            .resizable(true)
            .default_width(220.0)
            .show(ctx, |ui| {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.strong("Câmeras");
                    if !self.cameras.is_empty() && ui.small_button("🗑 limpar").clicked() {
                        self.cameras.clear();
                        self.selected = None;
                        self.streams.clear();
                        self.dirty = true;
                    }
                });
                ui.separator();
                if self.cameras.is_empty() {
                    ui.label("Nenhuma ainda.\nClique em “Procurar”.");
                }
                for i in 0..self.cameras.len() {
                    let cam = self.cameras[i].clone();
                    let selected = self.selected == Some(i);
                    let resp =
                        ui.selectable_label(selected, format!("🎥 {}\n   {}", cam.ip, cam.status));
                    if resp.clicked() {
                        self.selected = Some(i);
                        self.grid = false;
                        self.dirty = true;
                    }
                }
            });

        // 4) Área central = player.
        let desired = self.desired_streams();
        reconcile(&mut self.streams, &desired, ctx, &mut self.error);
        self.reconcile_audio();

        egui::CentralPanel::default().show(ctx, |ui| {
            if desired.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.label("Selecione uma câmera na lista, ou ative a “Grade”.");
                });
                return;
            }

            if self.grid {
                let n = desired.len();
                let cols = (n as f32).sqrt().ceil() as usize;
                let cols = cols.max(1);
                let spacing = 6.0;
                let avail_w = ui.available_width();
                let tile_w = ((avail_w - spacing * (cols as f32 + 1.0)) / cols as f32).max(160.0);
                let tile_h = tile_w * 9.0 / 16.0;
                let size = egui::vec2(tile_w, tile_h);

                egui::ScrollArea::vertical().show(ui, |ui| {
                    for row in desired.chunks(cols) {
                        ui.horizontal(|ui| {
                            for (key, ..) in row {
                                ui.vertical(|ui| {
                                    ui.small(stream_label(key));
                                    if let Some(ps) = self.streams.get_mut(key) {
                                        draw_stream(ui, ctx, ps, key, size);
                                    }
                                });
                            }
                        });
                    }
                });
            } else {
                let (key, ..) = &desired[0];
                let size = ui.available_size();
                if let Some(ps) = self.streams.get_mut(key) {
                    draw_stream(ui, ctx, ps, key, size);
                }
            }
        });

        // 5) Persiste a config se algo mudou.
        if self.dirty {
            self.persist();
        }

        // 6) Mantém animação fluindo enquanto há streams.
        if !self.streams.is_empty() {
            ctx.request_repaint_after(Duration::from_millis(80));
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.persist();
    }
}

/// Monta o caminho de saída (em ~/Vídeos ou ~/Pictures, senão no diretório atual).
fn output_path(name: &str) -> String {
    let dir = dirs_media().unwrap_or_else(|| ".".to_string());
    std::path::Path::new(&dir)
        .join(name)
        .to_string_lossy()
        .into_owned()
}

/// Descobre uma pasta de mídia do usuário, multiplataforma.
fn dirs_media() -> Option<String> {
    let base = directories::UserDirs::new()?;
    if let Some(v) = base.video_dir() {
        return Some(v.to_string_lossy().into_owned());
    }
    if let Some(p) = base.picture_dir() {
        return Some(p.to_string_lossy().into_owned());
    }
    Some(base.home_dir().to_string_lossy().into_owned())
}

/// Deriva um rótulo legível (IP) a partir da key do stream.
fn stream_label(key: &str) -> String {
    key.strip_prefix("rtsp://")
        .and_then(|s| s.split([':', '/']).next())
        .unwrap_or(key)
        .to_string()
}
