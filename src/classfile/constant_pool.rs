use indexmap::IndexSet;

type ConstantPoolReference = u32;

#[derive(Default)]
pub struct ConstantPool {
    entries: IndexSet<ConstantPoolEntry>
}

impl ConstantPool {
    pub fn write_str(&mut self, str: String) -> ConstantPoolReference {
        self.entries.insert_full(ConstantPoolEntry::String(str)).0 as ConstantPoolReference
    }
}

#[derive(Hash, PartialEq, Eq)]
enum ConstantPoolEntry {
    String(String)
}