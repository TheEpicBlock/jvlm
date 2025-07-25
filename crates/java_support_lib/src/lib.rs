use std::io::{Seek, Write};

use zip::{result::ZipResult, write::SimpleFileOptions, DateTime, ZipWriter};


pub struct BuiltinJavaCode {
    /// The name of the class (stored as a binary internal name, eg `java/lang/Thread`)
    pub name: &'static str,
    /// The bytes forming the classfile
    pub class: &'static [u8],
}

impl BuiltinJavaCode {
    pub fn write_to_zip(&self, zip: &mut ZipWriter<impl Write+Seek>) -> ZipResult<()> {
        let name = self.name;
        zip.start_file(format!("{name}.class"), SimpleFileOptions::default().last_modified_time(DateTime::default()))?;
        zip.write_all(self.class)?;

        Ok(())
    }
}

include!(concat!(env!("OUT_DIR"), "/java_compiled.rs"));
