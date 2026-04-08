//! WASAPI Loopback - Captura de Audio de Videollamadas (Windows)
//!
//! Este módulo implementa captura de loopback usando WASAPI nativo,
//! permitiendo capturar el audio del interlocutor en videollamadas
//! (Zoom, Meet, Teams) SIN necesidad de instalar software adicional.
//!
//! Funciona en Windows 10/11 sin configuración extra.

#![cfg(target_os = "windows")]

use windows::Win32::{
    Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0},
    Media::Audio::*,
    System::Com::*,
    System::Threading::{CreateEventW, WaitForSingleObject},
};

use anyhow::{anyhow, Result};
use futures_channel::mpsc;
use futures_util::{Stream, StreamExt};
use log::{debug, error, info};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};

/// Handle para mantener COM inicializado durante el lifetime de la captura
struct ComRuntime;

impl ComRuntime {
    fn new() -> Result<Self> {
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED)
                .ok()
                .map_err(|e| anyhow!("Failed to initialize COM: {:?}", e))?;
        }
        Ok(Self)
    }
}

impl Drop for ComRuntime {
    fn drop(&mut self) {
        unsafe {
            CoUninitialize();
        }
    }
}

/// FIX C1: RAII wrapper para WAVEFORMATEX que garantiza liberación de memoria
/// Previene memory leaks si Initialize() u otras operaciones fallan
struct WaveFormatGuard {
    ptr: *mut WAVEFORMATEX,
}

impl WaveFormatGuard {
    /// Envuelve un puntero WAVEFORMATEX para liberación automática
    unsafe fn new(ptr: *mut WAVEFORMATEX) -> Self {
        Self { ptr }
    }

    /// Obtiene referencia al formato (unsafe porque ptr debe ser válido)
    unsafe fn as_ref(&self) -> &WAVEFORMATEX {
        &*self.ptr
    }

    /// Obtiene el puntero raw (para pasar a Initialize)
    fn as_ptr(&self) -> *const WAVEFORMATEX {
        self.ptr as *const _
    }
}

impl Drop for WaveFormatGuard {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            unsafe {
                CoTaskMemFree(Some(self.ptr as *const _ as *const _));
            }
            debug!("🧹 WAVEFORMATEX memory freed via RAII");
        }
    }
}

/// Captura de audio del sistema via WASAPI loopback
///
/// Captura el audio que sale por los speakers/auriculares,
/// ideal para transcribir al interlocutor en videollamadas.
pub struct WasapiLoopbackCapture {
    _com: ComRuntime,
    sample_rate: u32,
    #[allow(dead_code)]
    channels: u16,
}

impl WasapiLoopbackCapture {
    /// Crear nueva instancia de captura WASAPI loopback
    ///
    /// Retorna Ok si WASAPI loopback está disponible en el sistema,
    /// Err si no se puede inicializar (ej: no hay dispositivo de salida)
    pub fn new() -> Result<Self> {
        let com = ComRuntime::new()?;

        unsafe {
            // 1. Crear enumerador de dispositivos
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                    .map_err(|e| anyhow!("Failed to create device enumerator: {:?}", e))?;

            // 2. Obtener dispositivo de salida por defecto (para loopback)
            let device = enumerator
                .GetDefaultAudioEndpoint(eRender, eConsole)
                .map_err(|e| anyhow!("No default output device for loopback: {:?}", e))?;

            info!("🎧 WASAPI loopback: Found default output device");

            // 3. Obtener formato de audio del dispositivo
            let audio_client: IAudioClient = device
                .Activate(CLSCTX_ALL, None)
                .map_err(|e| anyhow!("Failed to activate audio client: {:?}", e))?;

            let format_ptr = audio_client
                .GetMixFormat()
                .map_err(|e| anyhow!("Failed to get mix format: {:?}", e))?;

            let format = &*format_ptr;
            let sample_rate = format.nSamplesPerSec;
            let channels = format.nChannels;

            // Liberar formato
            CoTaskMemFree(Some(format_ptr as *const _ as *const _));

            info!(
                "✅ WASAPI loopback initialized: {}Hz, {} channels",
                sample_rate, channels
            );

            Ok(Self {
                _com: com,
                sample_rate,
                channels,
            })
        }
    }

    /// Verificar si WASAPI loopback está disponible en este sistema
    pub fn is_available() -> bool {
        Self::new().is_ok()
    }

    /// Obtener sample rate del dispositivo de salida
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Iniciar captura y retornar stream de muestras de audio
    ///
    /// El stream produce muestras f32 mono (si el dispositivo es estéreo,
    /// se mezclan los canales a mono).
    pub fn start_capture(&self) -> Result<WasapiLoopbackStream> {
        let (tx, rx) = mpsc::unbounded::<Vec<f32>>();
        let (drop_tx, drop_rx) = std::sync::mpsc::channel::<()>();

        let running = Arc::new(AtomicBool::new(true));
        let running_clone = running.clone();
        let sample_rate = self.sample_rate;

        // Iniciar thread de captura
        std::thread::spawn(move || {
            if let Err(e) = capture_loop(tx, drop_rx, running_clone) {
                error!("❌ WASAPI loopback error: {}", e);
            }
        });

        let receiver = rx.map(futures_util::stream::iter).flatten();

        Ok(WasapiLoopbackStream {
            _running: running,
            drop_tx,
            sample_rate,
            receiver: Box::pin(receiver),
        })
    }
}

/// Loop de captura WASAPI (corre en thread separado)
fn capture_loop(
    tx: mpsc::UnboundedSender<Vec<f32>>,
    drop_rx: std::sync::mpsc::Receiver<()>,
    running: Arc<AtomicBool>,
) -> Result<()> {
    unsafe {
        // Inicializar COM en este thread
        CoInitializeEx(None, COINIT_MULTITHREADED)
            .ok()
            .map_err(|e| anyhow!("COM init failed in capture thread: {:?}", e))?;

        // Obtener dispositivo de salida
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;

        let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole)?;

        // Obtener audio client
        let audio_client: IAudioClient = device.Activate(CLSCTX_ALL, None)?;

        // Obtener formato - FIX C1: Usar RAII guard para prevenir memory leak
        let format_ptr = audio_client.GetMixFormat()?;
        let format_guard = WaveFormatGuard::new(format_ptr);
        let format = format_guard.as_ref();
        let channels = format.nChannels as usize;
        let sample_rate = format.nSamplesPerSec;
        let bits_per_sample = format.wBitsPerSample;

        debug!(
            "🎧 WASAPI capture: {}Hz, {} ch, {} bits",
            sample_rate, channels, bits_per_sample
        );

        // Duración del buffer: 100ms en unidades de 100 nanosegundos
        let buffer_duration: i64 = 1_000_000; // 100ms

        // Inicializar con flag LOOPBACK
        // IMPORTANTE: Debe ser SHARED mode para loopback
        // Si Initialize() falla, format_guard se dropea automáticamente y libera memoria
        audio_client
            .Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                AUDCLNT_STREAMFLAGS_LOOPBACK,
                buffer_duration,
                0,
                format_guard.as_ptr(),
                None,
            )
            .map_err(|e| anyhow!("Failed to initialize loopback: {:?}", e))?;

        // Obtener capture client
        let capture_client: IAudioCaptureClient = audio_client.GetService()?;

        // Obtener tamaño del buffer
        let buffer_size = audio_client.GetBufferSize()?;
        debug!("🎧 WASAPI buffer: {} frames", buffer_size);

        // Crear evento para sincronización
        let event: HANDLE = CreateEventW(None, false, false, None)?;
        audio_client.SetEventHandle(event)?;

        // Iniciar captura
        audio_client.Start()?;
        info!("🎧 WASAPI loopback capture started");

        // Loop de captura
        'capture: loop {
            // Verificar si debemos parar
            if drop_rx.try_recv().is_ok() || !running.load(Ordering::Relaxed) {
                debug!("🎧 Stop signal received");
                break 'capture;
            }

            // Esperar evento o timeout (10ms para ser responsivo al stop)
            let wait_result = WaitForSingleObject(event, 10);

            // Solo procesar si el evento fue señalado o timeout
            if wait_result != WAIT_OBJECT_0 {
                continue;
            }

            // Procesar todos los paquetes disponibles
            loop {
                let packet_length = match capture_client.GetNextPacketSize() {
                    Ok(len) => len,
                    Err(_) => break,
                };

                if packet_length == 0 {
                    break;
                }

                let mut data_ptr: *mut u8 = std::ptr::null_mut();
                let mut frames_available = 0u32;
                let mut flags = 0u32;

                if capture_client
                    .GetBuffer(&mut data_ptr, &mut frames_available, &mut flags, None, None)
                    .is_err()
                {
                    break;
                }

                if frames_available > 0 && !data_ptr.is_null() {
                    // Verificar si el buffer tiene silencio (flags AUDCLNT_BUFFERFLAGS_SILENT)
                    let is_silent = (flags & 2) != 0; // AUDCLNT_BUFFERFLAGS_SILENT = 2

                    let mono_samples = if is_silent {
                        // Enviar ceros si es silencio
                        vec![0.0f32; frames_available as usize]
                    } else {
                        // Convertir samples a f32 mono
                        let samples = std::slice::from_raw_parts(
                            data_ptr as *const f32,
                            (frames_available as usize) * channels,
                        );

                        // Mezclar a mono si es estéreo
                        if channels > 1 {
                            samples
                                .chunks(channels)
                                .map(|chunk| chunk.iter().sum::<f32>() / channels as f32)
                                .collect()
                        } else {
                            samples.to_vec()
                        }
                    };

                    // Enviar samples (ignorar error si receiver cerrado)
                    if tx.unbounded_send(mono_samples).is_err() {
                        debug!("🎧 Receiver closed, stopping capture");
                        break 'capture;
                    }
                }

                let _ = capture_client.ReleaseBuffer(frames_available);
            }
        }

        // Cleanup - FIX C1: format_guard se dropea automáticamente al salir del scope
        let _ = audio_client.Stop();
        let _ = CloseHandle(event);
        // format_guard RAII libera memoria automáticamente
        drop(format_guard);
        CoUninitialize();

        info!("🎧 WASAPI loopback capture stopped");
    }

    Ok(())
}

/// Stream de muestras de audio del sistema (interlocutor)
pub struct WasapiLoopbackStream {
    _running: Arc<AtomicBool>,
    drop_tx: std::sync::mpsc::Sender<()>,
    sample_rate: u32,
    receiver: Pin<Box<dyn Stream<Item = f32> + Send + Sync>>,
}

impl Drop for WasapiLoopbackStream {
    fn drop(&mut self) {
        // Señalar al thread de captura que pare
        let _ = self.drop_tx.send(());
    }
}

impl Stream for WasapiLoopbackStream {
    type Item = f32;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        use futures_util::stream::StreamExt;
        self.receiver.as_mut().poll_next_unpin(cx)
    }
}

impl WasapiLoopbackStream {
    /// Obtener sample rate del stream
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }
}
