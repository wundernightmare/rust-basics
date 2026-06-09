use std::io::Write;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Sample {
    #[serde(default)]
    addr: String,
    #[serde(default)]
    workers: u32,
}

fn write_yaml(body: &str) -> tempfile::NamedTempFile {
    let mut f = tempfile::NamedTempFile::new().unwrap();
    f.write_all(body.as_bytes()).unwrap();
    f
}

#[test]
fn yaml_file_only() {
    let f = write_yaml("addr: \":9000\"\nworkers: 4\n");
    let cfg: Sample = httpx::load_yaml(Some(f.path().to_str().unwrap()), "APP_").unwrap();
    assert_eq!(cfg.addr, ":9000");
    assert_eq!(cfg.workers, 4);
}

#[test]
fn env_overlays_file_with_correct_types() {
    let f = write_yaml("addr: \":9000\"\nworkers: 4\n");
    // SAFETY: single-threaded test; no concurrent env access.
    unsafe {
        std::env::set_var("CFGTEST_ADDR", ":7777");
        std::env::set_var("CFGTEST_WORKERS", "9"); // parsed as an integer, not a string
    }
    let cfg: Sample = httpx::load_yaml(Some(f.path().to_str().unwrap()), "CFGTEST_").unwrap();
    assert_eq!(cfg.addr, ":7777", "env wins over the file");
    assert_eq!(cfg.workers, 9, "env scalar is typed, not left as a string");
    unsafe {
        std::env::remove_var("CFGTEST_ADDR");
        std::env::remove_var("CFGTEST_WORKERS");
    }
}

#[test]
fn missing_file_is_env_only() {
    // SAFETY: single-threaded test.
    unsafe {
        std::env::set_var("CFGONLY_ADDR", ":8080");
    }
    let cfg: Sample = httpx::load_yaml(Some("/nonexistent/config.yaml"), "CFGONLY_").unwrap();
    assert_eq!(cfg.addr, ":8080");
    assert_eq!(cfg.workers, 0);
    unsafe {
        std::env::remove_var("CFGONLY_ADDR");
    }
}
