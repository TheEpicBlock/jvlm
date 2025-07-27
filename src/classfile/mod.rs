use std::{collections::BTreeMap, io::{self, Write}};
use bytebuffer::ByteBuffer;
use byteorder::WriteBytesExt;
use constant_pool::{ConstantPool, ConstantPoolReference};
use descriptor::{DescriptorEntry, FieldDescriptor, MethodDescriptor};

use crate::java_types::{JInt, JLong};

mod constant_pool;
pub(crate) mod descriptor;

/// Index into the local variable table
pub type LVTi = u16;

pub struct ClassFileWriter<W: Write> {
    output: Option<W>,
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
            output: Some(output_stream),
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
        let out = self.output.as_mut().unwrap();
        out.write_u32::<byteorder::BigEndian>(0xCAFEBABE)?;
        out.write_u16::<byteorder::BigEndian>(0)?;  // Minor version
        out.write_u16::<byteorder::BigEndian>(65)?; // Major version: JAVA 21
        Ok(())
    }

    pub fn write_method(&mut self, metadata: MethodMetadata) -> MethodWriter<'_, W> {
        let method_index = self.methods.len();

        let access_flag = metadata.get_access_flag();
        let name_ref = self.constant_pool.utf8(metadata.name);
        let desc_ref = self.constant_pool.utf8(metadata.descriptor.to_string());

        let initial_frame = calculate_initial_stackframe(&metadata.descriptor);

        self.methods.push(MethodData {
            access_flag,
            name_ref,
            desc_ref,
            descriptor: metadata.descriptor,
            code: ByteBuffer::new(),
            max_stack_size: initial_frame.stack.len(),
            stackmaptable: Default::default(),
        });

        return MethodWriter {
            class_writer: self,
            method_index,
            current_frame: initial_frame,
        };
    }

    pub fn finalize(mut self) -> W {
        let code_attr = self.constant_pool.utf8("Code".into());
        let stack_map_table_attr = self.methods.iter()
            .any(|m| m.needs_stack_map_table())
            .then(|| self.constant_pool.utf8("StackMapTable".into()));
        let out = self.output.as_mut().unwrap();
        self.constant_pool.serialize(out).unwrap();
        out.write_u16::<byteorder::BigEndian>(self.access_flag).unwrap();
        out.write_u16::<byteorder::BigEndian>(self.this_ref).unwrap();
        out.write_u16::<byteorder::BigEndian>(self.super_ref).unwrap();
        out.write_u16::<byteorder::BigEndian>(0).unwrap(); // No interfaces
        out.write_u16::<byteorder::BigEndian>(0).unwrap(); // No fields
        out.write_u16::<byteorder::BigEndian>(self.methods.len() as u16).unwrap();
        for m in &self.methods {
            m.serialize(out, code_attr, stack_map_table_attr).unwrap();
        }
        out.write_u16::<byteorder::BigEndian>(0).unwrap(); // No attributes

        let w = std::mem::take(&mut self.output).expect("Already finalized");
        return w;
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
    current_frame: StackMapFrame,
}

impl <W> MethodWriter<'_, W> where W: Write {
    fn code(&mut self) -> &mut ByteBuffer {
        return &mut self.class_writer.methods[self.method_index].code;
    }
    fn code_immutable(&self) -> &ByteBuffer {
        return &self.class_writer.methods[self.method_index].code;
    }

    /// Record that we emitted an instruction that pushes something to the stack.
    /// 
    /// We keep track of what the stack and local variable table look like. To do this,
    /// we keep a virtual stack.
    fn record_push(&mut self, t: VerificationType) {
        self.current_frame.stack.push(t);
        let max_stack_size = &mut self.class_writer.methods[self.method_index].max_stack_size;
        if self.current_frame.stack.len() > *max_stack_size {
            *max_stack_size = self.current_frame.stack.len();
        }
    }
    fn record_push_ty(&mut self, t: JavaType) {
        match t {
            JavaType::Int => self.record_push(VerificationType::Integer),
            JavaType::Long => self.record_push(VerificationType::Long),
            // TODO hrmm, how to handle these
            JavaType::Float => self.record_push(VerificationType::Float),
            JavaType::Double => self.record_push(VerificationType::Double),
            // TODO oh no, we should probably keep better track of this
            JavaType::Reference => self.record_push(VerificationType::Object("java/lang/Object".to_owned())),
        }
    }

    /// Record that we emitted an instruction that pops something from the stack.
    /// 
    /// We keep track of what the stack and local variable table look like. To do this,
    /// we keep a virtual stack.
    fn record_pop(&mut self) -> VerificationType {
        self.current_frame.stack.pop().unwrap()
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
        self.record_push_ty(ty);
    }

    pub fn emit_store(&mut self, ty: JavaType, index: u16) {
        match ty {
            JavaType::Int => self.emit_load_store_inner(0x3b, 0x36, index),
            JavaType::Long => self.emit_load_store_inner(0x3f, 0x37, index),
            JavaType::Float => self.emit_load_store_inner(0x43, 0x38, index),
            JavaType::Double => self.emit_load_store_inner(0x47, 0x39, index),
            JavaType::Reference => self.emit_load_store_inner(0x4b, 0x40, index),
        }
        self.record_pop();
    }

    pub fn emit_constant_int(&mut self, integer: JInt) {
        let r = self.class_writer.constant_pool.int(integer);
        if let Some(r) = u8::try_from(r).ok() {
            self.code().write_u8(0x12); //LDC
            self.code().write_u8(r);
        } else {
            self.code().write_u8(0x13); //LDC_w
            self.code().write_u16(r);
        }
        self.record_push(VerificationType::Integer);
    }

    pub fn emit_constant_long(&mut self, long: JLong) {
        if let Some(i) = JInt::try_from(long).ok() {
            self.emit_constant_int(i);
            self.emit_i2l();
        } else {
            todo!()
        }
        // let r = self.class_writer.constant_pool.int(long);
        // if let Some(r) = u8::try_from(r).ok() {
        //     self.code().write_u8(0x12); //LDC
        //     self.code().write_u8(r);
        // } else {
        //     self.code().write_u8(0x13); //LDC_w
        //     self.code().write_u16(r);
        // }
        // self.record_push(VerificationType::Integer);
    }

    pub fn emit_i2l(&mut self) {
        self.record_pop();
        self.record_push(VerificationType::Long);
        self.code().write_u8(0x85);
    }

    pub fn emit_return(&mut self, ty: Option<JavaType>) {
        if ty.is_some() {
            self.record_pop();
        }
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

    pub fn emit_iinc(&mut self, local_variable: LVTi, constant: i16) {
        if let Some(local_variable) = u8::try_from(local_variable).ok() && 
                let Some(constant) = i8::try_from(constant).ok() {
            // Both variables fit in a single byte, write the normal version of IINC
            self.code().write_u8(0x84); // IINC
            self.code().write_u8(local_variable);
            self.code().write_i8(constant);
        } else {
            // One of the two is too big, use a WIDE
            self.code().write_u8(0xC4); // WIDE
            self.code().write_u8(0x84); // IINC
            self.code().write_u16(local_variable);
            self.code().write_i16(constant);
        }
    }

    pub fn emit_dup(&mut self) {
        self.code().write_u8(0x59);
        
        let p = self.record_pop();
        self.record_push(p.clone());
        self.record_push(p);
    }

    pub fn emit_add(&mut self, ty: JavaType) {
        match ty {
            JavaType::Int => self.code().write_u8(0x60),
            JavaType::Long => todo!(),
            JavaType::Float => todo!(),
            JavaType::Double => todo!(),
            JavaType::Reference => todo!(),
        }
        self.record_pop();
        self.record_pop();
        self.record_push_ty(ty);
    }

    pub fn emit_mul(&mut self, ty: JavaType) {
        match ty {
            JavaType::Int => self.code().write_u8(0x68),
            JavaType::Long => todo!(),
            JavaType::Float => todo!(),
            JavaType::Double => todo!(),
            JavaType::Reference => todo!(),
        }
        self.record_pop();
        self.record_pop();
        self.record_push_ty(ty);
    }

    #[must_use]
    pub fn emit_goto(&mut self) -> InstructionTarget {
        let i = self.current_location();
        // There is a wide variant for goto, but our code is architectured in a way that
        // assumes the target has a constant bit width. Luckily, there are other factors limiting
        // the size of a java method to 2^16 bytes, so the wide variant goes unused.
        self.code().write_u8(0xA7); // GOTO
        let j = self.code_immutable().get_wpos();
        self.code().write_u16(0xFEFE); // Temporary branch target
        return InstructionTarget {
            instruction_location: i,
            jump_location: j
        };
    }

    #[must_use]
    pub fn emit_if_icmp(&mut self, ty: ComparisonType) -> InstructionTarget {
        self.record_pop();
        self.record_pop();

        let i = self.current_location();
        // There is a wide variant for goto, but our code is architectured in a way that
        // assumes the target has a constant bit width. Luckily, there are other factors limiting
        // the size of a java method to 2^16 bytes, so the wide variant goes unused.
        self.code().write_u8(0x9F + (ty as u8)); // opcode
        let j = self.code_immutable().get_wpos();
        self.code().write_u16(0xFEFE); // Temporary branch target
        return InstructionTarget {
            instruction_location: i,
            jump_location: j
        };
    }

    /// Comparison between an int from the stack and constant zero
    #[must_use]
    pub fn emit_if(&mut self, ty: ComparisonType) -> InstructionTarget {
        self.record_pop();

        let i = self.current_location();
        // There is a wide variant for goto, but our code is architectured in a way that
        // assumes the target has a constant bit width. Luckily, there are other factors limiting
        // the size of a java method to 2^16 bytes, so the wide variant goes unused.
        self.code().write_u8(0x99 + (ty as u8)); // opcode
        let j = self.code_immutable().get_wpos();
        self.code().write_u16(0xFEFE); // Temporary branch target
        return InstructionTarget {
            instruction_location: i,
            jump_location: j
        };
    }

    pub fn emit_getstatic(&mut self, class: impl AsRef<str>, field: impl AsRef<str>, ty: FieldDescriptor) {
        let field_ref = self.class_writer.constant_pool.fieldref(class.as_ref().to_owned(), field.as_ref().to_owned(), ty.to_string());
        self.code().write_u8(0xB2); // getstatic
        self.code().write_u16(field_ref);
        self.record_push_ty((&ty).into());
    }

    pub fn emit_invokestatic(&mut self, class: impl AsRef<str>, name: impl AsRef<str>, desc: MethodDescriptor) {
        desc.0.iter().for_each(|_| { self.record_pop(); });
        if let Some(return_type) = &desc.1 {
            self.record_push_ty(return_type.into());
        }

        let method_ref = self.class_writer.constant_pool.methodref(class.as_ref().to_owned(), name.as_ref().to_owned(), desc.to_string());
        self.code().write_u8(0xB8); // invokestatic
        self.code().write_u16(method_ref);
    }

    pub fn emit_invokevirtual(&mut self, class: impl AsRef<str>, name: impl AsRef<str>, desc: MethodDescriptor) {
        self.record_pop(); // "this"
        desc.0.iter().for_each(|_| { self.record_pop(); });
        if let Some(return_type) = &desc.1 {
            self.record_push_ty(return_type.into());
        }

        let method_ref = self.class_writer.constant_pool.methodref(class.as_ref().to_owned(), name.as_ref().to_owned(), desc.to_string());
        self.code().write_u8(0xb6); // invokevirtual
        self.code().write_u16(method_ref);
    }

    pub fn emit_invokespecial(&mut self, class: impl AsRef<str>, name: impl AsRef<str>, desc: MethodDescriptor) {
        self.record_pop(); // "this"
        desc.0.iter().for_each(|_| { self.record_pop(); });
        if let Some(return_type) = &desc.1 {
            self.record_push_ty(return_type.into());
        }

        let method_ref = self.class_writer.constant_pool.methodref(class.as_ref().to_owned(), name.as_ref().to_owned(), desc.to_string());
        self.code().write_u8(0xb7); // invokespecial
        self.code().write_u16(method_ref);
    }

    pub fn emit_invokeinterface(&mut self, class: impl AsRef<str>, name: impl AsRef<str>, desc: MethodDescriptor) {
        let stack_s = self.current_frame.stack.len();
        self.record_pop(); // "this"
        desc.0.iter().for_each(|_| { self.record_pop(); });
        let arg_count = (stack_s - self.current_frame.stack.len()) as u8;
        if let Some(return_type) = &desc.1 {
            self.record_push_ty(return_type.into());
        }

        let method_ref = self.class_writer.constant_pool.interfacemethodref(class.as_ref().to_owned(), name.as_ref().to_owned(), desc.to_string());
        self.code().write_u8(0xb9); // invokeinterface
        self.code().write_u16(method_ref);

        self.code().write_u8(arg_count);
        self.code().write_u8(0);
    }

    pub fn emit_new(&mut self, class: impl AsRef<str>) {
        self.record_push(VerificationType::Object(class.as_ref().to_owned()));
        let class_ref = self.class_writer.constant_pool.class(class.as_ref().to_owned());
        self.code().write_u8(0xbb); // new
        self.code().write_u16(class_ref);
    }

    pub fn set_target(&mut self, target_index: InstructionTarget, target: CodeLocation) {
        let offset = target.0 - target_index.instruction_location.0;
        let wpos = self.code().get_wpos();
        self.code().set_wpos(target_index.jump_location);
        self.code().write_u16(offset as u16);
        self.code().set_wpos(wpos);
    }

    pub fn current_location(&self) -> CodeLocation {
        return CodeLocation(self.code_immutable().get_wpos());
    }

    pub fn get_current_stackframe(&self) -> StackMapFrame {
        return self.current_frame.clone();
    }

    pub fn set_current_stackframe(&mut self, frame: StackMapFrame) {
        self.current_frame = frame;
    }

    pub fn record_stackframe(&mut self, loc: CodeLocation, frame: StackMapFrame) {
        self.class_writer.methods[self.method_index].stackmaptable.insert(loc, frame);
    }
}

#[derive(Clone)]
pub struct StackMapFrame {
    stack: VerificationTypeList,
    locals: VerificationTypeList,
}

#[derive(Clone, PartialEq, Eq)]
pub struct VerificationTypeList(Vec<VerificationType>, usize);

impl From<Vec<VerificationType>> for VerificationTypeList {
    fn from(from: Vec<VerificationType>) -> Self {
        let mut len = 0;
        for v in &from {
            if *v == VerificationType::Long || *v == VerificationType::Double {
                len += 2;
            } else {
                len += 1;
            }
        }
        VerificationTypeList(from, len)
    }
}

impl VerificationTypeList {
    pub fn push(&mut self, element: VerificationType) {
        if element == VerificationType::Long || element == VerificationType::Double {
            self.1 += 2;
        } else {
            self.1 += 1;
        }
        self.0.push(element);
    }
    pub fn pop(&mut self) -> Option<VerificationType> {
        let r = self.0.pop();
        if let Some(v) = &r {
            if *v == VerificationType::Long || *v == VerificationType::Double {
                self.1 -= 2;
            } else {
                self.1 -= 1;
            }
        }
        return r;
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn len(&self) -> usize {
        self.1
    }

    pub fn iter(&self) -> impl Iterator<Item = &VerificationType> {
        return self.0.iter();
    }
}

#[derive(Clone, PartialEq, Eq)]
pub enum VerificationType {
    Top,
    Integer,
    Float,
    Long,
    Double,
    Null,
    UninitializedThis,
    Object(String),
    UninitializedVariable(CodeLocation),
}

impl VerificationType {
    fn serialize(&self, b: &mut ByteBuffer) {
        match self {
            VerificationType::Top => b.write_u8(0),
            VerificationType::Integer => b.write_u8(1),
            VerificationType::Float => b.write_u8(2),
            VerificationType::Long => {
                b.write_u8(4);
                VerificationType::Top.serialize(b);
            },
            VerificationType::Double => {
                b.write_u8(3);
                VerificationType::Top.serialize(b);
            },
            VerificationType::Null => b.write_u8(5),
            VerificationType::UninitializedThis => b.write_u8(6),
            VerificationType::Object(_) => todo!(),
            VerificationType::UninitializedVariable(code_location) => {
                b.write_u8(8);
                b.write_u16(code_location.0 as u16);
            },
        }
    }
}

fn calculate_initial_stackframe(descriptor: &MethodDescriptor) -> StackMapFrame {
    StackMapFrame {
        stack: Vec::new().into(), // Stack starts empty
        locals: descriptor.0.iter().flat_map(|d| match d {
            DescriptorEntry::Byte => vec![VerificationType::Integer],
            DescriptorEntry::Char => vec![VerificationType::Integer],
            DescriptorEntry::Double => vec![VerificationType::Double],
            DescriptorEntry::Float => vec![VerificationType::Float],
            DescriptorEntry::Int => vec![VerificationType::Integer],
            DescriptorEntry::Long => vec![VerificationType::Long],
            DescriptorEntry::Class(x) => vec![VerificationType::Object(x.to_string())],
            DescriptorEntry::Short => vec![VerificationType::Integer],
            DescriptorEntry::Boolean => vec![VerificationType::Integer],
            DescriptorEntry::Array(x) => vec![VerificationType::Object(format!("[{x}"))],
        }).collect::<Vec<_>>().into(),
    }
}

/// Represents an instruction inside of the code
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct CodeLocation(usize);

/// This represents the target of a branching instruction. The target can be any [`CodeLocation`].
/// The target can be queried and modified. This value is bound to the [`MethodWriter`] which created it,
/// it represents an index into a list of targets which is maintained by the [`MethodWriter`].
pub struct InstructionTarget {
    // The instruction from which the offset will be calculated
    instruction_location: CodeLocation,
    /// The bytes which contain an offset of where to jump to
    jump_location: usize,
}

#[repr(u8)]
pub enum ComparisonType {
    Equal = 0,
    NotEqual = 1,
    LessThan = 2,
    LessThanEqual = 5,
    GreaterThan = 4,
    GreaterThanEqual = 3,
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
    pub descriptor: MethodDescriptor,
}

pub struct MethodData {
    access_flag: u16,
    name_ref: ConstantPoolReference,
    desc_ref: ConstantPoolReference,
    code: ByteBuffer,
    max_stack_size: usize,
    descriptor: MethodDescriptor,
    stackmaptable: BTreeMap<CodeLocation, StackMapFrame>,
}

impl MethodData {
    fn serialize(&self, writer: &mut impl Write, code_attribute: ConstantPoolReference, stack_map_table_attr: Option<ConstantPoolReference>) -> io::Result<()> {
        // First of all, lets serialize the stack map table (if needed). We need to know its size
        let stack_map_table = self.needs_stack_map_table().then(|| {
            let mut stackmaptable_buf = ByteBuffer::new();

            // Every stack table starts with an implicit stack frame at index zero, calculated with the descriptor
            let mut previous_location = CodeLocation(0);
            let mut previous_frame: &StackMapFrame = &calculate_initial_stackframe(&self.descriptor);
            
            stackmaptable_buf.write_u16(self.stackmaptable.len() as u16);
            for (location, frame) in &self.stackmaptable {
                let offset = location.0 - previous_location.0;
                let stack_empty = frame.stack.is_empty();
                let local_eq = previous_frame.locals == frame.locals;

                if stack_empty && local_eq {
                    if offset < 64 {
                        // Same frame
                        stackmaptable_buf.write_u8(offset as u8);
                    } else {
                        // Same frame extended
                        stackmaptable_buf.write_u8(251);
                        stackmaptable_buf.write_u16(offset as u16);
                    }
                } else {
                    // Full frame
                    stackmaptable_buf.write_u8(255);
                    stackmaptable_buf.write_u16(offset as u16);
                    stackmaptable_buf.write_u16(frame.locals.len() as u16);
                    frame.locals.iter().for_each(|entry| entry.serialize(&mut stackmaptable_buf));
                    stackmaptable_buf.write_u16(frame.stack.len() as u16);
                    frame.stack.iter().for_each(|entry| entry.serialize(&mut stackmaptable_buf));
                }
                
                previous_location = CodeLocation(location.0 + 1);
                previous_frame = frame;
            }

            stackmaptable_buf
        });

        writer.write_u16::<byteorder::BigEndian>(self.access_flag)?;
        writer.write_u16::<byteorder::BigEndian>(self.name_ref)?;
        writer.write_u16::<byteorder::BigEndian>(self.desc_ref)?;
        writer.write_u16::<byteorder::BigEndian>(1)?; // One attribute
        writer.write_u16::<byteorder::BigEndian>(code_attribute)?; // That attribute is named "Code"
        writer.write_u32::<byteorder::BigEndian>((self.code.as_bytes().len() + 12 + stack_map_table.as_ref().map(|b| b.as_bytes().len() + 6).unwrap_or(0)) as u32)?;
        writer.write_u16::<byteorder::BigEndian>(self.max_stack_size as u16)?;
        writer.write_u16::<byteorder::BigEndian>(50)?; // TODO: max-locals
        writer.write_u32::<byteorder::BigEndian>(self.code.as_bytes().len() as u32)?;
        writer.write_all(self.code.as_bytes())?;
        writer.write_u16::<byteorder::BigEndian>(0)?; // No exception table
        if let Some(stack_map_table) = stack_map_table {
            writer.write_u16::<byteorder::BigEndian>(1)?; // One additional attribute
            writer.write_u16::<byteorder::BigEndian>(stack_map_table_attr.unwrap())?;
            writer.write_u32::<byteorder::BigEndian>(stack_map_table.as_bytes().len() as u32)?;
            writer.write_all(stack_map_table.as_bytes())?;
        } else {
            writer.write_u16::<byteorder::BigEndian>(0)?; // No additional attributes
        }
        Ok(())
    }

    fn needs_stack_map_table(&self) -> bool {
        !self.stackmaptable.is_empty()
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
#[derive(Debug, Clone, Copy)]
pub enum JavaType {
    Int,
    Long,
    Float,
    Double,
    Reference
}

impl From<&DescriptorEntry> for JavaType {
    fn from(value: &DescriptorEntry) -> Self {
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