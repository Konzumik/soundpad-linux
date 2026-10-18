//! Virtual microphone built from PulseAudio modules (served by pipewire-pulse):
//!
//!   real mic ──loopback (or our DSP)──┐
//!                                     ├──> soundpad_sink (null sink) ──monitor──> soundpad_mic (remap source)
//!   our mixer (pacat) ────────────────┘
//!
//! Apps (Discord, games, OBS) record from "Soundpad Microphone".

use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::paths::{sink_name, source_name};

pub struct VirtualMic {
    base_modules: Vec<u32>,
    loopback: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub name: String,
    pub description: String,
}

fn pactl(args: &[&str]) -> Result<String, String> {
    let out = Command::new("pactl")
        .env("LC_ALL", "C")
        .args(args)
        .output()
        .map_err(|e| tr!(format!("failed to run pactl: {e}"), format!("не удалось запустить pactl: {e}")))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(format!("pactl {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim()))
    }
}

fn load_module(args: &[&str]) -> Result<u32, String> {
    let mut full = vec!["load-module"];
    full.extend_from_slice(args);
    let out = pactl(&full)?;
    out.parse().map_err(|_| {
        tr!(format!("pactl returned an unexpected module id: {out:?}"), format!("pactl вернул неожиданный id модуля: {out:?}"))
    })
}

fn unload_module(id: u32) {
    let _ = pactl(&["unload-module", &id.to_string()]);
}

/// Removes this instance's modules left over from a run that crashed or was killed.
/// Arguments are matched exactly so other instances' devices are never touched.
pub fn cleanup_stale() {
    let Ok(list) = pactl(&["list", "short", "modules"]) else { return };
    let ours = [
        format!("sink_name={}", sink_name()),
        format!("source_name={}", source_name()),
        format!("sink={}", sink_name()),
        format!("master={}.monitor", sink_name()),
    ];
    for line in list.lines() {
        if line.split_whitespace().any(|tok| ours.iter().any(|o| o == tok)) {
            if let Some(id) = line.split_whitespace().next().and_then(|s| s.parse().ok()) {
                unload_module(id);
            }
        }
    }
}

pub fn default_source() -> Option<String> {
    pactl(&["get-default-source"])
        .ok()
        .filter(|s| !s.is_empty() && !s.starts_with("soundpad") && !s.ends_with(".monitor"))
}

fn list_devices(kind: &str, keep: impl Fn(&str) -> bool) -> Vec<DeviceInfo> {
    let Ok(text) = pactl(&["list", kind]) else { return Vec::new() };
    let mut result = Vec::new();
    let mut name: Option<String> = None;
    for line in text.lines() {
        let line = line.trim();
        if let Some(n) = line.strip_prefix("Name: ") {
            name = Some(n.to_string());
        } else if let Some(d) = line.strip_prefix("Description: ") {
            if let Some(n) = name.take() {
                if keep(&n) {
                    result.push(DeviceInfo { name: n, description: d.to_string() });
                }
            }
        }
    }
    result
}

/// Real capture devices (monitors and Soundpad's own devices are excluded).
pub fn list_sources() -> Vec<DeviceInfo> {
    list_devices("sources", |n| !n.starts_with("soundpad") && !n.ends_with(".monitor"))
}

/// Output devices usable for monitoring.
pub fn list_sinks() -> Vec<DeviceInfo> {
    list_devices("sinks", |n| !n.starts_with("soundpad"))
}

impl VirtualMic {
    pub fn setup() -> Result<VirtualMic, String> {
        let mut vm = VirtualMic { base_modules: Vec::new(), loopback: None };
        vm.base_modules.push(load_module(&[
            "module-null-sink",
            &format!("sink_name={}", sink_name()),
            "sink_properties=device.description=\"Soundpad Mixer\"",
            "rate=48000",
            "channels=2",
        ])?);
        vm.base_modules.push(load_module(&[
            "module-remap-source",
            &format!("master={}.monitor", sink_name()),
            &format!("source_name={}", source_name()),
            "source_properties=device.description=\"Soundpad Microphone\"",
        ])?);
        Ok(vm)
    }

    /// Routes the real mic straight into the virtual sink (lowest latency, no processing).
    pub fn set_loopback(&mut self, mic: Option<&str>) -> Result<(), String> {
        if let Some(id) = self.loopback.take() {
            unload_module(id);
        }
        if let Some(mic) = mic {
            self.loopback = Some(load_module(&[
                "module-loopback",
                &format!("source={mic}"),
                &format!("sink={}", sink_name()),
                "latency_msec=10",
                "source_dont_move=true",
                "sink_dont_move=true",
            ])?);
        }
        Ok(())
    }

    pub fn teardown(&mut self) {
        if let Some(id) = self.loopback.take() {
            unload_module(id);
        }
        for id in self.base_modules.drain(..).rev() {
            unload_module(id);
        }
    }
}

impl Drop for VirtualMic {
    fn drop(&mut self) {
        self.teardown();
    }
}
