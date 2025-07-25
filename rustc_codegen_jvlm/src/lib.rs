#![feature(rustc_private)]
#![allow(mutable_transmutes)]
#![allow(internal_features)]

#![allow(unused_variables)]

////////
#![feature(assert_matches)]
#![feature(exact_size_is_empty)]
#![feature(extern_types)]
#![feature(file_buffered)]
#![feature(if_let_guard)]
#![feature(impl_trait_in_assoc_type)]
#![feature(iter_intersperse)]
#![feature(rustdoc_internals)]
#![feature(slice_as_array)]
#![feature(try_blocks)]
////////

// Created by build.rs, contains some stuff needed so we can include codegen_llvm verbatim
include!(concat!(env!("OUT_DIR"), "/llvm_include.rs"));

extern crate rustc_driver;

use std::any::Any;
use std::fs::File;
use std::io::{self};
use std::path::Path;

use codegen_llvm::LlvmCodegenBackend;
use jvlm::linker;
use rustc_codegen_ssa::traits::{CodegenBackend, ExtraBackendMethods, WriteBackendMethods};
use rustc_codegen_ssa::{CodegenResults, TargetConfig};
use rustc_data_structures::fx::FxIndexMap;
use jvlm::options::JvlmCompileOptions;
use rustc_metadata::EncodedMetadata;
use rustc_middle::dep_graph::{WorkProduct, WorkProductId};
use rustc_middle::middle::dependency_format::Linkage;
use rustc_middle::ty::TyCtxt;
use rustc_session::output::out_filename;
use rustc_session::Session;
use rustc_session::config::{CrateType, OutputFilenames, OutputType};

#[derive(Clone)]
#[repr(transparent)]
struct JvlmBackend {
    llvm: LlvmCodegenBackend
}

impl CodegenBackend for JvlmBackend {
    fn locale_resource(&self) -> &'static str {
        LlvmCodegenBackend::locale_resource(&self.llvm)
    }

    fn init(&self, sess: &Session) {
        self.llvm.init(sess);
    }

    fn codegen_crate<'a, 'tcx>(
        &self,
        tcx: TyCtxt<'tcx>,
        metadata: EncodedMetadata,
        need_metadata_module: bool,
    ) -> Box<dyn Any> {
        // let b = &tcx.sess.opts.unstable_opts.no_codegen;
        // let b: &mut bool = unsafe { std::mem::transmute(b) }; // TODO
        // *b = true;
        return Box::new(rustc_codegen_ssa::base::codegen_crate(
            self.clone(),
            tcx,
            "Urgh".to_owned(), // TODO
            metadata,
            need_metadata_module,
        ));
    }

    fn join_codegen(
        &self,
        ongoing_codegen: Box<dyn Any>,
        sess: &Session,
        outputs: &OutputFilenames,
    ) -> (CodegenResults, FxIndexMap<WorkProductId, WorkProduct>) {
        let (codegen_results, work_products) = ongoing_codegen
            .downcast::<rustc_codegen_ssa::back::write::OngoingCodegen<JvlmBackend>>()
            .expect("Expected JvlmBackend's OngoingCodegen, found Box<Any>")
            .join(sess);

        // if sess.opts.unstable_opts.llvm_time_trace {
        //     sess.time("llvm_dump_timing_file", || {
        //         let file_name = outputs.with_extension("llvm_timings.json");
        //         llvm_util::time_trace_profiler_finish(&file_name);
        //     });
        // }

        (codegen_results, work_products)
    }

    fn link(&self, sess: &Session, codegen_results: CodegenResults, outputs: &OutputFilenames) {
        for crate_type in codegen_results.crate_info.crate_types {
            if outputs.outputs.should_link() {
                let output = out_filename(
                    sess,
                    crate_type,
                    outputs,
                    codegen_results.crate_info.local_crate_name,
                );
                let crate_name = format!("{}", codegen_results.crate_info.local_crate_name);
                let out_filename = output.file_for_writing(
                    outputs,
                    OutputType::Exe,
                    &crate_name,
                    sess.invocation_temp.as_deref(),
                );
                match crate_type {
                    CrateType::Cdylib => {
                        let mut object_files = vec![];

                        // TODO wtf do we do with natives

                        // Local crate object files
                        codegen_results.modules.iter().filter_map(|m| m.object.as_ref()).for_each(|o| object_files.push(o));
                        if matches!(crate_type, CrateType::Dylib | CrateType::ProcMacro)
                            && let Some(meta_module) = &codegen_results.metadata_module
                            && let Some(meta_obj) = &meta_module.object
                        {
                            object_files.push(meta_obj);
                        }
                        if let Some(obj) = codegen_results.allocator_module.as_ref().and_then(|m| m.object.as_ref()) {
                            object_files.push(obj);
                        }

                        // Dependency crates
                        let mut archive_files = vec![];
                        let linkage_info = codegen_results
                            .crate_info
                            .dependency_formats
                            .get(&crate_type)
                            .expect("failed to find crate type in dependency format list");
                        for &cnum in &codegen_results.crate_info.used_crates {
                            let linkage = linkage_info[cnum];
                            match linkage {
                                Linkage::Static | Linkage::IncludedFromDylib | Linkage::NotLinked => {
                                    let src = &codegen_results.crate_info.used_crate_source[&cnum];
                                    let cratepath = &src.rlib.as_ref().unwrap().0;
                                    archive_files.push(cratepath);
                                }
                                Linkage::Dynamic => {
                                    // We don't really need to do anything specific for dynamic dependencies.
                                    // We could add them to the manifest in the future maybe
                                }
                            }
                        }

                        // Run the linker
                        // TODO add archive_files
                        let source_jars = object_files.iter().map(|f| File::open(f).unwrap());
                        let output = File::create(out_filename).unwrap();
                        linker::link(source_jars, output).unwrap();
                    }
                    _ => todo!()
                }
            }
        }
    }

    fn target_config(&self, _sess: &Session) -> rustc_codegen_ssa::TargetConfig {
        TargetConfig {
            target_features: vec![],
            unstable_target_features: vec![],
            has_reliable_f16: false,
            has_reliable_f16_math: false,
            has_reliable_f128: false,
            has_reliable_f128_math: false,
        }
    }
}

impl WriteBackendMethods for JvlmBackend {
    type Module = <LlvmCodegenBackend as WriteBackendMethods>::Module;
    type TargetMachine = <LlvmCodegenBackend as WriteBackendMethods>::TargetMachine;
    type TargetMachineError = <LlvmCodegenBackend as WriteBackendMethods>::TargetMachineError;
    type ModuleBuffer = <LlvmCodegenBackend as WriteBackendMethods>::ModuleBuffer;
    type ThinData = <LlvmCodegenBackend as WriteBackendMethods>::ThinData;
    type ThinBuffer = <LlvmCodegenBackend as WriteBackendMethods>::ThinBuffer;

    fn run_link(
        cgcx: &rustc_codegen_ssa::back::write::CodegenContext<Self>,
        dcx: rustc_errors::DiagCtxtHandle<'_>,
        modules: Vec<rustc_codegen_ssa::ModuleCodegen<Self::Module>>,
    ) -> Result<rustc_codegen_ssa::ModuleCodegen<Self::Module>, rustc_errors::FatalError> {
        let cgcx = unsafe { std::mem::transmute(cgcx) };
        <LlvmCodegenBackend as WriteBackendMethods>::run_link(cgcx, dcx, modules)
    }

    fn run_fat_lto(
        cgcx: &rustc_codegen_ssa::back::write::CodegenContext<Self>,
        modules: Vec<rustc_codegen_ssa::back::write::FatLtoInput<Self>>,
        cached_modules: Vec<(rustc_codegen_ssa::back::lto::SerializedModule<Self::ModuleBuffer>, WorkProduct)>,
    ) -> Result<rustc_codegen_ssa::back::lto::LtoModuleCodegen<Self>, rustc_errors::FatalError> {
        let cgcx = unsafe { std::mem::transmute(cgcx) };
        let modules = unsafe { std::mem::transmute(modules) };
        let res = <LlvmCodegenBackend as WriteBackendMethods>::run_fat_lto(cgcx, modules, cached_modules);
        unsafe { std::mem::transmute(res) }
    }

    fn run_thin_lto(
        cgcx: &rustc_codegen_ssa::back::write::CodegenContext<Self>,
        modules: Vec<(String, Self::ThinBuffer)>,
        cached_modules: Vec<(rustc_codegen_ssa::back::lto::SerializedModule<Self::ModuleBuffer>, WorkProduct)>,
    ) -> Result<(Vec<rustc_codegen_ssa::back::lto::LtoModuleCodegen<Self>>, Vec<WorkProduct>), rustc_errors::FatalError> {
        let cgcx = unsafe { std::mem::transmute(cgcx) };
        let res = <LlvmCodegenBackend as WriteBackendMethods>::run_thin_lto(cgcx, modules, cached_modules);
        unsafe { std::mem::transmute(res) }
    }

    fn print_pass_timings(&self) {
        todo!()
    }

    fn print_statistics(&self) {
        todo!()
    }

    unsafe fn optimize(
        cgcx: &rustc_codegen_ssa::back::write::CodegenContext<Self>,
        dcx: rustc_errors::DiagCtxtHandle<'_>,
        module: &mut rustc_codegen_ssa::ModuleCodegen<Self::Module>,
        config: &rustc_codegen_ssa::back::write::ModuleConfig,
    ) -> Result<(), rustc_errors::FatalError> {
        let cgcx = unsafe { std::mem::transmute(cgcx) };
        let res = unsafe { <LlvmCodegenBackend as WriteBackendMethods>::optimize(cgcx, dcx, module, config) };
        unsafe { std::mem::transmute(res) }
    }

    fn optimize_fat(
        cgcx: &rustc_codegen_ssa::back::write::CodegenContext<Self>,
        llmod: &mut rustc_codegen_ssa::ModuleCodegen<Self::Module>,
    ) -> Result<(), rustc_errors::FatalError> {
        let cgcx = unsafe { std::mem::transmute(cgcx) };
        let res = <LlvmCodegenBackend as WriteBackendMethods>::optimize_fat(cgcx, llmod);
        unsafe { std::mem::transmute(res) }
    }

    unsafe fn optimize_thin(
        cgcx: &rustc_codegen_ssa::back::write::CodegenContext<Self>,
        thin: rustc_codegen_ssa::back::lto::ThinModule<Self>,
    ) -> Result<rustc_codegen_ssa::ModuleCodegen<Self::Module>, rustc_errors::FatalError> {
        let cgcx = unsafe { std::mem::transmute(cgcx) };
        let thin = unsafe { std::mem::transmute(thin) };
        let res = unsafe { <LlvmCodegenBackend as WriteBackendMethods>::optimize_thin(cgcx, thin) };
        unsafe { std::mem::transmute(res) }
    }

    unsafe fn codegen(
        cgcx: &rustc_codegen_ssa::back::write::CodegenContext<Self>,
        dcx: rustc_errors::DiagCtxtHandle<'_>,
        module: rustc_codegen_ssa::ModuleCodegen<Self::Module>,
        config: &rustc_codegen_ssa::back::write::ModuleConfig,
    ) -> Result<rustc_codegen_ssa::CompiledModule, rustc_errors::FatalError> {
        let cgcx = unsafe { std::mem::transmute(cgcx) };
        let res = unsafe { <LlvmCodegenBackend as WriteBackendMethods>::codegen(cgcx, dcx, module, config) };
        let res = unsafe { std::mem::transmute(res) };
        res
    }

    fn prepare_thin(
        module: rustc_codegen_ssa::ModuleCodegen<Self::Module>,
        want_summary: bool,
    ) -> (String, Self::ThinBuffer) {
        let res = <LlvmCodegenBackend as WriteBackendMethods>::prepare_thin(module, want_summary);
        unsafe { std::mem::transmute(res) }
    }

    fn serialize_module(module: rustc_codegen_ssa::ModuleCodegen<Self::Module>) -> (String, Self::ModuleBuffer) {
        <LlvmCodegenBackend as WriteBackendMethods>::serialize_module(module)
    }

    fn autodiff(
        cgcx: &rustc_codegen_ssa::back::write::CodegenContext<Self>,
        module: &rustc_codegen_ssa::ModuleCodegen<Self::Module>,
        diff_fncs: Vec<rustc_ast::expand::autodiff_attrs::AutoDiffItem>,
        config: &rustc_codegen_ssa::back::write::ModuleConfig,
    ) -> Result<(), rustc_errors::FatalError> {
        let cgcx = unsafe { std::mem::transmute(cgcx) };
        <LlvmCodegenBackend as WriteBackendMethods>::autodiff(cgcx, module, diff_fncs, config)
    }
}

impl ExtraBackendMethods for JvlmBackend {
    fn codegen_allocator<'tcx>(
        &self,
        tcx: TyCtxt<'tcx>,
        module_name: &str,
        kind: rustc_ast::expand::allocator::AllocatorKind,
        alloc_error_handler_kind: rustc_ast::expand::allocator::AllocatorKind,
    ) -> Self::Module {
        self.llvm.codegen_allocator(tcx, module_name, kind, alloc_error_handler_kind)
    }

    fn compile_codegen_unit(
        &self,
        tcx: TyCtxt<'_>,
        cgu_name: rustc_span::Symbol,
    ) -> (rustc_codegen_ssa::ModuleCodegen<Self::Module>, u64) {
        self.llvm.compile_codegen_unit(tcx, cgu_name)
    }

    fn target_machine_factory(
        &self,
        sess: &Session,
        opt_level: rustc_session::config::OptLevel,
        target_features: &[String],
    ) -> rustc_codegen_ssa::back::write::TargetMachineFactoryFn<Self> {
        self.llvm.target_machine_factory(sess, opt_level, target_features)
    }
}

pub(crate) fn write_output_file<'ll>(
    dcx: rustc_errors::DiagCtxtHandle<'_>,
    target: &'ll codegen_llvm::llvm::TargetMachine,
    no_builtins: bool,
    m: &'ll codegen_llvm::llvm::Module,
    output: &Path,
    dwo_output: Option<&Path>,
    file_type: codegen_llvm::llvm::FileType,
    self_profiler_ref: &rustc_data_structures::profiling::SelfProfilerRef,
    verify_llvm_ir: bool,
) -> Result<(), rustc_span::fatal_error::FatalError> {
    let r: io::Result<_> = try {
        let output = File::create(output)?;
        let llvm_mod = unsafe {jvlm::LlvmModule::new(std::mem::transmute(m))};
        jvlm::compile(llvm_mod, output, JvlmCompileOptions::default());
    };
    r.map_err(|_| dcx.emit_almost_fatal(codegen_llvm::errors::LlvmError::WriteOutput { path: output }))
}

/// This is the entrypoint for a hot plugged rustc_codegen_llvm
#[unsafe(no_mangle)]
pub fn __rustc_codegen_backend() -> Box<dyn CodegenBackend> {
    let llvm = LlvmCodegenBackend::new();
    let llvm = Box::into_raw(llvm);
    let llvm = llvm as *mut LlvmCodegenBackend ;
    let llvm = unsafe { Box::from_raw(llvm) };
    let llvm = *llvm;
    return Box::new(JvlmBackend{ llvm })
}