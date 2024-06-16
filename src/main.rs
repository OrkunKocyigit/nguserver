use std::collections::HashMap;
use std::env::current_dir;
use std::fmt::{Debug, Formatter};
use std::fs;
use std::fs::OpenOptions;

use axum::{Json, Router};
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::post;
use chrono::Local;
use serde::{de, Deserialize, Deserializer, Serialize};
use serde::de::{MapAccess, Visitor};
use serde_json::{json, Value};
use tower_http::cors::CorsLayer;

const DATE_FORMAT_STR: &str = "%Y-%m-%d %H:%M:%S%.3f";

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AppState {
    file_path: String,
    settings_path: String,
    settings_mapper: HashMap<String, String>,
}

impl AppState {
    pub fn file_path(&self) -> &str {
        &self.file_path
    }
    pub fn settings_path(&self) -> &str {
        &self.settings_path
    }
    pub fn settings_mapper(&self) -> &HashMap<String, String> {
        &self.settings_mapper
    }
}

#[tokio::main]
async fn main() {
    let state = load_state();
    let app = Router::new()
        .route("/", post(update_files))
        .layer(CorsLayer::permissive())
        .with_state(state);
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

fn load_state() -> AppState {
    let current_path = current_dir()
        .expect("Current dir not found")
        .join("settings.json");
    let settings = fs::read_to_string(current_path).expect("Error during reading settings.json");
    serde_json::from_str(&settings).expect("Loading state failed")
}

async fn update_files(
    State(state): State<AppState>,
    Json(optimizer): Json<Vec<Optimizer>>,
) -> StatusCode {
    println!(
        "{} Optimizer request received",
        Local::now().format(DATE_FORMAT_STR)
    );
    let optimizer_map: HashMap<_, _> = optimizer.iter().map(|o| (&o.label, &o.ids)).collect();
    update_profile(state.file_path(), &optimizer_map);
    update_settings(
        state.settings_path(),
        state.settings_mapper(),
        &optimizer_map,
    );
    println!("{} Files Updated", Local::now().format(DATE_FORMAT_STR));
    StatusCode::OK
}

fn update_settings(
    settings_path: &str,
    settings_mapper: &HashMap<String, String>,
    optimizer_map: &HashMap<&String, &Vec<u32>>,
) {
    let file = fs::File::open(settings_path).expect("Profile read failed");
    let mut settings: Value = serde_json::from_reader(file).expect("Settings is not valid json");
    for (optimizer_label, setting_label) in settings_mapper.into_iter() {
        if let Some(value) = settings.get_mut(setting_label) {
            if let Some(ids) = optimizer_map.get(&optimizer_label) {
                *value = json!(ids);
            }
        }
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(settings_path)
        .expect("Failed to open file for writing");
    serde_json::to_writer_pretty(&mut file, &settings).expect("Failed to serialize json");
}

fn update_profile(profile_path: &str, optimizer_map: &HashMap<&String, &Vec<u32>>) {
    let file = fs::File::open(profile_path).expect("Profile read failed");
    let mut profile: Profile = serde_json::from_reader(file).expect("Profile is not valid json");

    for gear in &mut profile.Breakpoints.Gear {
        if let Some(comment) = &gear.Comment {
            if let Some(ids) = optimizer_map.get(comment) {
                gear.ID.clone_from(ids); // Modify this line if ID can be changed without cloning
            }
        }
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(profile_path)
        .expect("Failed to open file for writing");
    serde_json::to_writer_pretty(&mut file, &profile).expect("Failed to serialize json");
}

#[derive(Debug)]
struct Optimizer {
    label: String,
    ids: Vec<u32>,
}

impl<'de> Deserialize<'de> for Optimizer {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct OptimizerVisitor;

        impl<'de> Visitor<'de> for OptimizerVisitor {
            type Value = Optimizer;

            fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
                formatter.write_str("a map with a single key-value pair")
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let entry = map
                    .next_entry::<String, Vec<u32>>()?
                    .ok_or_else(|| de::Error::invalid_length(0, &self))?;
                Ok(Optimizer {
                    label: entry.0,
                    ids: entry.1,
                })
            }
        }

        deserializer.deserialize_map(OptimizerVisitor)
    }
}

#[derive(Serialize, Deserialize, Debug)]
struct Profile {
    Breakpoints: Breakpoint,
}

#[derive(Serialize, Deserialize, Debug)]
struct Breakpoint {
    #[serde(flatten)]
    other_fields: Value,
    Gear: Vec<Gear>,
}

#[derive(Serialize, Deserialize, Debug)]
struct Gear {
    #[serde(flatten)]
    other_fields: Value,
    ID: Vec<u32>,
    Comment: Option<String>,
}
