// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! This is the main entry point to our compiler driver. This code accepts a few options that
//! can be used to configure goto-c compilation as well as all other flags supported by rustc.
//!
//! Like miri, clippy, and other tools developed on the top of rustc, we rely on the
//! rustc_private feature and a specific version of rustc.
#![deny(warnings)]
#![feature(extern_types)]
#![recursion_limit = "256"]
#![feature(box_patterns)]
#![feature(once_cell)]
#![feature(rustc_private)]
#![feature(more_qualified_paths)]
extern crate rustc_ast;
extern crate rustc_codegen_ssa;
extern crate rustc_data_structures;
extern crate rustc_driver;
extern crate rustc_errors;
extern crate rustc_hir;
extern crate rustc_index;
extern crate rustc_interface;
extern crate rustc_metadata;
extern crate rustc_middle;
extern crate rustc_session;
extern crate rustc_span;
extern crate rustc_target;

#[cfg(feature = "cprover")]
mod codegen_cprover_gotoc;
mod kani_middle;
mod parser;
mod session;
mod unsound_experiments;

use crate::kani_middle::stubbing;
use crate::parser::KaniCompilerParser;
use crate::session::init_session;
use clap::ArgMatches;
use kani_queries::{QueryDb, ReachabilityType, UserInput};
use rustc_data_structures::fx::FxHashMap;
use rustc_driver::{Callbacks, RunCompiler};
use std::ffi::OsStr;
use std::path::PathBuf;
use std::rc::Rc;
use std::{env, fs};

/// This function generates all rustc configurations required by our goto-c codegen.
fn rustc_gotoc_flags(lib_path: &str) -> Vec<String> {
    // The option below provides a mechanism by which definitions in the
    // standard library can be overriden. See
    // https://rust-lang.zulipchat.com/#narrow/stream/182449-t-compiler.2Fhelp/topic/.E2.9C.94.20Globally.20override.20an.20std.20macro/near/268873354
    // for more details.
    let kani_std_rlib = PathBuf::from(lib_path).join("libstd.rlib");
    let kani_std_wrapper = format!("noprelude:std={}", kani_std_rlib.to_str().unwrap());
    let args = vec![
        "-C",
        "overflow-checks=on",
        "-C",
        "panic=abort",
        "-Z",
        "unstable-options",
        "-Z",
        "panic_abort_tests=yes",
        "-Z",
        "trim-diagnostic-paths=no",
        "-Z",
        "human_readable_cgu_names",
        "-Z",
        "always-encode-mir",
        "--cfg=kani",
        "-Z",
        "crate-attr=feature(register_tool)",
        "-Z",
        "crate-attr=register_tool(kanitool)",
        "-L",
        lib_path,
        "--extern",
        "kani",
        "--extern",
        kani_std_wrapper.as_str(),
    ];
    args.iter().map(|s| s.to_string()).collect()
}

/// Main function. Configure arguments and run the compiler.
fn main() -> Result<(), &'static str> {
    let args = parser::command_arguments(&env::args().collect());
    let matches = parser::parser().get_matches_from(args);
    init_session(&matches);

    // Configure queries.
    let mut queries = QueryDb::default();
    queries.set_emit_vtable_restrictions(matches.get_flag(parser::RESTRICT_FN_PTRS));
    queries.set_check_assertion_reachability(matches.get_flag(parser::ASSERTION_REACH_CHECKS));
    queries.set_output_pretty_json(matches.get_flag(parser::PRETTY_OUTPUT_FILES));
    queries.set_ignore_global_asm(matches.get_flag(parser::IGNORE_GLOBAL_ASM));
    queries.set_reachability_analysis(matches.reachability_type());
    #[cfg(feature = "unsound_experiments")]
    crate::unsound_experiments::arg_parser::add_unsound_experiment_args_to_queries(
        &mut queries,
        &matches,
    );

    // Generate rustc args.
    let mut rustc_args = generate_rustc_args(&matches);

    // If appropriate, collect and set the stub mapping.
    if matches.get_flag(parser::ENABLE_STUBBING)
        && queries.get_reachability_analysis() == ReachabilityType::Harnesses
    {
        queries.set_stubbing_enabled(true);
        let all_stub_mappings =
            stubbing::collect_stub_mappings(&rustc_args).or(Err("Failed to compile crate"))?;
        let harness = matches.get_one::<String>(parser::HARNESS).unwrap();
        let mapping = find_harness_stub_mapping(harness, all_stub_mappings).unwrap_or_default();
        rustc_args.push(stubbing::mk_rustc_arg(mapping));
    }

    // Configure and run compiler.
    let mut callbacks = KaniCallbacks {};
    let mut compiler = RunCompiler::new(&rustc_args, &mut callbacks);
    if matches.get_flag("goto-c") {
        if cfg!(feature = "cprover") {
            compiler.set_make_codegen_backend(Some(Box::new(move |_cfg| {
                Box::new(codegen_cprover_gotoc::GotocCodegenBackend::new(&Rc::new(queries)))
            })));
        } else {
            return Err("Kani was configured without 'cprover' feature. You must enable this \
            feature in order to use --goto-c argument.");
        }
    }
    compiler.run().or(Err("Failed to compile crate."))
}

/// Empty struct since we don't support any callbacks yet.
struct KaniCallbacks {}

/// Use default function implementations.
impl Callbacks for KaniCallbacks {}

/// The Kani root folder has all binaries inside bin/ and libraries inside lib/.
/// This folder can also be used as a rustc sysroot.
fn kani_root() -> PathBuf {
    match env::current_exe() {
        Ok(exe_path) => {
            let mut path = fs::canonicalize(&exe_path).unwrap_or(exe_path);
            // Current folder (bin/)
            path.pop();
            // Top folder
            path.pop();
            path
        }
        Err(e) => panic!("Failed to get current exe path: {e}"),
    }
}

/// Generate the arguments to pass to rustc_driver.
fn generate_rustc_args(args: &ArgMatches) -> Vec<String> {
    let mut rustc_args = vec![String::from("rustc")];
    if args.get_flag(parser::GOTO_C) {
        let mut default_path = kani_root();
        if args.reachability_type() == ReachabilityType::Legacy {
            default_path.push("legacy-lib")
        } else {
            default_path.push("lib");
        }
        let gotoc_args = rustc_gotoc_flags(
            args.get_one::<String>(parser::KANI_LIB)
                .unwrap_or(&default_path.to_str().unwrap().to_string()),
        );
        rustc_args.extend_from_slice(&gotoc_args);
    }

    if args.get_flag(parser::RUSTC_VERSION) {
        rustc_args.push(String::from("--version"))
    }

    if args.get_flag(parser::JSON_OUTPUT) {
        rustc_args.push(String::from("--error-format=json"));
    }

    if let Some(extra_flags) = args.get_raw(parser::RUSTC_OPTIONS) {
        extra_flags.for_each(|arg| rustc_args.push(convert_arg(arg)));
    }
    let sysroot = sysroot_path(args);
    rustc_args.push(String::from("--sysroot"));
    rustc_args.push(convert_arg(sysroot.as_os_str()));
    tracing::debug!(?rustc_args, "Compile");
    rustc_args
}

/// Convert an argument from OsStr to String.
/// If conversion fails, panic with a custom message.
fn convert_arg(arg: &OsStr) -> String {
    arg.to_str().expect(format!("[Error] Cannot parse argument \"{arg:?}\".").as_str()).to_string()
}

/// Get the sysroot, for our specific version of Rust nightly.
///
/// Rust normally finds its sysroot by looking at where itself (the `rustc`
/// executable) is located. This will fail for us because we're `kani-compiler`
/// and not located under the rust sysroot.
///
/// We do know the actual name of the toolchain we need, however.
/// We look for our toolchain in the usual place for rustup.
///
/// We previously used to pass `--sysroot` in `KANIFLAGS` from `kani-driver`,
/// but this failed to have effect when building a `build.rs` file.
/// This wasn't used anywhere but passing down here, so we've just migrated
/// the code to find the sysroot path directly into this function.
///
/// This function will soon be removed.
#[deprecated]
fn toolchain_sysroot_path() -> PathBuf {
    // rustup sets some environment variables during build, but this is not clearly documented.
    // https://github.com/rust-lang/rustup/blob/master/src/toolchain.rs (search for RUSTUP_HOME)
    // We're using RUSTUP_TOOLCHAIN here, which is going to be set by our `rust-toolchain.toml` file.
    // This is a *compile-time* constant, not a dynamic lookup at runtime, so this is reliable.
    let toolchain = env!("RUSTUP_TOOLCHAIN");

    // We use the home crate to do a *runtime* determination of where rustup toolchains live
    let rustup = home::rustup_home().expect("Couldn't find RUSTUP_HOME");
    let path = rustup.join("toolchains").join(toolchain);

    if !path.exists() {
        panic!("Couldn't find Kani Rust toolchain {toolchain}. Tried: {}", path.display());
    }
    path
}

/// Get the sysroot relative to the binary location.
///
/// Kani uses a custom sysroot. The `std` library and dependencies are compiled in debug mode and
/// include the entire MIR definitions needed by Kani.
///
/// We do provide a `--sysroot` option that users may want to use instead.
#[allow(deprecated)]
fn sysroot_path(args: &ArgMatches) -> PathBuf {
    let sysroot_arg = args.get_one::<String>(parser::SYSROOT);
    let path = if let Some(s) = sysroot_arg {
        PathBuf::from(s)
    } else if args.reachability_type() == ReachabilityType::Legacy || !args.get_flag(parser::GOTO_C)
    {
        toolchain_sysroot_path()
    } else {
        kani_root()
    };

    if !path.exists() {
        panic!("Couldn't find Kani Rust toolchain {:?}.", path.display());
    }
    tracing::debug!(?path, ?sysroot_arg, "Sysroot path.");
    path
}

/// Find the stub mapping for the given harness.
///
/// This function is necessary because Kani currently allows a harness to be
/// specified by a partially qualified name, whereas stub mappings use fully
/// qualified names.
fn find_harness_stub_mapping(
    harness: &str,
    stub_mappings: FxHashMap<String, FxHashMap<String, String>>,
) -> Option<FxHashMap<String, String>> {
    let suffix = String::from("::") + harness;
    for (name, mapping) in stub_mappings {
        if name == harness || name.ends_with(&suffix) {
            return Some(mapping);
        }
    }
    None
}

#[cfg(test)]
mod args_test {
    use super::*;
    use crate::parser;
    #[cfg(unix)]
    #[test]
    #[should_panic]
    fn test_invalid_arg_fails() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStrExt;

        // The value 0x80 is an invalid character.
        let source = [0x68, 0x65, 0x6C, 0x6C, 0x80];
        let os_str = OsStr::from_bytes(&source[..]);
        assert_eq!(os_str.to_str(), None);

        let matches = parser::parser().get_matches_from(vec![
            OsString::from("kani-compiler").as_os_str(),
            OsString::from("--sysroot").as_os_str(),
            OsString::from("any").as_os_str(),
            os_str,
        ]);
        generate_rustc_args(&matches);
    }
}
