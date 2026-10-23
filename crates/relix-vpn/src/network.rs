/*
 * @author t0bx
 * @since 23.10.2026
 */

use std::collections::BTreeMap;
use std::net::IpAddr;
use std::time::{Duration, Instant};
use tokio::net::TcpStream;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterfaceInfo {
    pub name: String,
    pub addresses: Vec<IpAddr>,
}

pub trait NetworkInspector: Send + Sync {
    fn interfaces(&self) -> Vec<InterfaceInfo>;

    fn read_file(&self, path: &std::path::Path) -> Option<String> {
        std::fs::read_to_string(path).ok()
    }

    fn interface(&self, name: &str) -> Option<InterfaceInfo> {
        self.interfaces()
            .into_iter()
            .find(|interface| interface.name == name)
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SystemInspector;

impl NetworkInspector for SystemInspector {
    fn interfaces(&self) -> Vec<InterfaceInfo> {
        let mut grouped: BTreeMap<String, Vec<IpAddr>> = BTreeMap::new();
        for interface in if_addrs::get_if_addrs().unwrap_or_default() {
            grouped
                .entry(interface.name.clone())
                .or_default()
                .push(interface.ip());
        }
        grouped
            .into_iter()
            .map(|(name, addresses)| InterfaceInfo { name, addresses })
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeOutcome {
    Reachable { address: String, elapsed: Duration },
    UnresolvedHost(String),
    Refused(String),
    TimedOut,
}

impl ProbeOutcome {
    pub fn is_reachable(&self) -> bool {
        matches!(self, Self::Reachable { .. })
    }

    pub fn describe(&self) -> String {
        match self {
            Self::Reachable { address, elapsed } => {
                format!("reachable at {address} in {} ms", elapsed.as_millis())
            }
            Self::UnresolvedHost(message) => format!("name resolution failed: {message}"),
            Self::Refused(message) => format!("connection failed: {message}"),
            Self::TimedOut => "no answer (timed out)".into(),
        }
    }
}

pub async fn probe(host: &str, port: u16, timeout: Duration) -> ProbeOutcome {
    let started = Instant::now();
    let addresses = match tokio::net::lookup_host((host, port)).await {
        Ok(addresses) => addresses.collect::<Vec<_>>(),
        Err(error) => return ProbeOutcome::UnresolvedHost(error.to_string()),
    };
    let Some(first) = addresses.first().copied() else {
        return ProbeOutcome::UnresolvedHost(format!("{host} has no addresses"));
    };
    match tokio::time::timeout(timeout, TcpStream::connect(first)).await {
        Ok(Ok(_)) => ProbeOutcome::Reachable {
            address: first.to_string(),
            elapsed: started.elapsed(),
        },
        Ok(Err(error)) => ProbeOutcome::Refused(error.to_string()),
        Err(_) => ProbeOutcome::TimedOut,
    }
}

pub fn split_address(address: &str, default_port: u16) -> (String, u16) {
    if let Some(rest) = address.strip_prefix('[')
        && let Some((host, port)) = rest.split_once("]:")
    {
        return (host.to_owned(), port.parse().unwrap_or(default_port));
    }
    match address.rsplit_once(':') {
        Some((host, port)) if !host.contains(':') => {
            (host.to_owned(), port.parse().unwrap_or(default_port))
        }
        _ => (address.trim_matches(['[', ']']).to_owned(), default_port),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    #[test]
    fn addresses_split_into_host_and_port() {
        assert_eq!(
            split_address("10.0.0.15:5432", 1),
            ("10.0.0.15".into(), 5432)
        );
        assert_eq!(split_address("db.internal", 22), ("db.internal".into(), 22));
        assert_eq!(split_address("[fd00::1]:6379", 1), ("fd00::1".into(), 6379));
        assert_eq!(split_address("fd00::1", 80), ("fd00::1".into(), 80));
    }

    #[tokio::test]
    async fn probes_report_reachability() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let reachable = probe("127.0.0.1", port, Duration::from_secs(2)).await;
        assert!(reachable.is_reachable(), "{}", reachable.describe());
        drop(listener);
        let refused = probe("127.0.0.1", port, Duration::from_secs(2)).await;
        assert!(matches!(refused, ProbeOutcome::Refused(_)));
    }

    #[test]
    fn the_loopback_interface_is_listed() {
        let interfaces = SystemInspector.interfaces();
        assert!(
            interfaces
                .iter()
                .any(|interface| interface.addresses.iter().any(IpAddr::is_loopback))
        );
    }
}
