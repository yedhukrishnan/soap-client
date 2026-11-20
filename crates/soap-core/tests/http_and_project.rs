use soap_core::assertions::{Assertion, Check, evaluate_all};
use soap_core::envelope::WsSecurity;
use soap_core::http::{Auth, HeaderEntry, RequestSpec, send};
use soap_core::project::{self, Project, SavedRequest};
use soap_core::wsdl::SoapVersion;
use wiremock::matchers::{body_string_contains, header, method};
use wiremock::{Mock, MockServer, ResponseTemplate};

const ENV: &str = r#"<soapenv:Envelope xmlns:soapenv="http://schemas.xmlsoap.org/soap/envelope/" xmlns:tem="http://tempuri.org/"><soapenv:Header/><soapenv:Body><tem:Add><tem:intA>2</tem:intA><tem:intB>3</tem:intB></tem:Add></soapenv:Body></soapenv:Envelope>"#;
const RESP: &str = r#"<soap:Envelope xmlns:soap="http://schemas.xmlsoap.org/soap/envelope/"><soap:Body><AddResponse xmlns="http://tempuri.org/"><AddResult>5</AddResult></AddResponse></soap:Body></soap:Envelope>"#;

fn spec(endpoint: String) -> RequestSpec {
    RequestSpec {
        endpoint,
        soap_version: SoapVersion::Soap11,
        soap_action: Some("http://tempuri.org/Add".into()),
        body: ENV.into(),
        headers: vec![],
        auth: Auth::None,
        timeout_secs: 5,
        accept_invalid_certs: false,
    }
}

#[tokio::test]
async fn soap11_with_basic_auth_and_custom_header() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(header("SOAPAction", "\"http://tempuri.org/Add\""))
        .and(header("Content-Type", "text/xml;charset=UTF-8"))
        .and(header("Authorization", "Basic dXNlcjpwYXNz"))
        .and(header("X-Trace", "abc"))
        .respond_with(ResponseTemplate::new(200).set_body_string(RESP))
        .expect(1)
        .mount(&server)
        .await;

    let mut s = spec(server.uri());
    s.auth = Auth::Basic { username: "user".into(), password: "pass".into() };
    s.headers = vec![
        HeaderEntry { name: "X-Trace".into(), value: "abc".into(), enabled: true },
        HeaderEntry { name: "X-Off".into(), value: "no".into(), enabled: false },
    ];
    let r = send(&s).await.unwrap();
    assert_eq!(r.status, 200);
    assert!(r.body_pretty.as_deref().unwrap().contains("\n"));
    assert!(!r.request.headers.iter().any(|(n, _)| n == "X-Off"));

    let checks = vec![
        Assertion { id: "1".into(), enabled: true, check: Check::StatusCode { code: 200 } },
        Assertion { id: "2".into(), enabled: true, check: Check::NoSoapFault },
        Assertion {
            id: "3".into(),
            enabled: true,
            check: Check::XPathMatch { expression: "//*[local-name()='AddResult']".into(), expected: "5".into() },
        },
    ];
    assert!(evaluate_all(&checks, &r).iter().all(|a| a.passed));
}

#[tokio::test]
async fn soap12_action_in_content_type_and_wsse() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(header("Content-Type", "application/soap+xml;charset=UTF-8;action=\"urn:x\""))
        .and(body_string_contains("<wsse:Username>alice</wsse:Username>"))
        .respond_with(ResponseTemplate::new(500).set_body_string("oops"))
        .expect(1)
        .mount(&server)
        .await;

    let mut s = spec(server.uri());
    s.soap_version = SoapVersion::Soap12;
    s.soap_action = Some("urn:x".into());
    s.body = ENV.replace("http://schemas.xmlsoap.org/soap/envelope/", "http://www.w3.org/2003/05/soap-envelope");
    s.auth = Auth::WsSecurity(WsSecurity { username: "alice".into(), password: "pw".into(), ..Default::default() });
    let r = send(&s).await.unwrap();
    assert_eq!(r.status, 500);
    assert_eq!(r.body_pretty, None, "non-XML body is not pretty-printed");
}

#[tokio::test]
async fn bad_endpoint_is_an_error() {
    assert!(send(&spec("ftp://nope".into())).await.is_err());
    // Nothing listening on port 9.
    assert!(send(&spec("http://127.0.0.1:9/".into())).await.is_err());
}

#[tokio::test]
async fn project_round_trip() {
    let fixture = format!("{}/tests/fixtures/calculator.wsdl", env!("CARGO_MANIFEST_DIR"));
    let wsdl = project::import_wsdl(&fixture).await.unwrap();
    let iface = &wsdl.document.as_ref().unwrap().interfaces[0];
    let op = &iface.operations[0];
    let mut s = spec(iface.endpoints[0].clone());
    s.body = op.sample_request.clone();
    let p = Project {
        version: 1,
        name: "Calc".into(),
        requests: vec![SavedRequest {
            id: "r1".into(),
            name: "Add · Request 1".into(),
            wsdl_id: wsdl.id.clone(),
            interface_name: iface.name.clone(),
            operation_name: op.name.clone(),
            spec: s,
            assertions: vec![Assertion { id: "a".into(), enabled: true, check: Check::NoSoapFault }],
        }],
        wsdls: vec![wsdl],
    };

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("calc.soapproj.json");
    project::save(&p, &path).unwrap();
    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(!raw.contains("sampleRequest"), "parsed document must not be persisted");

    let loaded = project::load(&path).unwrap();
    assert_eq!(loaded.name, "Calc");
    assert_eq!(loaded.requests.len(), 1);
    assert_eq!(loaded.requests[0].assertions.len(), 1);
    assert_eq!(loaded.wsdls[0].document.as_ref().unwrap().interfaces.len(), 2);
}

/// Hits the public dneonline.com calculator. Run with `cargo test -- --ignored`.
#[tokio::test]
#[ignore = "requires network access"]
async fn live_calculator_add() {
    let wsdl = project::import_wsdl("http://www.dneonline.com/calculator.asmx?WSDL").await.unwrap();
    let iface = &wsdl.document.as_ref().unwrap().interfaces[0];
    let op = iface.operations.iter().find(|o| o.name == "Add").unwrap();
    let mut s = spec(iface.endpoints[0].clone());
    s.soap_action = op.soap_action.clone();
    s.body = op.sample_request.replacen('?', "2", 1).replacen('?', "3", 1);
    let r = send(&s).await.unwrap();
    let checks = vec![
        Assertion { id: "s".into(), enabled: true, check: Check::StatusCode { code: 200 } },
        Assertion {
            id: "x".into(),
            enabled: true,
            check: Check::XPathMatch { expression: "//*[local-name()='AddResult']".into(), expected: "5".into() },
        },
    ];
    for res in evaluate_all(&checks, &r) {
        assert!(res.passed, "{}: {}\n{}", res.id, res.message, r.body);
    }
}
