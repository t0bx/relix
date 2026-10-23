/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::error::VpnError;
use ipnet::IpNet;
use relix_core::VpnProvider;
use std::net::IpAddr;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProfileFile {
    pub routes: Vec<IpNet>,
    pub endpoints: Vec<String>,
    pub full_tunnel: bool,
}

pub fn read(provider: VpnProvider, path: &Path) -> Result<ProfileFile, VpnError> {
    let text = std::fs::read_to_string(path).map_err(|error| VpnError::ConfigFile {
        path: path.to_path_buf(),
        message: error.to_string(),
    })?;
    let parsed = match provider {
        VpnProvider::WireGuard => parse_wireguard(&text),
        VpnProvider::OpenVpn => parse_openvpn(&text),
        VpnProvider::Other => Ok(ProfileFile::default()),
    };
    parsed.map_err(|message| VpnError::ConfigFile {
        path: path.to_path_buf(),
        message,
    })
}

pub fn parse_wireguard(text: &str) -> Result<ProfileFile, String> {
    let mut file = ProfileFile::default();
    let mut section = String::new();
    let mut has_interface_key = false;
    let mut peers = 0;
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or_default().trim();
        if line.is_empty() {
            continue;
        }
        if let Some(name) = line
            .strip_prefix('[')
            .and_then(|rest| rest.strip_suffix(']'))
        {
            section = name.trim().to_ascii_lowercase();
            peers += usize::from(section == "peer");
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(format!("unexpected line '{line}'"));
        };
        let key = key.trim().to_ascii_lowercase();
        match (section.as_str(), key.as_str()) {
            ("interface", "privatekey") => has_interface_key = true,
            ("peer", "allowedips") => {
                for range in value
                    .split(',')
                    .map(str::trim)
                    .filter(|range| !range.is_empty())
                {
                    let network = parse_network(range)?;
                    file.full_tunnel |= network.prefix_len() == 0;
                    file.routes.push(network);
                }
            }
            ("peer", "endpoint") => file.endpoints.push(value.trim().to_owned()),
            _ => {}
        }
    }
    if !has_interface_key {
        return Err("missing [Interface] PrivateKey".into());
    }
    if peers == 0 {
        return Err("no [Peer] section".into());
    }
    Ok(file)
}

pub fn parse_openvpn(text: &str) -> Result<ProfileFile, String> {
    let mut file = ProfileFile::default();
    let mut has_remote = false;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty()
            || line.starts_with('#')
            || line.starts_with(';')
            || line.starts_with('<')
        {
            continue;
        }
        let mut words = line.split_whitespace();
        match words.next() {
            Some("remote") => {
                has_remote = true;
                file.endpoints.push(words.collect::<Vec<_>>().join(" "));
            }
            Some("route") => {
                let network = words.next().ok_or("route without network")?;
                let mask = words.next().unwrap_or("255.255.255.255");
                file.routes.push(ipv4_route(network, mask)?);
            }
            Some("redirect-gateway") => file.full_tunnel = true,
            _ => {}
        }
    }
    if !has_remote {
        return Err("no 'remote' server configured".into());
    }
    Ok(file)
}

pub fn overlapping(left: &[IpNet], right: &[IpNet]) -> Vec<IpNet> {
    left.iter()
        .filter(|a| {
            right
                .iter()
                .any(|b| a.contains(&b.network()) || b.contains(&a.network()))
        })
        .copied()
        .collect()
}

fn parse_network(value: &str) -> Result<IpNet, String> {
    value
        .parse::<IpNet>()
        .or_else(|_| value.parse::<IpAddr>().map(IpNet::from))
        .map_err(|_| format!("invalid address range '{value}'"))
}

fn ipv4_route(network: &str, mask: &str) -> Result<IpNet, String> {
    let address: std::net::Ipv4Addr = network
        .parse()
        .map_err(|_| format!("invalid route '{network}'"))?;
    let mask: std::net::Ipv4Addr = mask
        .parse()
        .map_err(|_| format!("invalid netmask '{mask}'"))?;
    let prefix = u32::from(mask).leading_ones() as u8;
    ipnet::Ipv4Net::new(address, prefix)
        .map(|net| IpNet::V4(net.trunc()))
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIREGUARD: &str = "
[Interface]
PrivateKey = aGVsbG8=
Address = 10.10.0.2/32

[Peer]
PublicKey = d29ybGQ=
Endpoint = vpn.example.com:51820
AllowedIPs = 10.10.0.0/24, 192.168.50.0/24 # office
";

    #[test]
    fn wireguard_routes_and_endpoints_are_read() {
        let file = parse_wireguard(WIREGUARD).unwrap();
        assert_eq!(file.endpoints, vec!["vpn.example.com:51820"]);
        assert_eq!(
            file.routes,
            vec![
                "10.10.0.0/24".parse::<IpNet>().unwrap(),
                "192.168.50.0/24".parse().unwrap()
            ]
        );
        assert!(!file.full_tunnel);
        assert!(parse_wireguard("[Interface]\nAddress = 1.2.3.4/32\n").is_err());
    }

    #[test]
    fn openvpn_routes_use_netmasks() {
        let file = parse_openvpn(
            "client\nremote vpn.example.com 1194 udp\nroute 10.20.0.0 255.255.0.0\nredirect-gateway def1\n",
        )
        .unwrap();
        assert_eq!(file.routes, vec!["10.20.0.0/16".parse::<IpNet>().unwrap()]);
        assert!(file.full_tunnel);
        assert!(parse_openvpn("client\n").is_err());
    }

    #[test]
    fn overlaps_are_detected_in_both_directions() {
        let office: Vec<IpNet> = vec!["10.10.0.0/16".parse().unwrap()];
        let datacenter: Vec<IpNet> = vec!["10.10.5.0/24".parse().unwrap()];
        let lab: Vec<IpNet> = vec!["172.16.0.0/12".parse().unwrap()];
        assert_eq!(overlapping(&office, &datacenter).len(), 1);
        assert_eq!(overlapping(&datacenter, &office).len(), 1);
        assert!(overlapping(&office, &lab).is_empty());
    }
}
