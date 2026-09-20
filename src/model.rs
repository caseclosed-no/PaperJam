use std::collections::{BTreeMap, HashMap};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Game {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub episodes: Vec<Episode>,
    pub documents: Vec<Document>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Episode {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    pub id: String,
    pub name: String,
    pub file: String,
    pub default_profile: String,
    #[serde(default = "one")]
    pub copies_per_page: u32,
    pub elements: Vec<Element>,
}

fn one() -> u32 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Element {
    pub id: String,
    pub name: String,
    pub page: u32,
    pub episode: String,
    #[serde(default)]
    pub profile: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default = "default_bind")]
    pub bind: String,
    #[serde(default = "default_pdf_dir")]
    pub pdf_directory: String,
    #[serde(default = "default_state_dir")]
    pub state_directory: String,
}

fn default_bind() -> String {
    "0.0.0.0:8080".into()
}
fn default_pdf_dir() -> String {
    "data/pdfs".into()
}
fn default_state_dir() -> String {
    "data/state".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrinterConfig {
    #[serde(default = "default_backend")]
    pub backend: String,
    pub destination: String,
    #[serde(default)]
    pub server: Option<String>,
    #[serde(default = "default_poll_ms")]
    pub poll_interval_ms: u64,
    #[serde(default)]
    pub default_options: BTreeMap<String, String>,
    pub profiles: BTreeMap<String, PrintProfile>,
}

fn default_backend() -> String {
    "dry-run".into()
}
fn default_poll_ms() -> u64 {
    1_000
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrintProfile {
    pub name: String,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub media: Option<String>,
    #[serde(default)]
    pub options: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InventoryFile {
    #[serde(default)]
    pub revision: u64,
    #[serde(default)]
    pub counts: BTreeMap<String, BTreeMap<String, u32>>,
}

impl InventoryFile {
    pub fn get(&self, game_id: &str, element_id: &str) -> u32 {
        self.counts
            .get(game_id)
            .and_then(|g| g.get(element_id))
            .copied()
            .unwrap_or(0)
    }
}

#[derive(Debug, Deserialize)]
pub struct InventoryUpdate {
    pub expected_revision: u64,
    pub counts: BTreeMap<String, BTreeMap<String, u32>>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InventoryMode {
    UseInventory,
    IgnoreInventory,
}

#[derive(Debug, Deserialize)]
pub struct ProductionPlanRequest {
    pub game_id: String,
    pub document_id: String,
    pub target_games: u32,
    pub selected_element_ids: Vec<String>,
    pub inventory_mode: InventoryMode,
    #[serde(default)]
    pub group_by_profile: bool,
    #[serde(default)]
    pub profile_override: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ManualPlanRequest {
    pub game_id: String,
    pub document_id: String,
    pub selections: Vec<ManualSelection>,
    #[serde(default)]
    pub group_by_profile: bool,
    #[serde(default)]
    pub profile_override: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ManualSelection {
    pub element_id: String,
    pub quantity: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlanKind {
    Production,
    Manual,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrintPlan {
    pub id: Uuid,
    pub kind: PlanKind,
    pub created_at: DateTime<Utc>,
    pub game_id: String,
    pub game_name: String,
    pub document_id: String,
    pub document_name: String,
    pub target_games: Option<u32>,
    pub inventory_mode: InventoryMode,
    pub inventory_revision: u64,
    pub copies_per_page: u32,
    pub requirements: Vec<ElementRequirement>,
    pub sets: Vec<PlanSet>,
    pub items: Vec<PlanItem>,
    #[serde(default)]
    pub group_by_profile: bool,
    #[serde(default)]
    pub profile_override: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElementRequirement {
    pub element_id: String,
    pub element_name: String,
    pub episode: String,
    pub page: u32,
    pub profile: String,
    pub have: u32,
    pub need: u32,
    pub consume_from_stock: u32,
    pub to_produce: u32,
    pub sheets: u32,
    pub produced: u32,
    pub surplus: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanSet {
    pub number: u32,
    pub elements: Vec<SetElement>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetElement {
    pub element_id: String,
    pub element_name: String,
    pub page: u32,
    pub source: ElementSource,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ElementSource {
    Print,
    Inventory,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanItem {
    pub set: Option<u32>,
    pub element_id: String,
    pub element_name: String,
    pub page: u32,
    pub profile: String,
    pub quantity: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanResponse {
    #[serde(flatten)]
    pub plan: PrintPlan,
    pub jobs: Vec<crate::printer::PrintJob>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartRunRequest {
    pub plan_id: Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Queued,
    Running,
    Completed,
    Failed,
    CancelRequested,
    Canceled,
    Interrupted,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Pending,
    Submitted,
    Completed,
    Failed,
    Canceled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunJob {
    pub job: crate::printer::PrintJob,
    pub status: JobStatus,
    #[serde(default)]
    pub cups_request_id: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrintRun {
    pub id: Uuid,
    pub plan_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub game_id: String,
    pub game_name: String,
    pub document_name: String,
    pub status: RunStatus,
    pub current_job: Option<usize>,
    pub jobs: Vec<RunJob>,
    #[serde(default)]
    pub consumed_inventory: HashMap<String, u32>,
    #[serde(default)]
    pub inventory_restored: bool,
    #[serde(default)]
    pub error: Option<String>,
}
