'use client'

import './globals.css'
import { Source_Sans_3 } from 'next/font/google'
import Sidebar from '@/components/Sidebar'
import { SidebarProvider } from '@/components/Sidebar/SidebarProvider'
import MainContent from '@/components/MainContent'
import AnalyticsProvider from '@/components/analytics/AnalyticsProvider'
import { Toaster, toast } from 'sonner'
import { useState, useEffect, useRef } from 'react'
import { listen, emit } from '@tauri-apps/api/event'
import { invoke } from '@tauri-apps/api/core'
import { TooltipProvider } from '@/components/ui/tooltip'
import { RecordingStateProvider } from '@/contexts/RecordingStateContext'
import { OllamaDownloadProvider } from '@/contexts/OllamaDownloadContext'
import { TranscriptProvider } from '@/contexts/TranscriptContext'
import { ConfigProvider } from '@/contexts/ConfigContext'
import { OnboardingProvider } from '@/contexts/OnboardingContext'
import { OnboardingFlow } from '@/components/Onboarding'
import { DownloadProgressToastProvider } from '@/components/shared/DownloadProgressToast'
import { UpdateCheckProvider } from '@/components/updates/UpdateCheckProvider'
import { RecordingPostProcessingProvider } from '@/contexts/RecordingPostProcessingProvider'
import { ErrorBoundary } from '@/components/shared/ErrorBoundary'
import { ChunkErrorRecovery } from '@/components/shared/ChunkErrorRecovery'
import { MeetingDetectionDialog } from '@/components/meeting-detection/MeetingDetectionDialog'
import { OfflineIndicator } from '@/components/shared/OfflineIndicator'
import { AuthProvider, useAuth } from '@/contexts/AuthContext'
import { ParakeetAutoDownloadProvider } from '@/contexts/ParakeetAutoDownloadContext'
import { LoginScreen } from '@/components/Auth'
import { CloudSyncInitializer } from '@/components/CloudSyncInitializer'
import { AnalysisPollingInitializer } from '@/components/AnalysisPollingInitializer'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { ThemeProvider } from '@/contexts/ThemeContext'
import Script from 'next/script'

// Create a client outside the component to avoid re-creating it on every render
const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: 1000 * 60 * 5, // 5 minutes
      retry: 1,
    },
  },
})

const sourceSans3 = Source_Sans_3({
  subsets: ['latin'],
  weight: ['400', '500', '600', '700'],
  variable: '--font-source-sans-3',
})

// export { metadata } from './metadata'

/**
 * SplashScreen: minimal loading screen with logo + spinner.
 * Uses inline styles so it renders instantly without waiting for CSS/fonts.
 */
function SplashScreen() {
  return (
    <>
    <style dangerouslySetInnerHTML={{ __html: '@keyframes spin{to{transform:rotate(360deg)}}' }} />
    <div style={{
      display: 'flex',
      flexDirection: 'column',
      alignItems: 'center',
      justifyContent: 'center',
      height: '100vh',
      backgroundColor: '#000',
      gap: '20px',
    }}>
      <img
        src="icon_128x128.png"
        alt="Maity"
        style={{ width: 56, height: 56, opacity: 0.9 }}
      />
      <svg
        style={{ width: 24, height: 24, animation: 'spin 1s linear infinite' }}
        xmlns="http://www.w3.org/2000/svg"
        fill="none"
        viewBox="0 0 24 24"
      >
        <circle opacity="0.25" cx="12" cy="12" r="10" stroke="#a78bfa" strokeWidth="4" />
        <path opacity="0.75" fill="#a78bfa" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z" />
      </svg>
    </div>
    </>
  )
}

/**
 * AuthGate: renders LoginScreen when not authenticated, otherwise renders children.
 * Must be used inside AuthProvider.
 */
function AuthGate({ children }: { children: React.ReactNode }) {
  const { isAuthenticated, isLoading, maityUser, maityUserError, retryFetchMaityUser, signOut } = useAuth()
  const [mounted, setMounted] = useState(false)
  const [timedOut, setTimedOut] = useState(false)

  useEffect(() => {
    setMounted(true)
  }, [])

  // Auto-retry + timeout: if authenticated but maityUser hasn't loaded, retry at 3s and 7s, timeout at 12s
  useEffect(() => {
    if (!isAuthenticated || maityUser || maityUserError) {
      setTimedOut(false)
      return
    }

    const retry1 = setTimeout(() => {
      console.log('[AuthGate] Auto-retry #1 for maityUser (3s)')
      retryFetchMaityUser()
    }, 3000)
    const retry2 = setTimeout(() => {
      console.log('[AuthGate] Auto-retry #2 for maityUser (7s)')
      retryFetchMaityUser()
    }, 7000)
    const timeout = setTimeout(() => setTimedOut(true), 12000)

    return () => {
      clearTimeout(retry1)
      clearTimeout(retry2)
      clearTimeout(timeout)
    }
  }, [isAuthenticated, maityUser, maityUserError, retryFetchMaityUser])

  // Signal Tauri to show the window once we have real content (not splash).
  // This runs once when auth resolves to any visible state.
  const hasSignaledReady = useRef(false)
  useEffect(() => {
    if (!mounted || isLoading || hasSignaledReady.current) return
    // At this point we have real UI to show (login, maityUser wait, or app)
    hasSignaledReady.current = true
    emit('app-ready').catch(() => {
      // Silently ignore — may fail outside Tauri (e.g. browser dev)
    })
  }, [mounted, isLoading])

  // SSG hydration placeholder
  if (!mounted) {
    return <SplashScreen />
  }

  // Auth loading state
  if (isLoading) {
    return <SplashScreen />
  }

  // Not authenticated — show login
  if (!isAuthenticated) {
    return <LoginScreen />
  }

  // Wait for maityUser to be populated (brief gap after fresh OAuth login)
  if (!maityUser) {
    const showError = maityUserError || timedOut

    return (
      <div className="flex flex-col items-center justify-center h-screen bg-background gap-6">
        {/* Logo Maity */}
        <div className="w-16 h-16 rounded-2xl bg-gradient-to-br from-[#ff0050]/10 to-[#485df4]/10 dark:from-[#ff0050]/20 dark:to-[#485df4]/20 flex items-center justify-center shadow-lg">
          <img src="icon_128x128.png" alt="Maity" className="w-12 h-12" />
        </div>

        {showError ? (
          <>
            <p className="text-red-400 text-sm text-center max-w-xs">
              {maityUserError || 'No se pudo conectar con el servidor. Verifica tu conexión e intenta de nuevo.'}
            </p>
            <div className="flex gap-3">
              <button
                onClick={() => { setTimedOut(false); retryFetchMaityUser() }}
                className="px-4 py-2 rounded-lg bg-violet-600 hover:bg-violet-500 text-white text-sm font-medium transition-colors"
              >
                Reintentar
              </button>
              <button
                onClick={() => signOut()}
                className="px-4 py-2 rounded-lg bg-zinc-700 hover:bg-zinc-600 text-white text-sm font-medium transition-colors"
              >
                Cerrar sesión
              </button>
            </div>
          </>
        ) : (
          <>
            {/* Spinner */}
            <svg className="animate-spin h-6 w-6 text-violet-400" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24">
              <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" />
              <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z" />
            </svg>
            <p className="text-muted-foreground text-sm">Preparando tu cuenta...</p>
          </>
        )}
      </div>
    )
  }

  // Authenticated with maityUser ready — render app content
  return <>{children}</>
}

/**
 * AppContent: the main app shell (onboarding check, sidebar, etc.)
 * Rendered only when the user is authenticated.
 */
function AppContent({ children }: { children: React.ReactNode }) {
  const [showOnboarding, setShowOnboarding] = useState(false)
  const [onboardingCompleted, setOnboardingCompleted] = useState(false)

  useEffect(() => {
    // Check onboarding status first
    invoke<{ completed: boolean } | null>('get_onboarding_status')
      .then((status) => {
        const isComplete = status?.completed ?? false
        setOnboardingCompleted(isComplete)

        if (!isComplete) {
          console.log('[Layout] Onboarding not completed, showing onboarding flow')
          setShowOnboarding(true)
        } else {
          console.log('[Layout] Onboarding completed, showing main app')
        }
      })
      .catch((error) => {
        console.error('[Layout] Failed to check onboarding status:', error)
        // Default to showing onboarding if we can't check
        setShowOnboarding(true)
        setOnboardingCompleted(false)
      })
  }, [])

  // Disable context menu in production
  useEffect(() => {
    if (process.env.NODE_ENV === 'production') {
      const handleContextMenu = (e: MouseEvent) => e.preventDefault();
      document.addEventListener('contextmenu', handleContextMenu);
      return () => document.removeEventListener('contextmenu', handleContextMenu);
    }
  }, []);

  useEffect(() => {
    // Listen for tray recording toggle request
    const unlisten = listen('request-recording-toggle', () => {
      console.log('[Layout] Received request-recording-toggle from tray');

      if (showOnboarding) {
        toast.error("Por favor completa la configuración primero", {
          description: "Necesitas terminar la configuración inicial antes de poder grabar."
        });
      } else {
        // If in main app, forward to useRecordingStart via window event
        console.log('[Layout] Forwarding to start-recording-from-sidebar');
        window.dispatchEvent(new CustomEvent('start-recording-from-sidebar'));
      }
    });

    return () => {
      unlisten.then(fn => fn());
    };
  }, [showOnboarding]);

  const handleOnboardingComplete = () => {
    console.log('[Layout] Onboarding completed, showing main app')
    setShowOnboarding(false)
    setOnboardingCompleted(true)
  }

  return (
    <RecordingStateProvider>
      <TranscriptProvider>
        <ConfigProvider>
          <ParakeetAutoDownloadProvider>
          <OllamaDownloadProvider>
            <OnboardingProvider>
              <SidebarProvider>
                <TooltipProvider>
                  <RecordingPostProcessingProvider>
                    {/* Download progress toast provider - listens for background downloads */}
                    <DownloadProgressToastProvider />

                    {/* Meeting detection dialog - listens for meeting-detected events */}
                    <MeetingDetectionDialog />

                    {/* Show onboarding or main app */}
                    {showOnboarding ? (
                      <OnboardingFlow onComplete={handleOnboardingComplete} />
                    ) : (
                      <div className="flex flex-col h-screen">
                        {/* Offline indicator at the top */}
                        <OfflineIndicator />
                        <div className="flex flex-1 overflow-hidden">
                          <Sidebar />
                          <MainContent>{children}</MainContent>
                        </div>
                      </div>
                    )}
                  </RecordingPostProcessingProvider>
                </TooltipProvider>
              </SidebarProvider>
            </OnboardingProvider>
          </OllamaDownloadProvider>
          </ParakeetAutoDownloadProvider>
        </ConfigProvider>
      </TranscriptProvider>
    </RecordingStateProvider>
  )
}

// Inline script to handle ChunkLoadError before React loads
const chunkErrorRecoveryScript = `
(function() {
  if (typeof window === 'undefined') return;

  var MAX_RETRIES = 3;
  var RETRY_DELAY = 2000;
  var STORAGE_KEY = 'chunkErrorRetryCount';
  var CLEAR_DELAY = 30000;

  // Get current retry count
  var retryCount = parseInt(sessionStorage.getItem(STORAGE_KEY) || '0', 10);

  // Clear retry count after successful load
  setTimeout(function() {
    sessionStorage.removeItem(STORAGE_KEY);
  }, CLEAR_DELAY);

  function handleChunkError(message) {
    if (retryCount >= MAX_RETRIES) {
      console.error('[ChunkErrorRecovery] Max retries (' + MAX_RETRIES + ') reached. Please restart dev server.');
      sessionStorage.removeItem(STORAGE_KEY);
      return;
    }

    console.warn('[ChunkErrorRecovery] Chunk load error detected: ' + message);
    console.log('[ChunkErrorRecovery] Reloading in ' + RETRY_DELAY + 'ms... (attempt ' + (retryCount + 1) + '/' + MAX_RETRIES + ')');

    sessionStorage.setItem(STORAGE_KEY, String(retryCount + 1));

    setTimeout(function() {
      window.location.reload();
    }, RETRY_DELAY);
  }

  // Catch errors
  window.addEventListener('error', function(e) {
    var msg = e.message || '';
    if (msg.indexOf('ChunkLoadError') !== -1 ||
        msg.indexOf('Loading chunk') !== -1 ||
        msg.indexOf('Failed to fetch') !== -1 ||
        (msg.indexOf('chunk') !== -1 && msg.indexOf('failed') !== -1)) {
      handleChunkError(msg);
    }
  });

  // Catch unhandled promise rejections (dynamic imports)
  window.addEventListener('unhandledrejection', function(e) {
    var reason = e.reason || {};
    var msg = reason.message || String(reason) || '';
    if (msg.indexOf('ChunkLoadError') !== -1 ||
        msg.indexOf('Loading chunk') !== -1 ||
        msg.indexOf('Failed to fetch dynamically imported module') !== -1) {
      handleChunkError(msg);
    }
  });
})();
`;

export default function RootLayout({
  children,
}: {
  children: React.ReactNode
}) {
  return (
    <html lang="es" className="dark" suppressHydrationWarning>
      <body className={`${sourceSans3.variable} font-sans antialiased`} suppressHydrationWarning>
        <Script
          id="chunk-error-recovery"
          strategy="beforeInteractive"
          dangerouslySetInnerHTML={{ __html: chunkErrorRecoveryScript }}
        />
        <ChunkErrorRecovery />
        <ErrorBoundary>
          <QueryClientProvider client={queryClient}>
            <ThemeProvider>
              <AnalyticsProvider>
                <AuthProvider>
                  <UpdateCheckProvider>
                    <AuthGate>
                      <CloudSyncInitializer />
                      <AnalysisPollingInitializer />
                      <AppContent>{children}</AppContent>
                    </AuthGate>
                  </UpdateCheckProvider>
                </AuthProvider>
              </AnalyticsProvider>
            </ThemeProvider>
          </QueryClientProvider>
        </ErrorBoundary>
        <Toaster position="bottom-right" richColors closeButton />
      </body>
    </html>
  )
}
