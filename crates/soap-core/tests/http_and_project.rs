use soap_core::assertions::{Assertion, Check, evaluate_all};
use soap_core::envelope::WsSecurity;
use soap_core::http::{Auth, HeaderEntry, RequestSpec, send};
use soap_core::project::{self, Environment, Project, SavedRequest, Variable};
use soap_core::session::{self, Session};
use soap_core::vars::{VarValue, Vars, resolve_spec, secret_values};
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
    let r = send(&s, &[]).await.unwrap();
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
    let r = send(&s, &[]).await.unwrap();
    assert_eq!(r.status, 500);
    assert_eq!(r.body_pretty, None, "non-XML body is not pretty-printed");
}

#[tokio::test]
async fn bad_endpoint_is_an_error() {
    assert!(send(&spec("ftp://nope".into()), &[]).await.is_err());
    // Nothing listening on port 9.
    assert!(send(&spec("http://127.0.0.1:9/".into()), &[]).await.is_err());
}

#[tokio::test]
async fn project_round_trip() {
    let fixture = format!("{}/tests/fixtures/calculator.wsdl", env!("CARGO_MANIFEST_DIR"));
    let mut wsdl = project::import_wsdl(&fixture).await.unwrap();
    wsdl.interface_labels.insert("CalculatorSoap".into(), "Calculator (prod)".into());
    let iface = &wsdl.document.as_ref().unwrap().interfaces[0];
    let op = &iface.operations[0];
    let mut s = spec(iface.endpoints[0].clone());
    s.body = op.sample_request.clone();
    let p = Project {
        version: 1,
        id: "p1".into(),
        name: "Calc".into(),
        variables: vec![Variable { name: "pw".into(), value: "leaked?".into(), secret: true }],
        environments: vec![Environment {
            id: "e1".into(),
            name: "Prod".into(),
            variables: vec![Variable { name: "id".into(), value: "C1".into(), secret: false }],
        }],
        active_environment: Some("e1".into()),
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
    assert!(!raw.contains("leaked?"), "secret values must never be written");

    let loaded = project::load(&path).unwrap();
    assert_eq!(loaded.name, "Calc");
    assert_eq!(loaded.requests.len(), 1);
    assert_eq!(loaded.requests[0].assertions.len(), 1);
    assert_eq!(loaded.wsdls[0].document.as_ref().unwrap().interfaces.len(), 2);
    assert_eq!(loaded.environments[0].variables[0].value, "C1");
    assert_eq!(loaded.wsdls[0].interface_labels["CalculatorSoap"], "Calculator (prod)");
    assert_eq!(loaded.active_environment.as_deref(), Some("e1"));

    // Session round trip keeps tabs and dirty state, and also strips secrets.
    let spath = dir.path().join("state/session.json");
    let sess = Session { project: p.clone(), path: None, dirty: true, open_tabs: vec!["r1".into()], active_id: Some("r1".into()) };
    session::save(&sess, &spath).unwrap();
    assert!(!std::fs::read_to_string(&spath).unwrap().contains("leaked?"));
    let back = session::load(&spath).unwrap().unwrap();
    assert!(back.dirty && back.open_tabs == ["r1"] && back.project.wsdls[0].document.is_some());

    // Missing session is not an error; a corrupt one is moved aside.
    assert!(session::load(&dir.path().join("none.json")).unwrap().is_none());
    std::fs::write(&spath, "{not json").unwrap();
    assert!(session::load(&spath).is_err());
    assert!(!spath.exists() && spath.with_extension("corrupt").exists());
}

#[tokio::test]
async fn secrets_are_masked_in_sent_request_copy() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(body_string_contains("<wsse:Password Type=\"http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-username-token-profile-1.0#PasswordText\">s3cr&amp;t</wsse:Password>"))
        .and(body_string_contains("<pw>body&lt;pw</pw>"))
        .respond_with(ResponseTemplate::new(200).set_body_string(RESP))
        .expect(1)
        .mount(&server)
        .await;

    let mut s = spec(server.uri());
    s.body = ENV.replace("<tem:intA>2</tem:intA>", "<pw>{{bodyPw}}</pw><id>{{id}}</id>");
    s.auth = Auth::WsSecurity(WsSecurity { username: "alice".into(), password: "{{authPw}}".into(), ..Default::default() });
    let vars: Vars = [
        ("bodyPw", "body<pw", true),
        ("authPw", "s3cr&t", true),
        ("id", "ACME-1001", false),
    ]
    .into_iter()
    .map(|(k, v, sec)| (k.to_string(), VarValue { value: v.into(), secret: sec }))
    .collect();
    let resolved = resolve_spec(&s, &vars).unwrap();
    let r = send(&resolved, &secret_values(&vars)).await.unwrap();
    let shown = &r.request.body;
    assert!(!shown.contains("s3cr") && !shown.contains("body&lt;pw"), "{shown}");
    assert!(shown.contains("<pw>••••••</pw>") && shown.contains("<id>ACME-1001</id>"), "{shown}");

    // Basic auth header is masked too.
    let mut b = spec(server.uri());
    b.auth = Auth::Basic { username: "u".into(), password: "pw".into() };
    let prepared = soap_core::http::prepare(&b).unwrap();
    let m = soap_core::http::masked(&prepared, &b, &[]);
    assert!(m.headers.iter().any(|(n, v)| n == "Authorization" && v == "Basic ••••••"));
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
    let r = send(&s, &[]).await.unwrap();
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
