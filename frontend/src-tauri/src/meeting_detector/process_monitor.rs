//! Process Monitor
//!
//! Monitors running processes to detect meeting applications.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use sysinfo::{ProcessRefreshKind, RefreshKind, System};

/// Known meeting applications with their process names
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MeetingApp {
    Zoom,
    MicrosoftTeams,
    GoogleMeet,
    Webex,
    Slack,
    Discord,
    Skype,
    Unknown(String),
}

impl MeetingApp {
    /// Get the display name for the meeting app
    pub fn display_name(&self) -> &str {
        match self {
            MeetingApp::Zoom => "Zoom",
            MeetingApp::MicrosoftTeams => "Microsoft Teams",
            MeetingApp::GoogleMeet => "Google Meet",
            MeetingApp::Webex => "Webex",
            MeetingApp::Slack => "Slack Huddle",
            MeetingApp::Discord => "Discord",
            MeetingApp::Skype => "Skype",
            MeetingApp::Unknown(name) => name,
        }
    }

    /// Get the icon name for the meeting app (for UI)
    pub fn icon_name(&self) -> &str {
        match self {
            MeetingApp::Zoom => "zoom",
            MeetingApp::MicrosoftTeams => "teams",
            MeetingApp::GoogleMeet => "meet",
            MeetingApp::Webex => "webex",
            MeetingApp::Slack => "slack",
            MeetingApp::Discord => "discord",
            MeetingApp::Skype => "skype",
            MeetingApp::Unknown(_) => "unknown",
        }
    }
}

/// Information about a detected meeting
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedMeeting {
    /// The meeting application detected
    pub app: MeetingApp,
    /// Process ID
    pub pid: u32,
    /// Process name
    pub process_name: String,
    /// Window title (if available)
    pub window_title: Option<String>,
    /// Suggested meeting name based on detection
    pub suggested_name: String,
    /// Timestamp when detected
    pub detected_at: u64,
}

/// Process patterns for detecting meeting apps
struct ProcessPattern {
    app: MeetingApp,
    /// Process names to match (lowercase)
    process_names: Vec<&'static str>,
    /// Browser tab patterns (for web-based meetings) - reserved for future use
    #[allow(dead_code)]
    browser_patterns: Vec<&'static str>,
}

/// Get the list of process patterns for meeting detection
fn get_process_patterns() -> Vec<ProcessPattern> {
    vec![
        ProcessPattern {
            app: MeetingApp::Zoom,
            process_names: vec![
                "zoom.exe",
                "zoom",
                "zoomit.exe",
                "zoom.us",
                "caphost.exe", // Zoom capture host
            ],
            browser_patterns: vec!["zoom.us/j/", "zoom.us/wc/"],
        },
        ProcessPattern {
            app: MeetingApp::MicrosoftTeams,
            process_names: vec![
                "teams.exe",
                "ms-teams.exe",
                "msteams.exe",
                "microsoft teams",
                "teams",
            ],
            browser_patterns: vec!["teams.microsoft.com", "teams.live.com"],
        },
        ProcessPattern {
            app: MeetingApp::GoogleMeet,
            process_names: vec![], // Google Meet is browser-only
            browser_patterns: vec!["meet.google.com"],
        },
        ProcessPattern {
            app: MeetingApp::Webex,
            process_names: vec![
                "webexmta.exe",
                "ciscowebexstart.exe",
                "atmgr.exe",
                "webex.exe",
                "webex",
            ],
            browser_patterns: vec!["webex.com"],
        },
        ProcessPattern {
            app: MeetingApp::Slack,
            process_names: vec!["slack.exe", "slack"],
            browser_patterns: vec!["app.slack.com/huddle"],
        },
        ProcessPattern {
            app: MeetingApp::Discord,
            process_names: vec!["discord.exe", "discord"],
            browser_patterns: vec!["discord.com/channels"],
        },
        ProcessPattern {
            app: MeetingApp::Skype,
            process_names: vec!["skype.exe", "skypeapp.exe", "skype"],
            browser_patterns: vec!["web.skype.com"],
        },
    ]
}

/// Browser process names to check for web-based meetings
fn get_browser_processes() -> Vec<&'static str> {
    vec![
        "chrome.exe",
        "msedge.exe",
        "firefox.exe",
        "brave.exe",
        "opera.exe",
        "vivaldi.exe",
        "chromium.exe",
        // macOS/Linux
        "google chrome",
        "microsoft edge",
        "firefox",
        "brave browser",
        "safari",
    ]
}

/// Monitor for meeting application processes
pub struct ProcessMonitor {
    system: System,
    previously_detected: HashSet<u32>,
}

impl ProcessMonitor {
    /// Create a new process monitor
    pub fn new() -> Self {
        let system = System::new_with_specifics(
            RefreshKind::new().with_processes(ProcessRefreshKind::everything()),
        );
        Self {
            system,
            previously_detected: HashSet::new(),
        }
    }

    /// Refresh the process list and detect meeting applications
    pub fn detect_meetings(&mut self) -> Vec<DetectedMeeting> {
        // Refresh process list
        self.system.refresh_processes_specifics(
            sysinfo::ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::new(),
        );

        let patterns = get_process_patterns();
        let browser_processes = get_browser_processes();
        let mut detected: Vec<DetectedMeeting> = Vec::new();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        for (pid, process) in self.system.processes() {
            let pid_u32 = pid.as_u32();
            let process_name = process.name().to_string_lossy().to_lowercase();

            // Check for direct meeting app processes
            for pattern in &patterns {
                for &name in &pattern.process_names {
                    if process_name.contains(name) || process_name == name {
                        detected.push(DetectedMeeting {
                            app: pattern.app.clone(),
                            pid: pid_u32,
                            process_name: process.name().to_string_lossy().to_string(),
                            window_title: None, // Will be populated by window detection
                            suggested_name: format!(
                                "{} - {}",
                                pattern.app.display_name(),
                                chrono::Local::now().format("%Y-%m-%d %H:%M")
                            ),
                            detected_at: now,
                        });
                        break;
                    }
                }
            }

            // Check if it's a browser (for web-based meetings)
            // Note: Actual browser tab detection would require more complex window title analysis
            let is_browser = browser_processes.iter().any(|&b| process_name.contains(b));
            if is_browser {
                // For browser-based detection, we'd need window title access
                // This is a placeholder - actual implementation would check window titles
                // for patterns like "meet.google.com", "teams.microsoft.com", etc.
            }
        }

        // Filter to only new detections
        let new_detected: Vec<DetectedMeeting> = detected
            .into_iter()
            .filter(|d| !self.previously_detected.contains(&d.pid))
            .collect();

        // Update previously detected
        for d in &new_detected {
            self.previously_detected.insert(d.pid);
        }

        // Clean up stale PIDs
        let current_pids: HashSet<u32> = self
            .system
            .processes()
            .keys()
            .map(|pid| pid.as_u32())
            .collect();
        self.previously_detected
            .retain(|pid| current_pids.contains(pid));

        new_detected
    }

    /// Check if any meeting app is currently running
    pub fn is_meeting_active(&mut self) -> bool {
        self.system.refresh_processes_specifics(
            sysinfo::ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::new(),
        );

        let patterns = get_process_patterns();

        for (_pid, process) in self.system.processes() {
            let process_name = process.name().to_string_lossy().to_lowercase();

            for pattern in &patterns {
                for &name in &pattern.process_names {
                    if process_name.contains(name) || process_name == name {
                        return true;
                    }
                }
            }
        }

        false
    }

    /// Get all currently running meeting apps
    pub fn get_active_meetings(&mut self) -> Vec<DetectedMeeting> {
        self.system.refresh_processes_specifics(
            sysinfo::ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::new(),
        );

        let patterns = get_process_patterns();
        let mut detected: Vec<DetectedMeeting> = Vec::new();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        for (pid, process) in self.system.processes() {
            let pid_u32 = pid.as_u32();
            let process_name = process.name().to_string_lossy().to_lowercase();

            for pattern in &patterns {
                for &name in &pattern.process_names {
                    if process_name.contains(name) || process_name == name {
                        detected.push(DetectedMeeting {
                            app: pattern.app.clone(),
                            pid: pid_u32,
                            process_name: process.name().to_string_lossy().to_string(),
                            window_title: None,
                            suggested_name: format!(
                                "{} - {}",
                                pattern.app.display_name(),
                                chrono::Local::now().format("%Y-%m-%d %H:%M")
                            ),
                            detected_at: now,
                        });
                        break;
                    }
                }
            }
        }

        detected
    }

    /// Clear the detection history (useful when user dismisses a notification)
    pub fn clear_detection_history(&mut self) {
        self.previously_detected.clear();
    }

    /// Ignore a specific PID (when user dismisses notification for that meeting)
    pub fn ignore_pid(&mut self, pid: u32) {
        self.previously_detected.insert(pid);
    }
}

impl Default for ProcessMonitor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_meeting_app_display_name() {
        assert_eq!(MeetingApp::Zoom.display_name(), "Zoom");
        assert_eq!(MeetingApp::MicrosoftTeams.display_name(), "Microsoft Teams");
        assert_eq!(MeetingApp::GoogleMeet.display_name(), "Google Meet");
    }

    #[test]
    fn test_process_monitor_creation() {
        let monitor = ProcessMonitor::new();
        assert!(monitor.previously_detected.is_empty());
    }
}
