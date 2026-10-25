//! How the console starts an installed loader (SPEC BOOT-1, ADR-0042):
//! the path the launcher uses and the lines the user adds to its
//! configuration on the console. YAOPLM never writes launcher configs.
//!
//! Stock configs (research/07 §2.2–§2.4, research/17 §4, checked in
//! israpps/FreeMcBoot-Installer@ac53a47, israpps/PlayStation2-Basic-
//! BootLoader@d2e81fa and pcm720/OSDMenu@a2b847a):
//! - FreeMcBoot 1.953–1.966: the "Open PS2 Loader" item tries
//!   `mass:/UPDT/OPL.ELF` first (`path1_OSDSYS_ITEM_3`);
//! - PS2BBL's stock `PS2BBL.INI`: `LK_AUTO_E1 = mass:/APPS/OPNPS2LD.ELF`;
//!   its MX4SIO build reads `massX:` paths;
//! - OSDMenu: `name_OSDSYS_ITEM_N` and `path1_OSDSYS_ITEM_N` in
//!   `OSDMENU.CNF`, `mass:` for USB and `mx4sio:` for MX4SIO; its stock
//!   config has no OPL item.

use super::catalog::{Launcher, Target};

/// One way to start the loader.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootHint {
    pub launcher: Launcher,
    /// The path as the launcher writes it (`mass:/UPDT/OPL.ELF`); `None`
    /// for OPL's Apps page.
    pub ps2_path: Option<String>,
    /// The launcher's stock config already starts it: nothing to edit.
    pub stock: bool,
    /// The file the lines go into, on the console.
    pub config: Option<&'static str>,
    /// Lines to add, `N` standing for a free item number.
    pub lines: Vec<String>,
    /// The app's name on OPL's Apps page.
    pub title: Option<String>,
}

const FMCB_STOCK: &str = "UPDT/OPL.ELF";
const PS2BBL_STOCK: &str = "APPS/OPNPS2LD.ELF";

/// The hints for an ELF at `path` on a `device`, for each launcher that
/// can start it. `title` names the menu item (an app's `title=`, else the
/// build).
pub fn hints(path: &str, device: Target, launchers: &[Launcher], title: &str) -> Vec<BootHint> {
    let mass = format!("mass:/{path}");
    launchers
        .iter()
        .filter(|l| fits(**l, device))
        .map(|&launcher| match launcher {
            Launcher::Fmcb => {
                let stock = path.eq_ignore_ascii_case(FMCB_STOCK);
                BootHint {
                    launcher,
                    ps2_path: Some(mass.clone()),
                    stock,
                    config: Some("mc?:/SYS-CONF/FREEMCB.CNF"),
                    lines: if stock {
                        Vec::new()
                    } else {
                        vec![
                            format!("name_OSDSYS_ITEM_N = {title}"),
                            format!("path1_OSDSYS_ITEM_N = {mass}"),
                        ]
                    },
                    title: None,
                }
            }
            Launcher::Ps2bbl => {
                let stock = path.eq_ignore_ascii_case(PS2BBL_STOCK);
                BootHint {
                    launcher,
                    ps2_path: Some(mass.clone()),
                    stock,
                    config: Some("mc?:/SYS-CONF/PS2BBL.INI"),
                    lines: if stock {
                        Vec::new()
                    } else {
                        vec![format!("LK_AUTO_E1 = {mass}")]
                    },
                    title: None,
                }
            }
            Launcher::Ps2bblMx4sio => {
                let path = format!("massX:/{path}");
                BootHint {
                    launcher,
                    lines: vec![format!("LK_AUTO_E1 = {path}")],
                    ps2_path: Some(path),
                    stock: false,
                    config: Some("mc?:/SYS-CONF/PS2BBL.INI"),
                    title: None,
                }
            }
            Launcher::Osdmenu => {
                let path = match device {
                    Target::Usb => mass.clone(),
                    Target::Mx4sio => format!("mx4sio:/{path}"),
                };
                BootHint {
                    launcher,
                    lines: vec![
                        format!("name_OSDSYS_ITEM_N = {title}"),
                        format!("path1_OSDSYS_ITEM_N = {path}"),
                    ],
                    ps2_path: Some(path),
                    stock: false,
                    config: Some("mc?:/SYS-CONF/OSDMENU.CNF"),
                    title: None,
                }
            }
            Launcher::OplApps => BootHint {
                launcher,
                ps2_path: None,
                stock: true,
                config: None,
                lines: Vec::new(),
                title: Some(title.to_owned()),
            },
        })
        .collect()
}

/// FreeMcBoot reads neither MX4SIO nor exFAT; PS2BBL's plain build and
/// its MX4SIO build are different programs (research/07 §2.1, §2.8).
fn fits(launcher: Launcher, device: Target) -> bool {
    match launcher {
        Launcher::Fmcb | Launcher::Ps2bbl => device == Target::Usb,
        Launcher::Ps2bblMx4sio => device == Target::Mx4sio,
        Launcher::Osdmenu | Launcher::OplApps => true,
    }
}

/// Launchers for a path the user typed: those that take any path.
pub fn custom_launchers(device: Target) -> Vec<Launcher> {
    match device {
        Target::Usb => vec![Launcher::Ps2bbl, Launcher::Osdmenu, Launcher::Fmcb],
        Target::Mx4sio => vec![Launcher::Ps2bblMx4sio, Launcher::Osdmenu],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stock_paths_need_no_edit() {
        let got = hints(
            "UPDT/OPL.ELF",
            Target::Usb,
            &[Launcher::Fmcb],
            "Open PS2 Loader",
        );
        assert_eq!(got.len(), 1);
        assert!(got[0].stock && got[0].lines.is_empty());
        assert_eq!(got[0].ps2_path.as_deref(), Some("mass:/UPDT/OPL.ELF"));

        let got = hints("APPS/OPNPS2LD.ELF", Target::Usb, &[Launcher::Ps2bbl], "x");
        assert!(got[0].stock);
        assert_eq!(got[0].config, Some("mc?:/SYS-CONF/PS2BBL.INI"));
    }

    #[test]
    fn an_app_on_mx4sio_gets_lines_for_launchers_that_read_it() {
        let launchers = [
            Launcher::OplApps,
            Launcher::Ps2bblMx4sio,
            Launcher::Osdmenu,
            Launcher::Fmcb,
        ];
        let got = hints(
            "APPS/OPL/OPNPS2LD.ELF",
            Target::Mx4sio,
            &launchers,
            "Open PS2 Loader",
        );
        let kinds: Vec<Launcher> = got.iter().map(|h| h.launcher).collect();
        // FreeMcBoot cannot start anything from MX4SIO.
        assert_eq!(
            kinds,
            [Launcher::OplApps, Launcher::Ps2bblMx4sio, Launcher::Osdmenu]
        );
        assert_eq!(got[0].title.as_deref(), Some("Open PS2 Loader"));
        assert_eq!(got[1].lines, ["LK_AUTO_E1 = massX:/APPS/OPL/OPNPS2LD.ELF"]);
        assert_eq!(
            got[2].lines,
            [
                "name_OSDSYS_ITEM_N = Open PS2 Loader",
                "path1_OSDSYS_ITEM_N = mx4sio:/APPS/OPL/OPNPS2LD.ELF"
            ]
        );
    }

    #[test]
    fn a_custom_path_is_never_stock() {
        let got = hints(
            "GAMES/OPL.ELF",
            Target::Usb,
            &custom_launchers(Target::Usb),
            "YAOPL",
        );
        assert!(got.iter().all(|h| !h.stock && !h.lines.is_empty()));
        assert_eq!(got[0].lines, ["LK_AUTO_E1 = mass:/GAMES/OPL.ELF"]);
        assert_eq!(got[2].lines[0], "name_OSDSYS_ITEM_N = YAOPL");
    }
}
