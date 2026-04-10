'use client';

import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { Database, AlertCircle, Loader2, CheckCircle2 } from 'lucide-react';

interface HomebrewDatabaseDetectorProps {
  onImportSuccess: () => void;
  onDecline: () => void;
}

// Homebrew paths differ between Intel and Apple Silicon Macs
const HOMEBREW_PATHS = [
  '/opt/homebrew/var/maity/meeting_minutes.db',  // Apple Silicon (M1/M2/M3)
  '/usr/local/var/maity/meeting_minutes.db',      // Intel Macs
];

export function HomebrewDatabaseDetector({ onImportSuccess, onDecline }: HomebrewDatabaseDetectorProps) {
  const [isChecking, setIsChecking] = useState(true);
  const [isImporting, setIsImporting] = useState(false);
  const [homebrewDbExists, setHomebrewDbExists] = useState(false);
  const [dbSize, setDbSize] = useState<number>(0);
  const [detectedPath, setDetectedPath] = useState<string>('');
  const [isDismissed, setIsDismissed] = useState(false);

  useEffect(() => {
    checkHomebrewDatabase();
  }, []);

  const checkHomebrewDatabase = async () => {
    try {
      setIsChecking(true);

      // Check all possible Homebrew locations
      for (const path of HOMEBREW_PATHS) {
        const result = await invoke<{ exists: boolean; size: number } | null>('check_homebrew_database', {
          path,
        });

        if (result && result.exists && result.size > 0) {
          setHomebrewDbExists(true);
          setDbSize(result.size);
          setDetectedPath(path);
          break; // Stop checking once we find a valid database
        }
      }
    } catch (error) {
      console.error('Error checking homebrew database:', error);
      // Silently fail - this is just auto-detection
    } finally {
      setIsChecking(false);
    }
  };

  const handleYes = async () => {
    try {
      setIsImporting(true);

      await invoke('import_and_initialize_database', {
        legacyDbPath: detectedPath,
      });

      toast.success('¡Base de datos importada exitosamente! Recargando...');

      // Wait 1 second for user to see success, then reload window to refresh all data
      setTimeout(() => {
        window.location.reload();
      }, 1000);
    } catch (error) {
      console.error('Error importing database:', error);
      toast.error(`Error de importación: ${error}`);
      setIsImporting(false);
    }
  };

  const handleNo = () => {
    setIsDismissed(true);
    onDecline();
  };

  if (isChecking || !homebrewDbExists || isDismissed) {
    return null;
  }

  const formatFileSize = (bytes: number): string => {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  };

  return (
    <div className="mb-4 p-4 bg-[#f0f2fe] border-2 border-[#a0b0f9] rounded-lg">
      <div className="flex items-start gap-3">
        <Database className="h-6 w-6 text-[#3a4ac3] mt-0.5 flex-shrink-0" />
        <div className="flex-1">
          <div className="flex items-center gap-2 mb-1">
            <AlertCircle className="h-4 w-4 text-[#3a4ac3]" />
            <h3 className="text-sm font-semibold text-[#141d4a]">
              ¡Instalación anterior de Maity detectada!
            </h3>
          </div>
          <p className="text-sm text-[#1e2a6e] mb-2">
            Encontramos una base de datos existente de tu instalación anterior de Maity (versión Python).
          </p>
          <div className="bg-white/50 rounded p-2 mb-3">
            <p className="text-xs text-[#2b3892] font-mono break-all">
              {detectedPath}
            </p>
            <p className="text-xs text-[#3a4ac3] mt-1">
              Tamaño: {formatFileSize(dbSize)}
            </p>
          </div>
          <p className="text-sm text-[#1e2a6e] mb-3">
            ¿Te gustaría importar tus reuniones anteriores, transcripciones y resúmenes?
          </p>
          
          {/* Yes/No Buttons */}
          <div className="flex gap-2">
            <button
              onClick={handleYes}
              disabled={isImporting}
              className="flex-1 flex items-center justify-center gap-2 px-4 py-2 bg-[#16bb7b] text-white rounded-lg hover:bg-[#108c5c] disabled:bg-[#8a8a8d] disabled:cursor-not-allowed transition-colors"
            >
              {isImporting ? (
                <>
                  <Loader2 className="h-4 w-4 animate-spin" />
                  <span>Importando...</span>
                </>
              ) : (
                <>
                  <CheckCircle2 className="h-4 w-4" />
                  <span>Sí, Importar</span>
                </>
              )}
            </button>

            <button
              onClick={handleNo}
              disabled={isImporting}
              className="flex-1 px-4 py-2 border-2 border-[#8090f7] text-[#2b3892] rounded-lg hover:bg-[#e0e5fd] disabled:bg-[#e7e7e9] disabled:cursor-not-allowed transition-colors"
            >
              No, Buscar Manualmente
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}

