'use client';

import React, { createContext, useContext, ReactNode } from 'react';
import { useParakeetAutoDownload } from '@/hooks/useParakeetAutoDownload';

interface ParakeetAutoDownloadContextType {
  isModelReady: boolean;
  isDownloading: boolean;
  downloadProgress: number;
  /** UX-LOADING-MODEL: modelo residente en memoria (listo para inference instantánea). */
  isModelLoaded: boolean;
  /** UX-LOADING-MODEL: preload del startup activo. */
  isPreloading: boolean;
  error: string | null;
}

const ParakeetAutoDownloadContext = createContext<ParakeetAutoDownloadContextType | undefined>(undefined);

export function ParakeetAutoDownloadProvider({ children }: { children: ReactNode }) {
  const state = useParakeetAutoDownload();

  return (
    <ParakeetAutoDownloadContext.Provider value={state}>
      {children}
    </ParakeetAutoDownloadContext.Provider>
  );
}

export function useParakeetAutoDownloadContext() {
  const context = useContext(ParakeetAutoDownloadContext);
  if (context === undefined) {
    throw new Error('useParakeetAutoDownloadContext must be used within a ParakeetAutoDownloadProvider');
  }
  return context;
}
