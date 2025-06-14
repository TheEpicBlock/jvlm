use std::{fmt::Write, io};
use bytebuffer::ByteBuffer;
use byteorder::WriteBytesExt;
use constant_pool::ConstantPool;

mod constant_pool;

pub struct ClassFileWriter<W: Write> {
    output: W,
    constant_pool: ConstantPool,
    method_count: u32,
    methods: ByteBuffer,
}

impl <W> ClassFileWriter<W> where W: Write + WriteBytesExt {
    pub fn write_classfile(output_stream: W) -> io::Result<Self> {
        let mut w = ClassFileWriter {
            output: output_stream,
            constant_pool: ConstantPool::default(),
            method_count: 0,
            methods: ByteBuffer::new()
        };
        w.methods.set_endian(bytebuffer::Endian::BigEndian);
        w.write_header()?;
        return Ok(w);
    }

    fn write_header(&mut self) -> io::Result<()> {
        self.output.write_u64::<byteorder::BigEndian>(0xCAFEBABE)?;
        self.output.write_u32::<byteorder::BigEndian>(0)?;  // Minor version
        self.output.write_u32::<byteorder::BigEndian>(65)?; // Major version: JAVA 21
        Ok(())
    }

    fn write_method(&mut self, metadata: MethodMetadata) -> MethodWriter<'_, W> {
        let mut m = MethodWriter {
            class_writer: self
        };
        m.write_header(metadata);
        return m;
    }
}

pub struct MethodWriter<'class_writer, W: Write> {
    class_writer: &'class_writer mut ClassFileWriter<W>
}

impl <W> MethodWriter<'_, W> where W: Write {
    fn write_header(&mut self, metadata: MethodMetadata) {
        self.class_writer.methods.write_u32(metadata.get_access_flag());
        let name_ref = self.class_writer.constant_pool.write_str(metadata.name);
        self.class_writer.methods.write_u32(name_ref);
        let desc_ref = self.class_writer.constant_pool.write_str(metadata.descriptor);
        self.class_writer.methods.write_u32(desc_ref);
    }
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

impl MethodMetadata {
    fn get_access_flag(&self) -> u32 {
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