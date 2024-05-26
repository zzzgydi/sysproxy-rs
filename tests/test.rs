#[cfg(test)]
mod tests {
    use serial_test::serial;
    use sysproxy::{Autoproxy, Sysproxy};

    #[test]
    fn test_sys_support() {
        assert!(Sysproxy::is_support());
    }

    #[test]
    fn test_auto_support() {
        assert!(Autoproxy::is_support());
    }

    #[test]
    fn test_sys_get() {
        Sysproxy::get_system_proxy().unwrap();
    }

    #[test]
    fn test_auto_get() {
        Autoproxy::get_auto_proxy().unwrap();
    }

    #[test]
    #[serial]
    fn test_system_enable() {
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

    #[test]
    #[serial]
    fn test_auto_enable() {
        let mut autoproxy = Autoproxy {
            enable: true,
            url: "http://127.0.0.1:1234/".into(),
        };
        autoproxy.set_auto_proxy().unwrap();

        let cur_proxy = Autoproxy::get_auto_proxy().unwrap();

        assert_eq!(cur_proxy, autoproxy);

        autoproxy.enable = false;
        autoproxy.url = "".into();
        autoproxy.set_auto_proxy().unwrap();

        let current = Autoproxy::get_auto_proxy().unwrap();
        assert_eq!(current, autoproxy);
    }
}
