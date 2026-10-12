//! Player embutido: roda `ffmpeg` decodificando o RTSP para frames RGBA crus
//! num pipe, lidos por uma thread e disponibilizados para a UI desenhar.
#![allow(dead_code)]

use std::io::{Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

/// Buffer compartilhado com o frame mais recente.
pub struct FrameBuf {
    pub w: usize,
    pub h: usize,
    buf: Mutex<Vec<u8>>,
    generation: AtomicU64,
}

impl FrameBuf {
    fn new(w: usize, h: usize) -> Self {
        FrameBuf {
            w,
            h,
            buf: Mutex::new(vec![0u8; w * h * 4]),
            generation: AtomicU64::new(0),
        }
    }

    /// Retorna uma cópia do frame se houver um mais novo que `last`.
    /// Atualiza `last` para a geração retornada.
    pub fn latest(&self, last: &mut u64) -> Option<Vec<u8>> {
        let g = self.generation.load(Ordering::Acquire);
        if g == *last {
            return None;
        }
        *last = g;
        Some(self.buf.lock().unwrap().clone())
    }

    /// Cópia do frame atual (para snapshot), ou None se nada foi recebido ainda.
    pub fn current(&self) -> Option<Vec<u8>> {
        if self.generation.load(Ordering::Acquire) == 0 {
            return None;
        }
        Some(self.buf.lock().unwrap().clone())
    }
}

/// Um stream de vídeo ativo (processo ffmpeg + thread leitora).
pub struct VideoStream {
    pub frame: Arc<FrameBuf>,
    pub url: String,
    child: Child,
    _reader: Option<JoinHandle<()>>,
}

impl VideoStream {
    /// Inicia o ffmpeg decodificando `url` para `w`x`h` RGBA.
    /// `on_frame` é chamado a cada frame novo (use para pedir repaint da UI).
    pub fn start<F>(url: &str, w: usize, h: usize, on_frame: F) -> std::io::Result<Self>
    where
        F: Fn() + Send + 'static,
    {
        let mut child = Command::new("ffmpeg")
            .args([
                "-nostdin",
                "-loglevel",
                "error",
                "-rtsp_transport",
                "tcp",
                "-fflags",
                "nobuffer",
                "-flags",
                "low_delay",
                "-i",
                url,
                "-an",
                "-f",
                "rawvideo",
                "-pix_fmt",
                "rgba",
                "-vf",
                &format!("scale={w}:{h}"),
                "pipe:1",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .stdin(Stdio::null())
            .spawn()?;

        let mut stdout = child
            .stdout
            .take()
            .ok_or_else(|| std::io::Error::other("sem stdout do ffmpeg"))?;

        let frame = Arc::new(FrameBuf::new(w, h));
        let frame_thread = frame.clone();
        let frame_size = w * h * 4;

        let reader = std::thread::spawn(move || {
            let mut tmp = vec![0u8; frame_size];
            loop {
                match read_full(&mut stdout, &mut tmp) {
                    Ok(true) => {
                        {
                            let mut guard = frame_thread.buf.lock().unwrap();
                            guard.copy_from_slice(&tmp);
                        }
                        frame_thread.generation.fetch_add(1, Ordering::Release);
                        on_frame();
                    }
                    _ => break, // EOF ou erro: ffmpeg terminou
                }
            }
        });

        Ok(VideoStream {
            frame,
            url: url.to_string(),
            child,
            _reader: Some(reader),
        })
    }
}

impl Drop for VideoStream {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Lê exatamente `buf.len()` bytes; Ok(true) completo, Ok(false) EOF, Err em erro.
fn read_full<R: Read>(r: &mut R, buf: &mut [u8]) -> std::io::Result<bool> {
    let mut read = 0;
    while read < buf.len() {
        match r.read(&mut buf[read..]) {
            Ok(0) => return Ok(false),
            Ok(n) => read += n,
            Err(ref e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(true)
}

/// Verifica se o `ffmpeg` está disponível no PATH.
pub fn ffmpeg_available() -> bool {
    bin_available("ffmpeg")
}

/// Verifica se o `ffplay` está disponível (usado para áudio e player externo).
pub fn ffplay_available() -> bool {
    bin_available("ffplay")
}

fn bin_available(bin: &str) -> bool {
    Command::new(bin)
        .arg("-version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Reproduz apenas o áudio do RTSP via `ffplay -nodisp`
/// (o SDL do ffplay escolhe sozinho o backend: PipeWire/Pulse/ALSA).
pub struct AudioStream {
    child: Child,
    pub url: String,
}

impl AudioStream {
    pub fn start(url: &str) -> std::io::Result<Self> {
        let child = Command::new("ffplay")
            .args([
                "-nodisp",
                "-autoexit",
                "-loglevel",
                "quiet",
                "-rtsp_transport",
                "tcp",
                "-fflags",
                "nobuffer",
                "-i",
                url,
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        Ok(AudioStream {
            child,
            url: url.to_string(),
        })
    }
}

impl Drop for AudioStream {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Grava o stream RTSP (vídeo + áudio) em arquivo, sem re-encode (`-c copy`).
pub struct RecordStream {
    child: Child,
    pub path: String,
}

impl RecordStream {
    pub fn start(url: &str, path: &str) -> std::io::Result<Self> {
        let child = Command::new("ffmpeg")
            .args([
                "-nostdin",
                "-loglevel",
                "error",
                "-rtsp_transport",
                "tcp",
                "-i",
                url,
                "-c",
                "copy",
                "-tag:v",
                "hvc1", // compatibilidade do H.265 em MP4
                "-y",
                path,
            ])
            // stdin aberto para enviar "q" e finalizar o MP4 corretamente
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        Ok(RecordStream {
            child,
            path: path.to_string(),
        })
    }
}

impl Drop for RecordStream {
    fn drop(&mut self) {
        // Finaliza graciosamente para fechar o container MP4 (moov atom).
        if let Some(mut stdin) = self.child.stdin.take() {
            let _ = stdin.write_all(b"q");
            let _ = stdin.flush();
        }
        let _ = self.child.wait();
    }
}
