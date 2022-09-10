use crate::{Error, Result, Sysproxy};
use std::{process::Command, str::from_utf8};

const CMD_KEY: &str = "org.gnome.system.proxy";

impl Sysproxy {
    pub fn get_system_proxy() -> Result<Sysproxy> {
        let enable = Sysproxy::get_enable()?;

        let mut socks = get_proxy("socks")?;
        let https = get_proxy("https")?;
        let http = get_proxy("http")?;

        if socks.host.len() == 0 {
            if http.host.len() > 0 {
                socks.host = http.host;
                socks.port = http.port;
            }
            if https.host.len() > 0 {
                socks.host = https.host;
                socks.port = https.port;
            }
        }

        socks.enable = enable;
        socks.bypass = Sysproxy::get_bypass().unwrap_or("".into());

        Ok(socks)
    }

    pub fn set_system_proxy(&self) -> Result<()> {
        self.set_enable()?;

        if self.enable {
            self.set_socks()?;
            self.set_https()?;
            self.set_http()?;
            self.set_bypass()?;
        }

        Ok(())
    }

    pub fn get_enable() -> Result<bool> {
        let mode = gsettings().args(["get", CMD_KEY, "mode"]).output()?;
        let mode = from_utf8(&mode.stdout).or(Err(Error::ParseStr))?.trim();
        Ok(mode == "manual")
    }

    pub fn get_bypass() -> Result<String> {
        // Todo: parse the ignore-hosts
        // ['aaa', 'bbb'] -> aaa,bbb
        let ignore = gsettings()
            .args(["get", CMD_KEY, "ignore-hosts"])
            .output()?;
        let ignore = from_utf8(&ignore.stdout).or(Err(Error::ParseStr))?;
        let bypass = ignore.to_string();
        Ok(bypass)
    }

    pub fn get_http() -> Result<Sysproxy> {
        get_proxy("http")
    }

    pub fn get_https() -> Result<Sysproxy> {
        get_proxy("https")
    }

    pub fn get_socks() -> Result<Sysproxy> {
        get_proxy("socks")
    }

    pub fn set_enable(&self) -> Result<()> {
        let mode = if self.enable { "'manual'" } else { "'none'" };
        gsettings().args(["set", CMD_KEY, "mode", mode]).status()?;
        Ok(())
    }

    pub fn set_bypass(&self) -> Result<()> {
        let bypass = self.bypass.as_str();
        gsettings()
            .args(["set", CMD_KEY, "ignore-hosts", bypass])
            .status()?;
        Ok(())
    }

    pub fn set_http(&self) -> Result<()> {
        set_proxy(self, "http")
    }

    pub fn set_https(&self) -> Result<()> {
        set_proxy(self, "https")
    }

    pub fn set_socks(&self) -> Result<()> {
        set_proxy(self, "socks")
    }
}

fn gsettings() -> Command {
    Command::new("gsettings")
}

fn set_proxy(proxy: &Sysproxy, service: &str) -> Result<()> {
    let schema = format!("{CMD_KEY}.{service}");
    let schema = schema.as_str();

    let host = format!("'{}'", proxy.host);
    let host = host.as_str();
    let port = format!("{}", proxy.port);
    let port = port.as_str();

    gsettings().args(["set", schema, "host", host]).status()?;
    gsettings().args(["set", schema, "port", port]).status()?;

    Ok(())
}

fn get_proxy(service: &str) -> Result<Sysproxy> {
    let schema = format!("{CMD_KEY}.{service}");
    let schema = schema.as_str();

    let host = gsettings().args(["get", schema, "host"]).output()?;
    let host = from_utf8(&host.stdout).or(Err(Error::ParseStr))?.trim();

    let port = gsettings().args(["get", schema, "port"]).output()?;
    let port = from_utf8(&port.stdout).or(Err(Error::ParseStr))?.trim();
    let port = port.parse().unwrap_or(80u16);

    Ok(Sysproxy {
        enable: false,
        host: String::from(host),
        port,
        bypass: "".into(),
    })
}
