//! Print every Vulkan command name from a Vulkan registry XML file.
//!
//! The registry is read in small chunks so that names, attributes, and tags
//! regularly cross chunk boundaries. The XML values stay borrowed `Pieces`
//! while the command names are printed.
//!
//! With the registry installed in the usual distribution location:
//!
//! ```text
//! cargo run --example vulkan
//! ```
//!
//! A different registry and chunk size can be supplied explicitly:
//!
//! ```text
//! cargo run --example vulkan -- path/to/vk.xml 7
//! ```

use std::env;
use std::error::Error;
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;

use aufhebung::{Event, Parser, Pieces};

const DEFAULT_REGISTRY: &str = "/usr/share/vulkan/registry/vk.xml";
const DEFAULT_CHUNK_SIZE: usize = 7;

#[derive(Default)]
struct Command<'a> {
    name: Vec<Pieces<'a, u8>>,
    name_from_attribute: bool,
    in_name: bool,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("vulkan demo: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let mut args = env::args_os();
    let _program = args.next();
    let path = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_REGISTRY));
    let chunk_size = match args.next() {
        Some(value) => value
            .to_str()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "chunk size must be UTF-8"))?
            .parse::<usize>()?,
        None => DEFAULT_CHUNK_SIZE,
    };
    if chunk_size == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "chunk size must be greater than zero",
        )
        .into());
    }

    let input = fs::read(&path)?;
    let chunks: Vec<&[u8]> = input.chunks(chunk_size).collect();
    let mut parser = Parser::new(&chunks);
    let mut elements: Vec<Pieces<'_, u8>> = Vec::new();
    let mut commands: Vec<Command<'_>> = Vec::new();
    let mut count = 0usize;

    println!("Vulkan functions from {}:", path.display());

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
                        command.name.push(attribute.value());
                        command.name_from_attribute = true;
                    }
                    commands.push(command);
                } else if parent.is_some_and(|parent| parent == b"proto")
                    && name == b"name"
                    && let Some(command) = commands.last_mut()
                    && !command.name_from_attribute
                {
                    command.name.clear();
                    command.in_name = true;
                }

                elements.push(name);
            }
            Event::Text(text) => {
                if let Some(command) = commands.last_mut()
                    && command.in_name
                    && !command.name_from_attribute
                {
                    command.name.push(text);
                }
            }
            Event::EndElement { name } => {
                if name == b"name" {
                    if let Some(command) = commands.last_mut() {
                        command.in_name = false;
                    }
                } else if name == b"command"
                    && elements.len() >= 2
                    && elements[elements.len() - 2] == b"commands"
                {
                    match commands.pop() {
                        Some(command) if !command.name.is_empty() => {
                            print_name(&command.name)?;
                            count += 1;
                        }
                        Some(_) | None => {}
                    }
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
                    let name = attribute.value();
                    print_name(std::slice::from_ref(&name))?;
                    count += 1;
                }
            }
            Event::XmlDeclaration { .. }
            | Event::ProcessingInstruction { .. }
            | Event::CData(_)
            | Event::Comment(_)
            | Event::Doctype(_) => {}
        }
    }

    println!("# {count} functions");
    Ok(())
}

fn print_name(name: &[Pieces<'_, u8>]) -> io::Result<()> {
    let stdout = io::stdout();
    let mut output = stdout.lock();
    for piece in name {
        write!(output, "{piece}")?;
    }
    writeln!(output)
}
