use std::io::{self, Write};
use byteorder::WriteBytesExt;

use indexmap::IndexSet;

pub type ConstantPoolReference = u16;

#[derive(Default)]
pub struct ConstantPool {
    entries: IndexSet<ConstantPoolEntry>
}

impl ConstantPool {
    pub fn utf8(&mut self, str: String) -> ConstantPoolReference {
        self.entries.insert_full(ConstantPoolEntry::Utf8(str)).0 as ConstantPoolReference + 1
    }
    pub fn class(&mut self, str: String) -> ConstantPoolReference {
        let utf8 = self.utf8(str);
        self.entries.insert_full(ConstantPoolEntry::Class(utf8)).0 as ConstantPoolReference + 1
    }
    pub fn int(&mut self, int: i32) -> ConstantPoolReference {
        self.entries.insert_full(ConstantPoolEntry::Int(int)).0 as ConstantPoolReference + 1
    }

    pub fn serialize(&self, output: &mut impl Write) -> io::Result<()> {
        output.write_u16::<byteorder::BigEndian>(self.entries.len() as u16 + 1)?;
        for entry in &self.entries {
            match entry {
                ConstantPoolEntry::Utf8(str) => {
                    output.write_u8(1)?;
                    output.write_u16::<byteorder::BigEndian>(str.as_bytes().len() as u16)?;
                    output.write_all(str.as_bytes())?;
                },
                ConstantPoolEntry::Class(v) => {
                    output.write_u8(7)?;
                    output.write_u16::<byteorder::BigEndian>(*v)?;
                },
                ConstantPoolEntry::Int(i) => {
                    output.write_u8(3)?;
                    output.write_i32::<byteorder::BigEndian>(*i)?;
                }
            }
        }
        Ok(())
    }
}

#[derive(Hash, PartialEq, Eq)]
enum ConstantPoolEntry {
    Class(ConstantPoolReference),
    Utf8(String),
    Int(i32),
}