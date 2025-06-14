use std::{io::Write, io};
use bytebuffer::ByteBuffer;
use byteorder::WriteBytesExt;
use constant_pool::{ConstantPool, ConstantPoolReference};

mod constant_pool;

pub struct ClassFileWriter<W: Write> {
    output: W,
    constant_pool: ConstantPool,
    methods: Vec<MethodData>,
}

impl <W> ClassFileWriter<W> where W: Write + WriteBytesExt {
    pub fn write_classfile(output_stream: W) -> io::Result<Self> {
        let mut w = ClassFileWriter {
            output: output_stream,
            constant_pool: ConstantPool::default(),
            methods: Vec::new()
        };
        w.write_header()?;
        return Ok(w);
    }

    fn write_header(&mut self) -> io::Result<()> {
        self.output.write_u32::<byteorder::BigEndian>(0xCAFEBABE)?;
        self.output.write_u16::<byteorder::BigEndian>(0)?;  // Minor version
        self.output.write_u16::<byteorder::BigEndian>(65)?; // Major version: JAVA 21
        Ok(())
    }

    pub fn write_method(&mut self, metadata: MethodMetadata) -> MethodWriter<'_, W> {
        let method_index = self.methods.len();

        let access_flag = metadata.get_access_flag();
        let name_ref = self.constant_pool.write_str(metadata.name);
        let desc_ref = self.constant_pool.write_str(metadata.descriptor);

        self.methods.push(MethodData {
            access_flag,
            name_ref,
            desc_ref,
            code: ByteBuffer::new()
        });
        
        return MethodWriter {
            class_writer: self,
            method_index
        };
    }
}

impl <W> Drop for ClassFileWriter<W> where W: Write {
    fn drop(&mut self) {
        let code_attr = self.constant_pool.write_str("Code".to_string());
        self.constant_pool.serialize(&mut self.output).unwrap();
        self.output.write_u16::<byteorder::BigEndian>(0).unwrap(); // TODO access flags
        self.output.write_u16::<byteorder::BigEndian>(0).unwrap(); // TODO this class
        self.output.write_u16::<byteorder::BigEndian>(0).unwrap(); // TODO super class
        self.output.write_u16::<byteorder::BigEndian>(0).unwrap(); // No interfaces
        self.output.write_u16::<byteorder::BigEndian>(0).unwrap(); // No fields
        self.output.write_u16::<byteorder::BigEndian>(self.methods.len() as u16).unwrap();
        for m in &self.methods {
            m.serialize(&mut self.output, code_attr).unwrap();
        }
        self.output.write_u16::<byteorder::BigEndian>(0).unwrap(); // No attributes
    }
}

pub struct MethodWriter<'class_writer, W: Write> {
    class_writer: &'class_writer mut ClassFileWriter<W>,
    method_index: usize,
}

impl <W> MethodWriter<'_, W> where W: Write {
}

pub struct MethodMetadata {
    pub visibility: Visibility,
    pub is_static: bool,
    pub is_final: bool,
    pub is_synchronized: bool,
    pub is_bridge: bool,
    pub is_varargs: bool,
    pub is_native: bool,
    pub is_abstract: bool,
    pub is_strictfp: bool,
    pub is_synthetic: bool,
    pub name: String,
    pub descriptor: String,
}

pub struct MethodData {
    access_flag: u16,
    name_ref: ConstantPoolReference,
    desc_ref: ConstantPoolReference,
    code: ByteBuffer,
}

impl MethodData {
    fn serialize(&self, writer: &mut impl Write, code_attribute: ConstantPoolReference) -> io::Result<()> {
        writer.write_u16::<byteorder::BigEndian>(self.access_flag)?;
        writer.write_u16::<byteorder::BigEndian>(self.name_ref)?;
        writer.write_u16::<byteorder::BigEndian>(self.desc_ref)?;
        writer.write_u16::<byteorder::BigEndian>(1)?; // One attribute
        writer.write_u16::<byteorder::BigEndian>(code_attribute)?;
        writer.write_u32::<byteorder::BigEndian>(self.code.as_bytes().len() as u32)?;
        writer.write_all(self.code.as_bytes())?;
        Ok(())
    }
}

impl MethodMetadata {
    fn get_access_flag(&self) -> u16 {
        let mut flag = match self.visibility {
            Visibility::PUBLIC => 0x0001,
            Visibility::PRIVATE => 0x0002,
            Visibility::PROTECTED => 0x0004,
        };
        if self.is_static       { flag |= 0x0008; }
        if self.is_final        { flag |= 0x0010; }
        if self.is_synchronized { flag |= 0x0020; }
        if self.is_bridge       { flag |= 0x0040; }
        if self.is_varargs      { flag |= 0x0080; }
        if self.is_native       { flag |= 0x0100; }
        if self.is_abstract     { flag |= 0x0400; }
        if self.is_strictfp     { flag |= 0x0800; }
        if self.is_synthetic    { flag |= 0x1000; }
        return flag;
    }
}

pub enum Visibility {
    PUBLIC,
    PRIVATE,
    PROTECTED,
}