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
    pub fn name_and_type(&mut self, name: String, descriptor: String) -> ConstantPoolReference {
        let name = self.utf8(name);
        let descriptor = self.utf8(descriptor);
        self.entries.insert_full(ConstantPoolEntry::NameAndType { name, descriptor }).0 as ConstantPoolReference + 1
    }
    pub fn fieldref(&mut self, class: String, name: String, descriptor: String) -> ConstantPoolReference {
        let class = self.utf8(class);
        let name_and_type = self.name_and_type(name, descriptor);
        self.entries.insert_full(ConstantPoolEntry::FieldRef { class, name_and_type }).0 as ConstantPoolReference + 1
    }
    pub fn methodref(&mut self, class: String, name: String, descriptor: String) -> ConstantPoolReference {
        let class = self.utf8(class);
        let name_and_type = self.name_and_type(name, descriptor);
        self.entries.insert_full(ConstantPoolEntry::MethodRef { class, name_and_type }).0 as ConstantPoolReference + 1
    }
    pub fn interfacemethodref(&mut self, class: String, name: String, descriptor: String) -> ConstantPoolReference {
        let class = self.utf8(class);
        let name_and_type = self.name_and_type(name, descriptor);
        self.entries.insert_full(ConstantPoolEntry::InterfaceMethodRef { class, name_and_type }).0 as ConstantPoolReference + 1
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
                },
                ConstantPoolEntry::FieldRef { class, name_and_type } => {
                    output.write_u8(9)?;
                    output.write_u16::<byteorder::BigEndian>(*class)?;
                    output.write_u16::<byteorder::BigEndian>(*name_and_type)?;
                },
                ConstantPoolEntry::MethodRef { class, name_and_type } => {
                    output.write_u8(10)?;
                    output.write_u16::<byteorder::BigEndian>(*class)?;
                    output.write_u16::<byteorder::BigEndian>(*name_and_type)?;
                },
                ConstantPoolEntry::InterfaceMethodRef { class, name_and_type } => {
                    output.write_u8(11)?;
                    output.write_u16::<byteorder::BigEndian>(*class)?;
                    output.write_u16::<byteorder::BigEndian>(*name_and_type)?;
                },
                ConstantPoolEntry::NameAndType { name, descriptor } => {
                    output.write_u8(12)?;
                    output.write_u16::<byteorder::BigEndian>(*name)?;
                    output.write_u16::<byteorder::BigEndian>(*descriptor)?;
                },
            }
        }
        Ok(())
    }
}

#[derive(Hash, PartialEq, Eq)]
enum ConstantPoolEntry {
    Class(ConstantPoolReference),
    // TODO proper java-like encoding of strings
    Utf8(String),
    Int(i32),
    FieldRef { class: ConstantPoolReference, name_and_type: ConstantPoolReference },
    MethodRef { class: ConstantPoolReference, name_and_type: ConstantPoolReference },
    InterfaceMethodRef { class: ConstantPoolReference, name_and_type: ConstantPoolReference },
    NameAndType { name: ConstantPoolReference, descriptor: ConstantPoolReference },
}