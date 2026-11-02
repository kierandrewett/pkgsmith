use crate::internal::codepage::CodePage;
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use std::collections::{BTreeSet, HashMap};
use std::io::{self, Read, Write};

// ========================================================================= //

const LONG_STRING_REFS_BIT: u32 = 0x8000_0000;

const MAX_STRING_REF: i32 = 0xff_ffff;

// ========================================================================= //

/// A reference to a string in the string pool.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct StringRef(i32);

impl StringRef {
    /// Reads a serialized StringRef and returns it, or `None` for a null
    /// reference.  The `long_string_refs` argument specifies whether to read
    /// three bytes (if true) or two (if false).
    pub fn read<R: Read>(
        reader: &mut R,
        long_string_refs: bool,
    ) -> io::Result<Option<Self>> {
        let mut number = reader.read_u16::<LittleEndian>()? as i32;
        if long_string_refs {
            number |= (reader.read_u8()? as i32) << 16;
        }
        Ok(if number == 0 { None } else { Some(Self(number)) })
    }

    /// Serializes a nullable StringRef.  The `long_string_refs` argument
    /// specifies whether to write three bytes (if true) or two (if false).
    pub fn write<W: Write>(
        writer: &mut W,
        string_ref: Option<Self>,
        long_string_refs: bool,
    ) -> io::Result<()> {
        let number = if let Some(Self(number)) = string_ref {
            debug_assert!(number > 0);
            debug_assert!(number <= MAX_STRING_REF);
            number
        } else {
            0
        };
        if long_string_refs {
            writer.write_u16::<LittleEndian>((number & 0xffff) as u16)?;
            writer.write_u8(((number >> 16) & 0xff) as u8)?;
        } else if number <= (u16::MAX as i32) {
            writer.write_u16::<LittleEndian>(number as u16)?;
        } else {
            invalid_input!(
                "Cannot write {:?} with long_string_refs=false",
                Self(number)
            );
        }
        Ok(())
    }

    /// Returns the reference number, that is, the 1-based index into the
    /// string pool for this reference.
    pub fn number(self) -> i32 {
        let Self(number) = self;
        debug_assert!(number > 0);
        debug_assert!(number <= MAX_STRING_REF);
        number
    }

    fn index(self) -> usize {
        let number = self.number();
        debug_assert!(number > 0);
        debug_assert!(number <= MAX_STRING_REF);
        (number - 1) as usize
    }
}

// ========================================================================= //

pub struct StringPoolBuilder {
    codepage: CodePage,
    long_string_refs: bool,
    lengths_and_refcounts: Vec<(u32, u16)>,
}

impl StringPoolBuilder {
    pub fn read_from_pool<R: Read>(mut reader: R) -> io::Result<Self> {
        let codepage_id = reader.read_u32::<LittleEndian>()?;
        let long_string_refs = (codepage_id & LONG_STRING_REFS_BIT) != 0;
        let codepage_id = (codepage_id & !LONG_STRING_REFS_BIT) as i32;
        let codepage = match CodePage::from_id(codepage_id) {
            Some(codepage) => codepage,
            None => invalid_data!(
                "Unknown codepage for string pool ({})",
                codepage_id
            ),
        };
        let mut lengths_and_refcounts = Vec::<(u32, u16)>::new();
        while let Ok(length) = reader.read_u16::<LittleEndian>() {
            let mut length = length as u32;
            let refcount = reader.read_u16::<LittleEndian>()?;
            if length == 0 && refcount > 0 {
                length = reader.read_u32::<LittleEndian>()?;
            }
            lengths_and_refcounts.push((length, refcount));
        }
        Ok(Self { codepage, long_string_refs, lengths_and_refcounts })
    }

    pub fn build_from_data<R: Read>(
        self,
        mut reader: R,
    ) -> io::Result<StringPool> {
        let mut strings = Vec::<(String, u16)>::new();
        for (length, refcount) in self.lengths_and_refcounts {
            let mut buffer = vec![0u8; length as usize];
            reader.read_exact(&mut buffer)?;
            strings.push((self.codepage.decode(&buffer), refcount));
        }
        Ok(StringPool::from_strings(
            self.codepage,
            strings,
            self.long_string_refs,
        ))
    }
}

// ========================================================================= //

/// The string pool for an MSI package.
pub struct StringPool {
    codepage: CodePage,
    strings: Vec<(String, u16)>,
    /// Maps each string that is in use to the indices of the entries that hold
    /// it, in ascending order.  There is more than one entry only when the
    /// refcount of an earlier one is saturated.
    index: HashMap<String, Vec<usize>>,
    /// The indices of the entries with a refcount of zero, which `incref`
    /// reuses before it grows the pool.
    free: BTreeSet<usize>,
    long_string_refs: bool,
    is_modified: bool,
}

impl StringPool {
    /// Creates a new, empty string pool.
    pub fn new(codepage: CodePage) -> Self {
        Self {
            codepage,
            strings: Vec::new(),
            index: HashMap::new(),
            free: BTreeSet::new(),
            long_string_refs: false,
            is_modified: true,
        }
    }

    fn from_strings(
        codepage: CodePage,
        strings: Vec<(String, u16)>,
        long_string_refs: bool,
    ) -> Self {
        let mut index = HashMap::<String, Vec<usize>>::new();
        let mut free = BTreeSet::new();
        for (position, (string, refcount)) in strings.iter().enumerate() {
            if *refcount == 0 {
                free.insert(position);
            } else {
                index.entry(string.clone()).or_default().push(position);
            }
        }
        Self {
            codepage,
            strings,
            index,
            free,
            long_string_refs,
            is_modified: false,
        }
    }

    /// Gets the code page used for serializing the string data.
    pub fn codepage(&self) -> CodePage {
        self.codepage
    }

    /// Sets the code page used for serializing the string data.
    pub fn set_codepage(&mut self, codepage: CodePage) {
        self.is_modified = true;
        self.codepage = codepage;
    }

    /// Returns the number of strings in the string pool (including empty
    /// entries).
    #[allow(dead_code)]
    pub fn num_strings(&self) -> u32 {
        self.strings.len() as u32
    }

    /// Returns true if string references should be serialized with three bytes
    /// instead of two.
    pub fn long_string_refs(&self) -> bool {
        self.long_string_refs
    }

    /// Switches the pool to string references of three bytes.  The caller
    /// must rewrite every table that holds string references.
    pub(crate) fn set_long_string_refs(&mut self) {
        if !self.long_string_refs {
            self.long_string_refs = true;
            self.is_modified = true;
        }
    }

    /// Returns true if string references of two bytes can still address
    /// `extra` more entries.
    pub(crate) fn fits_short_refs(&self, extra: usize) -> bool {
        self.strings.len().saturating_add(extra) <= u16::MAX as usize
    }

    /// Returns an upper bound for the number of entries that `incref` adds to
    /// the pool when it is called, for each `(string, count)`, `count` times.
    pub(crate) fn max_new_entries<'a>(
        &self,
        counts: impl IntoIterator<Item = (&'a str, usize)>,
    ) -> usize {
        let per_entry = u16::MAX as usize;
        let mut total = 0usize;
        for (string, count) in counts {
            // Entries that are free would be reused; they are ignored here, so
            // the result can be higher than what is added, never lower.
            let headroom: usize = self.index.get(string).map_or(0, |entries| {
                entries
                    .iter()
                    .map(|&entry| per_entry - self.strings[entry].1 as usize)
                    .sum()
            });
            total += count.saturating_sub(headroom).div_ceil(per_entry);
        }
        total
    }

    pub(crate) fn is_modified(&self) -> bool {
        self.is_modified
    }

    pub(crate) fn mark_unmodified(&mut self) {
        self.is_modified = false;
    }

    /// Returns the string in the pool for the given reference.
    pub fn get(&self, string_ref: StringRef) -> &str {
        let index = string_ref.index();
        if index < self.strings.len() {
            self.strings[index].0.as_str()
        } else {
            ""
        }
    }

    /// Returns the pool's refcount for the given string reference.
    #[allow(dead_code)]
    pub fn refcount(&self, string_ref: StringRef) -> u16 {
        let index = string_ref.index();
        if index < self.strings.len() { self.strings[index].1 } else { 0 }
    }

    /// Inserts a string into the pool, or increments its refcount if it's
    /// already in the pool, and returns the index of the string in the pool.
    ///
    /// The entry used is the first one, in pool order, that is free or that
    /// holds the string and has room in its refcount.
    pub fn incref(&mut self, string: String) -> StringRef {
        self.is_modified = true;
        let matching = self.index.get(&string).and_then(|entries| {
            entries
                .iter()
                .copied()
                .find(|&entry| self.strings[entry].1 < u16::MAX)
        });
        let free = self.free.first().copied();
        match (free, matching) {
            (Some(free), Some(matching)) if matching < free => {
                self.strings[matching].1 += 1;
                return StringRef((matching + 1) as i32);
            }
            (None, Some(matching)) => {
                self.strings[matching].1 += 1;
                return StringRef((matching + 1) as i32);
            }
            (Some(free), _) => {
                self.free.remove(&free);
                let entries = self.index.entry(string.clone()).or_default();
                let position = entries.partition_point(|&entry| entry < free);
                entries.insert(position, free);
                self.strings[free] = (string, 1);
                return StringRef((free + 1) as i32);
            }
            (None, None) => {}
        }
        if self.strings.len() >= u16::MAX as usize && !self.long_string_refs {
            panic!(
                "Too many strings for two-byte string refs; the package must \
                    switch to long string refs first"
            );
        }
        if self.strings.len() >= MAX_STRING_REF as usize {
            panic!("Too many distinct strings in string pool");
        }
        self.index.entry(string.clone()).or_default().push(self.strings.len());
        self.strings.push((string, 1));
        StringRef(self.strings.len() as i32)
    }

    /// Decrements the refcount of a string in the pool.
    pub fn decref(&mut self, string_ref: StringRef) {
        let index = string_ref.index();
        if index >= self.strings.len() {
            panic!(
                "decref: string_ref {} invalid, pool has only {} entries",
                string_ref.number(),
                self.strings.len()
            );
        }
        let (ref mut string, ref mut refcount) = self.strings[index];
        if *refcount < 1 {
            panic!("decref: string refcount is already zero");
        }
        self.is_modified = true;
        *refcount -= 1;
        if *refcount == 0 {
            if let Some(entries) = self.index.get_mut(string.as_str()) {
                entries.retain(|&entry| entry != index);
                if entries.is_empty() {
                    self.index.remove(string.as_str());
                }
            }
            string.clear();
            self.free.insert(index);
        }
    }

    /// Writes to the `_StringPool` table.
    pub fn write_pool<W: Write>(&self, mut writer: W) -> io::Result<()> {
        let mut codepage_id = self.codepage.id() as u32;
        if self.long_string_refs {
            codepage_id |= LONG_STRING_REFS_BIT;
        }
        writer.write_u32::<LittleEndian>(codepage_id)?;
        for &(ref string, refcount) in &self.strings {
            let length = self.codepage.encode(string).len() as u32;
            let short_length = u16::try_from(length).unwrap_or_default();
            writer.write_u16::<LittleEndian>(short_length)?;
            writer.write_u16::<LittleEndian>(refcount)?;
            if short_length == 0 && refcount > 0 {
                writer.write_u32::<LittleEndian>(length)?;
            }
        }
        Ok(())
    }

    /// Writes to the `_StringData` table.
    pub fn write_data<W: Write>(&self, mut writer: W) -> io::Result<()> {
        for (string, _) in &self.strings {
            writer.write_all(&self.codepage.encode(string.as_str()))?;
        }
        Ok(())
    }
}

// ========================================================================= //

#[cfg(test)]
mod tests {
    use super::{StringPool, StringPoolBuilder, StringRef};
    use crate::internal::codepage::CodePage;

    #[test]
    fn read_string_ref() {
        let mut input: &[u8] = b"\x00\x00";
        assert_eq!(StringRef::read(&mut input, false).unwrap(), None);

        let mut input: &[u8] = b"\x34\x12";
        assert_eq!(
            StringRef::read(&mut input, false).unwrap(),
            Some(StringRef(0x1234))
        );

        let mut input: &[u8] = b"\x00\x00\x00";
        assert_eq!(StringRef::read(&mut input, true).unwrap(), None);

        let mut input: &[u8] = b"\x00\x00\x02";
        assert_eq!(
            StringRef::read(&mut input, true).unwrap(),
            Some(StringRef(0x20000))
        );

        let mut input: &[u8] = b"\x56\x34\x12";
        assert_eq!(
            StringRef::read(&mut input, true).unwrap(),
            Some(StringRef(0x123456))
        );
    }

    #[test]
    fn write_string_ref() {
        let mut output = Vec::<u8>::new();
        StringRef::write(&mut output, None, false).unwrap();
        assert_eq!(&output as &[u8], b"\x00\x00");

        let mut output = Vec::<u8>::new();
        StringRef::write(&mut output, Some(StringRef(0x1234)), false).unwrap();
        assert_eq!(&output as &[u8], b"\x34\x12");

        let mut output = Vec::<u8>::new();
        StringRef::write(&mut output, None, true).unwrap();
        assert_eq!(&output as &[u8], b"\x00\x00\x00");

        let mut output = Vec::<u8>::new();
        StringRef::write(&mut output, Some(StringRef(0x123456)), true)
            .unwrap();
        assert_eq!(&output as &[u8], b"\x56\x34\x12");
    }

    #[test]
    fn new_string_pool() {
        let mut string_pool = StringPool::new(CodePage::default());
        assert!(!string_pool.long_string_refs());
        assert_eq!(string_pool.num_strings(), 0);
        assert_eq!(string_pool.incref("Foo".to_string()), StringRef(1));
        assert_eq!(string_pool.num_strings(), 1);
        assert_eq!(string_pool.incref("Quux".to_string()), StringRef(2));
        assert_eq!(string_pool.num_strings(), 2);
        assert_eq!(string_pool.incref("Foo".to_string()), StringRef(1));
        assert_eq!(string_pool.num_strings(), 2);
        assert_eq!(string_pool.get(StringRef(1)), "Foo");
        assert_eq!(string_pool.refcount(StringRef(1)), 2);
        assert_eq!(string_pool.get(StringRef(2)), "Quux");
        assert_eq!(string_pool.refcount(StringRef(2)), 1);
    }

    #[test]
    fn build_string_pool() {
        let pool: &[u8] = b"\xe9\xfd\x00\x00\x03\x00\x02\x00\x04\x00\x07\x00";
        let data: &[u8] = b"FooQuux";
        let builder = StringPoolBuilder::read_from_pool(pool).expect("pool");
        let string_pool = builder.build_from_data(data).expect("data");
        assert_eq!(string_pool.codepage(), CodePage::Utf8);
        assert!(!string_pool.long_string_refs());
        assert_eq!(string_pool.num_strings(), 2);
        assert_eq!(string_pool.get(StringRef(1)), "Foo");
        assert_eq!(string_pool.refcount(StringRef(1)), 2);
        assert_eq!(string_pool.get(StringRef(2)), "Quux");
        assert_eq!(string_pool.refcount(StringRef(2)), 7);
    }

    #[test]
    fn write_string_pool() {
        let mut string_pool = StringPool::new(CodePage::Windows1252);
        assert_eq!(string_pool.incref("Foo".to_string()), StringRef(1));
        assert_eq!(string_pool.incref("Quux".to_string()), StringRef(2));
        assert_eq!(string_pool.incref("Foo".to_string()), StringRef(1));
        let mut pool_output = Vec::<u8>::new();
        string_pool.write_pool(&mut pool_output).expect("pool");
        assert_eq!(
            &pool_output as &[u8],
            b"\xe4\x04\x00\x00\x03\x00\x02\x00\x04\x00\x01\x00"
        );
        let mut data_output = Vec::<u8>::new();
        string_pool.write_data(&mut data_output).expect("data");
        assert_eq!(&data_output as &[u8], b"FooQuux");
    }

    #[test]
    fn long_string_refs() {
        let pool: &[u8] = b"\xe4\x04\x00\x80\x03\x00\x02\x00\x04\x00\x07\x00";
        let data: &[u8] = b"FooQuux";
        let builder = StringPoolBuilder::read_from_pool(pool).expect("pool");
        let string_pool = builder.build_from_data(data).expect("data");
        assert_eq!(string_pool.codepage(), CodePage::Windows1252);
        assert!(string_pool.long_string_refs());
        assert_eq!(string_pool.num_strings(), 2);
    }

    #[test]
    fn repeated_string() {
        let pool: &[u8] = b"\xe9\xfd\x00\x00\x03\x00\x02\x00\x03\x00\x07\x00";
        let data: &[u8] = b"FooFoo";
        let builder = StringPoolBuilder::read_from_pool(pool).expect("pool");
        let string_pool = builder.build_from_data(data).expect("data");
        assert_eq!(string_pool.num_strings(), 2);
        assert_eq!(string_pool.get(StringRef(1)), "Foo");
        assert_eq!(string_pool.refcount(StringRef(1)), 2);
        assert_eq!(string_pool.get(StringRef(2)), "Foo");
        assert_eq!(string_pool.refcount(StringRef(2)), 7);
    }

    #[test]
    fn deserialize_string_over_64k() {
        // This byte array represents a serialized string pool with:
        // - codepage ID = 65001 (UTF-8)
        // - one string with length = 70,000
        // - refcount = 1
        let pool: &[u8] = b"\xe9\xfd\x00\x00\x00\x00\x01\x00\x70\x11\x01\x00";

        // Generate 70,000 bytes by repeating "rustmsi" 10,000 times
        let rustmsi = b"rustmsi";
        let mut rustmsi_70kb = [0u8; 70_000];
        for chunk in rustmsi_70kb.chunks_exact_mut(rustmsi.len()) {
            chunk.copy_from_slice(rustmsi);
        }

        // Deserialize the pool header
        let builder = StringPoolBuilder::read_from_pool(pool)
            .expect("Failed to read pool metadata");

        // Rebuild the pool from the raw string data
        let string_pool = builder
            .build_from_data(&rustmsi_70kb[..])
            .expect("Failed to build pool");

        // Verify pool metadata and content
        assert_eq!(string_pool.codepage(), CodePage::Utf8);
        assert!(!string_pool.long_string_refs());
        assert_eq!(string_pool.num_strings(), 1);
        assert_eq!(string_pool.get(StringRef(1)), "rustmsi".repeat(10_000));
        assert_eq!(string_pool.refcount(StringRef(1)), 1);
    }

    #[test]
    fn roundtrip_string_over_64k() {
        // Create a 70,000-byte string by repeating "rustmsi" 10,000 times
        let rustmsi_70kb = "rustmsi".repeat(10_000);
        assert!(rustmsi_70kb.len() > u16::MAX as usize); // Ensure it's > 64 KB

        // Construct a string pool and add the long string
        let mut pool = StringPool::new(CodePage::Utf8);
        pool.incref(rustmsi_70kb.clone());

        // Serialize the pool into binary format
        let mut pool_output = Vec::new();
        pool.write_pool(&mut pool_output)
            .expect("Failed to write string pool");

        // Deserialize the pool header from the binary output
        let builder = StringPoolBuilder::read_from_pool(&*pool_output)
            .expect("Failed to read string pool metadata");

        // Rebuild the pool from the original string data
        let string_pool = builder
            .build_from_data(rustmsi_70kb.as_bytes())
            .expect("Failed to build string pool from data");

        // Verify the deserialized string and its refcount
        assert_eq!(string_pool.get(StringRef(1)), rustmsi_70kb);
        assert_eq!(string_pool.refcount(StringRef(1)), 1);
    }

    #[test]
    fn max_refcount() {
        let pool: &[u8] = b"\xe9\xfd\x00\x00\x06\x00\xfe\xff";
        let data: &[u8] = b"Foobar";
        let builder = StringPoolBuilder::read_from_pool(pool).expect("pool");
        let mut string_pool = builder.build_from_data(data).expect("data");
        assert_eq!(string_pool.num_strings(), 1);
        assert_eq!(string_pool.get(StringRef(1)), "Foobar");
        assert_eq!(string_pool.refcount(StringRef(1)), 0xfffe);
        assert_eq!(string_pool.incref("Foobar".to_string()), StringRef(1));
        assert_eq!(string_pool.num_strings(), 1);
        assert_eq!(string_pool.incref("Foobar".to_string()), StringRef(2));
        assert_eq!(string_pool.num_strings(), 2);
        assert_eq!(string_pool.refcount(StringRef(1)), 0xffff);
        assert_eq!(string_pool.refcount(StringRef(2)), 1);
    }

    #[test]
    fn reuse_entries() {
        let mut string_pool = StringPool::new(CodePage::default());
        assert_eq!(string_pool.incref("Foo".to_string()), StringRef(1));
        assert_eq!(string_pool.incref("Bar".to_string()), StringRef(2));
        assert_eq!(string_pool.num_strings(), 2);
        string_pool.decref(StringRef(1));
        assert_eq!(string_pool.refcount(StringRef(1)), 0);
        assert_eq!(string_pool.get(StringRef(1)), "");
        assert_eq!(string_pool.num_strings(), 2);
        assert_eq!(string_pool.incref("Quux".to_string()), StringRef(1));
        assert_eq!(string_pool.num_strings(), 2);
        assert_eq!(string_pool.refcount(StringRef(1)), 1);
        assert_eq!(string_pool.get(StringRef(1)), "Quux");
    }

    #[test]
    fn free_entry_before_matching_entry_is_reused_first() {
        let mut string_pool = StringPool::new(CodePage::default());
        assert_eq!(string_pool.incref("Foo".to_string()), StringRef(1));
        assert_eq!(string_pool.incref("Bar".to_string()), StringRef(2));
        string_pool.decref(StringRef(1));
        // "Bar" is at 2, but the free entry at 1 comes first, as in a scan.
        assert_eq!(string_pool.incref("Bar".to_string()), StringRef(1));
        assert_eq!(string_pool.incref("Bar".to_string()), StringRef(1));
        assert_eq!(string_pool.num_strings(), 2);
        assert_eq!(string_pool.refcount(StringRef(1)), 2);
        assert_eq!(string_pool.refcount(StringRef(2)), 1);
        string_pool.decref(StringRef(2));
        assert_eq!(string_pool.get(StringRef(2)), "");
        assert_eq!(string_pool.incref("Foo".to_string()), StringRef(2));
    }

    #[test]
    fn counts_the_entries_that_a_query_may_add() {
        let pool: &[u8] = b"\xe9\xfd\x00\x00\x06\x00\xfe\xff";
        let data: &[u8] = b"Foobar";
        let builder = StringPoolBuilder::read_from_pool(pool).expect("pool");
        let string_pool = builder.build_from_data(data).expect("data");
        // "Foobar" has room for one more reference. A new string needs one
        // entry, however often it is used, until its refcount is full.
        assert_eq!(string_pool.max_new_entries([("Foobar", 1)]), 0);
        assert_eq!(string_pool.max_new_entries([("Foobar", 2)]), 1);
        assert_eq!(string_pool.max_new_entries([("Other", 65_535)]), 1);
        assert_eq!(string_pool.max_new_entries([("Other", 65_536)]), 2);
        assert_eq!(
            string_pool.max_new_entries([("Foobar", 1), ("Other", 3)]),
            1
        );
        assert!(string_pool.fits_short_refs(65_534));
        assert!(!string_pool.fits_short_refs(65_535));
    }

    #[test]
    #[should_panic(expected = "Unknown codepage for string pool (123456)")]
    fn invalid_codepage() {
        let pool: &[u8] = b"\x40\xe2\x01\x00\x06\x00\x01\x00";
        StringPoolBuilder::read_from_pool(pool).unwrap();
    }
}

// ========================================================================= //
