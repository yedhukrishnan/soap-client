use soap_core::wsdl::{SoapVersion, Style, parse_wsdl};

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

fn assert_well_formed(xml: &str) {
    roxmltree::Document::parse(xml).unwrap_or_else(|e| panic!("not well-formed: {e}\n{xml}"));
}

#[test]
fn calculator_doc_literal() {
    let doc = parse_wsdl(&fixture("calculator.wsdl")).unwrap();
    // The HTTP GET binding is skipped.
    let names: Vec<_> = doc.interfaces.iter().map(|i| i.name.as_str()).collect();
    assert_eq!(names, ["CalculatorSoap", "CalculatorSoap12"]);

    let soap11 = &doc.interfaces[0];
    assert_eq!(soap11.soap_version, SoapVersion::Soap11);
    assert_eq!(soap11.endpoints, ["http://www.dneonline.com/calculator.asmx"]);
    let add = &soap11.operations[0];
    assert_eq!(add.soap_action.as_deref(), Some("http://tempuri.org/Add"));
    assert_eq!(add.documentation.as_deref(), Some("Adds two integers."));
    assert_eq!(
        add.sample_request,
        r#"<soapenv:Envelope xmlns:soapenv="http://schemas.xmlsoap.org/soap/envelope/" xmlns:tns="http://tempuri.org/">
   <soapenv:Header/>
   <soapenv:Body>
      <tns:Add>
         <tns:intA>?</tns:intA>
         <tns:intB>?</tns:intB>
      </tns:Add>
   </soapenv:Body>
</soapenv:Envelope>"#
    );

    let soap12 = &doc.interfaces[1];
    assert_eq!(soap12.soap_version, SoapVersion::Soap12);
    assert!(soap12.operations[0].sample_request.starts_with(
        r#"<soap:Envelope xmlns:soap="http://www.w3.org/2003/05/soap-envelope""#
    ));
}

#[test]
fn rpc_literal_with_header() {
    let doc = parse_wsdl(&fixture("rpc.wsdl")).unwrap();
    let op = &doc.interfaces[0].operations[0];
    assert_eq!(op.style, Style::Rpc);
    let s = &op.sample_request;
    assert_well_formed(s);
    assert!(s.contains("xmlns:ns1=\"urn:examples:helloservice\""), "{s}");
    assert!(s.contains("<ns1:sayHello>"), "{s}");
    // rpc parts are unqualified accessors; Person's local elements are unqualified too.
    assert!(s.contains("<person>\n            <first>?</first>"), "{s}");
    assert!(s.contains("<greeting>?</greeting>"), "{s}");
    // soap:header part goes into the Header.
    assert!(s.contains("<soapenv:Header>\n      <token>?</token>\n   </soapenv:Header>"), "{s}");
}

#[test]
fn complex_schema_features() {
    let doc = parse_wsdl(&fixture("complex.wsdl")).unwrap();
    let iface = &doc.interfaces[0];
    assert_eq!(iface.soap_version, SoapVersion::Soap12);
    let s = &iface.operations[0].sample_request;
    assert_well_formed(s);
    // Global element is qualified; locals are unqualified (no elementFormDefault).
    assert!(s.contains("<ord:PlaceOrder>"));
    assert!(s.contains("<customer id=\"?\">"), "attribute from base type");
    assert!(s.contains("<name>?</name>"), "element from extension base");
    assert!(s.contains("<!--Optional:-->\n            <email>?</email>"));
    assert!(s.contains("<referrer/><!--recursive type omitted-->"));
    assert!(!s.contains("<!--Zero or more repetitions:-->"));
    assert!(s.contains("<!--1 or more repetitions:-->\n         <line>"));
    assert!(s.contains("<price currency=\"USD\">?</price>"), "simpleContent + default attr");
    assert!(s.contains("<!--You have a CHOICE of the next 2 items at this level-->"));
    assert!(s.contains("<!--Optional:-->\n         <ord:Note>?</ord:Note>"), "element ref is qualified");
    assert!(s.contains("<currency>USD</currency>"), "first enum value");
    assert!(s.contains("<!--You may enter ANY elements at this point-->"));
}

#[test]
fn rejects_non_wsdl() {
    assert!(parse_wsdl("<foo/>").is_err());
    assert!(parse_wsdl("not xml").is_err());
    assert!(parse_wsdl(r#"<description xmlns="http://www.w3.org/ns/wsdl"/>"#).is_err());
}
