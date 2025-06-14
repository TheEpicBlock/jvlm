use std::io::{self, Write};
use byteorder::WriteBytesExt;

use indexmap::IndexSet;

pub type ConstantPoolReference = u16;

#[derive(Default)]
pub struct ConstantPool {
    entries: IndexSet<ConstantPoolEntry>
}

impl ConstantPool {
    pub fn write_str(&mut self, str: String) -> ConstantPoolReference {
        self.entries.insert_full(ConstantPoolEntry::String(str)).0 as ConstantPoolReference
    }

    pub fn serialize(&self, output: &mut impl Write) -> io::Result<()> {
        output.write_u16::<byteorder::BigEndian>(self.entries.len() as u16 + 1)?;
        for entry in &self.entries {
            match entry {
                ConstantPoolEntry::String(str) => {
                    output.write_u8(1)?;
                    output.write_u16::<byteorder::BigEndian>(str.as_bytes().len() as u16)?;
                    output.write_all(str.as_bytes())?;
                },
            }
        }
        Ok(())
    }
}

#[derive(Hash, PartialEq, Eq)]
enum ConstantPoolEntry {
    String(String)
}