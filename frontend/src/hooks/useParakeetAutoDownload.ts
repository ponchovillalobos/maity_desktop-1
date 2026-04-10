import { useState, useEffect, useCallback, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { useAuth } from '@/contexts/AuthContext';

interface ParakeetAutoDownloadState {
  /** Model is downloaded and ready on disk */
  isModelReady: boolean;
  /** Model is currently downloading */
  isDownloading: boolean;
  downloadProgress: number;
  /**
   * UX-LOADING-MODEL (PERF-005): the model is loaded IN MEMORY and ready for inference.
   * After startup preload, this flips to true — that's when the record button should
   * become instant. If false AND isPreloading is true, show "Cargando modelo" state.
   */
  isModelLoaded: boolean;
  /** Startup preload is currently running */
  isPreloading: boolean;
  error: string | null;
}

const MODEL_NAME = 'parakeet-tdt-0.6b-v3-int8';
const RETRY_DELAY_MS = 30_000;

export function useParakeetAutoDownload(): ParakeetAutoDownloadState {
  const { isAuthenticated, maityUser } = useAuth();
  const [isModelReady, setIsModelReady] = useState(false);
  const [isDownloading, setIsDownloading] = useState(false);
  const [downloadProgress, setDownloadProgress] = useState(0);
  // UX-LOADING-MODEL: estado separado para "cargado en memoria" vs "descargado en disco".
  const [isModelLoaded, setIsModelLoaded] = useState(false);
  const [isPreloading, setIsPreloading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const hasTriggered = useRef(false);
  const retryTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const checkAndDownload = useCallback(async () => {
    try {
      // Initialize engine (idempotent)
      await invoke('parakeet_init');

      // Check if model is already available
      const hasModels = await invoke<boolean>('parakeet_has_available_models');
      if (hasModels) {
        console.log('[ParakeetAutoDownload] Model already available');
        setIsModelReady(true);
        setIsDownloading(false);
        setError(null);
        return;
      }

      // Check if a download is already in progress
      const models = await invoke<any[]>('parakeet_get_available_models');
      const alreadyDownloading = models.some(m =>
        m.status && (
          typeof m.status === 'object'
            ? 'Downloading' in m.status
            : m.status === 'Downloading'
        )
      );

      if (alreadyDownloading) {
        console.log('[ParakeetAutoDownload] Download already in progress');
        setIsDownloading(true);
        return;
      }

      // Check if model is corrupted and needs deletion first
      const corruptedModel = models.find(m =>
        m.name === MODEL_NAME && m.status && (
          typeof m.status === 'object'
            ? 'Corrupted' in m.status
            : m.status === 'Corrupted'
        )
      );

      if (corruptedModel) {
        console.log('[ParakeetAutoDownload] Corrupted model found, deleting before re-download');
        await invoke('parakeet_delete_corrupted_model', { modelName: MODEL_NAME });
      }

      // Start download
      console.log(`[ParakeetAutoDownload] Starting download of ${MODEL_NAME}`);
      setIsDownloading(true);
      setError(null);
      await invoke('parakeet_download_model', { modelName: MODEL_NAME });
    } catch (err) {
      const errorMsg = err instanceof Error ? err.message : String(err);
      console.error('[ParakeetAutoDownload] Error:', errorMsg);
      setError(errorMsg);
      setIsDownloading(false);

      // Schedule retry
      retryTimeoutRef.current = setTimeout(() => {
        console.log('[ParakeetAutoDownload] Retrying after error...');
        setError(null);
        checkAndDownload();
      }, RETRY_DELAY_MS);
    }
  }, []);

  // Listen for download events
  useEffect(() => {
    const unlisteners: (() => void)[] = [];

    const setup = async () => {
      const unProgress = await listen<any>('parakeet-model-download-progress', (event) => {
        const { progress, status } = event.payload;
        if (status === 'cancelled') {
          setIsDownloading(false);
          return;
        }
        setDownloadProgress(progress ?? 0);
        setIsDownloading(true);
      });
      unlisteners.push(unProgress);

      const unComplete = await listen<any>('parakeet-model-download-complete', () => {
        console.log('[ParakeetAutoDownload] Download complete');
        setIsModelReady(true);
        setIsDownloading(false);
        setDownloadProgress(100);
        setError(null);
      });
      unlisteners.push(unComplete);

      const unError = await listen<any>('parakeet-model-download-error', (event) => {
        const errorMsg = event.payload?.error || 'Download failed';
        console.error('[ParakeetAutoDownload] Download error:', errorMsg);
        setError(errorMsg);
        setIsDownloading(false);

        // Schedule retry
        retryTimeoutRef.current = setTimeout(() => {
          console.log('[ParakeetAutoDownload] Retrying after download error...');
          setError(null);
          checkAndDownload();
        }, RETRY_DELAY_MS);
      });
      unlisteners.push(unError);

      // UX-LOADING-MODEL (PERF-005): eventos emitidos por lib.rs durante el preload al arrancar
      const unPreStart = await listen<any>('parakeet-model-preload-started', () => {
        console.log('[ParakeetAutoDownload] Preload started');
        setIsPreloading(true);
        setIsModelLoaded(false);
      });
      unlisteners.push(unPreStart);

      const unPreDone = await listen<any>('parakeet-model-preload-completed', () => {
        console.log('[ParakeetAutoDownload] Preload completed — model in memory');
        setIsPreloading(false);
        setIsModelLoaded(true);
        setIsModelReady(true);
      });
      unlisteners.push(unPreDone);

      const unPreFail = await listen<any>('parakeet-model-preload-failed', (event) => {
        const msg = event.payload?.error || 'Preload failed';
        console.warn('[ParakeetAutoDownload] Preload failed (will lazy-load on first use):', msg);
        setIsPreloading(false);
        // No marcamos isModelLoaded=true — el primer uso lo cargará lazy (con delay).
      });
      unlisteners.push(unPreFail);
    };

    setup();

    return () => {
      unlisteners.forEach(fn => fn());
      if (retryTimeoutRef.current) {
        clearTimeout(retryTimeoutRef.current);
      }
    };
  }, [checkAndDownload]);

  // Trigger auto-download after authentication
  useEffect(() => {
    if (!isAuthenticated || !maityUser || hasTriggered.current) return;
    hasTriggered.current = true;
    checkAndDownload();
  }, [isAuthenticated, maityUser, checkAndDownload]);

  return {
    isModelReady,
    isDownloading,
    downloadProgress,
    isModelLoaded,
    isPreloading,
    error,
  };
}
