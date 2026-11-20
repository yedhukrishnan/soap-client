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
