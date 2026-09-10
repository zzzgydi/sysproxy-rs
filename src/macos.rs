//! macOS system proxy operations for named network services.
//!
//! [`Networksetup`] exposes independent protocol, PAC and bypass operations.
//! It does not choose application policy or provide transactions across writes.

use crate::{Autoproxy, Error, Result, Sysproxy};
use log::debug;
use std::net::{SocketAddr, UdpSocket};
use std::{process::Command, str::from_utf8};

impl Sysproxy {
    pub fn get_system_proxy() -> Result<Sysproxy> {
        let service = default_network_service().or_else(|e| {
            debug!("Failed to get network service: {:?}", e);
            default_network_service_by_ns()
        });
        if let Err(e) = service {
            debug!("Failed to get network service by networksetup: {:?}", e);
            return Err(e);
        }
        let service = service.unwrap();
        let service = service.as_str();

        let mut socks = Sysproxy::get_socks(service)?;
        debug!("Getting SOCKS proxy: {:?}", socks);

        let http = Sysproxy::get_http(service)?;
        debug!("Getting HTTP proxy: {:?}", http);

        let https = Sysproxy::get_https(service)?;
        debug!("Getting HTTPS proxy: {:?}", https);

        let bypass = Sysproxy::get_bypass(service)?;
        debug!("Getting bypass domains: {:?}", bypass);

        socks.bypass = bypass;

        if !socks.enable {
            if http.enable {
                socks.enable = true;
                socks.host = http.host;
                socks.port = http.port;
            }
            if https.enable {
                socks.enable = true;
                socks.host = https.host;
                socks.port = https.port;
            }
        }

        Ok(socks)
    }

    pub fn set_system_proxy(&self) -> Result<()> {
        // Reject invalid values before changing any protocol.
        validate_endpoint(self)?;
        bypass_domains(&self.bypass)?;
        let service = default_network_service().or_else(|e| {
            debug!("Failed to get network service: {:?}", e);
            default_network_service_by_ns()
        });
        if let Err(e) = service {
            debug!("Failed to get network service by networksetup: {:?}", e);
            return Err(e);
        }
        let service = service.unwrap();
        let service = service.as_str();

        debug!("Use network service: {}", service);

        debug!("Setting SOCKS proxy");
        self.set_socks(service)?;

        debug!("Setting HTTPS proxy");
        self.set_https(service)?;

        debug!("Setting HTTP proxy");
        self.set_http(service)?;

        debug!("Setting bypass domains");
        self.set_bypass(service)?;
        Ok(())
    }

    pub fn get_http(service: &str) -> Result<Sysproxy> {
        Networksetup::default().get_proxy(ProxyType::Http, service)
    }

    pub fn get_https(service: &str) -> Result<Sysproxy> {
        Networksetup::default().get_proxy(ProxyType::Https, service)
    }

    pub fn get_socks(service: &str) -> Result<Sysproxy> {
        Networksetup::default().get_proxy(ProxyType::Socks, service)
    }

    pub fn get_bypass(service: &str) -> Result<String> {
        Networksetup::default().get_bypass(service)
    }

    pub fn set_http(&self, service: &str) -> Result<()> {
        Networksetup::default().set_proxy(ProxyType::Http, service, self)
    }

    pub fn set_https(&self, service: &str) -> Result<()> {
        Networksetup::default().set_proxy(ProxyType::Https, service, self)
    }

    pub fn set_socks(&self, service: &str) -> Result<()> {
        Networksetup::default().set_proxy(ProxyType::Socks, service, self)
    }

    pub fn set_bypass(&self, service: &str) -> Result<()> {
        Networksetup::default().set_bypass(service, &self.bypass)
    }
}

impl Autoproxy {
    pub fn get_auto_proxy() -> Result<Autoproxy> {
        let service = default_network_service().or_else(|e| {
            debug!("Failed to get network service: {:?}", e);
            default_network_service_by_ns()
        });
        if let Err(e) = service {
            debug!("Failed to get network service by networksetup: {:?}", e);
            return Err(e);
        }
        let service = service.unwrap();
        let service = service.as_str();

        Self::get_auto_proxy_for_service(service)
    }

    /// Read PAC state for a named network service without resolving the default route.
    pub fn get_auto_proxy_for_service(service: &str) -> Result<Autoproxy> {
        Networksetup::default().get_auto_proxy(service)
    }

    pub fn set_auto_proxy(&self) -> Result<()> {
        let service = default_network_service().or_else(|e| {
            debug!("Failed to get network service: {:?}", e);
            default_network_service_by_ns()
        });
        if let Err(e) = service {
            debug!("Failed to get network service by networksetup: {:?}", e);
            return Err(e);
        }
        let service = service.unwrap();
        let service = service.as_str();

        self.set_auto_proxy_for_service(service)
    }

    /// Apply the URL and switch to a named service, using the legacy `""` argument
    /// for an empty disabled URL. Use [`Networksetup::set_auto_proxy_enabled`]
    /// to change only the switch and preserve the saved URL.
    pub fn set_auto_proxy_for_service(&self, service: &str) -> Result<()> {
        Networksetup::default().set_auto_proxy(service, self)
    }
}

/// Manual proxy protocols are kept independent, including their disabled endpoints.
#[derive(Clone, Copy, Debug)]
pub enum ProxyType {
    Http,
    Https,
    Socks,
}
impl ProxyType {
    fn target(self) -> &'static str {
        match self {
            Self::Http => "webproxy",
            Self::Https => "securewebproxy",
            Self::Socks => "socksfirewallproxy",
        }
    }
}

/// Injectable process boundary. Implementations return the unmodified exit status
/// and output; Networksetup checks failures for both reads and writes.
pub trait CommandRunner {
    fn run(&mut self, args: &[&str]) -> std::io::Result<std::process::Output>;
}
impl<R: CommandRunner + ?Sized> CommandRunner for &mut R {
    fn run(&mut self, args: &[&str]) -> std::io::Result<std::process::Output> {
        (**self).run(args)
    }
}

/// Runs `/usr/sbin/networksetup` with `LC_ALL=C` and captures its output.
#[derive(Default)]
pub struct ProcessRunner;
impl CommandRunner for ProcessRunner {
    fn run(&mut self, args: &[&str]) -> std::io::Result<std::process::Output> {
        Command::new("/usr/sbin/networksetup")
            .args(args)
            .env("LC_ALL", "C")
            .output()
    }
}

/// macOS proxy operations without application-specific ownership or rollback.
/// Setters can make more than one OS call; callers must restore their snapshot on failure.
///
/// ```no_run
/// # #[cfg(target_os = "macos")]
/// # fn main() -> sysproxy::Result<()> {
/// use sysproxy::macos::{Networksetup, ProxyType};
/// let mut network = Networksetup::default();
/// for service in network.list_network_services()? {
///     let http = network.get_proxy(ProxyType::Http, &service)?;
///     let pac = network.get_auto_proxy(&service)?;
///     println!("{service}: HTTP={http:?}, PAC={pac:?}");
/// }
/// # Ok(())
/// # }
/// # #[cfg(not(target_os = "macos"))]
/// # fn main() {}
/// ```
pub struct Networksetup<R = ProcessRunner> {
    runner: R,
}
impl Default for Networksetup<ProcessRunner> {
    fn default() -> Self {
        Self::new(ProcessRunner)
    }
}
impl<R: CommandRunner> Networksetup<R> {
    /// Create a backend with a caller-provided runner, which may be borrowed.
    pub fn new(runner: R) -> Self {
        Self { runner }
    }

    fn run(&mut self, args: &[&str]) -> Result<Vec<u8>> {
        for argument in args {
            if argument.chars().any(char::is_control) {
                return Err(Error::ParseStr(
                    "command argument contains control characters".into(),
                ));
            }
        }
        let output = self.runner.run(args)?;
        if !output.status.success() {
            return Err(Error::Io(std::io::Error::other(format!(
                "networksetup {} failed ({}): {}{}",
                args.first().copied().unwrap_or(""),
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr),
            ))));
        }
        Ok(output.stdout)
    }

    /// List enabled network services in system order.
    /// Returns [`Error::NetworkInterface`] when no enabled service is available.
    pub fn list_network_services(&mut self) -> Result<Vec<String>> {
        let output = self.run(&["-listallnetworkservices"])?;
        let text = from_utf8(&output).map_err(|_| Error::ParseStr("network services".into()))?;
        let mut lines = text.lines().peekable();
        if lines.peek() == Some(&"An asterisk (*) denotes that a network service is disabled.") {
            lines.next();
        }
        let services: Vec<String> = lines
            .filter(|s| !s.is_empty() && !s.starts_with('*'))
            .map(str::to_owned)
            .collect();
        if services.is_empty() {
            return Err(Error::NetworkInterface);
        }
        Ok(services)
    }

    /// Read one protocol. Bypass domains are separate; the returned `bypass` is empty.
    /// An unconfigured, disabled endpoint is represented by an empty host and port 0.
    /// Incomplete endpoints are rejected even when disabled, as in `set_proxy`.
    pub fn get_proxy(&mut self, kind: ProxyType, service: &str) -> Result<Sysproxy> {
        validate_service(service)?;
        let output = self.run(&[&format!("-get{}", kind.target()), service])?;
        let text = from_utf8(&output).map_err(|_| Error::ParseStr("output".into()))?;
        let enable = parse_enabled(text)?;
        let host = field(text, "Server:")?;
        let port = field(text, "Port:")?;
        let port = if !enable && host.is_empty() && port.is_empty() {
            0
        } else {
            port.parse().map_err(|_| Error::ParseStr("port".into()))?
        };
        let proxy = Sysproxy {
            enable,
            host: host.into(),
            port,
            bypass: String::new(),
        };
        validate_endpoint(&proxy)?;
        Ok(proxy)
    }

    /// Set one protocol's endpoint and switch; `proxy.bypass` is not applied.
    ///
    /// A valid endpoint is written even when disabled, then its switch is set.
    /// macOS enables the protocol during the endpoint write. If that write or the
    /// following switch write fails, the caller must restore any earlier state.
    /// A disabled empty endpoint only turns the switch off, preserving saved values.
    /// Incomplete endpoints are rejected even when disabled.
    pub fn set_proxy(&mut self, kind: ProxyType, service: &str, proxy: &Sysproxy) -> Result<()> {
        validate_service(service)?;
        validate_endpoint(proxy)?;
        if !proxy.host.is_empty() {
            self.run(&[
                &format!("-set{}", kind.target()),
                service,
                &proxy.host,
                &proxy.port.to_string(),
            ])?;
        }
        self.set_proxy_enabled(kind, service, proxy.enable)
    }

    /// Change only the switch, preserving the OS's saved host and port.
    pub fn set_proxy_enabled(
        &mut self,
        kind: ProxyType,
        service: &str,
        enabled: bool,
    ) -> Result<()> {
        validate_service(service)?;
        self.run(&[
            &format!("-set{}state", kind.target()),
            service,
            if enabled { "on" } else { "off" },
        ])?;
        Ok(())
    }

    /// Read PAC state for a named service, preserving a saved URL when disabled.
    /// Rejects values that fail the same validation as `set_auto_proxy`.
    pub fn get_auto_proxy(&mut self, service: &str) -> Result<Autoproxy> {
        validate_service(service)?;
        let output = self.run(&["-getautoproxyurl", service])?;
        let text = from_utf8(&output).map_err(|_| Error::ParseStr("auto".into()))?;
        let enable = parse_enabled(text)?;
        let url = strip_str(field(text, "URL:")?);
        let url = if url == "(null)" { "" } else { url };
        validate_auto_proxy(enable, url)?;
        Ok(Autoproxy {
            enable,
            url: url.into(),
        })
    }

    /// Write the PAC URL, then its switch. Writing a URL can temporarily enable PAC.
    /// An empty disabled URL uses the legacy `""` networksetup argument; the getter
    /// normalizes that representation to an empty string. To preserve the saved URL,
    /// use [`Self::set_auto_proxy_enabled`] instead.
    pub fn set_auto_proxy(&mut self, service: &str, proxy: &Autoproxy) -> Result<()> {
        validate_service(service)?;
        validate_auto_proxy(proxy.enable, &proxy.url)?;
        let url = if proxy.url.is_empty() {
            "\"\""
        } else {
            &proxy.url
        };
        self.run(&["-setautoproxyurl", service, url])?;
        self.set_auto_proxy_enabled(service, proxy.enable)
    }

    /// Change only the PAC switch, preserving the saved URL.
    pub fn set_auto_proxy_enabled(&mut self, service: &str, enabled: bool) -> Result<()> {
        validate_service(service)?;
        self.run(&[
            "-setautoproxystate",
            service,
            if enabled { "on" } else { "off" },
        ])?;
        Ok(())
    }

    /// Read comma-separated bypass domains; an unconfigured list is an empty string.
    /// Rejects entries that fail the same validation as `set_bypass`.
    pub fn get_bypass(&mut self, service: &str) -> Result<String> {
        validate_service(service)?;
        let output = self.run(&["-getproxybypassdomains", service])?;
        let text = from_utf8(&output).map_err(|_| Error::ParseStr("bypass".into()))?;
        if text.trim() == format!("There aren't any bypass domains set on {service}.")
            || text.trim() == format!("There are no bypass domains set on {service}.")
        {
            return Ok(String::new());
        }
        let bypass = text
            .lines()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(",");
        bypass_domains(&bypass)?;
        Ok(bypass)
    }

    /// Set comma-separated bypass domains, trimming surrounding whitespace and
    /// skipping empty entries. An empty list clears bypass using the OS's `Empty` token.
    /// A single `Empty` token (case-insensitive) also clears the list for compatibility.
    /// Mixing that token with domains is rejected.
    pub fn set_bypass(&mut self, service: &str, bypass: &str) -> Result<()> {
        validate_service(service)?;
        let domains = bypass_domains(bypass)?;
        let mut args = vec!["-setproxybypassdomains", service];
        args.extend(domains);
        self.run(&args)?;
        Ok(())
    }
}
fn validate_endpoint(proxy: &Sysproxy) -> Result<()> {
    if !proxy.enable && proxy.host.is_empty() && proxy.port == 0 {
        return Ok(());
    }
    if proxy.host.trim().is_empty() || proxy.host.chars().any(char::is_control) || proxy.port == 0 {
        return Err(Error::ParseStr("proxy endpoint".into()));
    }
    Ok(())
}

fn validate_auto_proxy(enable: bool, url: &str) -> Result<()> {
    if (enable && url.is_empty()) || url.chars().any(char::is_control) {
        return Err(Error::ParseStr("auto URL".into()));
    }
    Ok(())
}

fn bypass_domains(bypass: &str) -> Result<Vec<&str>> {
    if bypass.chars().any(char::is_control) {
        return Err(Error::ParseStr("bypass domain".into()));
    }
    let domains: Vec<_> = bypass
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if domains.is_empty() || (domains.len() == 1 && domains[0].eq_ignore_ascii_case("Empty")) {
        return Ok(vec!["Empty"]);
    }
    if domains
        .iter()
        .any(|s| s.starts_with('-') || s.eq_ignore_ascii_case("Empty"))
    {
        return Err(Error::ParseStr("bypass domain".into()));
    }
    Ok(domains)
}

fn validate_service(service: &str) -> Result<()> {
    if service.trim().is_empty()
        || service.starts_with('-')
        || service.chars().any(char::is_control)
    {
        return Err(Error::ParseStr("network service".into()));
    }
    Ok(())
}
fn run_networksetup(args: &[&str]) -> Result<Vec<u8>> {
    Networksetup::default().run(args)
}

fn field<'a>(text: &'a str, key: &str) -> Result<&'a str> {
    text.lines()
        .find_map(|line| line.strip_prefix(key))
        .map(str::trim)
        .ok_or_else(|| Error::ParseStr(key.into()))
}

fn parse_enabled(text: &str) -> Result<bool> {
    match field(text, "Enabled:")? {
        "Yes" => Ok(true),
        "No" => Ok(false),
        _ => Err(Error::ParseStr("Enabled:".into())),
    }
}

fn strip_str(text: &str) -> &str {
    text.strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or(text)
}

fn default_network_service() -> Result<String> {
    let socket = UdpSocket::bind("0.0.0.0:0")?;
    socket.connect("1.1.1.1:80")?;
    let ip = socket.local_addr()?.ip();
    let addr = SocketAddr::new(ip, 0);

    let interfaces = interfaces::Interface::get_all().or(Err(Error::NetworkInterface))?;
    let interface = interfaces
        .into_iter()
        .find(|i| i.addresses.iter().any(|a| a.addr == Some(addr)))
        .map(|i| i.name.to_owned());

    match interface {
        Some(interface) => {
            let service = get_server_by_order(interface)?;
            Ok(service)
        }
        None => Err(Error::NetworkInterface),
    }
}

fn default_network_service_by_ns() -> Result<String> {
    Networksetup::default()
        .list_network_services()?
        .into_iter()
        .next()
        .ok_or(Error::NetworkInterface)
}

#[allow(dead_code)]
fn get_service_by_device(device: String) -> Result<String> {
    let output = run_networksetup(&["-listallhardwareports"])?;
    let stdout = from_utf8(&output).or(Err(Error::ParseStr("output".into())))?;

    let hardware = stdout.split("Ethernet Address:").find_map(|s| {
        let lines = s.split('\n');
        let mut hardware = None;
        let mut device_ = None;

        for line in lines {
            if line.starts_with("Hardware Port:") {
                hardware = Some(&line[15..]);
            }
            if line.starts_with("Device:") {
                device_ = Some(&line[8..])
            }
        }

        if device == device_? {
            hardware
        } else {
            None
        }
    });

    match hardware {
        Some(hardware) => Ok(hardware.into()),
        None => Err(Error::NetworkInterface),
    }
}

fn get_server_by_order(device: String) -> Result<String> {
    let services = listnetworkserviceorder()?;
    let service = services
        .into_iter()
        .find(|(_, _, d)| d == &device)
        .map(|(s, _, _)| s);
    match service {
        Some(service) => Ok(service),
        None => Err(Error::NetworkInterface),
    }
}

fn listnetworkserviceorder() -> Result<Vec<(String, String, String)>> {
    let output = run_networksetup(&["-listnetworkserviceorder"])?;
    let stdout = from_utf8(&output).or(Err(Error::ParseStr("output".into())))?;

    let mut lines = stdout.lines();
    lines.next(); // ignore the tips

    let mut services = Vec::new();
    let mut p: Option<(String, String, String)> = None;

    for line in lines {
        if !line.starts_with('(') {
            continue;
        }

        if p.is_none() {
            let ri = line.find(')');
            if ri.is_none() {
                continue;
            }
            let ri = ri.unwrap();
            let service = line[ri + 1..].trim();
            p = Some((service.into(), "".into(), "".into()));
        } else {
            let line = &line[1..line.len() - 1];
            let pi = line.find("Port:");
            let di = line.find(", Device:");
            if pi.is_none() || di.is_none() {
                continue;
            }
            let pi = pi.unwrap();
            let di = di.unwrap();
            let port = line[pi + 5..di].trim();
            let device = line[di + 9..].trim();
            let (service, _, _) = p.as_mut().unwrap();
            *p.as_mut().unwrap() = (service.to_owned(), port.into(), device.into());
            services.push(p.take().unwrap());
        }
    }

    Ok(services)
}

#[test]
#[ignore = "reads real macOS network services"]
fn test_order() {
    let services = listnetworkserviceorder().unwrap();
    for (service, port, device) in services {
        println!("service: {}, port: {}, device: {}", service, port, device);
    }
}
