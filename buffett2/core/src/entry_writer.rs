use crate::tx_vault::Bank;
use bincode;
use crate::entry::Entry;
use std::io::{self, BufRead, Error, ErrorKind, Write};
use std::mem::size_of;

pub struct EntryWriter<'a, W> {
    bank: &'a Bank,
    writer: W,
}

impl<'a, W: Write> EntryWriter<'a, W> {
    
    pub fn new(bank: &'a Bank, writer: W) -> Self {
        EntryWriter { bank, writer }
    }

    fn write_entry(writer: &mut W, entry: &Entry) -> io::Result<()> {
        let entry_bytes =
            bincode::serialize(&entry).map_err(|e| Error::new(ErrorKind::Other, e.to_string()))?;

        let len = entry_bytes.len();
        let len_bytes =
            bincode::serialize(&len).map_err(|e| Error::new(ErrorKind::Other, e.to_string()))?;

        writer.write_all(&len_bytes[..])?;
        writer.write_all(&entry_bytes[..])?;
        writer.flush()
    }

    pub fn write_entries<I>(writer: &mut W, entries: I) -> io::Result<()>
    where
        I: IntoIterator<Item = Entry>,
    {
        for entry in entries {
            Self::write_entry(writer, &entry)?;
        }
        Ok(())
    }

    fn write_and_register_entry(&mut self, entry: &Entry) -> io::Result<()> {
        trace!("write_and_register_entry entry");
        self.bank.register_entry_id(&entry.id);

        Self::write_entry(&mut self.writer, entry)
    }

    pub fn write_and_register_entries(&mut self, entries: &[Entry]) -> io::Result<()> {
        for entry in entries {
            self.write_and_register_entry(&entry)?;
        }
        Ok(())
    }
}

struct EntryReader<R: BufRead> {
    reader: R,
    entry_bytes: Vec<u8>,
}

impl<R: BufRead> Iterator for EntryReader<R> {
    type Item = io::Result<Entry>;

    fn next(&mut self) -> Option<io::Result<Entry>> {
        let mut entry_len_bytes = [0u8; size_of::<usize>()];

        if self.reader.read_exact(&mut entry_len_bytes[..]).is_ok() {
            let entry_len = bincode::deserialize(&entry_len_bytes).unwrap();

            if entry_len > self.entry_bytes.len() {
                self.entry_bytes.resize(entry_len, 0);
            }

            if let Err(e) = self.reader.read_exact(&mut self.entry_bytes[..entry_len]) {
                Some(Err(e))
            } else {
                Some(
                    bincode::deserialize(&self.entry_bytes)
                        .map_err(|e| Error::new(ErrorKind::Other, e.to_string())),
                )
            }
        } else {
            None 
        }
    }
}


pub fn read_entries<R: BufRead>(reader: R) -> impl Iterator<Item = io::Result<Entry>> {
    EntryReader {
        reader,
        entry_bytes: Vec::new(),
    }
}

