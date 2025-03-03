use std::env;
use std::fs;
use std::path::Path;

use anyhow::{anyhow, Result};
use convert_case::{Case, Casing};
use serde::Deserialize;
use structopt::clap::AppSettings;
use structopt::StructOpt;
use toml::Value;

#[derive(StructOpt, Debug, PartialEq, Deserialize)]
#[structopt(setting(AppSettings::DeriveDisplayOrder))]
pub struct RawOpts {
    /// Path of input Rust code
    #[structopt(short, long)]
    pub rust_input: String,
    /// Path of output generated Dart code
    #[structopt(short, long)]
    pub dart_output: String,

    /// Path of output generated C header
    #[structopt(short, long)]
    pub c_output: Option<String>,
    /// Crate directory for your Rust project
    #[structopt(long)]
    pub rust_crate_dir: Option<String>,
    /// Path of output generated Rust code
    #[structopt(long)]
    pub rust_output: Option<String>,
    /// Generated class name
    #[structopt(long)]
    pub class_name: Option<String>,
    /// Line length for dart formatting
    #[structopt(long)]
    pub dart_format_line_length: Option<i32>,
    /// Skip automatically adding `mod bridge_generated;` to `lib.rs`
    #[structopt(long)]
    pub skip_add_mod_to_lib: bool,
    /// Path to the installed LLVM
    #[structopt(long)]
    pub llvm_path: Option<String>,
}

#[derive(Debug)]
pub struct Opts {
    pub rust_input_path: String,
    pub dart_output_path: String,
    pub c_output_path: String,
    pub rust_crate_dir: String,
    pub rust_output_path: String,
    pub class_name: String,
    pub dart_format_line_length: i32,
    pub skip_add_mod_to_lib: bool,
    pub llvm_path: String,
}

pub fn parse(raw: RawOpts) -> Opts {
    let rust_input_path = canon_path(&raw.rust_input);

    let rust_crate_dir = canon_path(&raw.rust_crate_dir.unwrap_or_else(|| {
        fallback_rust_crate_dir(&rust_input_path)
            .unwrap_or_else(|_| panic!("{}", format_fail_to_guess_error("rust_crate_dir")))
    }));
    let rust_output_path = canon_path(&raw.rust_output.unwrap_or_else(|| {
        fallback_rust_output_path(&rust_input_path)
            .unwrap_or_else(|_| panic!("{}", format_fail_to_guess_error("rust_output")))
    }));
    let class_name = raw.class_name.unwrap_or_else(|| {
        fallback_class_name(&*rust_crate_dir)
            .unwrap_or_else(|_| panic!("{}", format_fail_to_guess_error("class_name")))
    });
    let c_output_path = canon_path(&raw.c_output.unwrap_or_else(|| {
        fallback_c_output_path()
            .unwrap_or_else(|_| panic!("{}", format_fail_to_guess_error("c_output")))
    }));

    Opts {
        rust_input_path,
        dart_output_path: canon_path(&raw.dart_output),
        c_output_path,
        rust_crate_dir,
        rust_output_path,
        class_name,
        dart_format_line_length: raw.dart_format_line_length.unwrap_or(80),
        skip_add_mod_to_lib: raw.skip_add_mod_to_lib,
        llvm_path: raw.llvm_path.unwrap_or_else(|| "".to_string()),
    }
}

fn format_fail_to_guess_error(name: &str) -> String {
    format!(
        "fail to guess {}, please specify it manually in command line arguments",
        name
    )
}

fn fallback_rust_crate_dir(rust_input_path: &str) -> Result<String> {
    let mut dir_curr = Path::new(rust_input_path)
        .parent()
        .ok_or_else(|| anyhow!(""))?;

    loop {
        let path_cargo_toml = dir_curr.join("Cargo.toml");

        if path_cargo_toml.exists() {
            return Ok(dir_curr
                .as_os_str()
                .to_str()
                .ok_or_else(|| anyhow!(""))?
                .to_string());
        }

        if let Some(next_parent) = dir_curr.parent() {
            dir_curr = next_parent;
        } else {
            break;
        }
    }
    Err(anyhow!(
        "look at parent directories but none contains Cargo.toml"
    ))
}

fn fallback_c_output_path() -> Result<String> {
    let named_temp_file = Box::leak(Box::new(tempfile::Builder::new().suffix(".h").tempfile()?));
    Ok(named_temp_file
        .path()
        .to_str()
        .ok_or_else(|| anyhow!(""))?
        .to_string())
}

fn fallback_rust_output_path(rust_input_path: &str) -> Result<String> {
    Ok(Path::new(rust_input_path)
        .parent()
        .ok_or_else(|| anyhow!(""))?
        .join("bridge_generated.rs")
        .to_str()
        .ok_or_else(|| anyhow!(""))?
        .to_string())
}

fn fallback_class_name(rust_crate_dir: &str) -> Result<String> {
    let cargo_toml_path = Path::new(rust_crate_dir).join("Cargo.toml");
    let cargo_toml_content = fs::read_to_string(cargo_toml_path)?;

    let cargo_toml_value = cargo_toml_content.parse::<Value>()?;
    let package_name = cargo_toml_value
        .get("package")
        .ok_or_else(|| anyhow!("no `package` in Cargo.toml"))?
        .get("name")
        .ok_or_else(|| anyhow!("no `name` in Cargo.toml"))?
        .as_str()
        .ok_or_else(|| anyhow!(""))?;

    Ok(package_name.to_case(Case::Pascal))
}

fn canon_path(sub_path: &str) -> String {
    let mut path =
        env::current_dir().unwrap_or_else(|_| panic!("fail to parse path: {}", sub_path));
    path.push(sub_path);
    path.into_os_string()
        .into_string()
        .unwrap_or_else(|_| panic!("fail to parse path: {}", sub_path))
}

impl Opts {
    pub fn dart_api_class_name(&self) -> String {
        self.class_name.clone()
    }

    pub fn dart_api_impl_class_name(&self) -> String {
        format!("{}Impl", self.class_name)
    }

    pub fn dart_wire_class_name(&self) -> String {
        format!("{}Wire", self.class_name)
    }
}
