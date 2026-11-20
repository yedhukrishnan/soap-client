//! XML pretty-printing for display.

use quick_xml::events::Event;
use quick_xml::{Reader, Writer};

/// Re-indent XML with two spaces. Returns `None` if the input is not well-formed XML.
pub fn pretty_print(xml: &str) -> Option<String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut writer = Writer::new_with_indent(Vec::new(), b' ', 2);
    let mut depth: i64 = 0;
    let mut saw_root = false;
    loop {
        match reader.read_event() {
            Ok(Event::Eof) => break,
            Ok(e) => {
                match &e {
                    Event::Start(_) => {
                        depth += 1;
                        saw_root = true
                    }
                    Event::End(_) => depth -= 1,
                    Event::Empty(_) => saw_root = true,
                    _ => {}
                }
                writer.write_event(e).ok()?;
            }
            Err(_) => return None,
        }
    }
    if depth != 0 || !saw_root {
        return None;
    }
    String::from_utf8(writer.into_inner()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indents() {
        let out = pretty_print("<a><b>1</b><c/></a>").unwrap();
        assert_eq!(out, "<a>\n  <b>1</b>\n  <c/>\n</a>");
    }

    #[test]
    fn rejects_garbage() {
        assert!(pretty_print("not xml at all").is_none());
        assert!(pretty_print("<a><b></a>").is_none());
    }
}
