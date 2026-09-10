# sysproxy-rs

A library for getting and setting system proxies. Supports Windows, macOS and Linux (via gsettings/kconfig).

## macOS network services

The existing `Sysproxy::get_http`, `get_https`, `get_socks` and corresponding
setters operate on a named service such as `Wi-Fi`. `Autoproxy` also provides
`get_auto_proxy_for_service` and `set_auto_proxy_for_service` for PAC.
The default-service convenience methods remain available.

For service discovery, independent protocol control, or a custom command runner,
use `macos::Networksetup`:

```rust,no_run
use sysproxy::{macos::{Networksetup, ProxyType}, Sysproxy};

fn configure(service: &str) -> sysproxy::Result<()> {
    let mut network = Networksetup::default();
    let http = network.get_proxy(ProxyType::Http, service)?;
    let pac = network.get_auto_proxy(service)?;
    let bypass = network.get_bypass(service)?;
    // Save these values, and the other protocols, if your application needs rollback.
    println!("HTTP: {http:?}; PAC: {pac:?}; bypass: {bypass}");

    let proxy = Sysproxy {
        enable: true,
        host: "127.0.0.1".into(),
        port: 7897,
        bypass: String::new(),
    };
    network.set_proxy(ProxyType::Http, service, &proxy)?;
    network.set_bypass(service, "localhost,127.0.0.1,*.local")?;
    network.set_auto_proxy_enabled(service, false)?;
    Ok(())
}
```

`Networksetup::list_network_services()` returns enabled services in system order.
`get_proxy` and `set_proxy` handle one protocol; bypass is read and written
separately. `get_system_proxy()` combines protocol values into one convenience
value, so applications restoring independent settings should read each protocol.

The default runner invokes `/usr/sbin/networksetup` with `LC_ALL=C`. Every read
and write checks the exit status; command failures include stdout and stderr in
`Error::Io`. Malformed responses return `Error::ParseStr`.

### Write and restore behavior

- Setters can perform multiple OS commands and are **not atomic**. A successful
  endpoint/URL write followed by a failed switch write leaves partial changes.
  The application owns snapshots, rollback, persistence and policy across services.
- macOS enables a protocol when its endpoint is written. Restoring a disabled
  but configured endpoint writes the saved endpoint, then switches it off.
- An unconfigured disabled manual proxy is represented by an empty host and
  port 0. Reading a blank port also accepts that state. Active proxies require a
  nonempty host and nonzero port. Incomplete endpoints are rejected even when
  disabled; only an empty host with port 0 represents an unconfigured endpoint.
- Setting an empty disabled manual endpoint only switches it off; it does not
  erase the saved endpoint. `set_proxy_enabled` and `set_auto_proxy_enabled`
  always change only the switch and preserve the saved endpoint or PAC URL.
- PAC setters write both URL and switch. For an empty disabled URL they retain
  the original `networksetup -setautoproxyurl` behavior of passing two literal
  quote characters (`""`), followed by switching PAC off. The getter normalizes
  that representation to an empty string. This does not guarantee removal of
  the underlying SystemConfiguration key. Use `set_auto_proxy_enabled` when
  the saved URL should remain unchanged.
- Bypass uses comma-separated entries. Empty entries are skipped and whitespace
  is trimmed. An empty list or a single `Empty` token (case-insensitive) clears
  the OS list. Mixing `Empty` with domains, control characters and leading-option
  entries are rejected. `set_system_proxy` validates endpoint and bypass before
  changing any protocol.

### Custom runners and tests

`Networksetup::new(runner)` accepts an owned runner or `&mut runner`. Implement
`macos::CommandRunner::run(&mut self, args: &[&str]) -> std::io::Result<std::process::Output>`.
Return the actual status, stdout and stderr: `Networksetup` performs the checks
and parsing. This lets tests use the production logic without changing PATH or
system settings. See `tests/macos_networksetup.rs` for examples.

## Optional utilities

The default `iptools` feature enables `sysproxy::utils`. If only proxy control is
needed, use `default-features = false`; the utilities module is then omitted.

## Testing

```sh
cargo test
cargo test --no-default-features
cargo clippy --all-targets --all-features -- -D warnings
```

On macOS these run isolated backend tests. Tests requiring a configured desktop
network service or changing real system proxies are ignored by default. The
ignored setter smoke tests do not restore the original settings; run them only
on a disposable machine, selecting the exact test explicitly.

CI explicitly runs `cargo test --test test -- --ignored --test-threads=1` on
disposable Windows runners and on Linux with an isolated D-Bus session and
configuration directory. These exercise the actual platform backend. macOS CI
runs injected-runner tests; real macOS writes still require separate validation.
