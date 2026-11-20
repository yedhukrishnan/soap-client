use soap_core::project::import_wsdl;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const EXPECTED: &str = r#"   <soapenv:Body>
      <m:EchoRequest>
         <c:auth>
            <c:user>?</c:user>
         </c:auth>
         <m:text>?</m:text>
         <m:mode>LOUD</m:mode>
      </m:EchoRequest>
   </soapenv:Body>"#;

fn fixture_dir() -> String {
    format!("{}/tests/fixtures/multi", env!("CARGO_MANIFEST_DIR"))
}

#[tokio::test]
async fn follows_wsdl_and_schema_imports_from_files() {
    let entry = import_wsdl(&format!("{}/service.wsdl", fixture_dir())).await.unwrap();
    let names: Vec<_> = entry.imports.iter().map(|i| i.location.rsplit('/').next().unwrap()).collect();
    // Each document is loaded once even though common.xsd imports messages.xsd back.
    assert_eq!(names, ["abstract.wsdl", "messages.xsd", "common.xsd", "message-types.xsd"]);

    let iface = &entry.document.unwrap().interfaces[0];
    assert_eq!(iface.endpoints, ["http://localhost/echo"]);
    let s = &iface.operations[0].sample_request;
    assert!(s.contains(EXPECTED), "{s}");
    assert!(s.contains(r#"xmlns:m="urn:messages""#) && s.contains(r#"xmlns:c="urn:common""#), "{s}");
}

#[tokio::test]
async fn follows_relative_imports_over_http() {
    let server = MockServer::start().await;
    let dir = fixture_dir();
    for file in ["service.wsdl", "abstract.wsdl", "schemas/messages.xsd", "schemas/message-types.xsd", "schemas/common/common.xsd"] {
        let body = std::fs::read_to_string(format!("{dir}/{file}")).unwrap();
        Mock::given(method("GET"))
            .and(path(format!("/ws/{file}")))
            .respond_with(ResponseTemplate::new(200).set_body_string(body))
            .expect(1)
            .mount(&server)
            .await;
    }
    let entry = import_wsdl(&format!("{}/ws/service.wsdl", server.uri())).await.unwrap();
    assert!(entry.imports.iter().all(|i| i.location.starts_with("http://")));
    let s = &entry.document.unwrap().interfaces[0].operations[0].sample_request;
    assert!(s.contains(EXPECTED), "{s}");
}

#[tokio::test]
async fn missing_import_names_the_file() {
    let err = import_wsdl(&format!("{}/broken.wsdl", fixture_dir())).await.unwrap_err().to_string();
    assert!(err.contains("missing.xsd") && err.contains("broken.wsdl"), "{err}");
}

const WSDL_IMPORTING: &str = r#"<wsdl:definitions targetNamespace="urn:x" xmlns:wsdl="http://schemas.xmlsoap.org/wsdl/" xmlns:xsd="http://www.w3.org/2001/XMLSchema"><wsdl:types><xsd:schema><xsd:import namespace="urn:y" schemaLocation="LOCATION"/></xsd:schema></wsdl:types></wsdl:definitions>"#;

#[tokio::test]
async fn remote_wsdl_cannot_read_local_files() {
    let server = MockServer::start().await;
    let secret = std::env::temp_dir().join(format!("soap-local-{}.xsd", std::process::id()));
    std::fs::write(&secret, r#"<xsd:schema xmlns:xsd="http://www.w3.org/2001/XMLSchema"/>"#).unwrap();
    let body = WSDL_IMPORTING.replace("LOCATION", &format!("file://{}", secret.display()));
    Mock::given(method("GET"))
        .and(path("/evil.wsdl"))
        .respond_with(ResponseTemplate::new(200).set_body_string(body))
        .mount(&server)
        .await;
    let err = import_wsdl(&format!("{}/evil.wsdl", server.uri())).await.unwrap_err().to_string();
    std::fs::remove_file(&secret).unwrap();
    assert!(err.contains("may only import other web documents"), "{err}");
}

#[tokio::test]
async fn imports_must_be_schemas_or_wsdls() {
    let dir = std::env::temp_dir().join(format!("soap-imports-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("settings.xml"), "<plist><key>token</key><string>abc</string></plist>").unwrap();
    std::fs::write(dir.join("main.wsdl"), WSDL_IMPORTING.replace("LOCATION", "settings.xml")).unwrap();
    let err = import_wsdl(dir.join("main.wsdl").to_str().unwrap()).await.unwrap_err().to_string();
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(err.contains("is not an XML Schema or WSDL") && !err.contains("abc"), "{err}");
}

/// A WSDL that redirects to another server is still downloaded, but the original URL
/// (which may carry a token) is not sent along as a Referer.
#[tokio::test]
async fn wsdl_redirect_to_another_server_sends_no_referer() {
    let body = std::fs::read_to_string(format!("{}/../calculator.wsdl", fixture_dir())).unwrap();
    let cdn = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/calculator.wsdl"))
        .respond_with(ResponseTemplate::new(200).set_body_string(body))
        .expect(1)
        .mount(&cdn)
        .await;
    let origin = MockServer::start().await;
    Mock::given(path("/calculator.wsdl"))
        .respond_with(ResponseTemplate::new(302).insert_header("Location", format!("{}/calculator.wsdl", cdn.uri())))
        .expect(1)
        .mount(&origin)
        .await;

    let entry = import_wsdl(&format!("{}/calculator.wsdl?token=s3cret", origin.uri())).await.unwrap();
    assert!(entry.document.is_some());
    let received = cdn.received_requests().await.unwrap();
    assert!(received.iter().all(|q| !q.headers.contains_key("referer")));
}
