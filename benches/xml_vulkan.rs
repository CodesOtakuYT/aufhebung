//! Compare the Vulkan command-name extraction in the XML demo with
//! quick-xml's streaming reader.
//!
//! The benchmark uses a checked-in Vulkan-shaped fixture by default. Set
//! `AUFHEBUNG_VULKAN_XML` to benchmark a complete `vk.xml` file:
//!
//! ```text
//! AUFHEBUNG_VULKAN_XML=/path/to/vk.xml cargo bench --bench xml_vulkan
//! ```
//!
//! Both parsers extract the same direct `<commands>/<command>` definitions and
//! consume the complete document. The flat pair is the apples-to-apples
//! comparison; the seven-byte pair shows the cost of the demo's deliberately
//! fragmented input on both parsers.

use std::env;
use std::fs;
use std::io::{self, BufRead, Read};
use std::path::PathBuf;

use aufhebung::{Event, Parser, Pieces, XmlError};
use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use quick_xml::Reader;
use quick_xml::events::Event as QuickEvent;

const FIXTURE: &[u8] = include_bytes!("fixtures/vulkan.xml");
const FIXTURE_COMMAND_NAMES: &[&[u8]] = &[
    b"vkCreateInstance",
    b"vkDestroyInstance",
    b"vkGetInstanceProcAddr",
    b"vkEnumerateInstanceExtensionProperties",
    b"vkNameAttributeWins",
];
const DEMO_CHUNK_SIZE: usize = 7;
const HASH_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const HASH_PRIME: u64 = 0x0000_0100_0000_01b3;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Stats {
    count: usize,
    checksum: u64,
}

impl Stats {
    fn add_hash(&mut self, hash: u64) {
        self.count += 1;
        self.checksum = self
            .checksum
            .rotate_left(7)
            .wrapping_add(hash)
            .wrapping_add(0x9e37_79b9_7f4a_7c15);
    }

    fn add_bytes(&mut self, bytes: &[u8]) {
        self.add_hash(update_hash(HASH_OFFSET, bytes));
    }

    fn add_pieces(&mut self, pieces: Pieces<'_, u8>) {
        if let Some(hash) = hash_pieces(pieces) {
            self.add_hash(hash);
        }
    }
}

fn update_hash(mut hash: u64, bytes: &[u8]) -> u64 {
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(HASH_PRIME);
    }
    hash
}

fn update_hash_pieces(mut hash: u64, pieces: Pieces<'_, u8>) -> u64 {
    for part in pieces {
        hash = update_hash(hash, part);
    }
    hash
}

fn hash_pieces(pieces: Pieces<'_, u8>) -> Option<u64> {
    if pieces.byte_len() == 0 {
        None
    } else {
        Some(update_hash_pieces(HASH_OFFSET, pieces))
    }
}

fn stats_for_names(names: &[&[u8]]) -> Stats {
    let mut stats = Stats::default();
    for name in names {
        stats.add_bytes(name);
    }
    stats
}

// Both parsers use this same streaming state so the timed work differs in
// parsing, not in whether collected command names are stored or printed.
#[derive(Default)]
struct Command {
    name_from_attribute: bool,
    in_name: bool,
    has_name: bool,
    checksum: u64,
}

fn parse_aufhebung<'a>(chunks: &'a [&'a [u8]]) -> Result<Stats, XmlError> {
    let mut parser = Parser::new(chunks);
    let mut elements: Vec<Pieces<'a, u8>> = Vec::new();
    let mut commands: Vec<Command> = Vec::new();
    let mut stats = Stats::default();

    while let Some(event) = parser.next_event()? {
        match event {
            Event::StartElement { name, attributes } => {
                let parent = elements.last().copied();

                if name == b"command" && parent.is_some_and(|parent| parent == b"commands") {
                    let mut command = Command::default();
                    if let Some(attribute) = attributes
                        .iter()
                        .find(|attribute| attribute.name() == b"name")
                    {
                        command.name_from_attribute = true;
                        if let Some(hash) = hash_pieces(attribute.value()) {
                            command.has_name = true;
                            command.checksum = hash;
                        }
                    }
                    commands.push(command);
                } else if parent.is_some_and(|parent| parent == b"proto")
                    && name == b"name"
                    && let Some(command) = commands.last_mut()
                    && !command.name_from_attribute
                {
                    command.in_name = true;
                    command.has_name = false;
                    command.checksum = HASH_OFFSET;
                }

                elements.push(name);
            }
            Event::Text(text) => {
                if let Some(command) = commands.last_mut()
                    && command.in_name
                    && !command.name_from_attribute
                    && text.byte_len() != 0
                {
                    command.has_name = true;
                    command.checksum = update_hash_pieces(command.checksum, text);
                }
            }
            Event::EndElement { name } => {
                if name == b"name"
                    && let Some(command) = commands.last_mut()
                {
                    command.in_name = false;
                } else if name == b"command"
                    && elements.len() >= 2
                    && elements[elements.len() - 2] == b"commands"
                    && let Some(command) = commands.pop()
                    && command.has_name
                {
                    stats.add_hash(command.checksum);
                }

                elements.pop();
            }
            Event::EmptyElement { name, attributes } => {
                if name == b"command"
                    && elements
                        .last()
                        .copied()
                        .is_some_and(|parent| parent == b"commands")
                    && let Some(attribute) = attributes
                        .iter()
                        .find(|attribute| attribute.name() == b"name")
                {
                    stats.add_pieces(attribute.value());
                }
            }
            Event::XmlDeclaration { .. }
            | Event::ProcessingInstruction { .. }
            | Event::CData(_)
            | Event::Comment(_)
            | Event::Doctype(_) => {}
        }
    }

    Ok(stats)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Scope {
    Commands,
    Command,
    Proto,
    Name,
    Other,
}

fn scan_quick_attributes(
    element: &quick_xml::events::BytesStart<'_>,
    mut command: Option<&mut Command>,
) -> Result<(), quick_xml::Error> {
    for attribute in element.attributes() {
        let attribute = attribute?;
        if let Some(command) = command.as_mut()
            && attribute.key.as_ref() == "name"
        {
            command.name_from_attribute = true;
            if !attribute.value.is_empty() {
                command.has_name = true;
                command.checksum = update_hash(HASH_OFFSET, attribute.value.as_bytes());
            }
        }
    }
    Ok(())
}

fn parse_quick_xml<R: BufRead>(reader: R) -> Result<Stats, quick_xml::Error> {
    let mut reader = Reader::from_reader(reader);
    let mut buffer = Vec::new();
    let mut scopes = Vec::new();
    let mut commands: Vec<Command> = Vec::new();
    let mut stats = Stats::default();

    loop {
        match reader.read_event_into(&mut buffer)? {
            QuickEvent::Start(element) => {
                let parent = scopes.last().copied();
                let element_name = element.name();

                let is_command =
                    element_name.as_ref() == "command" && parent == Some(Scope::Commands);
                let mut command = if is_command {
                    Some(Command::default())
                } else {
                    None
                };
                scan_quick_attributes(&element, command.as_mut())?;

                if let Some(command) = command {
                    commands.push(command);
                    scopes.push(Scope::Command);
                } else if element_name.as_ref() == "commands" {
                    scopes.push(Scope::Commands);
                } else if element_name.as_ref() == "proto" && parent == Some(Scope::Command) {
                    scopes.push(Scope::Proto);
                } else if element_name.as_ref() == "name" && parent == Some(Scope::Proto) {
                    if let Some(command) = commands.last_mut()
                        && !command.name_from_attribute
                    {
                        command.in_name = true;
                        command.has_name = false;
                        command.checksum = HASH_OFFSET;
                    }
                    scopes.push(Scope::Name);
                } else {
                    scopes.push(Scope::Other);
                }
            }
            QuickEvent::Text(text) => {
                if let Some(command) = commands.last_mut()
                    && command.in_name
                    && !command.name_from_attribute
                    && !text.as_ref().is_empty()
                {
                    command.has_name = true;
                    command.checksum = update_hash(command.checksum, text.as_ref().as_bytes());
                }
            }
            // Match aufhebung's raw event data instead of resolving the entity.
            QuickEvent::GeneralRef(reference) => {
                if let Some(command) = commands.last_mut()
                    && command.in_name
                    && !command.name_from_attribute
                {
                    command.has_name = true;
                    command.checksum = update_hash(command.checksum, b"&");
                    command.checksum = update_hash(command.checksum, reference.as_ref().as_bytes());
                    command.checksum = update_hash(command.checksum, b";");
                }
            }
            QuickEvent::End(_) => match scopes.pop() {
                Some(Scope::Name) => {
                    if let Some(command) = commands.last_mut() {
                        command.in_name = false;
                    }
                }
                Some(Scope::Command) => {
                    if let Some(command) = commands.pop()
                        && command.has_name
                    {
                        stats.add_hash(command.checksum);
                    }
                }
                Some(Scope::Commands | Scope::Proto | Scope::Other) | None => {}
            },
            QuickEvent::Empty(element) => {
                let is_command =
                    element.name().as_ref() == "command" && scopes.last() == Some(&Scope::Commands);
                for attribute in element.attributes() {
                    let attribute = attribute?;
                    if is_command && attribute.key.as_ref() == "name" && !attribute.value.is_empty()
                    {
                        stats.add_bytes(attribute.value.as_bytes());
                    }
                }
            }
            QuickEvent::Eof => break,
            _ => {}
        }
    }

    Ok(stats)
}

struct FragmentedReader<'a> {
    chunks: &'a [&'a [u8]],
    chunk_index: usize,
    offset: usize,
}

impl<'a> FragmentedReader<'a> {
    fn new(chunks: &'a [&'a [u8]]) -> Self {
        Self {
            chunks,
            chunk_index: 0,
            offset: 0,
        }
    }
}

impl Read for FragmentedReader<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let available = self.fill_buf()?;
        let amount = available.len().min(buffer.len());
        buffer[..amount].copy_from_slice(&available[..amount]);
        self.consume(amount);
        Ok(amount)
    }
}

impl BufRead for FragmentedReader<'_> {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        while self.chunk_index < self.chunks.len() {
            let chunk = self.chunks[self.chunk_index];
            if self.offset < chunk.len() {
                return Ok(&chunk[self.offset..]);
            }
            self.chunk_index += 1;
            self.offset = 0;
        }
        Ok(&[])
    }

    fn consume(&mut self, amount: usize) {
        self.offset += amount;
    }
}

fn load_input() -> (Vec<u8>, bool) {
    if let Some(path) = env::var_os("AUFHEBUNG_VULKAN_XML") {
        let path = PathBuf::from(path);
        let input = fs::read(&path).unwrap_or_else(|error| {
            panic!(
                "could not read Vulkan registry {}: {error}; set AUFHEBUNG_VULKAN_XML to a vk.xml path",
                path.display()
            )
        });
        (input, false)
    } else {
        (FIXTURE.to_vec(), true)
    }
}

fn bench_vulkan(c: &mut Criterion) {
    let (input, is_fixture) = load_input();
    let flat_chunks = [input.as_slice()];
    let fragmented_chunks: Vec<&[u8]> = input.chunks(DEMO_CHUNK_SIZE).collect();

    let expected = parse_aufhebung(&flat_chunks).unwrap();
    assert_eq!(expected, parse_aufhebung(&fragmented_chunks).unwrap());
    assert_eq!(expected, parse_quick_xml(&input[..]).unwrap());
    assert_eq!(
        expected,
        parse_quick_xml(FragmentedReader::new(&fragmented_chunks)).unwrap()
    );
    assert!(expected.count > 0);
    if is_fixture {
        assert_eq!(expected, stats_for_names(FIXTURE_COMMAND_NAMES));
    }

    let group_name = if is_fixture {
        "vulkan_xml/fixture"
    } else {
        "vulkan_xml/registry"
    };
    let mut group = c.benchmark_group(group_name);
    group.sample_size(10);
    group.throughput(Throughput::Bytes(input.len() as u64));

    group.bench_with_input(
        BenchmarkId::new("aufhebung", "flat"),
        &flat_chunks,
        |b, chunks| {
            b.iter(|| black_box(parse_aufhebung(chunks).unwrap()));
        },
    );
    group.bench_with_input(
        BenchmarkId::new("aufhebung", "7-byte-chunks"),
        &fragmented_chunks,
        |b, chunks| {
            b.iter(|| black_box(parse_aufhebung(chunks).unwrap()));
        },
    );
    group.bench_with_input(
        BenchmarkId::new("quick-xml", "flat"),
        &input[..],
        |b, input| {
            b.iter(|| black_box(parse_quick_xml(input).unwrap()));
        },
    );
    group.bench_with_input(
        BenchmarkId::new("quick-xml", "7-byte-chunks"),
        &fragmented_chunks,
        |b, chunks| {
            b.iter(|| {
                let reader = FragmentedReader::new(chunks);
                black_box(parse_quick_xml(reader).unwrap())
            });
        },
    );

    group.finish();
}

criterion_group!(benches, bench_vulkan);
criterion_main!(benches);
