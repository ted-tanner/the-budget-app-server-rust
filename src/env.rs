use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::Read;

#[derive(Deserialize, Serialize)]
pub struct Conf {
    pub connections: Connections,
    pub hashing: Hashing,
    pub keys: Keys,
    pub lifetimes: Lifetimes,
    pub security: Security,
    pub workers: Workers,
}

#[derive(Deserialize, Serialize)]
pub struct Connections {
    pub database_uri: String,
}

#[derive(Deserialize, Serialize)]
pub struct Hashing {
    pub hash_length: usize,
    pub hash_iterations: u32,
    pub hash_mem_size_kib: u32,
    pub hash_lanes: u32,
    pub salt_length_bytes: usize,
}

#[derive(Deserialize, Serialize)]
pub struct Keys {
    pub hashing_key: String,
    pub token_signing_key: String,
    pub otp_key: String,
}

#[derive(Deserialize, Serialize)]
pub struct Lifetimes {
    pub access_token_lifetime_mins: u64,
    pub refresh_token_lifetime_days: u64,
    pub otp_lifetime_mins: u64,
}

#[derive(Deserialize, Serialize)]
pub struct Security {
    pub otp_max_attempts: i16,
    pub otp_attempts_reset_mins: i16,
    pub password_max_attempts: i16,
    pub password_attempts_reset_mins: i16,
}

#[derive(Deserialize, Serialize)]
pub struct Workers {
    pub actix_workers: usize,
}

lazy_static! {
    pub static ref APP_NAME: &'static str = "Budget App";
    pub static ref CONF: Conf = build_conf();
}

fn build_conf() -> Conf {
    const CONF_FILE_PATH: &str = "conf/budgetapp.toml";

    let mut conf_file = File::open(CONF_FILE_PATH).unwrap_or_else(|_| {
        eprintln!("Expected configuration file at '{}'", CONF_FILE_PATH);
        std::process::exit(1);
    });

    let mut contents = String::new();
    conf_file.read_to_string(&mut contents).unwrap_or_else(|_| {
        eprintln!(
            "Configuratioin file at '{}' should be a text file in the TOML format.",
            CONF_FILE_PATH
        );
        std::process::exit(1);
    });

    match toml::from_str::<Conf>(&contents) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("Parsing '{}' failed: {}", CONF_FILE_PATH, e);
            std::process::exit(1);
        }
    }
}

pub mod password {
    use crate::utils::common_password_set::CommonPasswordSet;

    lazy_static! {
        pub static ref COMMON_PASSWORDS_FILE_PATH: &'static str = "./assets/common-passwords.txt";
        pub static ref COMMON_PASSWORDS_SET: CommonPasswordSet = CommonPasswordSet::generate();
    }

    pub fn initialize() {
        let _ = *COMMON_PASSWORDS_FILE_PATH;
        let _ = *COMMON_PASSWORDS_SET;
    }
}

pub mod rand {
    use ring::rand::SystemRandom;

    lazy_static! {
        pub static ref SECURE_RANDOM_GENERATOR: SystemRandom = SystemRandom::new();
    }

    pub fn initialize() {
        let _ = *SECURE_RANDOM_GENERATOR;
    }
}

#[cfg(test)]
pub mod testing {
    use crate::definitions::*;

    use diesel::prelude::*;
    use diesel::r2d2::{self, ConnectionManager};

    lazy_static! {
        pub static ref DB_THREAD_POOL: DbThreadPool = r2d2::Pool::builder()
            .build(ConnectionManager::<PgConnection>::new(
                crate::env::CONF.connections.database_uri.as_str()
            ))
            .expect("Failed to create DB thread pool");
    }
}

pub fn initialize() {
    // Forego lazy initialization in order to validate conf file
    if !CONF.hashing.hash_mem_size_kib.is_power_of_two() {
        eprintln!(
            "Hash memory size must be a power of two. {} is not a power of two.",
            CONF.hashing.hash_mem_size_kib
        );
        std::process::exit(1);
    }

    password::initialize();
    rand::initialize();
}
