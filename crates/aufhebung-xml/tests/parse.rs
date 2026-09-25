//! Integration tests for the XML pull parser.

use aufhebung_xml::{Error, Event, Parser};

fn collect<'a>(chunks: &'a [&'a [u8]]) -> Result<Vec<Event<'a>>, Error> {
    let mut parser = Parser::new(chunks);
    let mut events = Vec::new();
    while let Some(event) = parser.next_event()? {
        events.push(event);
    }
    Ok(events)
}

#[test]
fn parses_elements_attributes_and_raw_entities() {
    let wire: &[u8] = br#"<?xml version = '1.0' encoding = 'UTF-8'?>
<root answer = "x&amp;y">
  <child>one &lt; two &gt; zero</child>
  <empty />
</root>
"#;
    let chunks = [wire];
    let events = collect(&chunks).unwrap();

    assert!(matches!(events[0], Event::XmlDeclaration { .. }));
    assert!(matches!(events[1], Event::Text(_)));
    match &events[2] {
        Event::StartElement { name, attributes } => {
            assert!(*name == b"root");
            assert_eq!(attributes.len(), 1);
            assert!(attributes[0].name() == b"answer");
            assert!(attributes[0].value() == b"x&amp;y");
        }
        _ => panic!("expected root start"),
    }

    let text = events
        .iter()
        .find_map(|event| match event {
            Event::Text(text) if text.contains_any(b"one") => Some(*text),
            _ => None,
        })
        .unwrap();
    assert!(text == b"one &lt; two &gt; zero");

    assert!(
        events.iter().any(|event| {
            matches!(event, Event::EmptyElement { name, .. } if *name == b"empty")
        })
    );
}

#[test]
fn every_split_point_preserves_the_event_stream() {
    let wire: &[u8] = br#"<?xml version="1.0"?>
<protocol name="demo">
  <interface name="thing">
    <request name="go"><arg name="id" type="uint"/></request>
  </interface>
</protocol>
"#;
    let flat = [wire];
    let baseline = collect(&flat).unwrap();

    for cut in 0..=wire.len() {
        let chunks = [&wire[..cut], &wire[cut..]];
        let events = collect(&chunks).unwrap();
        assert!(events == baseline, "split at byte {cut} diverged");
    }

    let chunks: Vec<&[u8]> = wire.chunks(1).collect();
    assert!(collect(&chunks).unwrap() == baseline);
}

#[test]
fn comments_cdata_processing_instructions_and_doctype_are_events() {
    let wire: &[u8] = concat!(
        "\u{feff}<!DOCTYPE root [ <!ELEMENT root ANY> ]>\n",
        "<?target data?>\n",
        "<!-- comment -->\n",
        "<root><![CDATA[a < b & c]]></root>",
    )
    .as_bytes();
    let chunks = [wire];
    let events = collect(&chunks).unwrap();

    assert!(matches!(events[0], Event::Doctype(_)));
    assert!(matches!(events[1], Event::Text(_)));
    assert!(matches!(events[2], Event::ProcessingInstruction { .. }));
    assert!(matches!(events[3], Event::Text(_)));
    assert!(matches!(events[4], Event::Comment(_)));
    assert!(matches!(events[5], Event::Text(_)));
    assert!(matches!(events[6], Event::StartElement { .. }));
    assert!(matches!(events[7], Event::CData(_)));
    assert!(matches!(events[8], Event::EndElement { .. }));

    match &events[0] {
        Event::Doctype(contents) => {
            assert!(*contents == b"root [ <!ELEMENT root ANY> ]");
        }
        _ => panic!("expected doctype"),
    }
    match &events[2] {
        Event::ProcessingInstruction { target, data } => {
            assert!(*target == b"target");
            assert!(*data == b"data");
        }
        _ => panic!("expected processing instruction"),
    }
    match &events[4] {
        Event::Comment(contents) => assert!(*contents == b" comment "),
        _ => panic!("expected comment"),
    }
    match &events[7] {
        Event::CData(contents) => assert!(*contents == b"a < b & c"),
        _ => panic!("expected CDATA"),
    }
}

#[test]
fn every_split_point_handles_markup_delimiters() {
    let wire: &[u8] = concat!(
        "\u{feff}<!DOCTYPE root [ <!ELEMENT root ANY> ]>\n",
        "<?target data?>\n",
        "<!-- comment -->\n",
        "<root><![CDATA[a < b & c]]></root>\n",
    )
    .as_bytes();
    let flat = [wire];
    let baseline = collect(&flat).unwrap();

    for cut in 0..=wire.len() {
        let chunks = [&wire[..cut], &wire[cut..]];
        assert!(collect(&chunks).unwrap() == baseline, "split at {cut}");
    }
    let chunks: Vec<&[u8]> = wire.chunks(1).collect();
    assert!(collect(&chunks).unwrap() == baseline);
}

#[test]
fn processing_instruction_data_is_borrowed_and_may_be_empty() {
    let wire: &[u8] = b"<?one?><?two ?x?><root/>";
    let chunks = [wire];
    let events = collect(&chunks).unwrap();
    assert!(matches!(
        &events[0],
        Event::ProcessingInstruction { target, data }
            if *target == b"one" && *data == b""
    ));
    assert!(matches!(
        &events[1],
        Event::ProcessingInstruction { target, data }
            if *target == b"two" && *data == b"?x"
    ));
}

#[test]
fn namespace_prefixes_remain_literal() {
    let wire: &[u8] = b"<p:root xmlns:p='urn:example'><p:child/></p:root>";
    let chunks = [wire];
    let events = collect(&chunks).unwrap();
    assert!(events.iter().any(|event| {
        matches!(event, Event::StartElement { name, attributes }
            if *name == b"p:root"
                && attributes.iter().any(|attribute| attribute.name() == b"xmlns:p"))
    }));
    assert!(
        events.iter().any(|event| {
            matches!(event, Event::EmptyElement { name, .. } if *name == b"p:child")
        })
    );
}

#[test]
fn utf8_text_can_straddle_chunks_without_decoding() {
    let wire: &[u8] = b"<r>\xc3\xa9\xe2\x98\x83</r>";
    let flat = [wire];
    let events = collect(&flat).unwrap();
    assert!(
        events.iter().any(|event| {
            matches!(event, Event::Text(text) if *text == b"\xc3\xa9\xe2\x98\x83")
        })
    );

    for cut in 0..=wire.len() {
        let chunks = [&wire[..cut], &wire[cut..]];
        assert!(collect(&chunks).unwrap() == events, "split at {cut}");
    }
}

#[test]
fn parses_wayland_shaped_xml() {
    let wire: &[u8] = br#"<?xml version="1.0" encoding="UTF-8"?>
<protocol name="demo">
  <interface name="demo_manager" version="1">
    <description summary="demo interface">A small description.</description>
    <request name="bind">
      <arg name="name" type="string" summary="object name"/>
    </request>
  </interface>
</protocol>
"#;
    let chunks = [wire];
    let events = collect(&chunks).unwrap();
    assert!(events.iter().any(|event| {
        matches!(event, Event::EmptyElement { name, attributes }
            if *name == b"arg"
                && attributes.iter().any(|attribute| attribute.value() == b"string"))
    }));
}

#[test]
fn parses_vulkan_shaped_nested_text_and_entities() {
    let wire: &[u8] = br#"<?xml version = '1.0' encoding = 'UTF-8'?>
<registry>
  <types>
    <type category="define">#define &lt;name&gt; &amp; value</type>
    <type category="include" name="X11/Xlib.h" />
  </types>
  <commands>
    <param len="p-&gt;count"><type>uint32_t</type>* <name>pCount</name></param>
  </commands>
</registry>
"#;
    let chunks = [wire];
    let events = collect(&chunks).unwrap();
    assert!(events.iter().any(|event| {
        matches!(event, Event::Text(text) if *text == b"#define &lt;name&gt; &amp; value")
    }));
    assert!(events.iter().any(|event| {
        matches!(event, Event::EmptyElement { name, attributes }
            if *name == b"type"
                && attributes.iter().any(|attribute| attribute.value() == b"X11/Xlib.h"))
    }));
}

#[test]
fn parses_texture_atlas_shaped_xml() {
    let wire: &[u8] = br#"<TextureAtlas imagePath="atlas.png" width="256" height="128">
  <SubTexture name="hero" x="0" y="0" width="32" height="64"/>
  <SubTexture name="tree&amp;rock" x="32" y="0" width="48" height="64" rotated="true"/>
</TextureAtlas>"#;
    let chunks = [wire];
    let events = collect(&chunks).unwrap();
    let subtextures: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            Event::EmptyElement { name, attributes } if *name == b"SubTexture" => Some(attributes),
            _ => None,
        })
        .collect();
    assert_eq!(subtextures.len(), 2);
    assert!(
        subtextures[1].iter().any(|attribute| {
            attribute.name() == b"name" && attribute.value() == b"tree&amp;rock"
        })
    );
}

#[test]
fn reports_incomplete_without_consuming_the_event() {
    let chunks: [&[u8]; 2] = [b"<root", b" child"];
    let mut parser = Parser::new(&chunks);
    assert!(matches!(parser.next_event(), Err(Error::Incomplete)));
    assert!(matches!(parser.next_event(), Err(Error::Incomplete)));
    assert!(!parser.is_finished());

    for wire in [
        &b"<root>"[..],
        &b"<root a='unterminated/>"[..],
        &b"<!-- unterminated"[..],
        &b"<root><![CDATA[unterminated</root>"[..],
    ] {
        let chunks = [wire];
        assert!(matches!(collect(&chunks), Err(Error::Incomplete)));
    }
}

#[test]
fn reports_structural_errors() {
    let cases: &[&[u8]] = &[
        b"<root></wrong>",
        b"<root a='1' a='2'/>",
        b"text<root/>",
        b"<root/>text",
        b"<root/><other/>",
        b"<root></root><other/>",
        b"<root a=1/>",
        b"<1root/>",
        b"<1",
        b"<root a='1'b='2'/>",
        b"<root a='x<'",
        b"</root>",
        b"<!-- bad -- comment --><root/>",
        b"<!DOCTYPE root><!DOCTYPE root><root/>",
        b"<!DOCTYPEX><root/>",
        b"<?target?x?><root/>",
        b"<?xml version='1.0'?><?xml version='1.0'?><root/>",
    ];

    for wire in cases {
        let chunks = [*wire];
        assert!(
            matches!(collect(&chunks), Err(Error::Malformed)),
            "{wire:?} should be malformed"
        );
    }
}

#[test]
fn rejects_duplicate_attributes_in_declarations_and_elements() {
    let declaration: &[u8] = b"<?xml version='1.0' version='1.1'?><root/>";
    let element: &[u8] = b"<root a='1' a='2'/>";
    let declaration_chunks = [declaration];
    let element_chunks = [element];
    assert!(matches!(
        collect(&declaration_chunks),
        Err(Error::Malformed)
    ));
    assert!(matches!(collect(&element_chunks), Err(Error::Malformed)));
}
