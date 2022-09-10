#[cfg(test)]
mod tests {
    use sysproxy::Sysproxy;

    #[test]
    fn test_support() {
        assert!(Sysproxy::is_support());
    }

    #[test]
    fn test_get() {
        assert!(Sysproxy::get_system_proxy().is_ok());
    }

    #[test]
    fn test_enable() {
        let mut sysproxy = Sysproxy {
            enable: true,
            host: "127.0.0.1".into(),
            port: 9090,
            bypass: "localhost,127.0.0.1/8".into(),
        };

        assert!(sysproxy.set_system_proxy().is_ok());

        let cur_proxy = Sysproxy::get_system_proxy().unwrap();
        assert_eq!(cur_proxy, sysproxy);

        sysproxy.enable = false;
        assert!(sysproxy.set_system_proxy().is_ok());

        let current = Sysproxy::get_system_proxy().unwrap();
        assert_eq!(current, sysproxy);
    }
}
