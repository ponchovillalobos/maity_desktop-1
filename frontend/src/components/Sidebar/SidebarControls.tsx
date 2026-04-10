'use client';

import React, { useState, useEffect } from 'react';
import { Settings, Mic, Square, User, LogOut } from 'lucide-react';
import { useRouter } from 'next/navigation';
import { getVersion } from '@tauri-apps/api/app';
import Info from '@/components/shared/Info';
import { useAuth } from '@/contexts/AuthContext';

interface SidebarControlsProps {
  isRecording: boolean;
  isCollapsed: boolean;
  onRecordingToggle: () => void;
}

export const SidebarControls: React.FC<SidebarControlsProps> = ({
  isRecording,
  isCollapsed,
  onRecordingToggle,
}) => {
  const router = useRouter();
  const [version, setVersion] = useState('');
  // UX-ACCOUNT-BADGE (2026-04-08): el usuario pidió saber con qué cuenta está
  // trabajando. Mostramos nombre + email visibles justo arriba del botón grabar.
  const { user, maityUser, signOut } = useAuth();

  useEffect(() => {
    getVersion().then(setVersion).catch(() => setVersion('0.2.3'));
  }, []);

  if (isCollapsed) return null;

  const displayName =
    (maityUser?.first_name
      ? `${maityUser.first_name}${maityUser.last_name ? ' ' + maityUser.last_name : ''}`
      : null) ||
    user?.user_metadata?.full_name ||
    user?.email?.split('@')[0] ||
    'Invitado';
  const displayEmail = maityUser?.email || user?.email || 'Sin sesión iniciada';
  const isSignedIn = !!user;

  return (
    <div className="flex-shrink-0 p-2 border-t border-gray-100 dark:border-gray-700">
      {/* Account badge */}
      <div
        className="w-full flex items-center gap-2 px-2 py-2 mb-2 rounded-lg bg-secondary/50 border border-border"
        title={isSignedIn ? `Sesión activa: ${displayEmail}` : 'Sin sesión iniciada'}
      >
        <div className="w-8 h-8 rounded-full bg-primary/15 text-primary flex items-center justify-center flex-shrink-0">
          <User className="w-4 h-4" />
        </div>
        <div className="flex-1 min-w-0">
          <div className="text-xs font-medium text-foreground truncate">{displayName}</div>
          <div className="text-[10px] text-muted-foreground truncate">{displayEmail}</div>
        </div>
        {isSignedIn && (
          <button
            onClick={() => {
              void signOut();
            }}
            aria-label="Cerrar sesión"
            title="Cerrar sesión"
            className="p-1 rounded hover:bg-secondary text-muted-foreground hover:text-foreground transition-colors"
          >
            <LogOut className="w-3.5 h-3.5" />
          </button>
        )}
      </div>

      <button
        onClick={onRecordingToggle}
        disabled={isRecording}
        aria-label={isRecording ? 'Grabación en progreso' : 'Iniciar grabación'}
        className={`w-full flex items-center justify-center px-3 py-2 text-sm font-medium text-white ${isRecording ? 'bg-primary/60 cursor-not-allowed' : 'bg-primary hover:bg-primary/80'} rounded-lg transition-colors shadow-sm`}
      >
        {isRecording ? (
          <>
            <Square className="w-4 h-4 mr-2" />
            <span>Grabación en progreso...</span>
          </>
        ) : (
          <>
            <Mic className="w-4 h-4 mr-2" />
            <span>Iniciar Grabación</span>
          </>
        )}
      </button>

      <button
        onClick={() => router.push('/settings')}
        aria-label="Abrir configuración"
        className="w-full flex items-center justify-center px-3 py-1.5 mt-1 mb-1 text-sm font-medium text-foreground bg-secondary hover:bg-secondary/80 rounded-lg transition-colors shadow-sm"
      >
        <Settings className="w-4 h-4 mr-2" />
        <span>Configuración</span>
      </button>
      <Info isCollapsed={isCollapsed} />
      <div className="w-full flex items-center justify-center px-3 py-1 text-xs text-muted-foreground">
        {version ? `v${version}` : ''}
      </div>
    </div>
  );
};
