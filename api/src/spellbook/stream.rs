//! Streaming reader for the upstream export: one JSON document of the shape
//! `{"timestamp": …, "version": …, "variants": [ {…}, {…}, … ]}`, ~650 MB inflated.
//!
//! `serde_json` parses a document whole, and this one can't be held whole. So this is a
//! byte-level **splitter**, not a parser: it walks the stream tracking only what it must —
//! whether it is inside a string (and an escape), and the brace depth — until it finds the
//! `"variants"` key's array, then hands back each top-level object of that array as its
//! own byte slice for `serde_json` to parse on its own. Memory is bounded by one variant
//! (a few KB), and the whole document is read exactly once.
//!
//! Deliberately not a general JSON tokenizer: it needs no number, literal or nested-key
//! knowledge, only string boundaries (a `{` inside a description must not open a level)
//! and depth. It reads `"variants"` at depth 1 only, so an object elsewhere carrying that
//! key can't hijack it, and an object over [`MAX_OBJECT_BYTES`] is an error rather than an
//! allocation — upstream's biggest variant is under 100 KB.

use std::io;

use tokio::io::{AsyncBufRead, AsyncBufReadExt};

/// The array whose elements are handed back.
const ARRAY_KEY: &[u8] = b"variants";

/// Largest element the splitter will buffer. A real variant is a few KB (the card image
/// URLs dominate); this is a guard against a malformed stream that never closes a brace.
pub const MAX_OBJECT_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// Before the root object's `{`.
    Start,
    /// Inside the root object, looking for the `"variants"` key.
    Root,
    /// Inside a string at the root level, collecting it as a candidate key.
    RootString,
    /// The `"variants"` string closed; expecting `:`.
    AfterKey,
    /// Saw the colon; expecting `[`.
    AfterColon,
    /// Inside the array, between elements.
    Array,
    /// Inside an element object, collecting its bytes.
    Object,
    /// Inside a root-level value that isn't the variants array (an object or list),
    /// skipping it whole.
    Skipping,
    /// The array closed (or the stream ended).
    Done,
}

/// Pulls the `variants` array's elements out of the document on `reader`, one at a time.
pub struct VariantSplitter<R> {
    reader: R,
    machine: Machine,
}

/// The byte-level state machine, apart from the reader so the two borrow disjointly.
struct Machine {
    phase: Phase,
    /// Brace depth inside the current element (1 = its own braces).
    depth: usize,
    in_string: bool,
    escaped: bool,
    /// The root-level string being read (a candidate key).
    key: Vec<u8>,
    /// The element being collected.
    object: Vec<u8>,
}

impl<R: AsyncBufRead + Unpin> VariantSplitter<R> {
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            machine: Machine {
                phase: Phase::Start,
                depth: 0,
                in_string: false,
                escaped: false,
                key: Vec::new(),
                object: Vec::new(),
            },
        }
    }

    /// The next element's bytes, or `None` once the array closes / the stream ends.
    ///
    /// A document with no `variants` array yields `None` immediately after the stream ends,
    /// which the ingest treats as "zero combos" — a failure to retry, never data.
    pub async fn next_object(&mut self) -> io::Result<Option<Vec<u8>>> {
        loop {
            if self.machine.phase == Phase::Done {
                return Ok(None);
            }
            let buf = self.reader.fill_buf().await?;
            if buf.is_empty() {
                // EOF. An element still open is a truncated stream.
                if self.machine.phase == Phase::Object {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "variants stream ended inside an object",
                    ));
                }
                self.machine.phase = Phase::Done;
                return Ok(None);
            }
            let mut consumed = 0;
            let mut found: Option<Vec<u8>> = None;
            for &byte in buf {
                consumed += 1;
                if self.machine.step(byte)? {
                    found = Some(std::mem::take(&mut self.machine.object));
                    break;
                }
            }
            self.reader.consume(consumed);
            if found.is_some() {
                return Ok(found);
            }
        }
    }
}

impl Machine {
    /// Feed one byte; `true` when it closed an element (now in `self.object`).
    fn step(&mut self, byte: u8) -> io::Result<bool> {
        match self.phase {
            Phase::Start => {
                if byte == b'{' {
                    self.phase = Phase::Root;
                } else if !byte.is_ascii_whitespace() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "variants document does not start with an object",
                    ));
                }
            }
            Phase::Root => match byte {
                b'"' => {
                    self.key.clear();
                    self.phase = Phase::RootString;
                }
                b'{' | b'[' => {
                    // A nested value at the root that isn't the variants array: skip it
                    // whole by depth-counting it as an element we then discard.
                    self.depth = 1;
                    self.in_string = false;
                    self.escaped = false;
                    self.phase = Phase::Skipping;
                }
                b'}' => self.phase = Phase::Done,
                _ => {}
            },
            Phase::RootString => {
                if self.escaped {
                    self.escaped = false;
                    self.key.push(byte);
                } else if byte == b'\\' {
                    self.escaped = true;
                } else if byte == b'"' {
                    self.phase = if self.key == ARRAY_KEY {
                        Phase::AfterKey
                    } else {
                        Phase::Root
                    };
                } else if self.key.len() < ARRAY_KEY.len() + 1 {
                    self.key.push(byte);
                } else {
                    // Longer than the key we want: can't match, stop collecting.
                    self.key.push(b'!');
                }
            }
            Phase::AfterKey => match byte {
                b':' => self.phase = Phase::AfterColon,
                b if b.is_ascii_whitespace() => {}
                // `"variants"` was a value, not a key (`"name": "variants"`): back to the root.
                _ => self.phase = Phase::Root,
            },
            Phase::AfterColon => match byte {
                b'[' => self.phase = Phase::Array,
                b if b.is_ascii_whitespace() => {}
                _ => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "the variants key does not hold an array",
                    ));
                }
            },
            Phase::Array => match byte {
                b'{' => {
                    self.object.clear();
                    self.object.push(byte);
                    self.depth = 1;
                    self.in_string = false;
                    self.escaped = false;
                    self.phase = Phase::Object;
                }
                b']' => self.phase = Phase::Done,
                b',' => {}
                b if b.is_ascii_whitespace() => {}
                _ => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "unexpected byte between variants",
                    ));
                }
            },
            Phase::Object => {
                self.object.push(byte);
                if self.object.len() > MAX_OBJECT_BYTES {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "a variant exceeds the maximum object size",
                    ));
                }
                if self.track(byte) {
                    self.phase = Phase::Array;
                    return Ok(true);
                }
            }
            Phase::Skipping => {
                if self.track(byte) {
                    self.phase = Phase::Root;
                }
            }
            Phase::Done => {}
        }
        Ok(false)
    }

    /// Track strings + depth through one byte of a nested value; `true` when the value
    /// that started at depth 1 has closed.
    fn track(&mut self, byte: u8) -> bool {
        if self.in_string {
            if self.escaped {
                self.escaped = false;
            } else if byte == b'\\' {
                self.escaped = true;
            } else if byte == b'"' {
                self.in_string = false;
            }
            return false;
        }
        match byte {
            b'"' => self.in_string = true,
            b'{' | b'[' => self.depth += 1,
            b'}' | b']' => {
                self.depth = self.depth.saturating_sub(1);
                if self.depth == 0 {
                    return true;
                }
            }
            _ => {}
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    async fn split(doc: &str) -> io::Result<Vec<String>> {
        // Tiny buffer so elements straddle reads the way they do on the wire.
        let reader = tokio::io::BufReader::with_capacity(7, Cursor::new(doc.as_bytes().to_vec()));
        let mut splitter = VariantSplitter::new(reader);
        let mut out = Vec::new();
        while let Some(bytes) = splitter.next_object().await? {
            out.push(String::from_utf8(bytes).expect("utf8"));
        }
        Ok(out)
    }

    #[tokio::test]
    async fn yields_each_variant_whole_across_read_boundaries() {
        let doc = r#"{"timestamp": "2026-09-08T19:09:08+00:00", "version": "6.4.0", "variants": [{"id": "1-2", "uses": [{"card": {"name": "A {b} \"c\""}}]}, {"id": "3-4", "description": "Do the thing.\nRepeat."}]}"#;
        let objects = split(doc).await.expect("split");
        assert_eq!(objects.len(), 2);
        assert_eq!(
            objects[0],
            r#"{"id": "1-2", "uses": [{"card": {"name": "A {b} \"c\""}}]}"#
        );
        // Each piece is valid JSON on its own.
        let parsed: serde_json::Value = serde_json::from_str(&objects[1]).expect("json");
        assert_eq!(parsed["id"], "3-4");
    }

    #[tokio::test]
    async fn braces_and_brackets_inside_strings_do_not_count() {
        let doc = r#"{"variants": [{"notes": "}] ] } {", "n": 1}, {"notes": "\\\"]}", "n": 2}]}"#;
        let objects = split(doc).await.expect("split");
        assert_eq!(objects.len(), 2);
        assert!(objects[1].ends_with(r#""n": 2}"#));
    }

    #[tokio::test]
    async fn a_variants_key_nested_elsewhere_is_ignored() {
        // A root value that is an object carrying the key: skipped whole.
        let doc = r#"{"meta": {"variants": [{"id": "decoy"}]}, "name": "variants", "variants": [{"id": "real"}]}"#;
        let objects = split(doc).await.expect("split");
        assert_eq!(objects, vec![r#"{"id": "real"}"#]);
    }

    #[tokio::test]
    async fn an_empty_array_and_a_missing_key_yield_nothing() {
        assert!(
            split(r#"{"variants": []}"#)
                .await
                .expect("split")
                .is_empty()
        );
        assert!(
            split(r#"{"version": "1"}"#)
                .await
                .expect("split")
                .is_empty()
        );
    }

    #[tokio::test]
    async fn a_truncated_stream_is_an_error_not_a_short_list() {
        let err = split(r#"{"variants": [{"id": "1"}, {"id": "2", "uses": ["#)
            .await
            .expect_err("truncated");
        assert_eq!(err.kind(), io::ErrorKind::UnexpectedEof);
        let err = split(r#"[1, 2]"#).await.expect_err("not an object");
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }
}
