//! Reads where Finder says each item in a Trash folder came from, for Put Back.
//!
//! Finder keeps this in the Trash folder's `.DS_Store` file, a small B-tree of records keyed by
//! file name. For each trashed item it writes `ptbL` (the folder it came from, relative to its
//! disk's root, such as `Users/name/Desktop/`) and `ptbN` (its name there, which can differ
//! from its name in the Trash). Nothing else in the file is read.
//!
//! The file is untrusted: every offset, length and count is checked, the tree's depth and size
//! are bounded, and anything malformed gives no records rather than an error or a panic.

use std::collections::HashMap;

/// Reading stops beyond this many records or nodes: a real Trash has far fewer.
const MAX_RECORDS: usize = 100_000;
const MAX_NODES: usize = 10_000;
/// How deep the tree may go before it is treated as malformed.
const MAX_DEPTH: usize = 32;

/// Where Finder says a trashed item came from: its folder relative to its disk's root, and its
/// name there.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Origin {
    pub folder: Option<String>,
    pub name: Option<String>,
}

/// Each trashed item's origin, by its name in the Trash; empty if the file can't be read.
pub fn origins(file: &[u8]) -> HashMap<String, Origin> {
    let mut found: HashMap<String, Origin> = HashMap::new();
    if let Some(records) = records(file) {
        for (name, code, value) in records {
            let origin = found.entry(name).or_default();
            match &code {
                b"ptbL" => origin.folder = Some(value),
                b"ptbN" => origin.name = Some(value),
                _ => {}
            }
        }
    }
    found.retain(|_, origin| origin.folder.is_some());
    found
}

struct Reader<'a> {
    data: &'a [u8],
}

impl Reader<'_> {
    fn u32_at(&self, at: usize) -> Option<u32> {
        let bytes = self.data.get(at..at.checked_add(4)?)?;
        Some(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn bytes_at(&self, at: usize, len: usize) -> Option<&[u8]> {
        self.data.get(at..at.checked_add(len)?)
    }
}

/// Every `ptbL`/`ptbN` record as (file name, code, text); `None` if the file is malformed.
fn records(file: &[u8]) -> Option<Vec<(String, [u8; 4], String)>> {
    // Offsets in the file count from byte 4, after a 4-byte prefix.
    let data = file.get(4..)?;
    let reader = Reader { data };
    if reader.bytes_at(0, 4)? != b"Bud1" {
        return None;
    }
    let root = usize::try_from(reader.u32_at(4)?).ok()?;
    // The root block: block addresses, then the table of contents.
    let count = usize::try_from(reader.u32_at(root)?).ok()?;
    if count > MAX_NODES {
        return None;
    }
    let mut blocks = Vec::with_capacity(count);
    for index in 0..count {
        let address = reader.u32_at(root + 8 + index * 4)?;
        // The low 5 bits are the block's size as a power of two; the rest is its offset.
        blocks.push(usize::try_from(address & !0x1f).ok()?);
    }
    // Addresses are padded to a multiple of 256 entries.
    let padded = count.div_ceil(256).max(1) * 256;
    let mut at = root + 8 + padded * 4;
    let toc_count = reader.u32_at(at)?;
    at += 4;
    let mut dsdb = None;
    for _ in 0..toc_count.min(64) {
        let len = usize::from(*reader.data.get(at)?);
        let name = reader.bytes_at(at + 1, len)?;
        let block = reader.u32_at(at + 1 + len)?;
        at += 1 + len + 4;
        if name == b"DSDB" {
            dsdb = Some(usize::try_from(block).ok()?);
        }
    }
    let header = *blocks.get(dsdb?)?;
    let root_node = usize::try_from(reader.u32_at(header)?).ok()?;
    let mut out = Vec::new();
    let mut visited = 0;
    walk(&reader, &blocks, root_node, 0, &mut visited, &mut out)?;
    Some(out)
}

/// Reads one node and, for an inner node, its children.
fn walk(
    reader: &Reader<'_>,
    blocks: &[usize],
    node: usize,
    depth: usize,
    visited: &mut usize,
    out: &mut Vec<(String, [u8; 4], String)>,
) -> Option<()> {
    *visited += 1;
    if depth > MAX_DEPTH || *visited > MAX_NODES || out.len() > MAX_RECORDS {
        return None;
    }
    let start = *blocks.get(node)?;
    let right = usize::try_from(reader.u32_at(start)?).ok()?;
    let count = usize::try_from(reader.u32_at(start + 4)?).ok()?;
    if count > MAX_RECORDS {
        return None;
    }
    let mut at = start + 8;
    for _ in 0..count {
        if right != 0 {
            // An inner node: each record follows the child holding the records before it.
            let child = usize::try_from(reader.u32_at(at)?).ok()?;
            at += 4;
            walk(reader, blocks, child, depth + 1, visited, out)?;
        }
        at = record(reader, at, out)?;
    }
    if right != 0 {
        walk(reader, blocks, right, depth + 1, visited, out)?;
    }
    Some(())
}

/// Reads the record at `at`, keeping it if it is a `ptbL`/`ptbN` string; returns where the
/// next one starts.
fn record(
    reader: &Reader<'_>,
    at: usize,
    out: &mut Vec<(String, [u8; 4], String)>,
) -> Option<usize> {
    let name_len = usize::try_from(reader.u32_at(at)?).ok()?;
    let name = utf16(reader.bytes_at(at + 4, name_len.checked_mul(2)?)?)?;
    let mut at = at + 4 + name_len * 2;
    let code: [u8; 4] = reader.bytes_at(at, 4)?.try_into().ok()?;
    let kind: [u8; 4] = reader.bytes_at(at + 4, 4)?.try_into().ok()?;
    at += 8;
    let size = match &kind {
        b"bool" => 1,
        b"long" | b"shor" | b"type" => 4,
        b"comp" | b"dutc" => 8,
        b"blob" => 4 + usize::try_from(reader.u32_at(at)?).ok()?,
        b"ustr" => {
            let len = usize::try_from(reader.u32_at(at)?).ok()?;
            if matches!(&code, b"ptbL" | b"ptbN") {
                let text = utf16(reader.bytes_at(at + 4, len.checked_mul(2)?)?)?;
                out.push((name, code, text));
            }
            4 + len.checked_mul(2)?
        }
        // An unknown type: its size can't be known, so nothing after it can be read.
        _ => return None,
    };
    reader.bytes_at(at, size)?;
    Some(at + size)
}

fn utf16(bytes: &[u8]) -> Option<String> {
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_be_bytes(*pair))
        .collect();
    String::from_utf16(&units).ok()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn utf16_be(text: &str) -> Vec<u8> {
        text.encode_utf16().flat_map(u16::to_be_bytes).collect()
    }

    /// A record: file name, code, then a `ustr` value.
    fn ustr(name: &str, code: &[u8; 4], value: &str) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend(
            u32::try_from(name.encode_utf16().count())
                .unwrap_or(0)
                .to_be_bytes(),
        );
        out.extend(utf16_be(name));
        out.extend(code);
        out.extend(b"ustr");
        out.extend(
            u32::try_from(value.encode_utf16().count())
                .unwrap_or(0)
                .to_be_bytes(),
        );
        out.extend(utf16_be(value));
        out
    }

    fn other(name: &str, code: &[u8; 4], kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend(
            u32::try_from(name.encode_utf16().count())
                .unwrap_or(0)
                .to_be_bytes(),
        );
        out.extend(utf16_be(name));
        out.extend(code);
        out.extend(kind);
        out.extend(payload);
        out
    }

    /// A `.DS_Store` with one leaf node holding `records`, laid out as Finder lays it out.
    pub(crate) fn file(records: &[Vec<u8>]) -> Vec<u8> {
        // Blocks (offsets from byte 4): 0x20 the DSDB header, 0x40 the leaf, 0x1000 the root.
        let mut data = vec![0_u8; 0x2000];
        data[0..4].copy_from_slice(b"Bud1");
        data[4..8].copy_from_slice(&0x1000_u32.to_be_bytes());
        // DSDB header: root node block 1.
        data[0x20..0x24].copy_from_slice(&1_u32.to_be_bytes());
        // Leaf node at 0x40.
        let mut leaf = Vec::new();
        leaf.extend(0_u32.to_be_bytes());
        leaf.extend(u32::try_from(records.len()).unwrap_or(0).to_be_bytes());
        for record in records {
            leaf.extend(record);
        }
        data[0x40..0x40 + leaf.len()].copy_from_slice(&leaf);
        // Root block at 0x1000: 2 block addresses (0x20 and 0x40, size bits 5), padded to 256.
        let mut root = Vec::new();
        root.extend(2_u32.to_be_bytes());
        root.extend(0_u32.to_be_bytes());
        root.extend((0x20_u32 | 5).to_be_bytes());
        root.extend((0x40_u32 | 5).to_be_bytes());
        root.extend(vec![0_u8; 254 * 4]);
        root.extend(1_u32.to_be_bytes());
        root.push(4);
        root.extend(b"DSDB");
        root.extend(0_u32.to_be_bytes());
        data[0x1000..0x1000 + root.len()].copy_from_slice(&root);
        let mut file = vec![0, 0, 0, 1];
        file.extend(data);
        file
    }

    #[test]
    fn finder_s_put_back_records_are_read_by_trashed_name() {
        let file = file(&[
            ustr("Report 2.pdf", b"ptbL", "Users/me/Documents/"),
            ustr("Report 2.pdf", b"ptbN", "Report.pdf"),
            other("Report 2.pdf", b"Iloc", b"blob", &[0, 0, 0, 2, 1, 2]),
            ustr("Photos", b"ptbL", "Users/me/Desktop/"),
            other("Photos", b"dilc", b"long", &[0, 0, 0, 9]),
        ]);
        let found = origins(&file);
        assert_eq!(
            found.get("Report 2.pdf"),
            Some(&Origin {
                folder: Some("Users/me/Documents/".to_owned()),
                name: Some("Report.pdf".to_owned()),
            })
        );
        assert_eq!(
            found
                .get("Photos")
                .and_then(|origin| origin.folder.as_deref()),
            Some("Users/me/Desktop/")
        );
        assert_eq!(found.len(), 2);
    }

    #[test]
    fn a_malformed_file_gives_nothing_and_never_panics() {
        assert!(origins(&[]).is_empty());
        assert!(origins(b"\0\0\0\x01Bud2").is_empty());
        let good = file(&[ustr("a", b"ptbL", "Users/me/")]);
        // Every truncation, and a corrupted byte at each position, is handled.
        for len in 0..good.len().min(0x1500) {
            let _ = origins(&good[..len]);
        }
        for at in (0..0x1100).step_by(7) {
            let mut bad = good.clone();
            bad[at] ^= 0xff;
            let _ = origins(&bad);
        }
        // An unknown value type stops reading rather than guessing.
        let unknown = file(&[
            other("a", b"xxxx", b"zzzz", &[1, 2, 3]),
            ustr("b", b"ptbL", "x/"),
        ]);
        assert!(origins(&unknown).is_empty());
    }
}
