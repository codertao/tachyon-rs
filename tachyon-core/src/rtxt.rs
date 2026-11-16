//! RTXT compiled string tables (`.bin`): `[group] key = value`. Format: `decomp/notes/formats.md`
//! "RTXT".

use anyhow::{ensure, Result};

#[derive(Debug, Clone, Default)]
pub struct Rtxt {
    /// `(group, key, value)` in record order.
    pub entries: Vec<(String, String, String)>,
}

impl Rtxt {
    pub fn parse(d: &[u8]) -> Result<Rtxt> {
        let u32_ = |o: usize| -> Result<usize> {
            Ok(u32::from_le_bytes(d.get(o..o + 4).ok_or_else(|| anyhow::anyhow!("RTXT: read past end at {o:#x}"))?.try_into().unwrap()) as usize)
        };
        let strz = |o: usize| -> Result<(String, usize)> {
            let n = d.get(o..).and_then(|t| t.iter().position(|&c| c == 0)).ok_or_else(|| anyhow::anyhow!("RTXT: bad string at {o:#x}"))?;
            Ok((d[o..o + n].iter().map(|&c| c as char).collect(), o + n + 1))
        };
        ensure!(d.len() >= 16 && &d[0..4] == b"RTXT", "not an RTXT file");
        let n = u32_(0xc)?;
        ensure!(n < 65536, "implausible RTXT record count");
        let values_base = 0x10 + 16 * n;
        let values: Vec<String> = (0..n).map(|i| Ok(strz(values_base + u32_(0x10 + 16 * i)?)?.0)).collect::<Result<_>>()?;
        let gto = u32_(4)?;
        let ngroups = u32_(gto)?;
        ensure!(ngroups < 4096, "implausible RTXT group count");
        let headers: Vec<(usize, usize)> = (0..ngroups).map(|g| Ok((u32_(gto + 4 + 8 * g)?, u32_(gto + 8 + 8 * g)?))).collect::<Result<_>>()?;
        let mut p = gto + 4 + 8 * ngroups;
        let mut names = Vec::with_capacity(ngroups);
        for _ in 0..ngroups {
            let (s, next) = strz(p)?;
            names.push(s);
            p = next;
        }
        let mut entries = Vec::with_capacity(n);
        for (name, (keys_off, count)) in names.iter().zip(&headers) {
            let mut k = gto + 4 + keys_off;
            for _ in 0..*count {
                let (key, next) = strz(k)?;
                let value = values.get(entries.len()).cloned().unwrap_or_default();
                entries.push((name.clone(), key, value));
                k = next;
            }
        }
        Ok(Rtxt { entries })
    }

    /// `Rtxt_FindKey`: the first record with this key in any group (exact match, as the engine's
    /// string compare).
    pub fn find_key(&self, key: &str) -> Option<&str> {
        self.entries.iter().find(|e| e.1 == key).map(|e| e.2.as_str())
    }
}
