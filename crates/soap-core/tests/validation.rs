use soap_core::validate::{Issue, Severity, Target, check_syntax, validate_request};
use soap_core::wsdl::{SoapVersion, WsdlDocument, parse_wsdl};

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

struct Case {
    wsdl: String,
    doc: WsdlDocument,
}

impl Case {
    fn new(name: &str) -> Self {
        let wsdl = fixture(name);
        let doc = parse_wsdl(&wsdl).unwrap();
        Case { wsdl, doc }
    }

    fn check_with(&self, body: &str, version: SoapVersion, action: Option<&str>) -> Vec<Issue> {
        let iface = &self.doc.interfaces[0];
        let op = &iface.operations[0];
        let t = Target {
            wsdl_text: &self.wsdl,
            imports: &[],
            binding_version: iface.soap_version,
            operation: op,
            soap_version: version,
            soap_action: action,
        };
        validate_request(body, Some(&t), &[])
    }

    /// Validate with the request's version and SOAPAction as the WSDL says.
    fn check(&self, body: &str) -> Vec<Issue> {
        let iface = &self.doc.interfaces[0];
        self.check_with(body, iface.soap_version, iface.operations[0].soap_action.as_deref())
    }
}

/// Messages, each with the body text it points at.
fn described(body: &str, issues: &[Issue]) -> Vec<String> {
    let units: Vec<u16> = body.encode_utf16().collect();
    issues
        .iter()
        .map(|i| match (i.start, i.end) {
            (Some(s), Some(e)) => format!("{} @ {}", i.message, String::from_utf16_lossy(&units[s as usize..e as usize])),
            _ => i.message.clone(),
        })
        .collect()
}

fn assert_issues(body: &str, issues: &[Issue], expected: &[&str]) {
    let got = described(body, issues);
    assert_eq!(got, expected, "\n{body}");
    assert!(issues.iter().all(|i| i.severity == Severity::Warning));
}

const SHOP_ENV: &str = r#"<soapenv:Envelope xmlns:soapenv="http://schemas.xmlsoap.org/soap/envelope/" xmlns:s="urn:shop"><soapenv:Body>BODY</soapenv:Body></soapenv:Envelope>"#;

fn shop(payload: &str) -> String {
    SHOP_ENV.replace("BODY", payload)
}

const VALID_ORDER: &str = r#"<s:PlaceOrder>
  <s:customer id="7"><s:name>Ann</s:name><s:email>a@example.com</s:email></s:customer>
  <s:line><s:sku>AB</s:sku><s:qty>2</s:qty><s:size>Medium</s:size><s:price currency="EU">9.95</s:price></s:line>
  <s:line><s:sku>CD</s:sku><s:qty>99</s:qty><s:size>auto</s:size><s:price>1</s:price></s:line>
  <s:address>Main St 1</s:address>
  <s:voucher>X1</s:voucher>
  <s:options><s:note>hi</s:note><s:giftWrap>true</s:giftWrap></s:options>
  <s:tags>1 2  3</s:tags>
  <s:due xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:nil="true"/>
  <s:channel>web</s:channel>
  <x:extra xmlns:x="urn:other"><anything/></x:extra>
</s:PlaceOrder>"#;

#[test]
fn valid_request_has_no_issues() {
    let c = Case::new("validation.wsdl");
    let body = shop(VALID_ORDER);
    assert_issues(&body, &c.check(&body), &[]);
}

#[test]
fn syntax_errors_are_errors_with_a_position() {
    let body = "<a>\n  <b></a>";
    let issue = check_syntax(body).unwrap();
    assert_eq!(issue.severity, Severity::Error);
    assert!(issue.message.starts_with("Not well-formed XML"), "{}", issue.message);
    let (s, e) = (issue.start.unwrap() as usize, issue.end.unwrap() as usize);
    assert_eq!(&body[s..e], "<", "{}", issue.message);
    assert!(check_syntax("<a>{{x}}</a>").is_none());

    // validate_request reports the same, with or without a WSDL.
    let c = Case::new("validation.wsdl");
    assert_eq!(c.check(body), vec![issue.clone()]);
    assert_eq!(validate_request(body, None, &[]), vec![issue]);
    assert!(validate_request("<a/>", None, &[]).is_empty());
}

#[test]
fn broken_placeholders_are_reported_without_a_wsdl() {
    let body = "<a><p>{{password}</p><q>{{ok}}</q></a>";
    assert_issues(
        body,
        &validate_request(body, None, &[]),
        &[r#""{{password}" is not a valid variable placeholder and would be sent as typed; use {{name}} @ {{password}"#],
    );

    // Single braces only count when they spell a defined variable.
    let body = "<a><p>{password}</p><q>{id</q><r>{3F25-AB}</r><s>{{password}</s></a>";
    let names = ["password".to_string(), "id".to_string()];
    assert_issues(
        body,
        &validate_request(body, None, &names),
        &[
            r#""{password}" would be sent as typed: did you mean {{password}}? @ {password}"#,
            r#""{id" would be sent as typed: did you mean {{id}}? @ {id"#,
            r#""{{password}" is not a valid variable placeholder and would be sent as typed; use {{name}} @ {{password}"#,
        ],
    );
}

#[test]
fn values_are_checked_against_simple_types() {
    let c = Case::new("validation.wsdl");
    let payload = VALID_ORDER
        .replace(r#"id="7""#, r#"id="seven""#)
        .replace("<s:sku>AB</s:sku>", "<s:sku>ABC</s:sku>")
        .replace("<s:qty>99</s:qty>", "<s:qty>100</s:qty>")
        .replace("<s:size>Medium</s:size>", "<s:size>medium</s:size>")
        .replace("<s:size>auto</s:size>", "<s:size>huge</s:size>")
        .replace(">9.95<", ">9.955<")
        .replace(r#"currency="EU""#, r#"currency="E""#)
        .replace("<s:giftWrap>true</s:giftWrap>", "<s:giftWrap>yes</s:giftWrap>")
        .replace("1 2  3", "1 two")
        .replace("<s:channel>web</s:channel>", "<s:channel> web </s:channel>");
    let body = shop(&payload);
    assert_issues(
        &body,
        &c.check(&body),
        &[
            r#"Attribute "id": "seven" is not a valid xs:int (whole number expected) @ seven"#,
            r#"<sku>: "ABC" is too long: 3 characters, at most 2 allowed @ ABC"#,
            r#"<size>: "medium" does not match any of the allowed types @ medium"#,
            r#"Attribute "currency": "E" is too short: 1 character, at least 2 required @ E"#,
            r#"<price>: "9.955" has too many decimal places (at most 2) @ 9.955"#,
            r#"<qty>: "100" must be at most 99 @ 100"#,
            r#"<size>: "huge" does not match any of the allowed types @ huge"#,
            r#"<giftWrap>: "yes" is not a valid xs:boolean @ yes"#,
            r#"<tags>: "two" is not a valid xs:int (whole number expected) @ 1 two"#,
            r#"<channel> must be "web" (fixed by the schema) @ web"#,
        ],
    );
}

#[test]
fn enumerations_suggest_the_closest_value() {
    let c = Case::new("complex.wsdl");
    let body = r#"<soap:Envelope xmlns:soap="http://www.w3.org/2003/05/soap-envelope" xmlns:o="http://example.com/orders"><soap:Body><o:PlaceOrder>
<customer id="1"><name>A</name></customer><line><sku>s</sku><price currency="usd">1</price></line><pickup>true</pickup><currency>GBP</currency>
</o:PlaceOrder></soap:Body></soap:Envelope>"#;
    assert_issues(
        body,
        &c.check(body),
        &[
            r#"Attribute "currency": "usd" is not an allowed value. Did you mean "USD"? @ usd"#,
            r#"<currency>: "GBP" is not an allowed value. Allowed: "USD", "EUR" @ GBP"#,
        ],
    );
}

#[test]
fn structure_problems() {
    let c = Case::new("validation.wsdl");
    let payload = r#"<s:PlaceOrder>
  <s:customer><s:email>a@b</s:email><s:name>Ann</s:name><s:age>3</s:age></s:customer>
  <s:line><s:qty>1</s:qty><s:price>1</s:price></s:line>
  <s:line><s:sku>AA</s:sku><s:qty>1</s:qty><s:price>1</s:price></s:line>
  <s:line><s:sku>AA</s:sku><s:qty>1</s:qty><s:price>1</s:price></s:line>
  <s:line><s:sku>AA</s:sku><s:qty>1</s:qty><s:price>1</s:price></s:line>
  <s:pickup>true</s:pickup>
  <s:payment>X</s:payment>
  <s:options><s:note>n</s:note></s:options>
  <due>2024-01-01</due>
  <s:extra/>
</s:PlaceOrder>"#;
    let body = shop(payload);
    assert_issues(
        &body,
        &c.check(&body),
        &[
            "<line> is not allowed here: it appears too many times or in the wrong position @ <s:line",
            "<payment> is abstract: use one of <card>, <voucher> instead @ <s:payment",
            "<due> is in the wrong namespace: expected \"urn:shop\", found no namespace @ <due",
            "<extra> is not allowed in <PlaceOrder> @ <s:extra",
            "<customer> is missing required attribute \"id\" @ <s:customer",
            "<name> is in the wrong position: it must come before <email> @ <s:name",
            "<age> is not allowed in <customer> @ <s:age",
            "Missing required element <sku> before <qty> @ <s:qty",
            "<options> is missing required element <giftWrap> @ <s:options",
        ],
    );
}

#[test]
fn missing_elements_choice_and_nil() {
    let c = Case::new("validation.wsdl");
    let payload = r#"<s:PlaceOrder xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
  <s:customer id="1" vip="yes"><s:name xsi:nil="true"/></s:customer>
  <s:line><s:sku>AA</s:sku><s:qty>1</s:qty><s:price>1</s:price></s:line>
  <s:card>c</s:card>
</s:PlaceOrder>"#;
    let body = shop(payload);
    assert_issues(
        &body,
        &c.check(&body),
        &[
            "Expected one of <pickup>, <address> before <card> @ <s:card",
            "<PlaceOrder> is missing required element <due> @ <s:PlaceOrder",
            "<customer> has no attribute \"vip\" @ vip",
            "<name> cannot be nil: the schema does not mark it nillable @ <s:name",
        ],
    );
}

#[test]
fn templates_and_placeholders() {
    let c = Case::new("validation.wsdl");
    // {{variables}} are not checked (their value is unknown until sending); `?` is flagged.
    let payload = VALID_ORDER
        .replace(r#"id="7""#, r#"id="{{customerId}}""#)
        .replace("<s:qty>2</s:qty>", "<s:qty>{{ qty }}</s:qty>")
        .replace("<s:sku>CD</s:sku>", "<s:sku>?</s:sku>");
    let body = shop(&payload);
    assert_issues(&body, &c.check(&body), &[r#"<sku> still has the placeholder value "?" @ ?"#]);
}

#[test]
fn envelope_version_action_and_body() {
    let c = Case::new("validation.wsdl");
    let soap12 = r#"<e:Envelope xmlns:e="http://www.w3.org/2003/05/soap-envelope"><e:Body><s:Other xmlns:s="urn:shop"/></e:Body></e:Envelope>"#;
    assert_issues(
        soap12,
        &c.check_with(soap12, SoapVersion::Soap11, Some("urn:wrong")),
        &[
            r#"SOAPAction is "urn:wrong", but the WSDL specifies "urn:shop:PlaceOrder" for PlaceOrder (Headers tab)"#,
            "This is a SOAP 1.2 envelope, but the request is set to send SOAP 1.1 (version selector next to the URL) @ <e:Envelope",
            "Expected <PlaceOrder> (operation PlaceOrder), found <Other> @ <s:Other",
        ],
    );
    let issues = c.check_with(soap12, SoapVersion::Soap12, Some("{{action}}"));
    assert!(described(soap12, &issues)[0].starts_with("The WSDL binding uses SOAP 1.1, but this is a SOAP 1.2 envelope"));

    let not_env = "<PlaceOrder/>";
    assert_issues(not_env, &c.check(not_env), &["The root element should be a SOAP Envelope, not <PlaceOrder> @ <PlaceOrder"]);

    let no_body = r#"<soapenv:Envelope xmlns:soapenv="http://schemas.xmlsoap.org/soap/envelope/"><soapenv:Header/></soapenv:Envelope>"#;
    assert_issues(no_body, &c.check(no_body), &["The envelope has no Body @ <soapenv:Envelope"]);

    let empty = shop("");
    assert_issues(&empty, &c.check(&empty), &["<Body> is missing <PlaceOrder> @ <soapenv:Body"]);

    let extra = shop(&format!("{VALID_ORDER}<s:PlaceOrder/>"));
    assert_issues(&extra, &c.check(&extra), &["<PlaceOrder> is not part of operation PlaceOrder @ <s:PlaceOrder"]);
}

#[test]
fn generated_samples_only_report_placeholders() {
    // (Samples of schemas with a choice list every alternative, so they are not valid.)
    for name in ["calculator.wsdl", "rpc.wsdl"] {
        let c = Case::new(name);
        for iface in &c.doc.interfaces {
            for op in &iface.operations {
                let t = Target {
                    wsdl_text: &c.wsdl,
                    imports: &[],
                    binding_version: iface.soap_version,
                    operation: op,
                    soap_version: iface.soap_version,
                    soap_action: op.soap_action.as_deref(),
                };
                let body = &op.sample_request;
                for i in validate_request(body, Some(&t), &[]) {
                    assert!(i.message.contains("placeholder"), "{name} {}: {}\n{body}", op.name, i.message);
                }
            }
        }
    }
}

#[test]
fn rpc_parts_are_checked() {
    let c = Case::new("rpc.wsdl");
    let env = |inner: &str| {
        format!(
            r#"<soapenv:Envelope xmlns:soapenv="http://schemas.xmlsoap.org/soap/envelope/" xmlns:h="urn:examples:helloservice"><soapenv:Body>{inner}</soapenv:Body></soapenv:Envelope>"#
        )
    };
    let ok = env("<h:sayHello><person><first>A</first><last>B</last></person><greeting>Hi</greeting></h:sayHello>");
    assert_issues(&ok, &c.check(&ok), &[]);

    let bad = env("<h:sayHello><person><first>A</first></person><greet>Hi</greet></h:sayHello>");
    assert_issues(
        &bad,
        &c.check(&bad),
        &["Expected <greeting> (part of operation sayHello), found <greet> @ <greet", "<person> is missing required element <last> @ <person"],
    );

    let wrong = env("<h:hello/>");
    assert_issues(&wrong, &c.check(&wrong), &["Expected <sayHello> (operation sayHello), found <hello> @ <h:hello"]);
}

#[test]
fn positions_are_utf16_offsets() {
    let c = Case::new("validation.wsdl");
    let payload = VALID_ORDER.replace("<s:name>Ann</s:name>", "<s:name>😀 Ann</s:name>").replace("<s:qty>2</s:qty>", "<s:qty>x</s:qty>");
    let body = shop(&payload);
    assert_issues(&body, &c.check(&body), &[r#"<qty>: "x" is not a valid xs:int (whole number expected) @ x"#]);
}

#[test]
fn unsupported_constructs_are_accepted() {
    // A type that cannot be resolved accepts anything inside it.
    let wsdl = fixture("validation.wsdl").replace(r#"<xs:element name="customer" type="Customer"/>"#, r#"<xs:element name="customer" type="s:Missing"/>"#);
    let doc = parse_wsdl(&wsdl).unwrap();
    let c = Case { wsdl, doc };
    let body = shop(&VALID_ORDER.replace("<s:name>Ann</s:name>", "<whatever/>"));
    assert_issues(&body, &c.check(&body), &[]);
}
