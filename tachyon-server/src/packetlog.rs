//! `--log-packets <file>`: every engine packet in and out, decoded to sub-messages with a hex
//! dump, so a failed real-client test can be diagnosed from the log alone. With `--log-detail`
//! each sub-message also gets its interpretation (`tachyon_core::net::describe`) and the hex
//! dump is not cut short.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::time::Instant;
use tachyon_core::net::{describe, packet};

pub struct PacketLog {
    out: BufWriter<File>,
    start: Instant,
    detail: bool,
}

impl PacketLog {
    pub fn create(path: &Path, detail: bool) -> std::io::Result<Self> {
        Ok(PacketLog { out: BufWriter::new(File::create(path)?), start: Instant::now(), detail })
    }

    pub fn note(&mut self, s: &str) {
        let t = self.start.elapsed().as_secs_f64();
        let _ = writeln!(self.out, "{t:9.3} # {s}");
    }

    pub fn packet(&mut self, incoming: bool, slot: u16, p: &[u8]) {
        let t = self.start.elapsed().as_secs_f64();
        let dir = if incoming { "<-" } else { "->" };
        let Some((seq, ack, subs)) = packet::parse(p) else {
            let _ = writeln!(self.out, "{t:9.3} {dir} slot {slot}: short packet {}", hex(p));
            return;
        };
        let _ = writeln!(self.out, "{t:9.3} {dir} slot {slot}: seq {seq} ack {ack}, {} bytes", p.len());
        for s in subs {
            let tag = s.tag.map_or(String::new(), |t| format!(" tag {t:#x}"));
            if !self.detail {
                let _ = writeln!(self.out, "            {:#06x}{tag} [{}] {}", s.kind, s.body.len(), hex(&s.body[..s.body.len().min(96)]));
                continue;
            }
            let name = describe::name(s.kind).unwrap_or("?");
            let _ = writeln!(self.out, "            {:#06x} {name}{tag} [{}]", s.kind, s.body.len());
            for line in describe::describe(s.kind, s.body, incoming) {
                let _ = writeln!(self.out, "              = {line}");
            }
            for chunk in s.body.chunks(32) {
                let _ = writeln!(self.out, "                {}", hex(chunk));
            }
        }
        let _ = self.out.flush();
    }
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect::<Vec<_>>().join(" ")
}
