use std::{collections::HashMap, env, fs, path::Path};

const DOTENV_PATH: &str = ".env";
const LND_BASE_URL_ENV: &str = "AUTO_ADB_WL_LND_BASE_URL";
const LND_BEARER_TOKEN_ENV: &str = "AUTO_ADB_WL_LND_BEARER_TOKEN";

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={DOTENV_PATH}");
    println!("cargo:rerun-if-env-changed={LND_BASE_URL_ENV}");
    println!("cargo:rerun-if-env-changed={LND_BEARER_TOKEN_ENV}");

    let dotenv = load_dotenv(Path::new(DOTENV_PATH));
    export_compile_time_env(LND_BASE_URL_ENV, &dotenv);
    export_compile_time_env(LND_BEARER_TOKEN_ENV, &dotenv);
}

fn export_compile_time_env(name: &str, dotenv: &HashMap<String, String>) {
    if let Some(value) = dotenv.get(name) {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            println!("cargo:rustc-env={name}={trimmed}");
        }
        return;
    }

    if let Ok(value) = env::var(name) {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            println!("cargo:rustc-env={name}={trimmed}");
        }
    }
}

fn load_dotenv(path: &Path) -> HashMap<String, String> {
    let Ok(contents) = fs::read_to_string(path) else {
        return HashMap::new();
    };

    let mut values = HashMap::new();
    for (index, raw_line) in contents.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((key, value)) = line.split_once('=') else {
            panic!(
                "invalid {DOTENV_PATH} line {}: expected KEY=VALUE",
                index + 1
            );
        };
        let key = key.trim();
        if key.is_empty() {
            panic!("invalid {DOTENV_PATH} line {}: empty key", index + 1);
        }
        values.insert(key.to_string(), strip_quotes(value.trim()).to_string());
    }
    values
}

fn strip_quotes(value: &str) -> &str {
    if value.len() >= 2 {
        let quoted_by_double = value.starts_with('"') && value.ends_with('"');
        let quoted_by_single = value.starts_with('\'') && value.ends_with('\'');
        if quoted_by_double || quoted_by_single {
            return &value[1..value.len() - 1];
        }
    }
    value
}
