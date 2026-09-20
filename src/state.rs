use std::{
    collections::{BTreeMap, HashMap},
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use anyhow::Context;
use chrono::Utc;
use tokio::sync::{Mutex as AsyncMutex, RwLock};
use uuid::Uuid;

use crate::{inventory, model::*, printer, store};

#[derive(Clone)]
pub struct AppState {
    pub games: Arc<Vec<Game>>,
    pub printer: Arc<PrinterConfig>,
    pub pdf_root: Arc<PathBuf>,
    pub state_directory: Arc<PathBuf>,
    pub warnings: Arc<Vec<String>>,
    pub inventory: Arc<RwLock<InventoryFile>>,
    pub plans: Arc<RwLock<HashMap<Uuid, PrintPlan>>>,
    pub runs: Arc<RwLock<BTreeMap<Uuid, PrintRun>>>,
    cancel_flags: Arc<Mutex<HashMap<Uuid, Arc<AtomicBool>>>>,
    printer_lock: Arc<AsyncMutex<()>>,
}

impl AppState {
    pub fn new(
        games: Vec<Game>,
        printer: PrinterConfig,
        pdf_root: PathBuf,
        state_directory: PathBuf,
        warnings: Vec<String>,
    ) -> anyhow::Result<Self> {
        fs::create_dir_all(state_directory.join("runs"))?;
        let inventory = inventory::load(&state_directory)?;
        let mut runs = load_runs(&state_directory)?;
        for run in runs.values_mut() {
            if matches!(
                run.status,
                RunStatus::Queued | RunStatus::Running | RunStatus::CancelRequested
            ) {
                run.status = RunStatus::Interrupted;
                run.error = Some("Paperjam restarted while this run was active; pending work was not resubmitted".into());
                run.updated_at = Utc::now();
                save_run_to(&state_directory, run)?;
            }
        }
        Ok(Self {
            games: Arc::new(games),
            printer: Arc::new(printer),
            pdf_root: Arc::new(pdf_root),
            state_directory: Arc::new(state_directory),
            warnings: Arc::new(warnings),
            inventory: Arc::new(RwLock::new(inventory)),
            plans: Arc::new(RwLock::new(HashMap::new())),
            runs: Arc::new(RwLock::new(runs)),
            cancel_flags: Arc::new(Mutex::new(HashMap::new())),
            printer_lock: Arc::new(AsyncMutex::new(())),
        })
    }

    pub async fn store_plan(&self, plan: PrintPlan) {
        self.plans.write().await.insert(plan.id, plan);
    }

    pub async fn start_run(&self, plan_id: Uuid) -> anyhow::Result<PrintRun> {
        let plan = self
            .plans
            .read()
            .await
            .get(&plan_id)
            .cloned()
            .context("unknown or expired plan")?;
        let (_, document) =
            store::find_game_document(&self.games, &plan.game_id, &plan.document_id)?;
        let jobs = printer::compile_jobs(document, &plan, &self.printer, &self.pdf_root)?;
        let consumed = {
            let mut inventory_guard = self.inventory.write().await;
            if plan.inventory_mode == InventoryMode::UseInventory {
                anyhow::ensure!(
                    inventory_guard.revision == plan.inventory_revision,
                    "inventory changed since preview; create a new plan"
                );
            }
            let consumed = inventory::consume(&mut inventory_guard, &plan)?;
            if !consumed.is_empty() {
                inventory::save(&self.state_directory, &inventory_guard)?;
            }
            consumed
        };
        let now = Utc::now();
        let id = Uuid::new_v4();
        let stock_only = jobs.is_empty();
        let run = PrintRun {
            id,
            plan_id,
            created_at: now,
            updated_at: now,
            game_id: plan.game_id.clone(),
            game_name: plan.game_name.clone(),
            document_name: plan.document_name.clone(),
            status: if stock_only {
                RunStatus::Completed
            } else {
                RunStatus::Queued
            },
            current_job: None,
            jobs: jobs
                .into_iter()
                .map(|job| RunJob {
                    job,
                    status: JobStatus::Pending,
                    cups_request_id: None,
                    error: None,
                })
                .collect(),
            consumed_inventory: consumed,
            inventory_restored: false,
            error: None,
        };
        self.runs.write().await.insert(id, run.clone());
        self.save_run(&run)?;
        if stock_only {
            return Ok(run);
        }
        let flag = Arc::new(AtomicBool::new(false));
        self.cancel_flags.lock().unwrap().insert(id, flag.clone());
        let state = self.clone();
        tokio::spawn(async move {
            state.process_run(id, flag).await;
        });
        Ok(run)
    }

    async fn process_run(&self, run_id: Uuid, cancel: Arc<AtomicBool>) {
        let _printer_guard = self.printer_lock.lock().await;
        if cancel.load(Ordering::Relaxed) {
            self.finish_canceled(run_id).await;
            return;
        }
        self.mutate_run(run_id, |r| {
            r.status = RunStatus::Running;
        })
        .await;
        let count = {
            self.runs
                .read()
                .await
                .get(&run_id)
                .map(|r| r.jobs.len())
                .unwrap_or(0)
        };
        for index in 0..count {
            if cancel.load(Ordering::Relaxed) {
                self.finish_canceled(run_id).await;
                return;
            }
            let job = {
                self.runs
                    .read()
                    .await
                    .get(&run_id)
                    .and_then(|r| r.jobs.get(index))
                    .map(|j| j.job.clone())
            };
            let Some(job) = job else { return };
            self.mutate_run(run_id, |r| r.current_job = Some(index))
                .await;
            match printer::submit(&job, &self.printer).await {
                Ok(request_id) => {
                    self.mutate_run(run_id, |r| {
                        r.jobs[index].status = JobStatus::Submitted;
                        r.jobs[index].cups_request_id = request_id.clone();
                    })
                    .await;
                    let shown = request_id.unwrap_or_else(|| format!("dry-run-{index}"));
                    if let Err(error) = printer::wait_until_complete(&shown, &self.printer, || {
                        cancel.load(Ordering::Relaxed)
                    })
                    .await
                    {
                        if cancel.load(Ordering::Relaxed) {
                            self.finish_canceled(run_id).await;
                        } else {
                            self.fail_run(run_id, index, error.to_string()).await;
                        }
                        return;
                    }
                    self.mutate_run(run_id, |r| r.jobs[index].status = JobStatus::Completed)
                        .await;
                }
                Err(error) => {
                    self.fail_run(run_id, index, error.to_string()).await;
                    return;
                }
            }
        }
        self.mutate_run(run_id, |r| {
            r.status = RunStatus::Completed;
            r.current_job = None;
        })
        .await;
        self.cancel_flags.lock().unwrap().remove(&run_id);
    }

    async fn fail_run(&self, id: Uuid, index: usize, error: String) {
        self.mutate_run(id, |r| {
            r.status = RunStatus::Failed;
            r.jobs[index].status = JobStatus::Failed;
            r.jobs[index].error = Some(error.clone());
            r.error = Some(format!("job {} failed: {error}", index + 1));
        })
        .await;
    }
    async fn finish_canceled(&self, id: Uuid) {
        self.mutate_run(id, |r| {
            r.status = RunStatus::Canceled;
            r.error = Some("Canceled by user; completed jobs may already have printed".into());
            for j in &mut r.jobs {
                if matches!(j.status, JobStatus::Pending | JobStatus::Submitted) {
                    j.status = JobStatus::Canceled;
                }
            }
        })
        .await;
    }
    async fn mutate_run(&self, id: Uuid, change: impl FnOnce(&mut PrintRun)) {
        let copy = {
            let mut runs = self.runs.write().await;
            if let Some(r) = runs.get_mut(&id) {
                change(r);
                r.updated_at = Utc::now();
                Some(r.clone())
            } else {
                None
            }
        };
        if let Some(r) = copy {
            let _ = self.save_run(&r);
        }
    }
    pub async fn cancel_run(&self, id: Uuid) -> anyhow::Result<PrintRun> {
        let flag = self
            .cancel_flags
            .lock()
            .unwrap()
            .get(&id)
            .cloned()
            .context("run is not active")?;
        flag.store(true, Ordering::Relaxed);
        self.mutate_run(id, |r| r.status = RunStatus::CancelRequested)
            .await;
        self.runs
            .read()
            .await
            .get(&id)
            .cloned()
            .context("unknown run")
    }
    pub async fn restore_stock(&self, id: Uuid) -> anyhow::Result<PrintRun> {
        let (game_id, consumed, already) = {
            let runs = self.runs.read().await;
            let r = runs.get(&id).context("unknown run")?;
            (
                r.game_id.clone(),
                r.consumed_inventory.clone(),
                r.inventory_restored,
            )
        };
        anyhow::ensure!(!already, "inventory has already been restored");
        anyhow::ensure!(!consumed.is_empty(), "this run did not consume inventory");
        {
            let mut inv = self.inventory.write().await;
            inventory::restore(&mut inv, &game_id, &consumed);
            inventory::save(&self.state_directory, &inv)?;
        }
        self.mutate_run(id, |r| r.inventory_restored = true).await;
        self.runs
            .read()
            .await
            .get(&id)
            .cloned()
            .context("unknown run")
    }
    fn save_run(&self, run: &PrintRun) -> anyhow::Result<()> {
        save_run_to(&self.state_directory, run)
    }
}

fn load_runs(state: &Path) -> anyhow::Result<BTreeMap<Uuid, PrintRun>> {
    let mut result = BTreeMap::new();
    let dir = state.join("runs");
    if !dir.exists() {
        return Ok(result);
    }
    for e in fs::read_dir(dir)? {
        let p = e?.path();
        if p.extension().and_then(|x| x.to_str()) != Some("json") {
            continue;
        }
        let run: PrintRun = serde_json::from_slice(&fs::read(&p)?)
            .with_context(|| format!("parsing {}", p.display()))?;
        result.insert(run.id, run);
    }
    Ok(result)
}
fn save_run_to(state: &Path, run: &PrintRun) -> anyhow::Result<()> {
    let dir = state.join("runs");
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.json", run.id));
    fs::write(&path, serde_json::to_vec_pretty(run)?)
        .with_context(|| format!("writing {}", path.display()))
}
