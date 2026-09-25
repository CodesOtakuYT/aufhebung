//! Integration coverage for the umbrella crate's XML re-exports.

use aufhebung::{Event, Parser, XmlError};

#[test]
fn umbrella_reexports_the_xml_pull_parser() {
    let chunks: &[&[u8]] = &[
        b"<?xml version='1.0'?>",
        b"<root value='raw &amp; data'><child/></root>",
    ];
    let mut parser = Parser::new(chunks);
    assert!(matches!(
        parser.next_event(),
        Ok(Some(Event::XmlDeclaration { .. }))
    ));
    assert!(matches!(
        parser.next_event(),
        Ok(Some(Event::StartElement { .. }))
    ));
    assert!(matches!(
        parser.next_event(),
        Ok(Some(Event::EmptyElement { .. }))
    ));
    assert!(matches!(
        parser.next_event(),
        Ok(Some(Event::EndElement { .. }))
    ));
    assert!(parser.finish().is_ok());
}

#[test]
fn umbrella_exposes_the_xml_error_without_shadowing_http_error() {
    let chunks: &[&[u8]] = &[b"<root>"];
    let mut parser = Parser::new(chunks);
    assert!(matches!(
        parser.next_event(),
        Ok(Some(Event::StartElement { .. }))
    ));
    assert!(matches!(parser.next_event(), Err(XmlError::Incomplete)));
}
