//! Device capability detection and tier classification.

use serde::{Deserialize, Serialize};

/// Hardware capability tier that determines available features.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CapabilityTier {
    /// FTS5 + graph only, no embeddings or local LLM.
    Base,
    /// Adds lazy embeddings (ONNX), basic vector search.
    Enhanced,
    /// Adds persistent vector index, reranker, local LLM extraction.
    Advanced,
    /// Full pipeline: all models, consolidation, LPRAG.
    Full,
}

impl std::fmt::Display for CapabilityTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Base => write!(f, "base"),
            Self::Enhanced => write!(f, "enhanced"),
            Self::Advanced => write!(f, "advanced"),
            Self::Full => write!(f, "full"),
        }
    }
}

/// Discovered hardware capabilities of the current device.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceCapabilities {
    /// Total system RAM in bytes.
    pub total_ram_bytes: u64,
    /// Available system RAM in bytes.
    pub available_ram_bytes: u64,
    /// Number of CPU cores.
    pub cpu_cores: usize,
    /// Whether a CUDA-capable GPU is detected.
    pub has_gpu: bool,
    /// GPU VRAM in bytes (0 if no GPU or shared memory).
    pub gpu_vram_bytes: u64,
    /// Whether this is a Jetson device (shared CPU/GPU memory).
    pub is_jetson: bool,
    /// Determined capability tier.
    pub tier: CapabilityTier,
}

impl DeviceCapabilities {
    /// Discover hardware capabilities of the current system.
    pub fn discover() -> Self {
        let total_ram_bytes = Self::get_total_ram();
        let available_ram_bytes = Self::get_available_ram();
        let cpu_cores = num_cpus();
        let is_jetson = Self::detect_jetson();
        let has_gpu = Self::detect_gpu();
        let gpu_vram_bytes = if is_jetson {
            // Jetson uses shared memory — report total RAM as "VRAM"
            total_ram_bytes
        } else if has_gpu {
            // Discrete GPU — we'd need nvml or similar; placeholder
            0
        } else {
            0
        };

        let tier = Self::determine_tier(total_ram_bytes, has_gpu, is_jetson);

        Self {
            total_ram_bytes,
            available_ram_bytes,
            cpu_cores,
            has_gpu,
            gpu_vram_bytes,
            is_jetson,
            tier,
        }
    }

    /// Determine capability tier based on hardware.
    fn determine_tier(total_ram: u64, has_gpu: bool, is_jetson: bool) -> CapabilityTier {
        let ram_gb = total_ram as f64 / (1024.0 * 1024.0 * 1024.0);

        if is_jetson || (has_gpu && ram_gb >= 6.0) {
            // Jetson Orin Nano (7.4GB shared) or decent GPU
            CapabilityTier::Full
        } else if has_gpu && ram_gb >= 4.0 {
            CapabilityTier::Advanced
        } else if ram_gb >= 2.0 {
            CapabilityTier::Enhanced
        } else {
            CapabilityTier::Base
        }
    }

    fn get_total_ram() -> u64 {
        #[cfg(target_os = "linux")]
        {
            use std::fs;
            if let Ok(meminfo) = fs::read_to_string("/proc/meminfo") {
                for line in meminfo.lines() {
                    if line.starts_with("MemTotal:") {
                        if let Some(kb_str) = line.split_whitespace().nth(1) {
                            if let Ok(kb) = kb_str.parse::<u64>() {
                                return kb * 1024;
                            }
                        }
                    }
                }
            }
            0
        }
        #[cfg(target_os = "macos")]
        {
            use std::process::Command;
            if let Ok(output) = Command::new("sysctl").arg("-n").arg("hw.memsize").output() {
                if let Ok(s) = String::from_utf8(output.stdout) {
                    if let Ok(bytes) = s.trim().parse::<u64>() {
                        return bytes;
                    }
                }
            }
            0
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            0
        }
    }

    fn get_available_ram() -> u64 {
        #[cfg(target_os = "linux")]
        {
            use std::fs;
            if let Ok(meminfo) = fs::read_to_string("/proc/meminfo") {
                for line in meminfo.lines() {
                    if line.starts_with("MemAvailable:") {
                        if let Some(kb_str) = line.split_whitespace().nth(1) {
                            if let Ok(kb) = kb_str.parse::<u64>() {
                                return kb * 1024;
                            }
                        }
                    }
                }
            }
            0
        }
        #[cfg(not(target_os = "linux"))]
        {
            Self::get_total_ram() / 2 // rough approximation
        }
    }

    fn detect_jetson() -> bool {
        #[cfg(target_os = "linux")]
        {
            use std::fs;
            // Jetson devices have /etc/nv_tegra_release or /proc/device-tree/model
            if fs::metadata("/etc/nv_tegra_release").is_ok() {
                return true;
            }
            if let Ok(model) = fs::read_to_string("/proc/device-tree/model") {
                if model.to_lowercase().contains("jetson") {
                    return true;
                }
            }
            false
        }
        #[cfg(not(target_os = "linux"))]
        {
            false
        }
    }

    fn detect_gpu() -> bool {
        #[cfg(target_os = "linux")]
        {
            use std::fs;
            // Check for NVIDIA GPU
            fs::metadata("/dev/nvidia0").is_ok()
                || fs::metadata("/dev/nvhost-gpu").is_ok() // Jetson
        }
        #[cfg(target_os = "macos")]
        {
            // macOS always has Metal GPU
            true
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            false
        }
    }
}

fn num_cpus() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
}
