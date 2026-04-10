# ============================================================================
# benchmark_performance.ps1 - Maity Desktop performance benchmark harness
# ----------------------------------------------------------------------------
# Measures user-visible performance metrics on every build to catch regressions.
#
# Usage:
#   powershell -ExecutionPolicy Bypass -File tests\benchmark_performance.ps1
#
# Exit codes:
#   0 = all metrics within thresholds
#   1 = at least one metric exceeded threshold
#   2 = harness error (binary missing, log not found, timeout, etc.)
#
# Requirements:
#   - Windows 11, PowerShell 5.1+
#   - No admin rights needed
#   - Existing debug binary at target\debug\maity-desktop.exe
# ============================================================================

[CmdletBinding()]
param(
    [string]$BinaryPath     = "D:\Maity_Desktop\target\debug\maity-desktop.exe",
    [string]$LogDir         = "C:\Users\alfon\AppData\Local\Maity\logs",
    [string]$ResultsDir     = "D:\Maity_Desktop\tests\results",
    [string]$MetricsHistory = "D:\Maity_Desktop\memory\METRICS_HISTORY.md",
    [int]   $MaxWaitSeconds = 60
)

$ErrorActionPreference = "Stop"

# ------------------------------ Thresholds ----------------------------------
# Edit these to tune CI gating. Units: milliseconds unless noted.
$Thresholds = @{
    ColdStartMs          = 3000      # <3s
    WebViewJsReadyMs     = 5000      # <5s
    WhisperPreloadMs     = 10000     # <10s for base model
    ClickToRecordMs      = 500       # <500ms (currently TODO, measured only if UIA present)
    TranscriptionRtf     = 1.5       # >1.5x realtime (audio_sec / proc_sec)
    RamPeakMb            = 2048      # <2 GB combined
    StopToSummaryMs      = 5000      # <5s for a 1-min clip
}

# ------------------------------ Helpers -------------------------------------
function Write-Step([string]$msg) { Write-Host "[bench] $msg" -ForegroundColor Cyan }
function Write-Ok  ([string]$msg) { Write-Host "[bench] OK  $msg" -ForegroundColor Green }
function Write-Warn([string]$msg) { Write-Host "[bench] WARN $msg" -ForegroundColor Yellow }
function Write-Err ([string]$msg) { Write-Host "[bench] ERR  $msg" -ForegroundColor Red }

function Stop-MaityProcesses {
    Get-Process -Name "maity-desktop" -ErrorAction SilentlyContinue | ForEach-Object {
        try { $_.Kill(); $_.WaitForExit(5000) } catch {}
    }
    Get-Process -Name "msedgewebview2" -ErrorAction SilentlyContinue | Where-Object {
        $_.MainModule.FileName -like "*Maity*" -or $_.Parent.ProcessName -eq "maity-desktop"
    } | ForEach-Object { try { $_.Kill() } catch {} }
    Start-Sleep -Milliseconds 500
}

function Get-TodayLogPath {
    $date = (Get-Date).ToString("yyyy-MM-dd")
    Join-Path $LogDir "maity.$date.log"
}

function Get-WebView2RamMb {
    $procs = Get-Process -Name "msedgewebview2" -ErrorAction SilentlyContinue
    if (-not $procs) { return 0 }
    $sum = ($procs | Measure-Object -Property WorkingSet64 -Sum).Sum
    return [math]::Round($sum / 1MB, 1)
}

function Get-CombinedRamMb {
    $maity = Get-Process -Name "maity-desktop" -ErrorAction SilentlyContinue
    $wv    = Get-Process -Name "msedgewebview2" -ErrorAction SilentlyContinue
    $sum = 0
    if ($maity) { $sum += ($maity | Measure-Object -Property WorkingSet64 -Sum).Sum }
    if ($wv)    { $sum += ($wv    | Measure-Object -Property WorkingSet64 -Sum).Sum }
    return [math]::Round($sum / 1MB, 1)
}

# ------------------------------ Pre-flight ----------------------------------
Write-Step "Pre-flight checks"
if (-not (Test-Path $BinaryPath)) {
    Write-Err "Binary not found: $BinaryPath"
    exit 2
}
if (-not (Test-Path $LogDir)) {
    Write-Err "Log directory not found: $LogDir"
    exit 2
}
New-Item -ItemType Directory -Force -Path $ResultsDir | Out-Null

Write-Step "Killing any existing maity-desktop / webview2 processes"
Stop-MaityProcesses

# Record baseline log size so we only scan new lines
$logPath = Get-TodayLogPath
$baselineOffset = 0
if (Test-Path $logPath) {
    $baselineOffset = (Get-Item $logPath).Length
    Write-Step "Baseline log offset: $baselineOffset bytes ($logPath)"
} else {
    Write-Step "Log file does not exist yet (will be created): $logPath"
}

# ------------------------------ Launch --------------------------------------
$results = [ordered]@{
    timestamp        = (Get-Date).ToString("o")
    binary           = $BinaryPath
    thresholds       = $Thresholds
    metrics          = [ordered]@{}
    errors           = @()
    pass             = $true
}

Write-Step "Launching maity-desktop.exe"
$launchSw = [System.Diagnostics.Stopwatch]::StartNew()
$proc = Start-Process -FilePath $BinaryPath -PassThru -WindowStyle Normal
$pidMaity = $proc.Id
Write-Ok "Launched PID=$pidMaity"

# --- Metric 1: Cold start (process responding) ------------------------------
Write-Step "Waiting for process to become Responding=True"
$t1 = $null
while ($launchSw.Elapsed.TotalSeconds -lt $MaxWaitSeconds) {
    $p = Get-Process -Id $pidMaity -ErrorAction SilentlyContinue
    if ($null -eq $p) { break }
    if ($p.Responding -and $p.MainWindowHandle -ne [IntPtr]::Zero) {
        $t1 = $launchSw.Elapsed.TotalMilliseconds
        break
    }
    Start-Sleep -Milliseconds 100
}
if ($null -eq $t1) {
    $results.errors += "ColdStart: process never became Responding within ${MaxWaitSeconds}s"
    $results.metrics.ColdStartMs = -1
    $results.pass = $false
} else {
    $results.metrics.ColdStartMs = [math]::Round($t1, 0)
    Write-Ok ("ColdStartMs = {0}" -f $results.metrics.ColdStartMs)
}

# --- Metric 2: WebView2 JS ready (RAM >= 200 MB) ----------------------------
Write-Step "Waiting for WebView2 RAM >= 200 MB"
$t2 = $null
while ($launchSw.Elapsed.TotalSeconds -lt $MaxWaitSeconds) {
    $ram = Get-WebView2RamMb
    if ($ram -ge 200) {
        $t2 = $launchSw.Elapsed.TotalMilliseconds
        break
    }
    Start-Sleep -Milliseconds 150
}
if ($null -eq $t2) {
    $results.errors += "WebViewJsReady: never reached 200 MB within ${MaxWaitSeconds}s"
    $results.metrics.WebViewJsReadyMs = -1
    $results.pass = $false
} else {
    $results.metrics.WebViewJsReadyMs = [math]::Round($t2, 0)
    Write-Ok ("WebViewJsReadyMs = {0}" -f $results.metrics.WebViewJsReadyMs)
}

# --- Metric 3: Whisper preload time (log scrape) ----------------------------
Write-Step "Scanning log for 'PERF-005: Whisper model ... preloaded'"
$t3          = $null
$preloadLine = $null
$preloadSw   = [System.Diagnostics.Stopwatch]::StartNew()
while ($preloadSw.Elapsed.TotalSeconds -lt $MaxWaitSeconds) {
    if (Test-Path $logPath) {
        $fs = [System.IO.File]::Open($logPath, 'Open', 'Read', 'ReadWrite')
        try {
            $fs.Seek($baselineOffset, 'Begin') | Out-Null
            $sr = New-Object System.IO.StreamReader($fs)
            $tail = $sr.ReadToEnd()
            $sr.Close()
        } finally { $fs.Dispose() }

        $match = [regex]::Match($tail, "PERF-005:\s*Whisper model\s*'([^']+)'\s*preloaded")
        if ($match.Success) {
            # Extract "HH:mm:ss.fff" or similar timestamp from the same line
            $lineMatch = [regex]::Match($tail, "(?m)^.*PERF-005:\s*Whisper model.*preloaded.*$")
            $preloadLine = $lineMatch.Value
            $t3 = $launchSw.Elapsed.TotalMilliseconds
            break
        }
    }
    Start-Sleep -Milliseconds 200
}
if ($null -eq $t3) {
    $results.errors += "WhisperPreload: PERF-005 line not found within ${MaxWaitSeconds}s"
    $results.metrics.WhisperPreloadMs = -1
    $results.metrics.WhisperPreloadLine = $null
    $results.pass = $false
} else {
    $results.metrics.WhisperPreloadMs   = [math]::Round($t3, 0)
    $results.metrics.WhisperPreloadLine = $preloadLine.Trim()
    Write-Ok ("WhisperPreloadMs = {0}" -f $results.metrics.WhisperPreloadMs)
}

# --- Metric 6: RAM peak (sampled for 5s after preload) ----------------------
Write-Step "Sampling RAM peak for 5s"
$ramPeak = 0
$sampleSw = [System.Diagnostics.Stopwatch]::StartNew()
while ($sampleSw.Elapsed.TotalSeconds -lt 5) {
    $r = Get-CombinedRamMb
    if ($r -gt $ramPeak) { $ramPeak = $r }
    Start-Sleep -Milliseconds 250
}
$results.metrics.RamPeakMb = $ramPeak
Write-Ok ("RamPeakMb = {0}" -f $ramPeak)

# --- Metrics 4,5,7: TODO (require UIAutomation / live audio injection) ------
$results.metrics.ClickToRecordMs   = -1   # TODO: UIAutomation click on "Grabar" button
$results.metrics.TranscriptionRtf  = -1   # TODO: inject fixture audio and measure
$results.metrics.StopToSummaryMs   = -1   # TODO: requires UIAutomation + event hook
$results.errors += "ClickToRecord / TranscriptionRtf / StopToSummary are TODO (UIAutomation out of scope)"

# ------------------------------ Teardown ------------------------------------
Write-Step "Stopping maity-desktop"
Stop-MaityProcesses

# ------------------------------ Threshold check -----------------------------
function Check-Metric([string]$name, [double]$value, [double]$limit, [string]$op) {
    if ($value -lt 0) { return $null }   # skipped/failed metric, already logged
    $ok = if ($op -eq "lt") { $value -lt $limit } else { $value -gt $limit }
    if (-not $ok) {
        Write-Warn "$name = $value violates threshold ($op $limit)"
        $script:results.pass = $false
        $script:results.errors += "$name=$value exceeds $op $limit"
    } else {
        Write-Ok "$name within threshold ($op $limit)"
    }
}

Check-Metric "ColdStartMs"       $results.metrics.ColdStartMs       $Thresholds.ColdStartMs       "lt"
Check-Metric "WebViewJsReadyMs"  $results.metrics.WebViewJsReadyMs  $Thresholds.WebViewJsReadyMs  "lt"
Check-Metric "WhisperPreloadMs"  $results.metrics.WhisperPreloadMs  $Thresholds.WhisperPreloadMs  "lt"
Check-Metric "RamPeakMb"         $results.metrics.RamPeakMb         $Thresholds.RamPeakMb         "lt"

# ------------------------------ Write JSON ----------------------------------
$stamp   = (Get-Date).ToString("yyyy-MM-dd_HHmmss")
$jsonOut = Join-Path $ResultsDir "perf_$stamp.json"
$results | ConvertTo-Json -Depth 6 | Out-File -FilePath $jsonOut -Encoding UTF8
Write-Ok "Wrote $jsonOut"

# ------------------------------ Append to history --------------------------
$passStr = if ($results.pass) { "PASS" } else { "FAIL" }
$line = "| {0} | perf | cold={1}ms wv={2}ms whisper={3}ms ram={4}MB | {5} |" -f `
    (Get-Date).ToString("yyyy-MM-dd HH:mm"), `
    $results.metrics.ColdStartMs, `
    $results.metrics.WebViewJsReadyMs, `
    $results.metrics.WhisperPreloadMs, `
    $results.metrics.RamPeakMb, `
    $passStr
Add-Content -Path $MetricsHistory -Value $line
Write-Ok "Appended summary to $MetricsHistory"

if ($results.pass) { exit 0 } else { exit 1 }
