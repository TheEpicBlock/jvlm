use std::{io::Write, io};
use bytebuffer::ByteBuffer;
use byteorder::WriteBytesExt;
use constant_pool::{ConstantPool, ConstantPoolReference};
use descriptor::DescriptorEntry;

mod constant_pool;
pub(crate) mod descriptor;

pub struct ClassFileWriter<W: Write> {
    output: W,
    access_flag: u16,
    this_ref: ConstantPoolReference,
    super_ref: ConstantPoolReference,
    constant_pool: ConstantPool,
    methods: Vec<MethodData>,
}

impl <W> ClassFileWriter<W> where W: Write + WriteBytesExt {
    pub fn write_classfile(output_stream: W, metadata: ClassMetadata) -> io::Result<Self> {
        let mut constant_pool = ConstantPool::default();
        let access_flag = metadata.get_access_flag();
        let this_ref = constant_pool.class(metadata.this_class);
        let super_ref = constant_pool.class(metadata.super_class);

        let mut w = ClassFileWriter {
            output: output_stream,
            constant_pool,
            access_flag,
            this_ref,
            super_ref,
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
        let name_ref = self.constant_pool.utf8(metadata.name);
        let desc_ref = self.constant_pool.utf8(metadata.descriptor);

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
        let code_attr = self.constant_pool.utf8("Code".to_string());
        self.constant_pool.serialize(&mut self.output).unwrap();
        self.output.write_u16::<byteorder::BigEndian>(self.access_flag).unwrap();
        self.output.write_u16::<byteorder::BigEndian>(self.this_ref).unwrap();
        self.output.write_u16::<byteorder::BigEndian>(self.super_ref).unwrap();
        self.output.write_u16::<byteorder::BigEndian>(0).unwrap(); // No interfaces
        self.output.write_u16::<byteorder::BigEndian>(0).unwrap(); // No fields
        self.output.write_u16::<byteorder::BigEndian>(self.methods.len() as u16).unwrap();
        for m in &self.methods {
            m.serialize(&mut self.output, code_attr).unwrap();
        }
        self.output.write_u16::<byteorder::BigEndian>(0).unwrap(); // No attributes
    }
}

pub struct ClassMetadata {
    pub is_public: bool,
    pub is_final: bool,
    pub is_interface: bool,
    pub is_abstract: bool,
    pub is_synthetic: bool,
    pub is_annotation: bool,
    pub is_enum: bool,
    pub is_module: bool,
    pub this_class: String,
    pub super_class: String,
}

impl ClassMetadata {
    fn get_access_flag(&self) -> u16 {
        let mut flag = 0;
        if self.is_public     { flag |= 0x0001 };
        if self.is_final      { flag |= 0x0010 };
        flag |= 0x0020; // is_super
        if self.is_interface  { flag |= 0x0200 };
        if self.is_abstract   { flag |= 0x0400 };
        if self.is_synthetic  { flag |= 0x1000 };
        if self.is_annotation { flag |= 0x2000 };
        if self.is_enum       { flag |= 0x4000 };
        if self.is_module     { flag |= 0x8000 };
        return flag;
    }
}

pub struct MethodWriter<'class_writer, W: Write> {
    class_writer: &'class_writer mut ClassFileWriter<W>,
    method_index: usize,
}

impl <W> MethodWriter<'_, W> where W: Write {
    fn code(&mut self) -> &mut ByteBuffer {
        return &mut self.class_writer.methods[self.method_index].code;
    }

    fn emit_opcode_referencing_local_var(&mut self, opcode: u8, index: u16) {
        if let Some(index) = u8::try_from(index).ok() {
            self.code().write_u8(opcode);
            self.code().write_u8(index);
        } else {
            // the local variable index doesn't fit into a byte, need to use a wide 
            self.code().write_u8(0xC4); // The WIDE opcode
            self.code().write_u8(opcode);
            self.code().write_u16(index);
        }
    }

    fn emit_load_store_inner(&mut self, shorthand: u8, long_form: u8, index: u16) {
        match index {
            0 => self.code().write_u8(shorthand),
            1 => self.code().write_u8(shorthand+1),
            2 => self.code().write_u8(shorthand+2),
            3 => self.code().write_u8(shorthand+3),
            _ => self.emit_opcode_referencing_local_var(long_form, index),
        }
    }

    pub fn emit_load(&mut self, ty: JavaType, index: u16) {
        match ty {
            JavaType::Int => self.emit_load_store_inner(0x1a, 0x15, index),
            JavaType::Long => self.emit_load_store_inner(0x1e, 0x16, index),
            JavaType::Float => self.emit_load_store_inner(0x22, 0x17, index),
            JavaType::Double => self.emit_load_store_inner(0x26, 0x18, index),
            JavaType::Reference => self.emit_load_store_inner(0x2a, 0x19, index),
        }
    }

    pub fn emit_store(&mut self, ty: JavaType, index: u16) {
        match ty {
            JavaType::Int => self.emit_load_store_inner(0x3b, 0x36, index),
            // JavaType::Long => self.emit_load_inner(0x1e, 0x16, index),
            // JavaType::Float => self.emit_load_inner(0x22, 0x17, index),
            // JavaType::Double => self.emit_load_inner(0x26, 0x18, index),
            // JavaType::Reference => self.emit_load_inner(0x2a, 0x19, index),
            _ => todo!()
        }
    }

    pub fn emit_constant_int(&mut self, integer: i32) {
        let r = self.class_writer.constant_pool.int(integer);
        if let Some(r) = u8::try_from(r).ok() {
            self.code().write_u8(0x12); //LDC
            self.code().write_u8(r);
        } else {
            self.code().write_u8(0x13); //LDC_w
            self.code().write_u16(r);
        }
    }

    pub fn emit_return(&mut self, ty: Option<JavaType>) {
        match ty {
            Some(ty) => match ty {
                JavaType::Int => self.code().write_u8(0xac),
                JavaType::Long => self.code().write_u8(0xad),
                JavaType::Float => self.code().write_u8(0xae),
                JavaType::Double => self.code().write_u8(0xaf),
                JavaType::Reference => self.code().write_u8(0xb0),
            },
            None => self.code().write_u8(0xb1), // Void return
        }
    }

    pub fn emit_dup(&mut self) {
        self.code().write_u8(0x59);
    }

    pub fn emit_add(&mut self, ty: JavaType) {
        match ty {
            JavaType::Int => self.code().write_u8(0x60),
            JavaType::Long => todo!(),
            JavaType::Float => todo!(),
            JavaType::Double => todo!(),
            JavaType::Reference => todo!(),
        }
    }

    pub fn emit_mul(&mut self, ty: JavaType) {
        match ty {
            JavaType::Int => self.code().write_u8(0x68),
            JavaType::Long => todo!(),
            JavaType::Float => todo!(),
            JavaType::Double => todo!(),
            JavaType::Reference => todo!(),
        }
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
        writer.write_u32::<byteorder::BigEndian>(self.code.as_bytes().len() as u32 + 12)?;
        writer.write_u16::<byteorder::BigEndian>(50)?; // TODO: max-stack
        writer.write_u16::<byteorder::BigEndian>(50)?; // TODO: max-locals
        writer.write_u32::<byteorder::BigEndian>(self.code.as_bytes().len() as u32)?;
        writer.write_all(self.code.as_bytes())?;
        writer.write_u16::<byteorder::BigEndian>(0)?; // No exception table
        writer.write_u16::<byteorder::BigEndian>(0)?; // No additional attributes
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

/// Represents a type inside of the java stack or the java local variable table
pub enum JavaType {
    Int,
    Long,
    Float,
    Double,
    Reference
}

impl From<DescriptorEntry> for JavaType {
    fn from(value: DescriptorEntry) -> Self {
        match value {
            DescriptorEntry::Byte => JavaType::Int,
            DescriptorEntry::Char => JavaType::Int,
            DescriptorEntry::Double => JavaType::Double,
            DescriptorEntry::Float => JavaType::Float,
            DescriptorEntry::Int => JavaType::Int,
            DescriptorEntry::Long => JavaType::Long,
            DescriptorEntry::Class(_) => JavaType::Reference,
            DescriptorEntry::Short => JavaType::Int,
            DescriptorEntry::Boolean => JavaType::Int,
            DescriptorEntry::Array(_) => JavaType::Reference,
        }
    }
}

impl JavaType {
    pub fn desc(&self) -> &'static str {
        // TODO this needs to be changed
        match self {
            JavaType::Int => "I",
            JavaType::Long => "J",
            JavaType::Float => "F",
            JavaType::Double => "D",
            JavaType::Reference => panic!(),
        }
    }
}