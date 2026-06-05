//! Zone definitions — CSI's `.zon` files, in Styx.
//!
//! A zone maps widget names to [`Action`]s. Keys may be qualified
//! with modifiers (`shift+select`); zones may `include` another zone
//! (child bindings override) and pin a navigator mode that activates
//! with the zone. `@GoZone{zone …}` actions move between zones at
//! runtime — that's the whole layering model: sends mode, FX pages,
//! folder mode are all just zones.
//!
//! ```styx
//! zones {
//!     home {
//!         navigator @Track
//!         strip {
//!             fader  @TrackVolume
//!             vpot   @TrackPan
//!             select @TrackSelect
//!             shift+select @TrackRecordArm
//!             lcd_top @TrackName
//!         }
//!         buttons {
//!             play @Play
//!             bank_right @Bank{amount 8}
//!             global_view @GoZone{zone folder}
//!         }
//!     }
//!     folder {
//!         include home
//!         navigator @Folder
//!         strip {select @FolderSpill, lcd_bottom @FolderIndicator}
//!         buttons {global_view @GoZone{zone home}}
//!     }
//! }
//! ```

use std::collections::HashMap;

use eyre::{WrapErr, eyre};
use facet::Facet;

use crate::action::Action;
use crate::mcu::Button;
use crate::navigator::NavMode;

/// The built-in X-Touch zone set, embedded so the surface works with
/// zero configuration. `FTS_CSI_ZONES=<path>` replaces it wholesale.
pub const DEFAULT_ZONES: &str = include_str!("../config/xtouch.zones.styx");

// ── Styx wire shapes ────────────────────────────────────────────────

/// Top level of a zones file.
#[derive(Facet, Debug)]
pub struct ZonesFile {
    /// Zone activated at startup.
    pub home: Option<String>,
    pub zones: HashMap<String, ZoneDef>,
}

/// One zone as written in the file.
#[derive(Facet, Debug, Default)]
pub struct ZoneDef {
    /// Inherit every binding from this zone; local entries override.
    pub include: Option<String>,
    /// Navigator mode pinned while this zone is active.
    pub navigator: Option<NavigatorKind>,
    /// Per-strip widget bindings (apply to all 8 strips).
    pub strip: Option<HashMap<String, Action>>,
    /// Master-section widget bindings.
    pub buttons: Option<HashMap<String, Action>>,
}

#[repr(u8)]
#[derive(Facet, Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavigatorKind {
    Track,
    Folder,
}

impl From<NavigatorKind> for NavMode {
    fn from(k: NavigatorKind) -> Self {
        match k {
            NavigatorKind::Track => NavMode::Track,
            NavigatorKind::Folder => NavMode::Folder,
        }
    }
}

// ── Widget + modifier keys ──────────────────────────────────────────

/// Modifier bitmask (Shift / Option / Control / Alt — the four MCU
/// modifier keys).
pub type Modifiers = u8;
pub const SHIFT: Modifiers = 1 << 0;
pub const OPTION: Modifiers = 1 << 1;
pub const CONTROL: Modifiers = 1 << 2;
pub const ALT: Modifiers = 1 << 3;

/// Widgets that exist once per channel strip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StripWidget {
    Fader,
    VPot,
    VPotPress,
    Rec,
    Solo,
    Mute,
    Select,
    LcdTop,
    LcdBottom,
}

impl StripWidget {
    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "fader" => Self::Fader,
            "vpot" => Self::VPot,
            "vpot_press" => Self::VPotPress,
            "rec" => Self::Rec,
            "solo" => Self::Solo,
            "mute" => Self::Mute,
            "select" => Self::Select,
            "lcd_top" => Self::LcdTop,
            "lcd_bottom" => Self::LcdBottom,
            _ => return None,
        })
    }
}

/// Master-section widgets bindable from the `buttons` block. Named
/// buttons map onto the MCU note map; `master_fader` and `jog` are
/// the two non-button gestures that live here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GlobalWidget {
    MasterFader,
    Jog,
    Button(Button),
}

fn parse_global_widget(s: &str) -> Option<GlobalWidget> {
    Some(match s {
        "master_fader" => GlobalWidget::MasterFader,
        "jog" => GlobalWidget::Jog,
        "play" => GlobalWidget::Button(Button::Play),
        "stop" => GlobalWidget::Button(Button::Stop),
        "record" => GlobalWidget::Button(Button::Record),
        "rewind" => GlobalWidget::Button(Button::Rewind),
        "fast_forward" => GlobalWidget::Button(Button::FastForward),
        "cycle" => GlobalWidget::Button(Button::Cycle),
        "bank_left" => GlobalWidget::Button(Button::BankLeft),
        "bank_right" => GlobalWidget::Button(Button::BankRight),
        "channel_left" => GlobalWidget::Button(Button::ChannelLeft),
        "channel_right" => GlobalWidget::Button(Button::ChannelRight),
        "flip" => GlobalWidget::Button(Button::Flip),
        "global_view" => GlobalWidget::Button(Button::GlobalView),
        "marker" => GlobalWidget::Button(Button::Marker),
        "nudge" => GlobalWidget::Button(Button::Nudge),
        "drop" => GlobalWidget::Button(Button::Drop),
        "replace" => GlobalWidget::Button(Button::Replace),
        "click" => GlobalWidget::Button(Button::Click),
        "solo_global" => GlobalWidget::Button(Button::SoloGlobal),
        "up" => GlobalWidget::Button(Button::Up),
        "down" => GlobalWidget::Button(Button::Down),
        "left" => GlobalWidget::Button(Button::Left),
        "right" => GlobalWidget::Button(Button::Right),
        "zoom" => GlobalWidget::Button(Button::Zoom),
        "scrub" => GlobalWidget::Button(Button::Scrub),
        s if s.starts_with('f') => {
            let n: u8 = s[1..].parse().ok()?;
            (1..=8)
                .contains(&n)
                .then(|| GlobalWidget::Button(Button::Function(n - 1)))?
        }
        _ => return None,
    })
}

/// Split `shift+option+select` into (modifier mask, widget name).
fn parse_qualified(key: &str) -> eyre::Result<(Modifiers, &str)> {
    let mut mods: Modifiers = 0;
    let mut rest = key;
    loop {
        let Some((head, tail)) = rest.split_once('+') else {
            return Ok((mods, rest));
        };
        mods |= match head {
            "shift" => SHIFT,
            "option" => OPTION,
            "control" => CONTROL,
            "alt" => ALT,
            other => return Err(eyre!("unknown modifier '{other}' in binding '{key}'")),
        };
        rest = tail;
    }
}

// ── Compiled zones ──────────────────────────────────────────────────

/// One zone after include-resolution and key parsing.
#[derive(Debug, Default, Clone)]
pub struct CompiledZone {
    pub navigator: Option<NavMode>,
    pub strip: HashMap<(Modifiers, StripWidget), Action>,
    pub global: HashMap<(Modifiers, GlobalWidget), Action>,
}

impl CompiledZone {
    /// Look up a strip binding: exact modifier match first, then the
    /// unmodified binding (CSI's fallback rule — a missing
    /// `shift+mute` falls back to `mute`).
    pub fn strip_action(&self, mods: Modifiers, widget: StripWidget) -> Option<&Action> {
        self.strip
            .get(&(mods, widget))
            .or_else(|| self.strip.get(&(0, widget)))
    }

    pub fn global_action(&self, mods: Modifiers, widget: GlobalWidget) -> Option<&Action> {
        self.global
            .get(&(mods, widget))
            .or_else(|| self.global.get(&(0, widget)))
    }
}

/// Every zone, compiled, plus the home zone name.
#[derive(Debug)]
pub struct ZoneSet {
    pub home: String,
    zones: HashMap<String, CompiledZone>,
}

impl ZoneSet {
    /// Parse + compile a zones file.
    pub fn parse(source: &str) -> eyre::Result<Self> {
        let file: ZonesFile =
            facet_styx::from_str(source).map_err(|e| eyre!("zones file parse: {e}"))?;
        Self::compile(file)
    }

    /// The built-in X-Touch set.
    pub fn builtin() -> Self {
        Self::parse(DEFAULT_ZONES).expect("embedded default zones must compile")
    }

    /// Load from `FTS_CSI_ZONES` when set, else the built-in set.
    pub fn load() -> eyre::Result<Self> {
        match std::env::var("FTS_CSI_ZONES") {
            Ok(path) if !path.is_empty() => {
                let text = std::fs::read_to_string(&path)
                    .wrap_err_with(|| format!("read zones file {path}"))?;
                Self::parse(&text).wrap_err_with(|| format!("compile zones file {path}"))
            }
            _ => Ok(Self::builtin()),
        }
    }

    pub fn zone(&self, name: &str) -> Option<&CompiledZone> {
        self.zones.get(name)
    }

    fn compile(file: ZonesFile) -> eyre::Result<Self> {
        let home = file.home.clone().unwrap_or_else(|| "home".to_string());
        if !file.zones.contains_key(&home) {
            return Err(eyre!("home zone '{home}' is not defined"));
        }
        let mut compiled: HashMap<String, CompiledZone> = HashMap::new();
        for name in file.zones.keys() {
            compile_zone(name, &file.zones, &mut compiled, &mut Vec::new())?;
        }
        Ok(Self {
            home,
            zones: compiled,
        })
    }
}

/// Depth-first include resolution with cycle detection. Parent
/// bindings land first; the zone's own entries override.
fn compile_zone(
    name: &str,
    defs: &HashMap<String, ZoneDef>,
    out: &mut HashMap<String, CompiledZone>,
    visiting: &mut Vec<String>,
) -> eyre::Result<CompiledZone> {
    if let Some(done) = out.get(name) {
        return Ok(done.clone());
    }
    if visiting.iter().any(|v| v == name) {
        return Err(eyre!("include cycle through zone '{name}'"));
    }
    let def = defs
        .get(name)
        .ok_or_else(|| eyre!("zone '{name}' referenced but not defined"))?;

    visiting.push(name.to_string());
    let mut zone = match &def.include {
        Some(parent) => compile_zone(parent, defs, out, visiting)?,
        None => CompiledZone::default(),
    };
    visiting.pop();

    if let Some(nav) = def.navigator {
        zone.navigator = Some(nav.into());
    }
    if let Some(strip) = &def.strip {
        for (key, action) in strip {
            let (mods, widget) = parse_qualified(key)?;
            let widget = StripWidget::parse(widget)
                .ok_or_else(|| eyre!("zone '{name}': unknown strip widget '{key}'"))?;
            zone.strip.insert((mods, widget), action.clone());
        }
    }
    if let Some(buttons) = &def.buttons {
        for (key, action) in buttons {
            let (mods, widget) = parse_qualified(key)?;
            let widget = parse_global_widget(widget)
                .ok_or_else(|| eyre!("zone '{name}': unknown button widget '{key}'"))?;
            zone.global.insert((mods, widget), action.clone());
        }
    }
    out.insert(name.to_string(), zone.clone());
    Ok(zone)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_zones_compile() {
        let set = ZoneSet::builtin();
        assert_eq!(set.home, "home");
        let home = set.zone("home").expect("home zone");
        assert_eq!(
            home.strip_action(0, StripWidget::Fader),
            Some(&Action::TrackVolume)
        );
        assert_eq!(
            home.global_action(0, GlobalWidget::Button(Button::Play)),
            Some(&Action::Play)
        );
        // Folder zone inherits home's fader binding and overrides select.
        let folder = set.zone("folder").expect("folder zone");
        assert_eq!(
            folder.strip_action(0, StripWidget::Fader),
            Some(&Action::TrackVolume)
        );
        assert_eq!(
            folder.strip_action(0, StripWidget::Select),
            Some(&Action::FolderSpill)
        );
        assert_eq!(folder.navigator, Some(NavMode::Folder));
    }

    #[test]
    fn modifier_bindings_and_fallback() {
        let set = ZoneSet::parse(
            r#"
zones {
    home {
        strip {
            select @TrackSelect
            shift+select @TrackRecordArm
        }
        buttons {
            play @Play
        }
    }
}
"#,
        )
        .unwrap();
        let home = set.zone("home").unwrap();
        assert_eq!(
            home.strip_action(0, StripWidget::Select),
            Some(&Action::TrackSelect)
        );
        assert_eq!(
            home.strip_action(SHIFT, StripWidget::Select),
            Some(&Action::TrackRecordArm)
        );
        // Unbound modifier combo falls back to the plain binding.
        assert_eq!(
            home.strip_action(SHIFT | ALT, StripWidget::Select),
            Some(&Action::TrackSelect)
        );
        // Modified button with no modified binding → plain binding.
        assert_eq!(
            home.global_action(SHIFT, GlobalWidget::Button(Button::Play)),
            Some(&Action::Play)
        );
    }

    #[test]
    fn include_cycle_is_an_error() {
        let err = ZoneSet::parse(
            r#"
zones {
    home {include other}
    other {include home}
}
"#,
        )
        .unwrap_err();
        assert!(err.to_string().contains("cycle"), "{err}");
    }

    #[test]
    fn unknown_widget_is_an_error() {
        let err = ZoneSet::parse(
            r#"
zones {
    home {
        strip {frobnicator @TrackVolume}
    }
}
"#,
        )
        .unwrap_err();
        assert!(err.to_string().contains("frobnicator"), "{err}");
    }

    #[test]
    fn parameterized_actions_parse() {
        let set = ZoneSet::parse(
            r#"
zones {
    home {
        buttons {
            bank_right @Bank{amount 8}
            global_view @GoZone{zone folder}
            rewind @NudgePosition{seconds -5}
        }
    }
    folder {include home}
}
"#,
        )
        .unwrap();
        let home = set.zone("home").unwrap();
        assert_eq!(
            home.global_action(0, GlobalWidget::Button(Button::BankRight)),
            Some(&Action::Bank { amount: 8 })
        );
        assert_eq!(
            home.global_action(0, GlobalWidget::Button(Button::GlobalView)),
            Some(&Action::GoZone {
                zone: "folder".into()
            })
        );
    }
}
