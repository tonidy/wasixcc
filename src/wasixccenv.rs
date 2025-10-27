use std::path::PathBuf;

use crate::{
    args::{gather_user_settings, UserSettings},
    download::TagSpec,
};
use anyhow::Result;
use clap::Parser;

#[cfg(unix)]
const COMMANDS: &[&str] = &["cc", "++", "cc++", "ar", "nm", "ranlib", "ld"];

#[derive(Parser)]
// The config help text assumes an 80-character terminal width, so replicate that for
// clap output as well.
#[command(max_term_width = 80)]
struct Args {
    #[command(subcommand)]
    command: WasixccCommand,

    /// User settings in the form KEY=VALUE, see 'help-config' output for details
    #[arg(short = 's')]
    // Needed to let clap parse the user settings passed via -sKEY=VALUE
    user_settings: Vec<String>,
}

#[derive(Parser)]
enum WasixccCommand {
    /// Install wasixcc executables (via symlinks to this binary) to the
    /// specified path
    InstallExecutables { path: PathBuf },
    /// Download the WASIX sysroot
    DownloadSysroot {
        /// The tag from which to download the sysroot, either 'latest' or a
        /// specific tag starting with 'v'. Defaults to 'latest'.
        tag: Option<TagSpec>,
    },
    /// Download the custom LLVM toolchain (Linux only)
    DownloadLlvm {
        /// The tag from which to download the LLVM toolchain, either 'latest' or a
        /// specific tag starting with 'v'. Defaults to 'latest'.
        tag: Option<TagSpec>,
    },
    /// Download and install everything
    InstallAll {
        #[arg(long)]
        /// The tag from which to download the sysroot, either 'latest' or a
        /// specific tag starting with 'v'. Defaults to 'latest'.
        sysroot_tag: Option<TagSpec>,
        #[arg(long)]
        /// The tag from which to download the LLVM toolchain, either 'latest' or a
        /// specific tag starting with 'v'. Defaults to 'latest'.
        llvm_tag: Option<TagSpec>,
        /// The path where the wasixcc executables will be installed
        path: PathBuf,
    },
    /// Print the sysroot location according to current configuration
    PrintSysroot,
    /// Print version information
    Version,
    /// Print help information about wasixcc configuration options
    HelpConfig,
}

pub(crate) fn run() -> Result<()> {
    let args = Args::parse();
    let user_settings = gather_user_settings(&args.user_settings)?;

    match args.command {
        WasixccCommand::InstallExecutables { path } => install_executables(path),
        WasixccCommand::DownloadSysroot { tag } => {
            download_sysroot(tag.unwrap_or(TagSpec::Latest), &user_settings)
        }
        WasixccCommand::DownloadLlvm { tag } => {
            download_llvm(tag.unwrap_or(TagSpec::Latest), &user_settings)
        }
        WasixccCommand::InstallAll {
            llvm_tag,
            sysroot_tag,
            path,
        } => {
            download_llvm(llvm_tag.unwrap_or(TagSpec::Latest), &user_settings)?;
            download_sysroot(sysroot_tag.unwrap_or(TagSpec::Latest), &user_settings)?;
            install_executables(path)?;
            Ok(())
        }
        WasixccCommand::PrintSysroot => print_sysroot(&user_settings),
        WasixccCommand::Version => {
            print_version();
            Ok(())
        }
        WasixccCommand::HelpConfig => {
            print_configuration_help();
            Ok(())
        }
    }
}

pub fn download_sysroot(tag_spec: TagSpec, user_settings: &UserSettings) -> Result<()> {
    tracing::info!("Downloading sysroot: {:?}", tag_spec);

    crate::download::download_sysroot(tag_spec, user_settings)
}

#[cfg(target_os = "linux")]
pub fn download_llvm(tag_spec: TagSpec, user_settings: &UserSettings) -> Result<()> {
    tracing::info!("Downloading LLVM: {:?}", tag_spec);

    crate::download::download_llvm(tag_spec, user_settings)
}

#[cfg(not(target_os = "linux"))]
pub fn download_llvm(_tag_spec: TagSpec) -> Result<()> {
    bail!("LLVM download is only supported on Linux");
}

#[cfg_attr(target_vendor = "wasmer", allow(unused_variables))]
fn install_executables(path: PathBuf) -> Result<()> {
    #[cfg(not(unix))]
    {
        bail!("wasixcc only supports installation on unix systems at this time");
    }

    #[cfg(unix)]
    {
        use std::{env, fs, os::unix::fs as unix_fs};

        use anyhow::Context;

        fs::create_dir_all(&path)
            .with_context(|| format!("Failed to create directory at {path:?}"))?;

        let exe_path = env::current_exe().context("Failed to get current executable path")?;

        for command in COMMANDS {
            let target = path.join(format!("wasix{}", command));

            if fs::metadata(&target).is_ok() {
                use anyhow::Context;

                fs::remove_file(&target)
                    .with_context(|| format!("Failed to remove existing file at {target:?}"))?;
            }

            unix_fs::symlink(&exe_path, &target)
                .with_context(|| format!("Failed create symlink at {target:?}"))?;
            let permissions = unix_fs::PermissionsExt::from_mode(0o755);
            fs::set_permissions(&target, permissions)
                .with_context(|| format!("Failed to set permissions for {target:?}"))?;

            println!("Created command {target:?}");
        }

        Ok(())
    }
}

fn print_version() {
    let version = env!("CARGO_PKG_VERSION");
    println!("{version}");
}

fn print_sysroot(user_settings: &UserSettings) -> Result<()> {
    let sysroot = user_settings.ensure_sysroot_location()?;
    println!("{}", sysroot.display());
    Ok(())
}

fn print_configuration_help() {
    println!(
        r#"wasixcc can be configured using various options to control its behavior
when building WebAssembly modules.

Configuration options can be provided on the command line using the
'-s' flag, or using environment variables prefixed with 'WASIXCC_'.

'-sKEY=VALUE' can be specified when running any of the wasixcc commands
(e.g., 'wasixcc -sSYSROOT=/path/to/sysroot code.c -o code.wasm').

This is also true for wasixccenv, which uses the same mechanism to figure out
where to download the sysroot and LLVM toolchain to, as well as when using
`print-sysroot`. Note that when running wasixccenv, the '-s' flags must be
specified first (e.g., 'wasixccenv -sSYSROOT=... download-sysroot').

The following configuration options are available:
  SYSROOT=<PATH>           Set the sysroot location directly; this option
                           overrides SYSROOT_PREFIX. It is recommended to use
                           SYSROOT_PREFIX instead when possible.
  SYSROOT_PREFIX=<PREFIX>  Set the sysroot prefix, which is expected to
                           contain 3 subdirectories: 'sysroot',
                           'sysroot-eh', and 'sysroot-ehpic'.
  LLVM_LOCATION=<PATH>     Set the location of LLVM toolchain which will be
                           invoked without a version suffix. The path must
                           point to the installation directory of the
                           toolchain, NOT the bin directory inside it; tools
                           will be executed from LLVM_LOCATION/bin/tool-name.
                           Note that wasixcc does not use system-wide
                           installations of LLVM by default since it requires
                           a patched version of LLVM.
  COMPILER_FLAGS=<FLAGS>   Extra flags to pass to the compiler, separated
                           by colons (':')
  COMPILER_POST_FLAGS=<FLAGS>
                           Extra flags to pass to the compiler, separated
                           by colons (':'), passed after the arguments
                           provided on the command line. This is useful for
                           overriding command-line flags, such as for disabling
                           warnings.
  COMPILER_FLAGS_C=<FLAGS> Same as COMPILER_FLAGS, but only for C
                           files. This is useful for passing flags that are
                           not compatible with C++.
  COMPILER_POST_FLAGS_C=<FLAGS>
                           Same as COMPILER_POST_FLAGS, but only for C files.
  COMPILER_FLAGS_CXX=<FLAGS>
                           Same as COMPILER_FLAGS, but only for C++ files.
                           This is useful for passing flags that are not
                           compatible with C.
  COMPILER_POST_FLAGS_CXX=<FLAGS>
                           Same as COMPILER_POST_FLAGS, but only for C++ files.
  LINKER_FLAGS=<FLAGS>     Extra flags to pass to the linker, separated
                           by colons (':')
  INCLUDE_CPP_SYMBOLS=<BOOL>
                           Whether to include C++ symbols when building a
                           dynamic main module from C sources. This is useful
                           when the main module is expected to be able to load
                           side modules implemented in C++.
  RUN_WASM_OPT=<BOOL>      Whether to run `wasm-opt` on the output of the
                           compiler. If this setting is left out, wasixcc
                           will look at compiler flags to determine whether
                           to run `wasm-opt`. If no flags are found, default
                           behavior is to run `wasm-opt`.
  WASM_OPT_FLAGS=<FLAGS>   Extra flags to pass to `wasm-opt`, separated by
                           colons (':'). Specifying a non-empty list of
                           extra flags for wasm-opt will imply
                           `RUN_WASM_OPT=yes` unless an explicit value is
                           provided for `RUN_WASM_OPT`.
  WASM_OPT_SUPPRESS_DEFAULT=<BOOL>
                           Whether to suppress the default flags wasixcc
                           passes to wasm-opt. The default flags are:
                           * `-O*` for all modules. The optimization
                                 level is determined by the `-O` flag passed
                                 to the compiler. 
                           * `--emit-exnref` for modules with exception
                                 handling enabled, required for running
                                 the module with engines that only support
                                 the 'new' exnref proposal (e.g. the LLVM
                                 backend in Wasmer)
                           * `--asyncify` for modules without exception
                                 handling enabled, required for forks and
                                 setjmp/longjmp to work
  WASM_OPT_PRESERVE_UNOPTIMIZED=<BOOL>
                           Whether to preserve a copy of the unoptimized
                           artifact before running wasm-opt. If the wasm-opt
                           invocation fails, the unoptimized artifact will be
                           preserved at a temporary location and its path
                           will be printed to stderr. This is useful for
                           debugging wasm-opt failures. By default, wasm-opt
                           runs in-place and the unoptimized artifact is
                           deleted.
  MODULE_KIND=<KIND>       The kind of module to generate. wasixcc can
                           guess this setting most of the time based on
                           compiler/linker flags. Valid values are:
                           * static-main: An executable main module with no
                                 dynamic linking capability
                           * dynamic-main: A main module capable of loading
                                 dynamically-linked side modules at runtime
                           * shared-library: A dynamically-linked side module
                                 which can be loaded by a dynamic main
                           * object-file: An object file
  WASM_EXCEPTIONS=<BOOL>   Whether to enable WebAssembly exception handling
                           support. This value can be deduced from the
                           `-fwasm-exceptions`/`-fno-wasm-exceptions` flags
                           passed to the compiler.
  PIC=<BOOL>               Whether to enable position-independent code (PIC),
                           required for dynamic linking. PIC will be enabled
                           if module kind is `dynamic-main` or `shared-library`,
                           or if the `-fPIC` flag is passed to the compiler.
  LINK_SYMBOLIC=<BOOL>     Whether to link the output with `-Bsymbolic`, which
                           binds defined symbols locally, hence preventing
                           similarly named symbols from other modules from
                           overriding the module's local symbols. This is
                           enabled by default, but can be disabled by setting
                           this option to `no`. This option is only relevant
                           for dynamic main modules and shared libraries.
"#
    );
}
