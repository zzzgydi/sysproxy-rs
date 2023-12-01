#[cfg(test)]
mod tests {
    use sysproxy::Sysproxy;

    #[test]
    fn test_support() {
        assert!(Sysproxy::is_support());
    }

    #[test]
    fn test_get() {
        Sysproxy::get_system_proxy().unwrap();
    }

    #[test]
    fn test_enable() {
        let mut sysproxy = Sysproxy {
            enable: true,
            host: "127.0.0.1".into(),
            port: 9090,
            #[cfg(target_os = "windows")]
            bypass: "localhost;127.*".into(),
            #[cfg(not(target_os = "windows"))]
            bypass: "localhost,127.0.0.1/8".into(),
        };
        sysproxy.set_system_proxy().unwrap();

        let cur_proxy = Sysproxy::get_system_proxy().unwrap();

        assert_eq!(cur_proxy, sysproxy);

        sysproxy.enable = false;
        sysproxy.set_system_proxy().unwrap();

        let current = Sysproxy::get_system_proxy().unwrap();
        assert_eq!(current, sysproxy);
    }
}
