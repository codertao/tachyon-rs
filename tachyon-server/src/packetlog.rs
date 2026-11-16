//! `--log-packets <file>`: every engine packet in and out, decoded to sub-messages with a hex
//! dump, so a failed real-client test can be diagnosed from the log alone. Netsock session
//! messages (enumerate, join, create player, leave, and their replies) are logged too, keyed by
//! address since there's no slot yet. With `--log-detail`
//! each sub-message also gets its interpretation (`tachyon_core::net::describe`) and the hex
//! dump is not cut short.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::net::SocketAddr;
use std::path::Path;
use std::time::Instant;
use tachyon_core::net::{describe, netsock, packet};

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

    /// A Netsock session message (anything but 0x1080 game data), always interpreted.
    pub fn session(&mut self, incoming: bool, addr: SocketAddr, msg: &[u8]) {
        let t = self.start.elapsed().as_secs_f64();
        let dir = if incoming { "<-" } else { "->" };
        let (name, lines) = describe::session(msg);
        let kind = netsock::kind(msg);
        let _ = writeln!(self.out, "{t:9.3} {dir} {addr}: netsock {kind:#06x} {name} [{} bytes]", msg.len());
        for line in lines {
            let _ = writeln!(self.out, "              = {line}");
        }
        if self.detail {
            for chunk in msg.chunks(32) {
                let _ = writeln!(self.out, "                {}", hex(chunk));
            }
        }
        let _ = self.out.flush();
    }

    /// A datagram that failed the Netsock CRC check.
    pub fn bad_datagram(&mut self, addr: SocketAddr, dgram: &[u8]) {
        let t = self.start.elapsed().as_secs_f64();
        let _ = writeln!(self.out, "{t:9.3} <- {addr}: bad CRC, {} bytes: {}", dgram.len(), hex(&dgram[..dgram.len().min(64)]));
        let _ = self.out.flush();
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
