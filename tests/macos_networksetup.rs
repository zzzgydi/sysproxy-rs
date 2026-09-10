#![cfg(target_os = "macos")]

use std::{
    collections::VecDeque,
    os::unix::process::ExitStatusExt,
    process::{ExitStatus, Output},
};
use sysproxy::{
    macos::{CommandRunner, Networksetup, ProxyType},
    Autoproxy, Sysproxy,
};

#[derive(Default)]
struct FakeRunner {
    replies: VecDeque<Output>,
    calls: Vec<Vec<String>>,
}
impl CommandRunner for FakeRunner {
    fn run(&mut self, args: &[&str]) -> std::io::Result<Output> {
        self.calls
            .push(args.iter().map(|s| (*s).to_owned()).collect());
        Ok(self
            .replies
            .pop_front()
            .unwrap_or_else(|| output(0, "", "")))
    }
}
fn output(exit: i32, stdout: &str, stderr: &str) -> Output {
    Output {
        status: ExitStatus::from_raw(exit << 8),
        stdout: stdout.as_bytes().to_vec(),
        stderr: stderr.as_bytes().to_vec(),
    }
}
fn proxy() -> Sysproxy {
    Sysproxy {
        enable: true,
        host: "127.0.0.1".into(),
        port: 7897,
        bypass: String::new(),
    }
}
fn pac() -> Autoproxy {
    Autoproxy {
        enable: true,
        url: "https://example.test/proxy.pac".into(),
    }
}
fn runner(replies: Vec<Output>) -> FakeRunner {
    FakeRunner {
        replies: replies.into(),
        ..Default::default()
    }
}

#[test]
fn every_read_and_write_reports_failed_exit_status() {
    for operation in 0..9 {
        let mut fake = runner(vec![output(
            1,
            "networksetup failed: ",
            "permission denied",
        )]);
        let mut backend = Networksetup::new(&mut fake);
        let result = match operation {
            0 => backend.get_proxy(ProxyType::Http, "Wi-Fi").map(|_| ()),
            1 => backend.set_proxy(ProxyType::Https, "Wi-Fi", &proxy()),
            2 => backend.get_bypass("Wi-Fi").map(|_| ()),
            3 => backend.set_bypass("Wi-Fi", "localhost"),
            4 => backend.get_auto_proxy("Wi-Fi").map(|_| ()),
            5 => backend.set_auto_proxy("USB Ethernet", &pac()),
            6 => backend.set_proxy_enabled(ProxyType::Socks, "Wi-Fi", false),
            7 => backend.set_auto_proxy_enabled("Wi-Fi", false),
            _ => backend.list_network_services().map(|_| ()),
        };
        let error = result.unwrap_err().to_string();
        assert!(
            error.contains("networksetup failed: permission denied"),
            "{operation}: {error}"
        );
        assert_eq!(fake.calls.len(), 1);
    }
}

#[test]
fn second_write_failure_is_reported_and_stops_the_operation() {
    let mut fake = runner(vec![output(0, "", ""), output(1, "", "state failed")]);
    let error = Networksetup::new(&mut fake)
        .set_proxy(ProxyType::Http, "Wi-Fi", &proxy())
        .unwrap_err();
    assert!(error.to_string().contains("state failed"));
    assert_eq!(fake.calls.len(), 2);
}

#[test]
fn disabled_empty_endpoints_accept_blank_or_zero_port() {
    for port in ["", "0"] {
        let mut fake = runner(vec![output(
            0,
            &format!("Enabled: No\nServer: \nPort: {port}\n"),
            "",
        )]);
        let mut backend = Networksetup::new(&mut fake);
        let value = backend.get_proxy(ProxyType::Http, "Wi-Fi").unwrap();
        assert_eq!(value, Sysproxy::default());
        backend.set_proxy(ProxyType::Http, "Wi-Fi", &value).unwrap();
        assert_eq!(
            fake.calls,
            [
                vec!["-getwebproxy", "Wi-Fi"],
                vec!["-setwebproxystate", "Wi-Fi", "off"]
            ]
        );
    }
}

#[test]
fn pac_second_write_failure_is_reported() {
    let mut fake = runner(vec![output(0, "", ""), output(1, "", "PAC state failed")]);
    let error = Networksetup::new(&mut fake)
        .set_auto_proxy("Wi-Fi", &pac())
        .unwrap_err();
    assert!(error.to_string().contains("PAC state failed"));
    assert_eq!(fake.calls.len(), 2);
}

#[test]
fn process_start_failure_preserves_io_error() {
    struct Unavailable;
    impl CommandRunner for Unavailable {
        fn run(&mut self, _: &[&str]) -> std::io::Result<Output> {
            Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "missing executable",
            ))
        }
    }
    let error = Networksetup::new(Unavailable)
        .list_network_services()
        .unwrap_err();
    let sysproxy::Error::Io(error) = error else {
        panic!("expected IO error")
    };
    assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
}

#[test]
fn switch_only_operations_never_write_endpoints() {
    let mut fake = FakeRunner::default();
    let mut backend = Networksetup::new(&mut fake);
    for kind in [ProxyType::Http, ProxyType::Https, ProxyType::Socks] {
        backend.set_proxy_enabled(kind, "Wi-Fi", false).unwrap();
    }
    backend.set_auto_proxy_enabled("Wi-Fi", true).unwrap();
    assert_eq!(
        fake.calls,
        [
            vec!["-setwebproxystate", "Wi-Fi", "off"],
            vec!["-setsecurewebproxystate", "Wi-Fi", "off"],
            vec!["-setsocksfirewallproxystate", "Wi-Fi", "off"],
            vec!["-setautoproxystate", "Wi-Fi", "on"],
        ]
    );
}

#[test]
fn malformed_pac_and_invalid_utf8_are_rejected() {
    for text in [
        "Enabled: No",
        "URL: (null)",
        "URL: \nEnabled: Yes",
        "URL: (null)\nEnabled: Yes",
        "URL: http://host/proxy.pac\nEnabled: Maybe",
        "URL: http://bad\turl/proxy.pac\nEnabled: Yes",
        "URL: http://bad\turl/proxy.pac\nEnabled: No",
        "URL: http://bad\0url/proxy.pac\nEnabled: Yes",
        "URL: http://bad\0url/proxy.pac\nEnabled: No",
    ] {
        let mut fake = runner(vec![output(0, text, "")]);
        assert!(
            Networksetup::new(&mut fake)
                .get_auto_proxy("Wi-Fi")
                .is_err(),
            "{text}"
        );
    }
    let mut invalid = output(0, "", "");
    invalid.stdout = vec![0xff];
    let mut fake = runner(vec![invalid]);
    assert!(Networksetup::new(&mut fake)
        .get_proxy(ProxyType::Http, "Wi-Fi")
        .is_err());
}

#[test]
fn malformed_or_incomplete_endpoints_are_rejected_on_read() {
    for text in [
        "error",
        "Enabled: Maybe\nServer: host\nPort: 80",
        "Enabled: No\nServer: host\nPort: abc",
        "Enabled: Yes\nServer: \nPort: 0",
        "Enabled: No\nServer: host\nPort: 0",
        "Enabled: No\nServer: \nPort: 8080",
        "Enabled: No\nServer: bad\tserver\nPort: 8080",
        "Enabled: Yes\nServer: bad\tserver\nPort: 8080",
        "Enabled: No\nServer: host",
        "Enabled: No\nPort: 80",
    ] {
        for kind in [ProxyType::Http, ProxyType::Https, ProxyType::Socks] {
            let mut fake = runner(vec![output(0, text, "")]);
            let result = Networksetup::new(&mut fake).get_proxy(kind, "Wi-Fi");
            assert!(result.is_err(), "{kind:?}: {text}: {result:?}");
            assert_eq!(fake.calls.len(), 1);
        }
    }
}

#[test]
fn protocol_state_and_endpoints_stay_independent() {
    let mut fake = runner(vec![
        output(0, "Enabled: Yes\nServer: http.local\nPort: 8080", ""),
        output(0, "Enabled: No\nServer: https.local\nPort: 8443", ""),
        output(0, "Enabled: Yes\nServer: socks.local\nPort: 1080", ""),
    ]);
    let mut backend = Networksetup::new(&mut fake);
    for (kind, enabled, host, port) in [
        (ProxyType::Http, true, "http.local", 8080),
        (ProxyType::Https, false, "https.local", 8443),
        (ProxyType::Socks, true, "socks.local", 1080),
    ] {
        let actual = backend.get_proxy(kind, "Wi-Fi").unwrap();
        assert_eq!(
            (actual.enable, actual.host.as_str(), actual.port),
            (enabled, host, port)
        );
    }
    assert_eq!(
        fake.calls.iter().map(|c| c[0].as_str()).collect::<Vec<_>>(),
        [
            "-getwebproxy",
            "-getsecurewebproxy",
            "-getsocksfirewallproxy"
        ]
    );
}

#[test]
fn disabled_empty_endpoint_only_changes_switch() {
    let mut fake = FakeRunner::default();
    Networksetup::new(&mut fake)
        .set_proxy(ProxyType::Http, "Wi-Fi", &Sysproxy::default())
        .unwrap();
    assert_eq!(fake.calls, [vec!["-setwebproxystate", "Wi-Fi", "off"]]);
}

#[test]
fn disabled_configured_endpoint_is_preserved_on_restore() {
    let mut fake = runner(vec![output(
        0,
        "Enabled: No\nServer: 127.0.0.1\nPort: 7897",
        "",
    )]);
    let mut value = proxy();
    value.enable = false;
    let mut backend = Networksetup::new(&mut fake);
    let snapshot = backend.get_proxy(ProxyType::Socks, "USB Ethernet").unwrap();
    assert_eq!(snapshot, value);
    backend
        .set_proxy(ProxyType::Socks, "USB Ethernet", &snapshot)
        .unwrap();
    assert_eq!(
        fake.calls,
        [
            vec!["-getsocksfirewallproxy", "USB Ethernet"],
            vec![
                "-setsocksfirewallproxy",
                "USB Ethernet",
                "127.0.0.1",
                "7897"
            ],
            vec!["-setsocksfirewallproxystate", "USB Ethernet", "off"]
        ]
    );
}

#[test]
fn bypass_empty_read_and_write_match_networksetup_contract() {
    for prefix in ["There aren't any", "There are no"] {
        let mut fake = runner(vec![output(
            0,
            &format!("{prefix} bypass domains set on USB Ethernet.\n"),
            "",
        )]);
        let mut backend = Networksetup::new(&mut fake);
        assert_eq!(backend.get_bypass("USB Ethernet").unwrap(), "");
        backend.set_bypass("USB Ethernet", " , ").unwrap();
        assert_eq!(
            fake.calls[1],
            ["-setproxybypassdomains", "USB Ethernet", "Empty"]
        );
    }
}

#[test]
fn bypass_entries_are_normalized_and_passed_as_separate_arguments() {
    let mut fake = runner(vec![output(0, " localhost\n*.local\n\n", "")]);
    let mut backend = Networksetup::new(&mut fake);
    let snapshot = backend.get_bypass("Wi-Fi").unwrap();
    assert_eq!(snapshot, "localhost,*.local");
    backend.set_bypass("Wi-Fi", &snapshot).unwrap();
    backend
        .set_bypass("Wi-Fi", " localhost, ,*.local ")
        .unwrap();
    assert_eq!(
        fake.calls[1],
        ["-setproxybypassdomains", "Wi-Fi", "localhost", "*.local"]
    );
    assert_eq!(fake.calls[1], fake.calls[2]);
}

#[test]
fn invalid_bypass_entries_are_rejected_on_read() {
    for text in [
        "localhost\nbad\tdomain\n",
        "localhost\nbad\0domain\n",
        "-internal\n",
        "localhost\nEmpty\n",
        "Empty\nlocalhost\n",
    ] {
        let mut fake = runner(vec![output(0, text, "")]);
        let result = Networksetup::new(&mut fake).get_bypass("Wi-Fi");
        assert!(result.is_err(), "{text:?}: {result:?}");
        assert_eq!(fake.calls, [vec!["-getproxybypassdomains", "Wi-Fi"]]);
    }
}

#[test]
fn pac_reads_crlf_quotes_and_null_url() {
    for (text, expected) in [
        (
            "URL: \"https://example.test/proxy.pac\"\r\nEnabled: Yes\r\n",
            pac(),
        ),
        ("URL: (null)\nEnabled: No", Autoproxy::default()),
        (
            "Enabled: No\r\nURL: https://example.test/saved.pac\r\n",
            Autoproxy {
                enable: false,
                url: "https://example.test/saved.pac".into(),
            },
        ),
        ("URL: \"\"\nEnabled: No", Autoproxy::default()),
    ] {
        let mut fake = runner(vec![output(0, text, "")]);
        let mut backend = Networksetup::new(&mut fake);
        let snapshot = backend.get_auto_proxy("USB Ethernet").unwrap();
        assert_eq!(snapshot, expected);
        backend.set_auto_proxy("USB Ethernet", &snapshot).unwrap();
        assert_eq!(
            fake.calls,
            [
                vec!["-getautoproxyurl", "USB Ethernet"],
                vec![
                    "-setautoproxyurl",
                    "USB Ethernet",
                    if expected.url.is_empty() {
                        "\"\""
                    } else {
                        &expected.url
                    },
                ],
                vec![
                    "-setautoproxystate",
                    "USB Ethernet",
                    if expected.enable { "on" } else { "off" },
                ],
            ]
        );
    }
}

#[test]
fn pac_writes_named_service_and_disable_preserves_saved_url() {
    let mut fake = FakeRunner::default();
    let mut backend = Networksetup::new(&mut fake);
    backend.set_auto_proxy("USB Ethernet", &pac()).unwrap();
    backend
        .set_auto_proxy_enabled("USB Ethernet", false)
        .unwrap();
    assert_eq!(
        fake.calls,
        [
            vec![
                "-setautoproxyurl",
                "USB Ethernet",
                "https://example.test/proxy.pac"
            ],
            vec!["-setautoproxystate", "USB Ethernet", "on"],
            vec!["-setautoproxystate", "USB Ethernet", "off"]
        ]
    );
}

#[test]
fn invalid_inputs_never_reach_the_runner() {
    let mut fake = FakeRunner::default();
    let mut backend = Networksetup::new(&mut fake);
    assert!(backend
        .set_proxy(
            ProxyType::Http,
            "Wi-Fi",
            &Sysproxy {
                enable: true,
                ..Default::default()
            }
        )
        .is_err());
    assert!(backend
        .set_auto_proxy(
            "Wi-Fi",
            &Autoproxy {
                enable: true,
                ..Default::default()
            }
        )
        .is_err());
    assert!(backend
        .set_auto_proxy(
            "Wi-Fi",
            &Autoproxy {
                enable: true,
                url: "http://bad\nurl".into()
            }
        )
        .is_err());
    assert!(backend.set_bypass("Wi-Fi", "--option").is_err());
    assert!(backend.set_bypass("Wi-Fi", "localhost,Empty").is_err());
    assert!(backend.set_bypass("Wi-Fi", "Empty,localhost").is_err());
    assert!(backend.set_bypass("Wi-Fi", "a\0b").is_err());
    assert!(backend.set_bypass("Wi-Fi", "localhost,\n*.local").is_err());
    assert!(backend.get_proxy(ProxyType::Http, "").is_err());
    assert!(backend.get_proxy(ProxyType::Http, "-getinfo").is_err());
    assert!(backend
        .set_proxy(
            ProxyType::Http,
            "Wi-Fi",
            &Sysproxy {
                host: "bad\nserver".into(),
                ..proxy()
            }
        )
        .is_err());
    assert!(fake.calls.is_empty());
}

#[test]
fn service_discovery_skips_disabled_and_header_lines() {
    let mut fake = runner(vec![output(
        0,
        "An asterisk (*) denotes that a network service is disabled.\nWi-Fi\n*Ethernet\nUSB Ethernet\n",
        "",
    )]);
    assert_eq!(
        Networksetup::new(&mut fake)
            .list_network_services()
            .unwrap(),
        ["Wi-Fi", "USB Ethernet"]
    );
    let mut fake = runner(vec![output(
        0,
        "An asterisk (*) denotes that a network service is disabled.\n*Ethernet\n",
        "",
    )]);
    assert!(Networksetup::new(&mut fake)
        .list_network_services()
        .is_err());
}

#[test]
fn legacy_empty_bypass_marker_clears_the_list() {
    for bypass in ["Empty", " Empty ", "empty", "EMPTY", ", Empty, "] {
        let mut fake = FakeRunner::default();
        Networksetup::new(&mut fake)
            .set_bypass("Wi-Fi", bypass)
            .unwrap();
        assert_eq!(
            fake.calls,
            [vec!["-setproxybypassdomains", "Wi-Fi", "Empty"]]
        );
    }
}

#[test]
fn incomplete_disabled_endpoints_are_rejected_before_any_command() {
    for (host, port) in [("host", 0), ("", 8080), ("bad\nserver", 0), (" ", 80)] {
        let mut fake = FakeRunner::default();
        let value = Sysproxy {
            enable: false,
            host: host.into(),
            port,
            bypass: String::new(),
        };
        assert!(
            Networksetup::new(&mut fake)
                .set_proxy(ProxyType::Http, "Wi-Fi", &value)
                .is_err(),
            "{host:?}:{port}"
        );
        assert!(fake.calls.is_empty());
    }
}

#[test]
fn service_discovery_preserves_names_and_only_skips_the_initial_header() {
    let header = "An asterisk (*) denotes that a network service is disabled.";
    let mut fake = runner(vec![output(
        0,
        &format!(
            "{header}\r\n Office LAN \r\nAn asterisk company LAN\r\n{header}\r\n*Disabled LAN\r\n"
        ),
        "",
    )]);
    let mut backend = Networksetup::new(&mut fake);
    let services = backend.list_network_services().unwrap();
    assert_eq!(
        services,
        [" Office LAN ", "An asterisk company LAN", header]
    );
    for service in &services {
        backend
            .set_proxy_enabled(ProxyType::Http, service, false)
            .unwrap();
    }
    for (call, service) in fake.calls[1..].iter().zip(services) {
        assert_eq!(call[1], service);
    }
}

#[test]
fn empty_pac_uses_the_legacy_url_write_before_disabling() {
    let mut fake = FakeRunner::default();
    Networksetup::new(&mut fake)
        .set_auto_proxy("Wi-Fi", &Autoproxy::default())
        .unwrap();
    assert_eq!(
        fake.calls,
        [
            vec!["-setautoproxyurl", "Wi-Fi", "\"\""],
            vec!["-setautoproxystate", "Wi-Fi", "off"]
        ]
    );
}

#[test]
fn empty_pac_url_write_failure_does_not_report_success_or_write_the_switch() {
    let mut fake = runner(vec![output(1, "", "URL write failed")]);
    let error = Networksetup::new(&mut fake)
        .set_auto_proxy("Wi-Fi", &Autoproxy::default())
        .unwrap_err();
    assert!(error.to_string().contains("URL write failed"));
    assert_eq!(fake.calls, [vec!["-setautoproxyurl", "Wi-Fi", "\"\""]]);
}

#[test]
fn disabling_pac_with_a_saved_url_still_writes_the_requested_url() {
    let mut fake = FakeRunner::default();
    let mut value = pac();
    value.enable = false;
    Networksetup::new(&mut fake)
        .set_auto_proxy("Wi-Fi", &value)
        .unwrap();
    assert_eq!(
        fake.calls,
        [
            vec!["-setautoproxyurl", "Wi-Fi", value.url.as_str()],
            vec!["-setautoproxystate", "Wi-Fi", "off"]
        ]
    );
}
