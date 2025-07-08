use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use tracing::level_filters::LevelFilter;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

#[cfg(unix)]
const COMMANDS: &[&str] = &["cc", "++", "cc++", "ar", "nm", "ranlib", "ld"];

enum WasixccCommand {
    Help,
    Version,
    InstallExecutables(PathBuf),
    RunTool,
}

fn setup_tracing() {
    let fmt_layer = fmt::layer()
        .with_target(true)
        .with_ansi(true)
        .with_thread_ids(true)
        .with_span_events(fmt::format::FmtSpan::CLOSE)
        .with_writer(std::io::stderr)
        .compact();

    let filter_layer = EnvFilter::builder()
        .with_default_directive(LevelFilter::OFF.into())
        .from_env_lossy();

    tracing_subscriber::registry()
        .with(filter_layer)
        .with(fmt_layer)
        .init();
}

fn get_executable_name() -> Result<String> {
    let executable_path = std::env::args().next().context("Empty argument list")?;
    let executable_path = std::path::Path::new(&executable_path);
    Ok(executable_path
        .file_name()
        .context("Failed to get executable file name")?
        .to_str()
        .context("Non-UTF8 characters in executable name")?
        .to_owned())
}

fn get_command(exe_name: &str) -> Result<String> {
    if let Some(command_name) = exe_name.strip_prefix("wasix-") {
        Ok(command_name.to_owned())
    } else if let Some(command_name) = exe_name.strip_prefix("wasix") {
        Ok(command_name.to_owned())
    } else {
        bail!(
            "Failed to get command name; this binary must be run with a name in \
            the form 'wasix-<command-name>' or 'wasix<command-name>`, such as \
            wasix-cc; given {exe_name}",
        )
    }
}

fn install_executables(path: PathBuf) -> Result<()> {
    #[cfg(not(unix))]
    {
        bail!("wasixcc only supports installation on unix systems at this time");
    }

    #[cfg(unix)]
    {
        use std::{env, fs, os::unix::fs as unix_fs};

        fs::create_dir_all(&path)
            .with_context(|| format!("Failed to create directory at {path:?}"))?;

        let exe_path = env::current_exe().context("Failed to get current executable path")?;

        for command in COMMANDS {
            let target = path.join(format!("wasix{}", command));

            if fs::metadata(&target).is_ok() {
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

fn print_version(exe_name: &str) {
    let version = env!("CARGO_PKG_VERSION");

    println!("{exe_name} version: {version}");
}

fn print_help(exe_name: &str) {
    println!(
        r#"Usage: {exe_name} [OPTIONS] -- [PASS-THROUGH OPTIONS]

Options:
  --help, -h                     Print this help message
  --version, -v                  Print version information
  -s[CONFIG]=[VALUE]             Set a configuration value, see list below
  --install-executables <PATH>   Install executables to the specified path

Configuration options can be provided on the command line using the
'-s' flag, or using environment variables prefixed with 'WASIXCC_'.
The following configuration options are available:");
  SYSROOT=<PATH>           Set the sysroot location
  SYSROOT_PREFIX=<PREFIX>  Set the sysroot prefix, which is expected to
                           contain 3 subdirectories: 'sysroot',
                           'sysroot-eh', and 'sysroot-ehpic'.
  LLVM_LOCATION=<PATH>     Set the location of LLVM binaries which will be
                           invoked without a version suffix. If this option
                           is left out, LLVM binaries will be invoked with
                           a -20 version suffix (e.g. clang-20).
  COMPILER_FLAGS=<FLAGS>   Extra flags to pass to the compiler, separated
                           by colons (':')
  LINKER_FLAGS=<FLAGS>     Extra flags to pass to the linker, separated
                           by colons (':')
  RUN_WASM_OPT=<BOOL>      Whether to run `wasm-opt` on the output of the
                           compiler. If this setting is left out, {exe_name}
                           will look at compiler flags to determine whether
                           to run `wasm-opt`. If no flags are found, default
                           behavior is to run `wasm-opt`.
  WASM_OPT_FLAGS=<FLAGS>   Extra flags to pass to `wasm-opt`, separated by
                           colons (':'). Specifying a non-empty list of
                           extra flags for wasm-opt will imply
                           `RUN_WASM_OPT=yes` unless an explicit value is
                           provided for `RUN_WASM_OPT`.
  WASM_OPT_SUPPRESS_DEFAULT=<BOOL>
                           Whether to suppress the default flags {exe_name}
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
  MODULE_KIND=<KIND>       The kind of module to generate. {exe_name} can
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

Note: Pass-through options are passed directly to the underlying
LLVM executables (e.g., clang, wasm-ld, etc.). This is useful for
getting version information or help messages from the underlying
tools, but has little use otherwise.
"#
    );
}

fn get_wasixcc_command(exe_name: &str) -> WasixccCommand {
    let mut args = std::env::args().skip(1);

    while let Some(arg) = args.next() {
        return match arg.as_str() {
            "--help" | "-h" => WasixccCommand::Help,

            "--version" | "-v" => WasixccCommand::Version,

            "--install-executables" => {
                let Some(path) = args.next() else {
                    println!("Usage: {exe_name} --install-executables <PATH>");
                    std::process::exit(1);
                };
                WasixccCommand::InstallExecutables(PathBuf::from(path))
            }

            "--" => WasixccCommand::RunTool,

            _ => continue,
        };
    }

    WasixccCommand::RunTool
}

fn run() -> Result<()> {
    let exe_name = get_executable_name()?;

    let command = get_wasixcc_command(&exe_name);

    match command {
        WasixccCommand::Help => {
            print_help(&exe_name);
            Ok(())
        }
        WasixccCommand::Version => {
            print_version(&exe_name);
            Ok(())
        }
        WasixccCommand::InstallExecutables(path) => install_executables(path),
        WasixccCommand::RunTool => {
            let command_name = get_command(&exe_name)?;
            match command_name.as_str() {
                "cc" => wasixcc::run_compiler(false),
                "++" | "cc++" => wasixcc::run_compiler(true),
                "ld" => wasixcc::run_linker(),
                "ar" => wasixcc::run_ar(),
                "nm" => wasixcc::run_nm(),
                "ranlib" => wasixcc::run_ranlib(),
                cmd => bail!("Unknown command {cmd}"),
            }
        }
    }
}

fn main() {
    setup_tracing();

    match run() {
        Ok(()) => (),
        Err(e) => {
            eprintln!("Error: {:?}", e);
            std::process::exit(1);
        }
    }
}
