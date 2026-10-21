use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

const BASE: &str = "/sys/devices/platform/vhci_hcd.0";

const ST_NULL: &str = "004";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortRow {
    pub hub_super: bool,
    pub port: u32,
    pub state: String,
    pub busid: String,
}

fn status_files() -> Vec<PathBuf> {
    let mut v = vec![Path::new(BASE).join("status")];
    for i in 1.. {
        let p = Path::new(BASE).join(format!("status.{i}"));
        if !p.exists() {
            break;
        }
        v.push(p);
    }
    v
}

pub fn parse_status(text: &str) -> Vec<PortRow> {
    text.lines()
        .skip(1) // header
        .filter_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            if f.len() < 7 {
                return None;
            }
            Some(PortRow {
                hub_super: f[0] == "ss",
                port: f[1].parse().ok()?,
                state: f[2].to_string(),
                busid: f[6].to_string(),
            })
        })
        .collect()
}

pub fn ports() -> Result<Vec<PortRow>> {
    if !Path::new(BASE).exists() {
        bail!("vhci_hcd is not loaded (try: sudo modprobe vhci-hcd)");
    }
    let mut rows = Vec::new();
    for f in status_files() {
        let text = std::fs::read_to_string(&f).with_context(|| format!("reading {}", f.display()))?;
        rows.extend(parse_status(&text));
    }
    Ok(rows)
}

pub fn find_free_port(speed: u32) -> Result<u32> {
    pick_free(&ports()?, speed)
}

fn pick_free(rows: &[PortRow], speed: u32) -> Result<u32> {
    let want_super = speed >= 5;
    rows.iter()
        .find(|r| r.hub_super == want_super && r.state == ST_NULL)
        .map(|r| r.port)
        .with_context(|| "no free vhci_hcd port for this device speed")
}

pub fn attach(port: u32, sockfd: i32, devid: u32, speed: u32) -> Result<()> {
    let line = format!("{port} {sockfd} {devid} {speed}");
    std::fs::write(Path::new(BASE).join("attach"), &line).with_context(|| format!("writing {line:?} to vhci_hcd attach (needs root)"))
}

pub fn detach(port: u32) -> Result<()> {
    std::fs::write(Path::new(BASE).join("detach"), port.to_string()).context("writing vhci_hcd detach (needs root)")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "hub port sta spd dev      sockfd local_busid\n\
hs  0000 004 000 00000000 000000 0-0\n\
hs  0001 006 003 00050002 000007 5-1\n\
ss  0015 004 000 00000000 000000 0-0\n";

    #[test]
    fn parses_status_rows() {
        let rows = parse_status(SAMPLE);
        assert_eq!(rows.len(), 3);
        assert_eq!(
            rows[1],
            PortRow {
                hub_super: false,
                port: 1,
                state: "006".into(),
                busid: "5-1".into()
            }
        );
        assert!(rows[2].hub_super);
    }

    #[test]
    fn picks_first_free_port_on_the_right_hub() {
        let rows = parse_status(SAMPLE);
        assert_eq!(pick_free(&rows, 2).unwrap(), 0);
        assert_eq!(pick_free(&rows, 5).unwrap(), 15);
    }

    #[test]
    fn skips_busy_ports_and_errors_when_none_free() {
        let mut rows = parse_status(SAMPLE);
        rows[0].state = "006".into();
        assert!(pick_free(&rows, 3).is_err());
    }
}
