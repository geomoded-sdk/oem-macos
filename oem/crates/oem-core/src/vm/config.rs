//! The one guest's configuration, as the installer writes it and the boot reads it.
//!
//! It is deliberately small. Every field is something OEM Core acts on; nothing is stored for
//! a settings screen that does not exist. The values that matter for correctness — the disk,
//! the OpenCore image, the firmware — are validated here, before a start is attempted, so a
//! broken install fails with a readable reason rather than a QEMU error in a log nobody reads.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Which graphics device the guest is given.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Graphics {
    /// `reims-graphics-pci`: the accelerated path, on a host GPU that meets the requirement.
    ReimsVgpu,
    /// `vmware-svga`: no acceleration. The honest fallback when no usable Vulkan device exists.
    VmwareSvga,
}

impl Graphics {
    /// The word `reims-ctl --graphics` takes.
    pub fn as_arg(&self) -> &'static str {
        match self {
            Graphics::ReimsVgpu => "reims-vgpu",
            Graphics::VmwareSvga => "vmware-svga",
        }
    }

    /// Whether the guest can get GPU acceleration on this device at all.
    pub fn can_accelerate(&self) -> bool {
        matches!(self, Graphics::ReimsVgpu)
    }
}

/// How the guest's display is presented.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Display {
    /// The window's size in the guest's points, `WIDTHxHEIGHT`.
    pub resolution: String,
    /// A borderless window filling the physical screen: the normal mode.
    pub borderless: bool,
}

impl Default for Display {
    fn default() -> Self {
        Display {
            resolution: "1920x1080".into(),
            borderless: true,
        }
    }
}

impl Display {
    /// `WIDTHxHEIGHT`, both non-zero. Validated because it reaches QEMU's argument list.
    pub fn check(&self) -> Result<(), String> {
        let (w, h) = self
            .resolution
            .split_once('x')
            .ok_or_else(|| format!("resolution {:?} is not WIDTHxHEIGHT", self.resolution))?;
        let w: u32 = w
            .parse()
            .map_err(|_| format!("width {:?} is not a number", w))?;
        let h: u32 = h
            .parse()
            .map_err(|_| format!("height {:?} is not a number", h))?;
        if w == 0 || h == 0 {
            return Err(format!("resolution {}x{} has a zero dimension", w, h));
        }
        Ok(())
    }
}

/// The guest's configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    /// The VM's name in `reims-ctl`'s registry. One host, one guest, so this is stable.
    pub name: String,
    pub cpus: u32,
    /// Guest RAM in MiB. This is the guest's memory, never counted as host memory.
    pub memory_mib: u64,
    /// The guest's disk image.
    pub disk: PathBuf,
    /// The OpenCore image `reims-ctl` made for this guest.
    pub opencore: PathBuf,
    /// OVMF's code and variables, which OpenCore boots under.
    pub firmware_code: PathBuf,
    pub firmware_vars: PathBuf,
    pub graphics: Graphics,
    pub display: Display,
    /// Whether the guest has a network card (user-mode NAT).
    pub network: bool,
    /// Whether the guest has audio devices.
    pub audio: bool,
}

impl Config {
    /// The directory the VM lives in: where `reims-ctl` keeps `disk.qcow2`, `opencore.qcow2`
    /// and the firmware, so the config's paths and the engine's agree by construction.
    pub fn dir(&self) -> &Path {
        self.disk.parent().unwrap_or_else(|| Path::new("."))
    }

    /// Every path the guest cannot start without.
    pub fn required_paths(&self) -> Vec<(&'static str, &Path)> {
        vec![
            ("the disk", &self.disk),
            ("the OpenCore image", &self.opencore),
            ("the firmware code", &self.firmware_code),
            ("the firmware variables", &self.firmware_vars),
        ]
    }

    /// Checks everything a start needs, before a start is attempted.
    ///
    /// The order matters for the message the user sees: a missing disk is reported before a
    /// bad resolution, because a missing disk is the more likely and more serious fault.
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("the guest has no name".into());
        }
        // `reims-ctl`'s own name rule: it becomes a directory name and a systemd unit name.
        if !self
            .name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Err(format!(
                "the guest name {:?} may hold only letters, digits, '-' and '_'",
                self.name
            ));
        }
        if self.cpus == 0 {
            return Err("the guest needs at least one CPU".into());
        }
        if self.memory_mib < 4096 {
            // macOS will not install with less; upstream's own examples start at 8G.
            return Err(format!(
                "{} MiB is too little for macOS: give at least 4096",
                self.memory_mib
            ));
        }
        for (what, path) in self.required_paths() {
            if !path.exists() {
                return Err(format!("{what} {} is missing", path.display()));
            }
        }
        self.display.check()?;
        Ok(())
    }

    /// The `reims-ctl start` arguments for this guest, as the boot runs them.
    ///
    /// Only `start` is used: the guest is created by the installer, and the boot never creates
    /// or deletes anything. Keeping the boot's vocabulary to one verb is what makes the
    /// lifecycle auditable.
    /// The `reims-ctl start` arguments.
    ///
    /// The display flags are not decoration. `reims-ctl start` opens the guest's window under
    /// its own defaults unless told otherwise, and its default is a decorated window that does
    /// not fill the screen — a title bar, a border and a desktop behind it, which is exactly
    /// the "this is a virtual machine" tell the product must not have. `--borderless` makes the
    /// window the whole screen; `--resolution` sizes it to the panel the installer measured.
    ///
    /// The flags are only sent for the accelerated path. A `vmware-svga` guest has no
    /// reims-vgpu window to configure, and passing them would be rejected by a reims-ctl that
    /// was built without the library.
    pub fn start_args(&self) -> Vec<String> {
        let mut args = vec!["start".to_owned(), self.name.clone()];
        if self.graphics == Graphics::ReimsVgpu {
            args.push("--resolution".to_owned());
            args.push(self.display.resolution.clone());
            if self.display.borderless {
                args.push("--borderless".to_owned());
            }
        }
        args
    }

    /// The guest's QMP socket, at the path `reims-ctl` puts it
    /// (`crates/reims-ctl/src/vm.rs`, `qmp_socket_in`): `qmp.sock` beside the config, which is
    /// what applies whenever the VM directory is a normal length.
    pub fn qmp_socket(&self) -> std::path::PathBuf {
        self.dir().join("qmp.sock")
    }

    /// The `reims-ctl stop` arguments, for a shutdown.
    pub fn stop_args(&self) -> Vec<String> {
        vec!["stop".to_owned(), self.name.clone()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The guest must fill the screen with no window decoration. `reims-ctl`'s default is a
    /// decorated window, so the flags have to be sent explicitly or the user sees a title bar
    /// and a host desktop behind the guest.
    #[test]
    fn start_asks_for_a_borderless_window_at_the_panel_resolution() {
        let dir = Path::new("/var/lib/oem/vm");
        let mut c = good(dir);
        c.display.resolution = "2560x1600".into();
        let args = c.start_args();
        assert_eq!(args[0], "start");
        assert_eq!(args[1], "mac");
        let at = |flag: &str| args.iter().position(|a| a == flag);
        assert!(at("--borderless").is_some(), "args: {args:?}");
        let res = at("--resolution").expect("resolution flag");
        assert_eq!(args[res + 1], "2560x1600");
    }

    /// A software guest has no reims-vgpu window, so the display flags are not sent: reims-ctl
    /// would reject them.
    #[test]
    fn a_software_guest_gets_no_display_flags() {
        let dir = Path::new("/var/lib/oem/vm");
        let mut c = good(dir);
        c.graphics = Graphics::VmwareSvga;
        assert_eq!(c.start_args(), vec!["start".to_owned(), "mac".to_owned()]);
    }

    /// Turning the borderless option off is honoured, which is what a maintenance session with
    /// a window manager would want.
    #[test]
    fn a_windowed_display_is_possible() {
        let dir = Path::new("/var/lib/oem/vm");
        let mut c = good(dir);
        c.display.borderless = false;
        let args = c.start_args();
        assert!(!args.iter().any(|a| a == "--borderless"), "args: {args:?}");
        assert!(args.iter().any(|a| a == "--resolution"), "args: {args:?}");
    }

    fn good(dir: &Path) -> Config {
        Config {
            name: "mac".into(),
            cpus: 4,
            memory_mib: 8192,
            disk: dir.join("disk.qcow2"),
            opencore: dir.join("opencore.qcow2"),
            firmware_code: dir.join("OVMF_CODE.fd"),
            firmware_vars: dir.join("OVMF_VARS.fd"),
            graphics: Graphics::ReimsVgpu,
            display: Display::default(),
            network: true,
            audio: true,
        }
    }

    fn touch_all(c: &Config) {
        for (_, p) in c.required_paths() {
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, b"x").unwrap();
        }
    }

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "oem-vmcfg-{}-{}-{name}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_complete_config_validates() {
        let d = temp("ok");
        let c = good(&d);
        touch_all(&c);
        assert_eq!(c.validate(), Ok(()));
        assert_eq!(c.dir(), d);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_missing_disk_is_named() {
        let d = temp("nodisk");
        let c = good(&d);
        touch_all(&c);
        std::fs::remove_file(&c.disk).unwrap();
        let err = c.validate().unwrap_err();
        assert!(err.contains("the disk"), "{err}");
        assert!(err.contains("disk.qcow2"), "{err}");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// The disk is checked before the resolution: a missing disk is the more serious fault.
    #[test]
    fn disk_is_reported_before_a_bad_resolution() {
        let d = temp("order");
        let mut c = good(&d);
        touch_all(&c);
        std::fs::remove_file(&c.disk).unwrap();
        c.display.resolution = "wide".into();
        assert!(c.validate().unwrap_err().contains("the disk"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn too_little_memory_is_refused() {
        let d = temp("mem");
        let mut c = good(&d);
        touch_all(&c);
        c.memory_mib = 2048;
        assert!(c.validate().unwrap_err().contains("4096"));
        c.memory_mib = 4096;
        assert_eq!(c.validate(), Ok(()));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_name_with_a_slash_is_refused() {
        let d = temp("name");
        let mut c = good(&d);
        touch_all(&c);
        c.name = "../escape".into();
        assert!(c.validate().unwrap_err().contains("letters"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn resolution_rules() {
        assert!(Display {
            resolution: "1920x1080".into(),
            borderless: true
        }
        .check()
        .is_ok());
        assert!(Display {
            resolution: "0x1080".into(),
            borderless: true
        }
        .check()
        .is_err());
        assert!(Display {
            resolution: "1920".into(),
            borderless: true
        }
        .check()
        .is_err());
        assert!(Display {
            resolution: "axb".into(),
            borderless: true
        }
        .check()
        .is_err());
    }

    #[test]
    fn graphics_words_match_reims_ctl() {
        assert_eq!(Graphics::ReimsVgpu.as_arg(), "reims-vgpu");
        assert_eq!(Graphics::VmwareSvga.as_arg(), "vmware-svga");
        assert!(Graphics::ReimsVgpu.can_accelerate());
        assert!(!Graphics::VmwareSvga.can_accelerate());
    }

    /// `start` carries the display flags and `stop` carries nothing extra. Both name the guest,
    /// which is how `reims-ctl` finds it in its registry.
    #[test]
    fn the_boot_speaks_start_and_stop() {
        let d = temp("verbs");
        let c = good(&d);
        let args = c.start_args();
        assert_eq!(args[0], "start");
        assert_eq!(args[1], "mac");
        assert_eq!(c.stop_args(), vec!["stop", "mac"]);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// The config round-trips through the TOML the installer writes.
    #[test]
    fn config_round_trips_through_toml() {
        let d = temp("toml");
        let c = good(&d);
        let text = toml::to_string(&c).expect("serialise");
        let back: Config = toml::from_str(&text).expect("deserialise");
        assert_eq!(c, back);
        assert!(
            text.contains("reims-vgpu"),
            "graphics is written by its kebab-case name"
        );
        let _ = std::fs::remove_dir_all(&d);
    }
}
