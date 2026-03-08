use std::io::{self, BufWriter, Read, Write};
use std::process;

use byo::emitter::{Emitter, write_props};
use byo::lexer::{Token, tokenize};
use byo::parser::parse;
use byo::protocol::{APC_START, Command, PROTOCOL_ID, PragmaKind, Prop, ST};
use byo::scanner::{Handler, Scanner};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        usage();
        process::exit(1);
    }
    let result = match args[1].as_str() {
        "emit" => cmd_emit(&args[2..]),
        "parse" => cmd_parse(&args[2..]),
        "prop" => cmd_prop(&args[2..]),
        "props" => cmd_props(&args[2..]),
        "children" => cmd_children(&args[2..]),
        "-h" | "--help" | "help" => {
            usage();
            Ok(())
        }
        other => {
            eprintln!("byo: unknown command '{other}'");
            usage();
            process::exit(1);
        }
    };
    if let Err(e) = result {
        eprintln!("byo: {e}");
        process::exit(1);
    }
}

fn usage() {
    eprint!(
        "\
Usage: byo <command> [flags]

Commands:
  emit       Validate and wrap BYO syntax in APC framing
  parse      Parse APC stream into tab-delimited records
  prop       Extract a single property value from a props blob
  props      List all properties from a props blob
  children   Parse a body blob into tab-delimited records

Flags:
  emit --raw          Skip APC framing, output bare validated syntax
  emit --no-validate  Skip validation, just wrap in APC framing
  parse --raw         Input is bare BYO syntax (no APC framing)
  parse --json        Output JSON lines instead of tab-delimited
  children --json     Output JSON lines
"
    );
}

// -- emit ---------------------------------------------------------------------

fn cmd_emit(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let mut raw = false;
    let mut no_validate = false;
    for arg in args {
        match arg.as_str() {
            "--raw" => raw = true,
            "--no-validate" => no_validate = true,
            other => return Err(format!("emit: unknown flag '{other}'").into()),
        }
    }
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    if !no_validate {
        parse(&input)?;
    }
    let stdout = io::stdout();
    let mut out = stdout.lock();
    if raw {
        out.write_all(input.as_bytes())?;
    } else {
        out.write_all(APC_START)?;
        out.write_all(&[PROTOCOL_ID])?;
        out.write_all(input.as_bytes())?;
        out.write_all(ST)?;
    }
    out.flush()?;
    Ok(())
}

// -- parse --------------------------------------------------------------------

struct RecordWriter<'a> {
    out: &'a mut dyn Write,
    json: bool,
    empty: &'a str,
    err: Option<io::Error>,
}

impl Handler for RecordWriter<'_> {
    fn on_byo(&mut self, payload: &[u8]) {
        if self.err.is_some() {
            return;
        }
        let s = match std::str::from_utf8(payload) {
            Ok(s) => s,
            Err(e) => {
                self.err = Some(io::Error::new(io::ErrorKind::InvalidData, e));
                return;
            }
        };
        let cmds = match parse(s) {
            Ok(c) => c,
            Err(e) => {
                self.err = Some(io::Error::new(io::ErrorKind::InvalidData, e));
                return;
            }
        };
        if let Err(e) = write_records(&cmds, self.out, self.json, self.empty) {
            self.err = Some(e);
        }
    }
}

fn cmd_parse(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let mut raw = false;
    let mut json = false;
    let mut empty: Option<String> = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--raw" => raw = true,
            "--json" => json = true,
            "--empty" => {
                empty = Some(
                    iter.next()
                        .ok_or("parse: --empty requires a value")?
                        .clone(),
                );
            }
            other if other.starts_with("--empty=") => {
                empty = Some(other["--empty=".len()..].to_string());
            }
            other => return Err(format!("parse: unknown flag '{other}'").into()),
        }
    }
    // Resolve: --empty flag > BYO_EMPTY env > default "_"
    let empty =
        empty.unwrap_or_else(|| std::env::var("BYO_EMPTY").unwrap_or_else(|_| "_".to_string()));
    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    if raw {
        let mut input = String::new();
        io::stdin().read_to_string(&mut input)?;
        let cmds = parse(&input)?;
        write_records(&cmds, &mut out, json, &empty)?;
    } else {
        let stdin = io::stdin();
        let mut reader = stdin.lock();
        let mut scanner = Scanner::new();
        let mut buf = [0u8; 8192];
        loop {
            let n = reader.read(&mut buf)?;
            if n == 0 {
                break;
            }
            let err = {
                let mut handler = RecordWriter {
                    out: &mut out,
                    json,
                    empty: &empty,
                    err: None,
                };
                scanner.feed(&buf[..n], &mut handler);
                handler.err.take()
            };
            if let Some(e) = err {
                return Err(e.into());
            }
            out.flush()?;
        }
    }
    out.flush()?;
    Ok(())
}

// -- prop ---------------------------------------------------------------------

fn cmd_prop(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let name = args.first().ok_or("prop: missing property name")?.as_str();
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    let input = input.trim();
    if input.is_empty() {
        process::exit(1);
    }
    let tokens = tokenize(input)?;
    let mut i = 0;
    while i < tokens.len() {
        match &tokens[i].token {
            Token::Tilde => {
                // ~key (remove) — skip
                i += if i + 1 < tokens.len() { 2 } else { 1 };
            }
            Token::Word(word) => {
                if i + 1 < tokens.len() && matches!(tokens[i + 1].token, Token::Eq) {
                    // key=value
                    let is_match = *word == name;
                    i += 2; // skip key and =
                    if i < tokens.len() {
                        if is_match {
                            match &tokens[i].token {
                                Token::Word(v) => {
                                    println!("{v}");
                                    return Ok(());
                                }
                                Token::Str(v) => {
                                    println!("{v}");
                                    return Ok(());
                                }
                                _ => {}
                            }
                        }
                        i += 1; // skip value
                    }
                } else {
                    // Boolean flag
                    if *word == name {
                        return Ok(());
                    }
                    i += 1;
                }
            }
            _ => {
                i += 1;
            }
        }
    }
    process::exit(1);
}

// -- props --------------------------------------------------------------------

fn cmd_props(_args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    let input = input.trim();
    if input.is_empty() {
        return Ok(());
    }
    let tokens = tokenize(input)?;
    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    let mut i = 0;
    while i < tokens.len() {
        match &tokens[i].token {
            Token::Tilde => {
                // ~key (remove)
                if i + 1 < tokens.len() {
                    if let Token::Word(key) = &tokens[i + 1].token {
                        writeln!(out, "{key}\t\tremove")?;
                    }
                    i += 2;
                } else {
                    i += 1;
                }
            }
            Token::Word(word) => {
                if i + 1 < tokens.len() && matches!(tokens[i + 1].token, Token::Eq) {
                    // key=value
                    i += 2; // skip key and =
                    if i < tokens.len() {
                        let value = match &tokens[i].token {
                            Token::Word(v) => *v,
                            Token::Str(v) => v.as_ref(),
                            _ => {
                                i += 1;
                                continue;
                            }
                        };
                        writeln!(out, "{word}\t{value}\tvalue")?;
                        i += 1;
                    }
                } else {
                    // Boolean flag
                    writeln!(out, "{word}\t\tflag")?;
                    i += 1;
                }
            }
            _ => {
                i += 1;
            }
        }
    }
    out.flush()?;
    Ok(())
}

// -- children -----------------------------------------------------------------

fn cmd_children(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let mut json = false;
    let mut empty: Option<String> = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--json" => json = true,
            "--empty" => {
                empty = Some(
                    iter.next()
                        .ok_or("children: --empty requires a value")?
                        .clone(),
                );
            }
            other if other.starts_with("--empty=") => {
                empty = Some(other["--empty=".len()..].to_string());
            }
            other => return Err(format!("children: unknown flag '{other}'").into()),
        }
    }
    let empty =
        empty.unwrap_or_else(|| std::env::var("BYO_EMPTY").unwrap_or_else(|_| "_".to_string()));
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    let cmds = parse(&input)?;
    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    write_records(&cmds, &mut out, json, &empty)?;
    out.flush()?;
    Ok(())
}

// -- record formatting --------------------------------------------------------

fn write_records(
    cmds: &[Command],
    out: &mut (impl Write + ?Sized),
    json: bool,
    empty: &str,
) -> io::Result<()> {
    let mut i = 0;
    while i < cmds.len() {
        if matches!(cmds[i], Command::Push { .. } | Command::Pop) {
            i += 1;
            continue;
        }
        let (body, next) = collect_children(cmds, i);
        if json {
            write_json_obj(&cmds[i], body, out)?;
            out.write_all(b"\n")?;
        } else {
            write_tsv_record(&cmds[i], body, empty, out)?;
        }
        i = next;
    }
    Ok(())
}

/// For a command at index `i`, check if Push/Pop children follow.
/// Returns (children slice or None, next index to process).
fn collect_children(cmds: &[Command], i: usize) -> (Option<&[Command]>, usize) {
    let next = i + 1;
    if matches!(cmds[i], Command::Upsert { .. } | Command::Patch { .. })
        && next < cmds.len()
        && matches!(cmds[next], Command::Push { .. })
    {
        let start = next + 1;
        let mut depth = 1usize;
        let mut j = start;
        while j < cmds.len() {
            match cmds[j] {
                Command::Push { .. } => depth += 1,
                Command::Pop => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
            j += 1;
        }
        (Some(&cmds[start..j]), j + 1)
    } else {
        (None, next)
    }
}

// -- TSV output ---------------------------------------------------------------

fn write_tsv_record(
    cmd: &Command,
    body_cmds: Option<&[Command]>,
    empty: &str,
    out: &mut (impl Write + ?Sized),
) -> io::Result<()> {
    // Fields: OP  KIND  SEQ  ID  PROPS  BODY
    // Empty fields use the placeholder to prevent bash `read` IFS tab-collapsing.
    let e = empty;
    let f = |s: String| -> String { if s.is_empty() { e.to_string() } else { s } };
    let body = body_cmds
        .map(serialize_body)
        .filter(|b| !b.is_empty())
        .unwrap_or_else(|| e.to_string());
    match cmd {
        Command::Upsert { kind, id, props } => {
            writeln!(
                out,
                "+\t{kind}\t{e}\t{id}\t{}\t{body}",
                f(serialize_props(props))
            )
        }
        Command::Destroy { kind, id } => {
            writeln!(out, "-\t{kind}\t{e}\t{id}\t{e}\t{e}")
        }
        Command::Patch { kind, id, props } => {
            writeln!(
                out,
                "@\t{kind}\t{e}\t{id}\t{}\t{body}",
                f(serialize_props(props))
            )
        }
        Command::Event {
            kind,
            seq,
            id,
            props,
        } => writeln!(
            out,
            "!\t{}\t{seq}\t{id}\t{}\t{e}",
            kind.as_str(),
            f(serialize_props(props))
        ),
        Command::Ack { kind, seq, props } => writeln!(
            out,
            "!ack\t{}\t{seq}\t{e}\t{}\t{e}",
            kind.as_str(),
            f(serialize_props(props))
        ),
        Command::Request {
            kind,
            seq,
            targets,
            props,
        } => {
            writeln!(
                out,
                "?\t{}\t{seq}\t{}\t{}\t{e}",
                kind.as_str(),
                f(targets.join(",")),
                f(serialize_props(props))
            )
        }
        Command::Response {
            kind,
            seq,
            props,
            body: resp_body,
        } => {
            let b = resp_body
                .as_ref()
                .map(|b| serialize_body(b))
                .filter(|b| !b.is_empty())
                .unwrap_or_else(|| e.to_string());
            writeln!(
                out,
                ".\t{}\t{seq}\t{e}\t{}\t{b}",
                kind.as_str(),
                f(serialize_props(props))
            )
        }
        Command::Pragma(pragma) => {
            writeln!(out, "#\t{}\t{e}\t{e}\t{e}\t{e}", pragma.as_str())
        }
        Command::Message {
            kind,
            target,
            props,
            body,
        } => {
            let b = body
                .as_ref()
                .map(|b| serialize_body(b))
                .filter(|b| !b.is_empty())
                .unwrap_or_else(|| e.to_string());
            writeln!(
                out,
                ".\t{}\t{e}\t{target}\t{}\t{b}",
                kind.as_str(),
                f(serialize_props(props))
            )
        }
        Command::Push { .. } | Command::Pop => Ok(()),
    }
}

fn serialize_props(props: &[Prop]) -> String {
    if props.is_empty() {
        return String::new();
    }
    let mut buf = Vec::new();
    write_props(&mut buf, props).unwrap();
    String::from_utf8(buf).unwrap().trim_start().to_string()
}

fn serialize_body(cmds: &[Command]) -> String {
    if cmds.is_empty() {
        return String::new();
    }
    let mut buf = Vec::new();
    let mut em = Emitter::new(&mut buf);
    em.commands(cmds).unwrap();
    let s = String::from_utf8(buf).unwrap();
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

// -- JSON output --------------------------------------------------------------

fn write_json_obj(
    cmd: &Command,
    body_cmds: Option<&[Command]>,
    out: &mut (impl Write + ?Sized),
) -> io::Result<()> {
    out.write_all(b"{")?;
    match cmd {
        Command::Upsert { kind, id, props } => {
            out.write_all(b"\"op\":\"+\",\"kind\":")?;
            write_json_str(out, kind)?;
            out.write_all(b",\"id\":")?;
            write_json_str(out, id)?;
            write_json_props_field(out, props)?;
            if let Some(body) = body_cmds {
                write_json_body_field(out, body)?;
            }
        }
        Command::Destroy { kind, id } => {
            out.write_all(b"\"op\":\"-\",\"kind\":")?;
            write_json_str(out, kind)?;
            out.write_all(b",\"id\":")?;
            write_json_str(out, id)?;
        }
        Command::Patch { kind, id, props } => {
            out.write_all(b"\"op\":\"@\",\"kind\":")?;
            write_json_str(out, kind)?;
            out.write_all(b",\"id\":")?;
            write_json_str(out, id)?;
            write_json_props_field(out, props)?;
            if let Some(body) = body_cmds {
                write_json_body_field(out, body)?;
            }
        }
        Command::Event {
            kind,
            seq,
            id,
            props,
        } => {
            out.write_all(b"\"op\":\"!\",\"kind\":")?;
            write_json_str(out, kind.as_str())?;
            write!(out, ",\"seq\":{seq}")?;
            out.write_all(b",\"id\":")?;
            write_json_str(out, id)?;
            write_json_props_field(out, props)?;
        }
        Command::Ack { kind, seq, props } => {
            out.write_all(b"\"op\":\"!ack\",\"kind\":")?;
            write_json_str(out, kind.as_str())?;
            write!(out, ",\"seq\":{seq}")?;
            write_json_props_field(out, props)?;
        }
        Command::Request {
            kind,
            seq,
            targets,
            props,
        } => {
            out.write_all(b"\"op\":\"?\",\"kind\":")?;
            write_json_str(out, kind.as_str())?;
            write!(out, ",\"seq\":{seq}")?;
            out.write_all(b",\"targets\":[")?;
            for (i, t) in targets.iter().enumerate() {
                if i > 0 {
                    out.write_all(b",")?;
                }
                write_json_str(out, t)?;
            }
            out.write_all(b"]")?;
            write_json_props_field(out, props)?;
        }
        Command::Response {
            kind,
            seq,
            props,
            body,
        } => {
            out.write_all(b"\"op\":\".\",\"kind\":")?;
            write_json_str(out, kind.as_str())?;
            write!(out, ",\"seq\":{seq}")?;
            write_json_props_field(out, props)?;
            if let Some(body) = body {
                write_json_body_field(out, body)?;
            }
        }
        Command::Pragma(pragma) => {
            use PragmaKind::*;
            out.write_all(b"\"op\":\"#\",\"kind\":")?;
            write_json_str(out, pragma.as_str())?;
            out.write_all(b",\"targets\":[")?;
            match pragma {
                Claim(types) | Unclaim(types) | Observe(types) | Unobserve(types) => {
                    for (i, t) in types.iter().enumerate() {
                        if i > 0 {
                            out.write_all(b",")?;
                        }
                        write_json_str(out, t)?;
                    }
                }
                Handle(targets) | Unhandle(targets) => {
                    for (i, (t, r)) in targets.iter().enumerate() {
                        if i > 0 {
                            out.write_all(b",")?;
                        }
                        write_json_str(out, &format!("{t}?{r}"))?;
                    }
                }
                Redirect(target) => {
                    write_json_str(out, target)?;
                }
                Unredirect => {}
                Other { targets, .. } => {
                    for (i, t) in targets.iter().enumerate() {
                        if i > 0 {
                            out.write_all(b",")?;
                        }
                        write_json_str(out, t)?;
                    }
                }
            }
            out.write_all(b"]")?;
        }
        Command::Message {
            kind,
            target,
            props,
            body,
        } => {
            out.write_all(b"\"op\":\".\",\"kind\":")?;
            write_json_str(out, kind.as_str())?;
            out.write_all(b",\"target\":")?;
            write_json_str(out, target)?;
            write_json_props_field(out, props)?;
            if let Some(body) = body {
                write_json_body_field(out, body)?;
            }
        }
        Command::Push { .. } | Command::Pop => {}
    }
    out.write_all(b"}")
}

fn write_json_props_field(out: &mut (impl Write + ?Sized), props: &[Prop]) -> io::Result<()> {
    if props.is_empty() {
        return Ok(());
    }
    out.write_all(b",\"props\":{")?;
    let mut first = true;
    for prop in props {
        if !first {
            out.write_all(b",")?;
        }
        first = false;
        match prop {
            Prop::Value { key, value } => {
                write_json_str(out, key)?;
                out.write_all(b":")?;
                write_json_str(out, value)?;
            }
            Prop::Boolean { key } => {
                write_json_str(out, key)?;
                out.write_all(b":true")?;
            }
            Prop::Remove { key } => {
                let k = format!("~{key}");
                write_json_str(out, &k)?;
                out.write_all(b":true")?;
            }
        }
    }
    out.write_all(b"}")
}

fn write_json_body_field(out: &mut (impl Write + ?Sized), cmds: &[Command]) -> io::Result<()> {
    out.write_all(b",\"body\":[")?;
    let mut i = 0;
    let mut first = true;
    while i < cmds.len() {
        if matches!(cmds[i], Command::Push { .. } | Command::Pop) {
            i += 1;
            continue;
        }
        if !first {
            out.write_all(b",")?;
        }
        first = false;
        let (body, next) = collect_children(cmds, i);
        write_json_obj(&cmds[i], body, out)?;
        i = next;
    }
    out.write_all(b"]")
}

fn write_json_str(out: &mut (impl Write + ?Sized), s: &str) -> io::Result<()> {
    out.write_all(b"\"")?;
    for ch in s.chars() {
        match ch {
            '"' => out.write_all(b"\\\"")?,
            '\\' => out.write_all(b"\\\\")?,
            '\n' => out.write_all(b"\\n")?,
            '\r' => out.write_all(b"\\r")?,
            '\t' => out.write_all(b"\\t")?,
            c if c.is_control() => write!(out, "\\u{:04x}", c as u32)?,
            c => write!(out, "{c}")?,
        }
    }
    out.write_all(b"\"")
}
