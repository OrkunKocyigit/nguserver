use std::collections::HashMap;
use std::env::current_dir;
use std::fmt::{Debug, Formatter};
use std::fs;
use std::fs::OpenOptions;
use std::panic::panic_any;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use axum::{Json, Router};
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::post;
use chrono::Local;
use notify_debouncer_mini::{DebounceEventHandler, DebounceEventResult, new_debouncer};
use notify_debouncer_mini::notify::RecursiveMode;
use serde::{de, Deserialize, Deserializer, Serialize};
use serde::de::{MapAccess, Visitor};
use serde_json::{json, Value};
use tower_http::cors::CorsLayer;

const DATE_FORMAT_STR: &str = "%Y-%m-%d %H:%M:%S%.3f";
#[derive(Clone)]
struct AppState {
    settings: Arc<RwLock<Settings>>,
    optimizer: Arc<RwLock<Option<Vec<Optimizer>>>>,
}

impl DebounceEventHandler for AppState {
    fn handle_event(&mut self, res: DebounceEventResult) {
        match res {
            Ok(event) => {
                println!(
                    "{} Settings file refreshed",
                    Local::now().format(DATE_FORMAT_STR)
                );
                if let Some(s) = event.first().map(|e| load_settings(&e.path)) {
                    let mut guard = self.settings.write().unwrap();
                    let optimizer_guard = self.optimizer.read().unwrap();
                    if let Some(o) = optimizer_guard.as_ref() {
                        println!(
                            "{} Refreshing builds with last optimizer data.",
                            Local::now().format(DATE_FORMAT_STR)
                        );
                        update_game_files(o, s.file_path(), s.settings_path(), s.settings_mapper());
                        println!("{} Files Updated", Local::now().format(DATE_FORMAT_STR));
                    } else {
                        println!(
                            "{} No previous optimizer result found, skipping refresh.",
                            Local::now().format(DATE_FORMAT_STR)
                        )
                    }
                    *guard = s;
                }
            }
            Err(e) => panic_any(e),
        }
    }
}

#[derive(Clone, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct Settings {
    file_path: String,
    settings_path: String,
    settings_mapper: HashMap<String, String>,
}

impl Settings {
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
    let state_path = current_dir()
        .expect("Current dir not found")
        .join("settings.json");
    let state = load_state(&state_path);
    let mut debouncer =
        new_debouncer(Duration::from_secs(1), state.clone()).expect("Failed to create debouncer");
    debouncer
        .watcher()
        .watch(state_path.as_path(), RecursiveMode::NonRecursive)
        .expect("Failed to watch settings file");
    let app = Router::new()
        .route("/", post(update_files))
        .layer(CorsLayer::permissive())
        .with_state(state);
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

fn load_state(path_buf: &PathBuf) -> AppState {
    AppState {
        settings: Arc::new(RwLock::new(load_settings(path_buf))),
        optimizer: Arc::new(RwLock::new(None)),
    }
}

fn load_settings(path_buf: &PathBuf) -> Settings {
    let settings_str = fs::read_to_string(path_buf).expect("Error during reading settings.json");
    serde_json::from_str(&settings_str).expect("Loading state failed")
}

async fn update_files(
    State(state): State<AppState>,
    Json(optimizer): Json<Vec<Optimizer>>,
) -> StatusCode {
    println!(
        "{} Optimizer request received",
        Local::now().format(DATE_FORMAT_STR)
    );
    let settings = state.settings.read().unwrap();
    let mut optimizer_guard = state.optimizer.write().unwrap();
    if optimizer_guard.as_ref().map_or(true, |v| v != &optimizer) {
        *optimizer_guard = Some(optimizer);
        update_game_files(
            optimizer_guard.as_ref().unwrap(),
            settings.file_path(),
            settings.settings_path(),
            settings.settings_mapper(),
        );
        println!("{} Files Updated", Local::now().format(DATE_FORMAT_STR));
    } else {
        println!(
            "{} Optimizer result is same. Files won't be updated.",
            Local::now().format(DATE_FORMAT_STR)
        )
    }
    StatusCode::OK
}

fn update_game_files(
    optimizer: &[Optimizer],
    file_path: &str,
    settings_path: &str,
    settings_mapper: &HashMap<String, String>,
) {
    let optimizer_map: HashMap<_, _> = optimizer.iter().map(|o| (&o.label, &o.ids)).collect();
    update_profile(file_path, &optimizer_map);
    update_settings(settings_path, settings_mapper, &optimizer_map);
}

fn update_settings(
    settings_path: &str,
    settings_mapper: &HashMap<String, String>,
    optimizer_map: &HashMap<&String, &Vec<u32>>,
) {
    let file = fs::File::open(settings_path).expect("Profile read failed");
    let mut settings: Value = serde_json::from_reader(file).expect("Settings is not valid json");
    for (optimizer_label, setting_label) in settings_mapper.iter() {
        if let Some(value) = settings.get_mut(setting_label) {
            if let Some(ids) = optimizer_map.get(&optimizer_label) {
                if let Some(value_ids) = value.as_array() {
                    let gear_ids: Vec<u32> = value_ids
                        .iter()
                        .flat_map(|v| v.as_u64().map(|x| x as u32))
                        .collect();
                    if !vectors_equal(&gear_ids, ids) {
                        println!("Setting {setting_label} updated with {optimizer_label}");
                        *value = json!(ids);
                    }
                } else {
                    println!("Setting {setting_label} updated with {optimizer_label}");
                    *value = json!(ids);
                }
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

    for gear in &mut profile.breakpoints.gear {
        if let Some(comment) = &gear.comment {
            if let Some(ids) = optimizer_map.get(comment) {
                if !vectors_equal(&gear.id, ids) {
                    gear.id.clone_from(ids);
                    println!(
                        "{} Gear object with {comment} is updated",
                        Local::now().format(DATE_FORMAT_STR)
                    )
                }
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

fn vectors_equal<T>(first_ids: &[T], other_ids: &[T]) -> bool
where
    T: PartialEq,
{
    first_ids.len() == other_ids.len() && first_ids.iter().all(|x| other_ids.contains(x))
}

impl PartialEq for Optimizer {
    fn eq(&self, other: &Self) -> bool {
        self.label == other.label && vectors_equal(&self.ids, &other.ids)
    }
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
    #[serde(rename = "Breakpoints")]
    breakpoints: Breakpoint,
}

#[derive(Serialize, Deserialize, Debug)]
struct Breakpoint {
    #[serde(flatten)]
    other_fields: Value,
    #[serde(rename = "Gear")]
    gear: Vec<Gear>,
}

#[derive(Serialize, Deserialize, Debug)]
struct Gear {
    #[serde(flatten)]
    other_fields: Value,
    #[serde(rename = "ID")]
    id: Vec<u32>,
    #[serde(rename = "Comment")]
    comment: Option<String>,
}
