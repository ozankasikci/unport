use std::path::PathBuf;
use unport_cli::types::{
    Request, Response, Service, PORT_RANGE_END, PORT_RANGE_START,
    pid_path, registry_path, socket_path, unport_dir,
};

mod service_tests {
    use super::*;

    #[test]
    fn test_service_creation() {
        let service = Service {
            domain: "api.localhost".to_string(),
            port: 4000,
            pid: 12345,
            directory: PathBuf::from("/home/user/api"),
        };

        assert_eq!(service.domain, "api.localhost");
        assert_eq!(service.port, 4000);
        assert_eq!(service.pid, 12345);
        assert_eq!(service.directory, PathBuf::from("/home/user/api"));
    }

    #[test]
    fn test_service_clone() {
        let service = Service {
            domain: "api.localhost".to_string(),
            port: 4000,
            pid: 12345,
            directory: PathBuf::from("/home/user/api"),
        };

        let cloned = service.clone();
        assert_eq!(service, cloned);
    }

    #[test]
    fn test_service_serialize() {
        let service = Service {
            domain: "api.localhost".to_string(),
            port: 4000,
            pid: 12345,
            directory: PathBuf::from("/home/user/api"),
        };

        let json = serde_json::to_string(&service).unwrap();
        assert!(json.contains("api.localhost"));
        assert!(json.contains("4000"));
        assert!(json.contains("12345"));
    }

    #[test]
    fn test_service_deserialize() {
        let json = r#"{
            "domain": "api.localhost",
            "port": 4000,
            "pid": 12345,
            "directory": "/home/user/api"
        }"#;

        let service: Service = serde_json::from_str(json).unwrap();
        assert_eq!(service.domain, "api.localhost");
        assert_eq!(service.port, 4000);
        assert_eq!(service.pid, 12345);
    }

    #[test]
    fn test_service_roundtrip() {
        let service = Service {
            domain: "test.localhost".to_string(),
            port: 4500,
            pid: 99999,
            directory: PathBuf::from("/tmp/test"),
        };

        let json = serde_json::to_string(&service).unwrap();
        let deserialized: Service = serde_json::from_str(&json).unwrap();
        assert_eq!(service, deserialized);
    }
}

mod request_tests {
    use super::*;

    #[test]
    fn test_register_request() {
        let req = Request::Register {
            domain: "api.localhost".to_string(),
            port: 4000,
            pid: 12345,
            directory: PathBuf::from("/home/user/api"),
        };

        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("Register"));
        assert!(json.contains("api.localhost"));
    }

    #[test]
    fn test_unregister_request() {
        let req = Request::Unregister {
            domain: "api.localhost".to_string(),
        };

        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("Unregister"));
        assert!(json.contains("api.localhost"));
    }

    #[test]
    fn test_getport_request() {
        let req = Request::GetPort;
        let json = serde_json::to_string(&req).unwrap();
        assert_eq!(json, "\"GetPort\"");
    }

    #[test]
    fn test_list_request() {
        let req = Request::List;
        let json = serde_json::to_string(&req).unwrap();
        assert_eq!(json, "\"List\"");
    }

    #[test]
    fn test_stop_request() {
        let req = Request::Stop {
            domain: "api.localhost".to_string(),
        };

        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("Stop"));
        assert!(json.contains("api.localhost"));
    }

    #[test]
    fn test_shutdown_request() {
        let req = Request::Shutdown;
        let json = serde_json::to_string(&req).unwrap();
        assert_eq!(json, "\"Shutdown\"");
    }

    #[test]
    fn test_request_deserialize_register() {
        let json = r#"{"Register":{"domain":"api.localhost","port":4000,"pid":12345,"directory":"/home/user/api"}}"#;
        let req: Request = serde_json::from_str(json).unwrap();

        match req {
            Request::Register { domain, port, pid, .. } => {
                assert_eq!(domain, "api.localhost");
                assert_eq!(port, 4000);
                assert_eq!(pid, 12345);
            }
            _ => panic!("Expected Register request"),
        }
    }

    #[test]
    fn test_https_status_request() {
        let req = Request::HttpsStatus;
        let json = serde_json::to_string(&req).unwrap();
        assert_eq!(json, "\"HttpsStatus\"");
    }

    #[test]
    fn test_https_status_request_deserialize() {
        let json = "\"HttpsStatus\"";
        let req: Request = serde_json::from_str(json).unwrap();
        assert_eq!(req, Request::HttpsStatus);
    }
}

mod response_tests {
    use super::*;

    #[test]
    fn test_ok_response_with_message() {
        let resp = Response::Ok(Some("Success".to_string()));
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("Ok"));
        assert!(json.contains("Success"));
    }

    #[test]
    fn test_ok_response_without_message() {
        let resp = Response::Ok(None);
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("Ok"));
        assert!(json.contains("null"));
    }

    #[test]
    fn test_port_response() {
        let resp = Response::Port(4000);
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("Port"));
        assert!(json.contains("4000"));
    }

    #[test]
    fn test_services_response_empty() {
        let resp = Response::Services(vec![]);
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("Services"));
        assert!(json.contains("[]"));
    }

    #[test]
    fn test_services_response_with_items() {
        let services = vec![
            Service {
                domain: "api.localhost".to_string(),
                port: 4000,
                pid: 1000,
                directory: PathBuf::from("/app/api"),
            },
            Service {
                domain: "web.localhost".to_string(),
                port: 4001,
                pid: 1001,
                directory: PathBuf::from("/app/web"),
            },
        ];

        let resp = Response::Services(services);
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("api.localhost"));
        assert!(json.contains("web.localhost"));
    }

    #[test]
    fn test_error_response() {
        let resp = Response::Error("Something went wrong".to_string());
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("Error"));
        assert!(json.contains("Something went wrong"));
    }

    #[test]
    fn test_response_roundtrip() {
        let resp = Response::Port(4500);
        let json = serde_json::to_string(&resp).unwrap();
        let deserialized: Response = serde_json::from_str(&json).unwrap();
        assert_eq!(resp, deserialized);
    }

    #[test]
    fn test_https_enabled_response_true() {
        let resp = Response::HttpsEnabled(true);
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("HttpsEnabled"));
        assert!(json.contains("true"));
    }

    #[test]
    fn test_https_enabled_response_false() {
        let resp = Response::HttpsEnabled(false);
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("HttpsEnabled"));
        assert!(json.contains("false"));
    }

    #[test]
    fn test_https_enabled_response_deserialize() {
        let json = r#"{"HttpsEnabled":true}"#;
        let resp: Response = serde_json::from_str(json).unwrap();
        assert_eq!(resp, Response::HttpsEnabled(true));

        let json = r#"{"HttpsEnabled":false}"#;
        let resp: Response = serde_json::from_str(json).unwrap();
        assert_eq!(resp, Response::HttpsEnabled(false));
    }

    #[test]
    fn test_https_enabled_response_roundtrip() {
        let resp = Response::HttpsEnabled(true);
        let json = serde_json::to_string(&resp).unwrap();
        let deserialized: Response = serde_json::from_str(&json).unwrap();
        assert_eq!(resp, deserialized);
    }
}

mod port_range_tests {
    use super::*;

    #[test]
    fn test_port_range_start() {
        assert_eq!(PORT_RANGE_START, 4000);
    }

    #[test]
    fn test_port_range_end() {
        assert_eq!(PORT_RANGE_END, 5000);
    }

    #[test]
    fn test_port_range_valid() {
        assert!(PORT_RANGE_END > PORT_RANGE_START);
    }

    #[test]
    fn test_port_range_size() {
        assert_eq!(PORT_RANGE_END - PORT_RANGE_START, 1000);
    }

    #[test]
    fn test_port_in_range() {
        let port = 4500;
        assert!(port >= PORT_RANGE_START && port <= PORT_RANGE_END);
    }

    #[test]
    fn test_port_below_range() {
        let port = 3000;
        assert!(port < PORT_RANGE_START);
    }

    #[test]
    fn test_port_above_range() {
        let port = 6000;
        assert!(port > PORT_RANGE_END);
    }
}

mod path_tests {
    use super::*;

    #[test]
    fn test_unport_dir() {
        let dir = unport_dir();
        assert!(dir.ends_with(".unport"));
    }

    #[test]
    fn test_socket_path() {
        let path = socket_path();
        assert!(path.ends_with("unport.sock"));
    }

    #[test]
    fn test_pid_path() {
        let path = pid_path();
        assert!(path.ends_with("unport.pid"));
    }

    #[test]
    fn test_registry_path() {
        let path = registry_path();
        assert!(path.ends_with("registry.json"));
    }

    #[test]
    fn test_paths_in_unport_dir() {
        let dir = unport_dir();
        let sock = socket_path();
        let pid = pid_path();
        let reg = registry_path();

        assert!(sock.starts_with(&dir));
        assert!(pid.starts_with(&dir));
        assert!(reg.starts_with(&dir));
    }
}
